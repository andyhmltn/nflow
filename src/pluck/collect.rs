//! Collects and tokenises text candidates from every visible text and link
//! element.
//!
//! Pluck walks the accessibility trees of all on-screen windows, retaining
//! every text element (`AXStaticText`, `AXTextField`, `AXTextArea`, `AXComboBox`,
//! `AXSearchField`) *and* every link (`AXLink`). Links matter because a single
//! sentence on a web page is usually split across several AX elements -- a run
//! of static text, then a link, then more static text:
//!
//! ```text
//! For the meat served as part of such a dish, see Patty. For other uses,
//! see Hamburger (disambiguation).
//! ```
//!
//! Here "Patty" and "Hamburger (disambiguation)" are links, each its own
//! `AXLink` element. Reading each element's `AXValue` in isolation yields three
//! fragments and loses the link URLs entirely. So pluck records each piece's
//! screen frame and window id, then **reconstructs visual lines** by grouping
//! adjacent pieces that share a window and overlap vertically (with a
//! horizontal gap cap, so a new visual column starts a new line). The
//! reconstructed line carries its plain text and, separately, a markdown
//! rendering where link pieces become `[text](url)`.
//!
//! Tokens are deduplicated and re-extracted when the mode cycles, but the raw
//! pieces are read once at collection time so `ctrl-f` does not re-walk the
//! accessibility tree.

use std::collections::HashSet;

use crate::hint::collect::{self, AxElement};
use crate::types::Rect;

/// Tokens shorter than this are discarded. Five matches the terminal `pluck`:
/// it keeps the palette to meaningful words rather than every `a`, `the`, `of`.
const MIN_TOKEN_LEN: usize = 5;

/// Pieces whose horizontal gap exceeds this are treated as a new visual line,
/// even when they overlap vertically. Catches text that wraps to a new column
/// or sits in a sidebar.
const LINE_GAP: f64 = 40.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Words,
    Lines,
}

impl Mode {
    pub fn next(self) -> Mode {
        match self {
            Mode::Words => Mode::Lines,
            Mode::Lines => Mode::Words,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Mode::Words => "words",
            Mode::Lines => "lines",
        }
    }
}

/// A single inline text piece with its screen geometry. A link piece carries
/// its URL so the line can be rendered as markdown.
#[derive(Debug, Clone)]
pub struct Piece {
    pub text: String,
    pub frame: Rect,
    pub window_id: Option<u32>,
    pub url: Option<String>,
}

/// A reconstructed visual line: the plain text of its pieces joined, and the
/// same pieces kept so a markdown rendering can be produced on demand.
#[derive(Debug, Clone)]
pub struct Line {
    pub plain: String,
    pieces: Vec<Piece>,
}

impl Line {
    /// Render the line as markdown: each link piece becomes `[text](url)`,
    /// plain pieces contribute their text verbatim. Returns `None` when the
    /// line contains no links (the plain text is sufficient).
    pub fn markdown(&self) -> Option<String> {
        if !self.pieces.iter().any(|p| p.url.is_some()) {
            return None;
        }
        let mut out = String::new();
        for p in &self.pieces {
            if let Some(url) = &p.url {
                out.push('[');
                out.push_str(&p.text);
                out.push_str("](");
                out.push_str(url);
                out.push(')');
            } else {
                out.push_str(&p.text);
            }
        }
        Some(out)
    }
}

/// Walk every on-screen window and gather text + link pieces.
pub fn collect_pieces(screen: Rect) -> Vec<Piece> {
    let mut out = Vec::new();
    for target in collect::collect_text_and_link_targets(screen) {
        let Some(element) = target.element else {
            continue;
        };
        let text = element_text(&element);
        if text.trim().is_empty() {
            continue;
        }
        let url = if is_link_element(&element) {
            element.url()
        } else {
            None
        };
        out.push(Piece {
            text,
            frame: target.frame,
            window_id: element.window_id(),
            url,
        });
    }
    out
}

