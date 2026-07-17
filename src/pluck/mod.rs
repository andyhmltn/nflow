//! Pluck: a fuzzy finder over all the visible text on screen.
//!
//! Trigger the hotkey and nflow collects every text and link element the
//! accessibility tree exposes across all on-screen windows, reconstructs visual
//! lines (so a sentence split across static text and inline links appears
//! whole), tokenises it into words (or lines), and renders a centered palette.
//! Type to fuzzy-filter, navigate with `ctrl-j`/`ctrl-k`, and press `Enter` to
//! copy the highlighted token (or every `Tab`-marked token) to the clipboard.
//! `ctrl-f` cycles the tokenisation mode; `ctrl-m` copies the selection rendered
//! as markdown, with links expanded to `[text](url)`.
//!
//! Pluck reuses hint-mode's text/link collector and menu-search's fuzzy
//! matcher, so it sees exactly what the accessibility tree sees and ranks
//! candidates the same way menu-search ranks menu items.

mod collect;
mod overlay;

use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use objc2_app_kit::{NSPasteboard, NSPasteboardTypeString};
use objc2_foundation::{MainThreadMarker, NSString};

use collect::{Candidate, Mode};
use overlay::{PluckOverlay, PluckRow, PluckSnapshot};

use crate::types::Rect;

static ACTIVE: AtomicBool = AtomicBool::new(false);
static SESSION: Mutex<Option<Session>> = Mutex::new(None);

const MAX_VISIBLE_ROWS: usize = 14;
const MAX_FILTERED: usize = 200;

struct Session {
    lines: Vec<collect::Line>,
    candidates: Vec<Candidate>,
    mode: Mode,
    query: String,
    filtered: Vec<usize>,
    selected: usize,
    marked: HashSet<usize>,
    overlay: PluckOverlay,
}

unsafe impl Send for Session {}

pub fn is_active() -> bool {
    ACTIVE.load(Ordering::SeqCst)
}

pub fn toggle(screen: Rect) {
    let _ = MainThreadMarker::new().expect("pluck must run on the main thread");
    if is_active() {
        return;
    }
    let pieces = collect::collect_pieces(screen);
    let lines = collect::reconstruct_lines(pieces);
    if lines.is_empty() {
        crate::toast::show("No text on screen");
        return;
    }
    let mode = Mode::Lines;
    let candidates = collect::extract(&lines, mode);
    if candidates.is_empty() {
        crate::toast::show("No text on screen");
        return;
    }
    let overlay = PluckOverlay::show();
    let mut session = Session {
        lines,
        candidates,
        mode,
        query: String::new(),
        filtered: Vec::new(),
        selected: 0,
        marked: HashSet::new(),
        overlay,
    };
    recompute(&mut session);
    render(&session);
    *SESSION.lock().unwrap_or_else(|e| e.into_inner()) = Some(session);
    ACTIVE.store(true, Ordering::SeqCst);
}

pub fn handle_key(
    keycode: u32,
    typed: Option<char>,
    modifiers: u32,
    is_escape: bool,
    is_backspace: bool,
    is_return: bool,
) {
    let _ = MainThreadMarker::new().expect("pluck must run on the main thread");
    if is_escape {
        end_session();
        return;
    }

    let ctrl = modifiers & crate::hotkey::CONTROL_KEY != 0;

    if ctrl {
        if let Some(ch) = crate::hotkey::char_for_keycode(keycode) {
            if ch == 'f' {
                let mut guard = SESSION.lock().unwrap_or_else(|e| e.into_inner());
                let Some(session) = guard.as_mut() else {
                    return;
                };
                session.mode = session.mode.next();
                session.candidates = collect::extract(&session.lines, session.mode);
                session.marked.clear();
                session.selected = 0;
                recompute(session);
                render(session);
                return;
            }
            if ch == 'm' {
                let mut guard = SESSION.lock().unwrap_or_else(|e| e.into_inner());
                let Some(session) = guard.as_mut() else {
                    return;
                };
                let to_copy = join_selection_markdown(
                    &session.candidates,
                    &session.filtered,
                    session.selected,
                    &session.marked,
                );
                drop(guard);
                if let Some(text) = to_copy {
                    write_plain(&text);
                    crate::toast::show("Copied markdown");
                }
                end_session();
                return;
            }
        }
    }

    let mut guard = SESSION.lock().unwrap_or_else(|e| e.into_inner());
    let Some(session) = guard.as_mut() else {
        return;
    };

    if is_return {
        let to_copy = join_selection(
            &session.candidates,
            &session.filtered,
            session.selected,
            &session.marked,
            session.mode,
        );
        drop(guard);
        if let Some(text) = to_copy {
            write_plain(&text);
            crate::toast::show("Copied");
        }
        end_session();
        return;
    }

    if ctrl || is_arrow(keycode) {
        if let Some(delta) = nav_delta(keycode, ctrl) {
            let len = session.filtered.len();
            if len > 0 {
                let next = (session.selected as isize + delta).clamp(0, len as isize - 1) as usize;
                if next != session.selected {
                    session.selected = next;
                    render(session);
                }
            }
            return;
        }
    }

    if is_tab(keycode) {
        if let Some(&item_idx) = session.filtered.get(session.selected) {
            if !session.marked.insert(item_idx) {
                session.marked.remove(&item_idx);
            }
            render(session);
        }
        return;
    }

    if is_backspace {
        if session.query.pop().is_some() {
            session.selected = 0;
            recompute(session);
            render(session);
        }
        return;
    }

    if let Some(ch) = typed {
        session.query.push(ch);
        session.selected = 0;
        recompute(session);
        render(session);
    }
}

