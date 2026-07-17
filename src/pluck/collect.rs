//! Collects and tokenises text candidates from every visible text element.
//!
//! Pluck reuses hint-mode's `collect_text_targets` to walk the accessibility
//! trees of all on-screen windows and retain every text element (`AXStaticText`,
//! `AXTextField`, `AXTextArea`, `AXComboBox`, `AXSearchField`). Each retained
//! element's `AXValue` is read once and cached; tokenisation then runs over the
//! cached strings, so cycling modes with `ctrl-f` does not re-query the tree.

use std::collections::HashSet;

use crate::hint::collect;
use crate::types::Rect;

/// Tokens shorter than this are discarded. Five matches the terminal `pluck`:
/// it keeps the palette to meaningful words rather than every `a`, `the`, `of`.
const MIN_TOKEN_LEN: usize = 5;

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

/// Walk every on-screen window and read the `AXValue` of each text element.
/// Returns the raw values; `extract` turns them into candidates.
pub fn collect_text_values(screen: Rect) -> Vec<String> {
    let mut out = Vec::new();
    for target in collect::collect_text_targets(screen) {
        let Some(element) = target.element else {
            continue;
        };
        if let Some(value) = element.value() {
            if !value.is_empty() {
                out.push(value);
            }
        }
    }
    out
}

/// Tokenise the collected values into deduplicated candidates for `mode`,
/// preserving first-seen order so the palette is stable across keystrokes.
pub fn extract(values: &[String], mode: Mode) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for value in values {
        for token in tokenize(value, mode) {
            if token.len() >= MIN_TOKEN_LEN && seen.insert(token.clone()) {
                out.push(token);
            }
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

    #[test]
    fn words_split_and_trim_punctuation() {
        let values = vec!["(hello), world!".to_string()];
        let out = extract(&values, Mode::Words);
        assert_eq!(out, vec!["hello", "world!"]);
    }

    #[test]
    fn short_tokens_dropped() {
        let values = vec!["a ab abc abcd abcde".to_string()];
        let out = extract(&values, Mode::Words);
        assert_eq!(out, vec!["abcde"]);
    }

    #[test]
    fn dedups_preserving_first_seen() {
        let values = vec!["alpha gamma".to_string(), "gamma delta".to_string()];
        let out = extract(&values, Mode::Words);
        assert_eq!(out, vec!["alpha", "gamma", "delta"]);
    }

    #[test]
    fn lines_mode_keeps_whole_lines() {
        let values = vec!["first line\nsecond line\nx".to_string()];
        let out = extract(&values, Mode::Lines);
        assert_eq!(out, vec!["first line", "second line"]);
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