fn element_text(element: &AxElement) -> String {
    // Links often expose their visible label via AXTitle/AXDescription rather
    // than AXValue; fall back to value (and to descendant text for grouped
    // links) the same way hint-mode's copy-link does.
    if is_link_element(element) {
        if let Some(t) = element.link_text() {
            return t;
        }
    }
    element.value().unwrap_or_default()
}

fn is_link_element(element: &AxElement) -> bool {
    element.string_attr("AXRole").as_deref() == Some("AXLink")
}

/// Reconstruct visual lines from collected pieces. Pieces are grouped by
/// window, then sorted by reading order (top-to-bottom, left-to-right), then
/// merged into a line while they share a vertical band and stay within
/// `LINE_GAP` horizontally of the running line's right edge.
pub fn reconstruct_lines(pieces: Vec<Piece>) -> Vec<Line> {
    if pieces.is_empty() {
        return Vec::new();
    }

    // Group by window id so text from adjacent tiled windows never merges.
    let mut by_window: Vec<(Option<u32>, Vec<Piece>)> = Vec::new();
    for p in pieces {
        if let Some(slot) = by_window.iter_mut().find(|(w, _)| *w == p.window_id) {
            slot.1.push(p);
        } else {
            by_window.push((p.window_id, vec![p]));
        }
    }

    let mut lines: Vec<Line> = Vec::new();
    for (_, mut group) in by_window {
        // Reading order: top by line-center, then left by left edge.
        group.sort_by(|a, b| {
            let ay = a.frame.y + a.frame.height / 2.0;
            let by = b.frame.y + b.frame.height / 2.0;
            ay.partial_cmp(&by)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then(
                    a.frame
                        .x
                        .partial_cmp(&b.frame.x)
                        .unwrap_or(std::cmp::Ordering::Equal),
                )
        });

        let mut current: Option<Vec<Piece>> = None;
        for p in group {
            let start = match &mut current {
                None => {
                    current = Some(vec![p]);
                    continue;
                }
                Some(c) => c,
            };
            let last = start.last().unwrap();
            let same_band = v_overlap(&last.frame, &p.frame);
            let gap = p.frame.x - (last.frame.x + last.frame.width);
            if same_band && gap <= LINE_GAP {
                start.push(p);
            } else {
                lines.push(join_line(std::mem::take(start)));
                *start = vec![p];
            }
        }
        if let Some(c) = current.take() {
            lines.push(join_line(c));
        }
    }
    lines
}

fn v_overlap(a: &Rect, b: &Rect) -> bool {
    let a_top = a.y;
    let a_bottom = a.y + a.height;
    let b_top = b.y;
    let b_bottom = b.y + b.height;
    let overlap = a_bottom.min(b_bottom) - a_top.max(b_top);
    // Require the smaller element to overlap the larger by at least half its
    // height, so a small superscript or footnote marker on the same line is
    // absorbed but a genuinely different line is not.
    let smaller = a.height.min(b.height);
    overlap >= smaller * 0.5
}

fn join_line(pieces: Vec<Piece>) -> Line {
    let mut plain = String::new();
    let mut prev: Option<&Piece> = None;
    for p in &pieces {
        if let Some(prev) = prev {
            let gap = p.frame.x - (prev.frame.x + prev.frame.width);
            // Insert a space when pieces don't already touch and neither side
            // carries its own whitespace. Skip the space when the next piece
            // starts with punctuation (`.`, `,`, ...) so "Patty" + "." reads as
            // "Patty." rather than "Patty .".
            let starts_with_punct = p.text.chars().next().is_some_and(|c| {
                matches!(
                    c,
                    '.' | ',' | ';' | ':' | '!' | '?' | ')' | ']' | '\u{2019}' | '\''
                )
            });
            let needs_space = gap > 1.0
                && !starts_with_punct
                && !prev.text.ends_with(char::is_whitespace)
                && !p.text.starts_with(char::is_whitespace);
            if needs_space {
                plain.push(' ');
            }
        }
        plain.push_str(&p.text);
        prev = Some(p);
    }
    Line { plain, pieces }
}