fn end_session() {
    let _ = MainThreadMarker::new().expect("pluck must run on the main thread");
    let session = SESSION.lock().unwrap_or_else(|e| e.into_inner()).take();
    ACTIVE.store(false, Ordering::SeqCst);
    if let Some(mut s) = session {
        s.overlay.close();
    }
}

fn recompute(session: &mut Session) {
    if session.query.is_empty() {
        session.filtered = session
            .candidates
            .iter()
            .enumerate()
            .map(|(i, _)| i)
            .collect();
        return;
    }
    let mut scored: Vec<(i64, usize)> = Vec::new();
    for (idx, candidate) in session.candidates.iter().enumerate() {
        let Some(m) = crate::menusearch::fuzzy::match_query(&session.query, &candidate.display) else {
            continue;
        };
        scored.push((m.score, idx));
    }
    scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    session.filtered = scored
        .into_iter()
        .map(|(_, idx)| idx)
        .take(MAX_FILTERED)
        .collect();
}

fn render(session: &Session) {
    let snapshot = build_snapshot(session);
    session.overlay.set_snapshot(snapshot);
}

fn build_snapshot(session: &Session) -> PluckSnapshot {
    let top = window_top(session.selected, session.filtered.len());
    let end = (top + MAX_VISIBLE_ROWS).min(session.filtered.len());
    let rows: Vec<PluckRow> = session.filtered[top..end]
        .iter()
        .enumerate()
        .map(|(vis, &item_idx)| {
            let candidate = &session.candidates[item_idx];
            let display = candidate.display.clone();
            let positions = if session.query.is_empty() {
                Vec::new()
            } else {
                crate::menusearch::fuzzy::match_query(&session.query, &display)
                    .map(|m| m.positions)
                    .unwrap_or_default()
            };
            PluckRow {
                display,
                matched_positions: positions,
                selected: vis == (session.selected - top),
                marked: session.marked.contains(&item_idx),
                md: candidate.markdown.is_some(),
            }
        })
        .collect();

    PluckSnapshot {
        query: session.query.clone(),
        mode: session.mode.name().to_string(),
        cursor_visible: true,
        rows,
        marked_count: session.marked.len(),
    }
}

fn join_selection(
    candidates: &[Candidate],
    filtered: &[usize],
    selected: usize,
    marked: &HashSet<usize>,
    mode: Mode,
) -> Option<String> {
    let sep = collect::join_separator(mode);
    if !marked.is_empty() {
        let mut picked: Vec<&str> = Vec::new();
        for &item_idx in filtered {
            if marked.contains(&item_idx) {
                picked.push(&candidates[item_idx].display);
            }
        }
        if picked.is_empty() {
            return None;
        }
        return Some(picked.join(sep));
    }
    filtered
        .get(selected)
        .map(|&i| candidates[i].display.clone())
}

fn join_selection_markdown(
    candidates: &[Candidate],
    filtered: &[usize],
    selected: usize,
    marked: &HashSet<usize>,
) -> Option<String> {
    let render = |c: &Candidate| c.markdown.clone().unwrap_or_else(|| c.display.clone());
    if !marked.is_empty() {
        let mut picked: Vec<String> = Vec::new();
        for &item_idx in filtered {
            if marked.contains(&item_idx) {
                picked.push(render(&candidates[item_idx]));
            }
        }
        if picked.is_empty() {
            return None;
        }
        return Some(picked.join("\n"));
    }
    filtered.get(selected).map(|&i| render(&candidates[i]))
}

fn window_top(selected: usize, len: usize) -> usize {
    if len <= MAX_VISIBLE_ROWS {
        return 0;
    }
    let half = MAX_VISIBLE_ROWS / 2;
    let top = selected.saturating_sub(half);
    let max_top = len - MAX_VISIBLE_ROWS;
    top.min(max_top)
}

fn is_arrow(keycode: u32) -> bool {
    matches!(keycode, 0x7D | 0x7E)
}

fn is_tab(keycode: u32) -> bool {
    keycode == 0x30
}

fn nav_delta(keycode: u32, ctrl: bool) -> Option<isize> {
    if ctrl {
        match crate::hotkey::char_for_keycode(keycode)? {
            'j' | 'n' => Some(1),
            'k' | 'p' => Some(-1),
            _ => None,
        }
    } else if keycode == 0x7D {
        Some(1)
    } else if keycode == 0x7E {
        Some(-1)
    } else {
        None
    }
}

fn write_plain(text: &str) {
    unsafe {
        let pb = NSPasteboard::generalPasteboard();
        pb.clearContents();
        pb.setString_forType(&NSString::from_str(text), NSPasteboardTypeString);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cand(display: &str, md: Option<&str>) -> Candidate {
        Candidate {
            display: display.to_string(),
            markdown: md.map(str::to_string),
        }
    }

    #[test]
    fn window_top_keeps_selection_centered() {
        assert_eq!(window_top(20, 100), 13);
        assert_eq!(window_top(1, 100), 0);
        assert_eq!(window_top(99, 100), 86);
    }

    #[test]
    fn window_top_returns_zero_for_short_list() {
        assert_eq!(window_top(3, 5), 0);
    }

    #[test]
    fn selection_copies_highlighted_when_none_marked() {
        let candidates = vec![cand("alpha", None), cand("beta", None), cand("gamma", None)];
        let filtered = vec![0, 1, 2];
        let marked = HashSet::new();
        assert_eq!(
            join_selection(&candidates, &filtered, 2, &marked, Mode::Words).as_deref(),
            Some("gamma")
        );
    }

    #[test]
    fn selection_copies_marked_in_filtered_order() {
        let candidates = vec![
            cand("alpha", None),
            cand("beta", None),
            cand("gamma", None),
            cand("delta", None),
        ];
        let filtered = vec![2, 0, 3, 1];
        let marked = HashSet::from([0, 2]);
        assert_eq!(
            join_selection(&candidates, &filtered, 1, &marked, Mode::Words).as_deref(),
            Some("gamma alpha")
        );
    }

    #[test]
    fn selection_uses_newline_separator_in_lines_mode() {
        let candidates = vec![cand("first line", None), cand("second line", None)];
        let filtered = vec![0, 1];
        let marked = HashSet::from([0, 1]);
        assert_eq!(
            join_selection(&candidates, &filtered, 0, &marked, Mode::Lines).as_deref(),
            Some("first line\nsecond line")
        );
    }

    #[test]
    fn markdown_copy_uses_markdown_when_available() {
        let candidates = vec![
            cand("see Patty", Some("see [Patty](https://e/p)")),
            cand("plain", None),
        ];
        let filtered = vec![0, 1];
        let marked = HashSet::new();
        assert_eq!(
            join_selection_markdown(&candidates, &filtered, 0, &marked).as_deref(),
            Some("see [Patty](https://e/p)")
        );
    }

    #[test]
    fn markdown_copy_falls_back_to_plain_for_words() {
        let candidates = vec![cand("alpha", None), cand("beta", None)];
        let filtered = vec![0, 1];
        let marked = HashSet::new();
        assert_eq!(
            join_selection_markdown(&candidates, &filtered, 1, &marked).as_deref(),
            Some("beta")
        );
    }

    #[test]
    fn markdown_copy_joins_marked_with_newline() {
        let candidates = vec![
            cand("see Patty", Some("see [Patty](https://e/p)")),
            cand("see Hamburger", Some("see [Hamburger](https://e/h)")),
        ];
        let filtered = vec![0, 1];
        let marked = HashSet::from([0, 1]);
        assert_eq!(
            join_selection_markdown(&candidates, &filtered, 0, &marked).as_deref(),
            Some("see [Patty](https://e/p)\nsee [Hamburger](https://e/h)")
        );
    }

    #[test]
    fn arrow_keys_are_recognised() {
        assert!(is_arrow(0x7D));
        assert!(is_arrow(0x7E));
        assert!(!is_arrow(0x26));
    }

    #[test]
    fn tab_keycode_recognised() {
        assert!(is_tab(0x30));
        assert!(!is_tab(0x26));
    }

    #[test]
    fn ctrl_jk_navigate() {
        assert_eq!(nav_delta(0x26, true), Some(1));
        assert_eq!(nav_delta(0x28, true), Some(-1));
    }

    #[test]
    fn plain_letters_are_not_navigation_without_ctrl() {
        assert_eq!(nav_delta(0x26, false), None);
    }
}