/// A palette candidate. `markdown` is `Some` only for line-mode candidates
/// reconstructed from pieces that include links, so `ctrl-m` can copy the line
/// with links rendered as `[text](url)`.
#[derive(Debug, Clone)]
pub struct Candidate {
    pub display: String,
    pub markdown: Option<String>,
}

/// Tokenise the collected lines into deduplicated candidates for `mode`,
/// preserving first-seen order so the palette is stable across keystrokes.
/// Words come from the plain text (no markdown); lines come from the
/// reconstructed visual lines (so a line that mixes text and links still
/// appears whole, and carries its markdown rendering when it has links).
pub fn extract(lines: &[Line], mode: Mode) -> Vec<Candidate> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for line in lines {
        for token in tokenize(&line.plain, mode) {
            if token.len() < MIN_TOKEN_LEN || !seen.insert(token.clone()) {
                continue;
            }
            let markdown = match mode {
                Mode::Lines => line.markdown(),
                Mode::Words => None,
            };
            out.push(Candidate {
                display: token,
                markdown,
            });
        }
    }
    out
}

/// The join separator used when copying multiple marked tokens at once.
pub fn join_separator(mode: Mode) -> &'static str {
    match mode {
        Mode::Words => " ",
        Mode::Lines => "\n",
    }
}

fn tokenize(value: &str, mode: Mode) -> Vec<String> {
    match mode {
        Mode::Words => value
            .split_whitespace()
            .map(|t| trim_token(t).to_owned())
            .collect(),
        Mode::Lines => value.lines().map(|l| l.trim().to_owned()).collect(),
    }
}

/// Strip surrounding brackets/quotes and trailing sentence punctuation, the
/// same trimming the terminal `pluck` uses so `(foo),` and `foo:` collapse to
/// `foo`.
fn trim_token(token: &str) -> &str {
    let stripped = token.trim_matches(|c| {
        matches!(
            c,
            '(' | ')' | '[' | ']' | '{' | '}' | '<' | '>' | '\'' | '"' | '`' | ',' | ';'
        )
    });
    stripped.trim_end_matches(['.', ':'])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn piece(text: &str, x: f64, y: f64, w: f64, h: f64, url: Option<&str>) -> Piece {
        Piece {
            text: text.to_string(),
            frame: Rect {
                x,
                y,
                width: w,
                height: h,
            },
            window_id: Some(1),
            url: url.map(str::to_string),
        }
    }

    fn line(plain: &str, pieces: Vec<Piece>) -> Line {
        Line {
            plain: plain.to_string(),
            pieces,
        }
    }

    #[test]
    fn reconstruct_merges_inline_link_pieces_into_one_line() {
        let pieces = vec![
            piece(
                "For the meat served as part of such a dish, see ",
                10.0,
                100.0,
                300.0,
                20.0,
                None,
            ),
            piece(
                "Patty",
                312.0,
                100.0,
                40.0,
                20.0,
                Some("https://en.wikipedia.org/wiki/Patty"),
            ),
            piece(". For other uses, see ", 354.0, 100.0, 150.0, 20.0, None),
            piece(
                "Hamburger (disambiguation)",
                506.0,
                100.0,
                200.0,
                20.0,
                Some("https://en.wikipedia.org/wiki/Hamburger_(disambiguation)"),
            ),
            piece(".", 708.0, 100.0, 5.0, 20.0, None),
        ];
        let lines = reconstruct_lines(pieces);
        assert_eq!(lines.len(), 1);
        assert_eq!(
            lines[0].plain,
            "For the meat served as part of such a dish, see Patty. For other uses, see Hamburger (disambiguation)."
        );
        let md = lines[0].markdown().unwrap();
        assert!(md.contains("[Patty](https://en.wikipedia.org/wiki/Patty)"));
        assert!(md.contains("[Hamburger (disambiguation)](https://en.wikipedia.org/wiki/Hamburger_(disambiguation))"));
        assert!(md.contains("For the meat served as part of such a dish, see "));
    }

    #[test]
    fn reconstruct_splits_on_new_visual_line() {
        let pieces = vec![
            piece("first line", 10.0, 100.0, 80.0, 20.0, None),
            piece("second line", 10.0, 140.0, 90.0, 20.0, None),
        ];
        let lines = reconstruct_lines(pieces);
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].plain, "first line");
        assert_eq!(lines[1].plain, "second line");
    }

    #[test]
    fn reconstruct_splits_on_large_horizontal_gap() {
        let pieces = vec![
            piece("left column text", 10.0, 100.0, 120.0, 20.0, None),
            piece("right column text", 700.0, 100.0, 120.0, 20.0, None),
        ];
        let lines = reconstruct_lines(pieces);
        assert_eq!(lines.len(), 2);
    }

    #[test]
    fn reconstruct_separates_windows() {
        let mut pieces = vec![
            piece("window one text", 10.0, 100.0, 120.0, 20.0, None),
            piece("window two text", 700.0, 100.0, 120.0, 20.0, None),
        ];
        pieces[1].window_id = Some(2);
        let lines = reconstruct_lines(pieces);
        assert_eq!(lines.len(), 2);
    }

    #[test]
    fn markdown_none_when_no_links() {
        let l = line(
            "plain text",
            vec![piece("plain text", 0.0, 0.0, 80.0, 20.0, None)],
        );
        assert!(l.markdown().is_none());
    }

    #[test]
    fn words_extract_from_reconstructed_line() {
        let pieces = vec![
            piece("see ", 0.0, 0.0, 30.0, 20.0, None),
            piece("Patty", 32.0, 0.0, 40.0, 20.0, Some("https://e/p")),
        ];
        let lines = reconstruct_lines(pieces);
        let out: Vec<String> = extract(&lines, Mode::Words)
            .into_iter()
            .map(|c| c.display)
            .collect();
        assert!(out.contains(&"Patty".to_string()));
        // Words never carry markdown.
        assert!(extract(&lines, Mode::Words)
            .iter()
            .all(|c| c.markdown.is_none()));
    }

    #[test]
    fn lines_extract_whole_reconstructed_line_with_markdown() {
        let pieces = vec![
            piece("For the dish, see ", 0.0, 0.0, 120.0, 20.0, None),
            piece("Patty", 122.0, 0.0, 40.0, 20.0, Some("https://e/p")),
        ];
        let lines = reconstruct_lines(pieces);
        let out = extract(&lines, Mode::Lines);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].display, "For the dish, see Patty");
        let md = out[0].markdown.as_ref().unwrap();
        assert!(md.contains("[Patty](https://e/p)"));
    }

    #[test]
    fn words_split_and_trim_punctuation() {
        let lines = vec![line("(hello), world!", vec![])];
        let out: Vec<String> = extract(&lines, Mode::Words)
            .into_iter()
            .map(|c| c.display)
            .collect();
        assert_eq!(out, vec!["hello", "world!"]);
    }

    #[test]
    fn short_tokens_dropped() {
        let lines = vec![line("a ab abc abcd abcde", vec![])];
        let out: Vec<String> = extract(&lines, Mode::Words)
            .into_iter()
            .map(|c| c.display)
            .collect();
        assert_eq!(out, vec!["abcde"]);
    }

    #[test]
    fn dedups_preserving_first_seen() {
        let lines = vec![line("alpha gamma", vec![]), line("gamma delta", vec![])];
        let out: Vec<String> = extract(&lines, Mode::Words)
            .into_iter()
            .map(|c| c.display)
            .collect();
        assert_eq!(out, vec!["alpha", "gamma", "delta"]);
    }

    #[test]
    fn trim_handles_trailing_dot_and_colon() {
        assert_eq!(trim_token("end."), "end");
        assert_eq!(trim_token("key:"), "key");
        assert_eq!(trim_token("[bracketed]"), "bracketed");
        assert_eq!(trim_token("'quoted'"), "quoted");
    }

    #[test]
    fn mode_cycles() {
        assert_eq!(Mode::Words.next(), Mode::Lines);
        assert_eq!(Mode::Lines.next(), Mode::Words);
    }

    #[test]
    fn join_separator_matches_mode() {
        assert_eq!(join_separator(Mode::Words), " ");
        assert_eq!(join_separator(Mode::Lines), "\n");
    }
}
