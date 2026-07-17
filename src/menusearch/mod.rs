//! Menu search: a fuzzy command palette over the frontmost app's menu bar.
//!
//! Trigger the hotkey and nflow collects every pressable, enabled leaf in the
//! active application's menu bar, assigns each a stable hint code, and renders a
//! centered palette. The palette has two phases:
//!
//! - **Search** (default): type a query and the list fuzzy-filters live.
//!   Navigate with `ctrl-j`/`ctrl-k` (also arrow keys and `ctrl-n`/`ctrl-p`),
//!   confirm with `Enter`. `Esc` drops into Code phase.
//! - **Code**: type a hint code to select that item instantly, hint-mode style.
//!   `Backspace` deletes, `Enter` selects the first code match, `Esc` exits.
//!
//! Selecting an item performs `AXPress` on its `AXMenuItem`, the same action
//! the system posts when you click the menu entry.

mod collect;
pub mod fuzzy;
mod overlay;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use objc2_foundation::MainThreadMarker;

use collect::MenuItem;
use overlay::{MenuOverlay, MenuRow, MenuSnapshot};

use crate::hint::labels;
use crate::hint::matcher::{self, MatchResult};

static ACTIVE: AtomicBool = AtomicBool::new(false);
static SESSION: Mutex<Option<Session>> = Mutex::new(None);

const MAX_VISIBLE_ROWS: usize = 14;
const MAX_FILTERED: usize = 200;

enum SessionPhase {
    Search {
        query: String,
        filtered: Vec<usize>,
        selected: usize,
    },
    Code {
        typed: String,
    },
}

struct Session {
    items: Vec<MenuItem>,
    codes: Vec<String>,
    phase: SessionPhase,
    overlay: MenuOverlay,
}

unsafe impl Send for Session {}

pub fn is_active() -> bool {
    ACTIVE.load(Ordering::SeqCst)
}

pub fn toggle() {
    let _ = MainThreadMarker::new().expect("menu search must run on the main thread");
    if is_active() {
        return;
    }
    let items = collect::collect_menu_items();
    if items.is_empty() {
        crate::toast::show("No menu items");
        return;
    }
    let codes = labels::generate(items.len());
    let overlay = MenuOverlay::show();
    let mut session = Session {
        items,
        codes,
        phase: SessionPhase::Search {
            query: String::new(),
            filtered: Vec::new(),
            selected: 0,
        },
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
    let _ = MainThreadMarker::new().expect("menu search must run on the main thread");
    if is_escape {
        let mut guard = SESSION.lock().unwrap_or_else(|e| e.into_inner());
        let Some(session) = guard.as_mut() else {
            return;
        };
        match session.phase {
            SessionPhase::Search { .. } => {
                session.phase = SessionPhase::Code {
                    typed: String::new(),
                };
                render(session);
            }
            SessionPhase::Code { .. } => {
                drop(guard);
                end_session();
            }
        }
        return;
    }

    let mut guard = SESSION.lock().unwrap_or_else(|e| e.into_inner());
    let Some(session) = guard.as_mut() else {
        return;
    };

    let ctrl = modifiers & crate::hotkey::CONTROL_KEY != 0;
    match &mut session.phase {
        SessionPhase::Search {
            query,
            filtered,
            selected,
        } => {
            if is_return {
                if let Some(&item_idx) = filtered.get(*selected) {
                    if !session.items[item_idx].enabled {
                        return;
                    }
                    let pressed = session.items[item_idx].element.press();
                    drop(guard);
                    end_session();
                    if !pressed {
                        log::warn!("menu search: AXPress failed");
                    }
                }
                return;
            }
            if ctrl || is_arrow(keycode) {
                if let Some(delta) = nav_delta(keycode, ctrl) {
                    let len = filtered.len();
                    if len > 0 {
                        let next = (*selected as isize + delta).clamp(0, len as isize - 1) as usize;
                        if next != *selected {
                            *selected = next;
                            render(session);
                        }
                    }
                    return;
                }
            }
            if is_backspace {
                if query.pop().is_some() {
                    *selected = 0;
                    recompute(session);
                    render(session);
                }
                return;
            }
            if let Some(ch) = typed {
                query.push(ch);
                *selected = 0;
                recompute(session);
                render(session);
            }
        }
        SessionPhase::Code { typed: code_typed } => {
            if is_return {
                let hit = session
                    .codes
                    .iter()
                    .position(|c| c.starts_with(code_typed.as_str()));
                if let Some(i) = hit {
                    let pressed = session.items[i].element.press();
                    let code = code_typed.clone();
                    drop(guard);
                    end_session();
                    if !pressed {
                        log::warn!("menu search: AXPress failed for code {code:?}");
                    }
                }
                return;
            }
            if is_backspace {
                if code_typed.pop().is_some() {
                    render(session);
                }
                return;
            }
            let Some(ch) = typed
                .map(|c| c.to_ascii_lowercase())
                .or_else(|| crate::hotkey::char_for_keycode(keycode))
            else {
                return;
            };
            let candidate = format!("{code_typed}{ch}");
            match matcher::classify(&session.codes, &candidate) {
                MatchResult::NoMatch => {}
                MatchResult::Pending => {
                    *code_typed = candidate;
                    render(session);
                }
                MatchResult::Hit(i) => {
                    let pressed = session.items[i].element.press();
                    drop(guard);
                    end_session();
                    if !pressed {
                        log::warn!("menu search: AXPress failed for code {candidate:?}");
                    }
                }
            }
        }
    }
}

fn end_session() {
    let _ = MainThreadMarker::new().expect("menu search must run on the main thread");
    let session = SESSION.lock().unwrap_or_else(|e| e.into_inner()).take();
    ACTIVE.store(false, Ordering::SeqCst);
    if let Some(mut s) = session {
        s.overlay.close();
    }
}

fn recompute(session: &mut Session) {
    let SessionPhase::Search {
        query, filtered, ..
    } = &mut session.phase
    else {
        return;
    };
    if query.is_empty() {
        *filtered = (0..session.items.len()).collect();
        return;
    }
    let mut scored: Vec<(i64, usize)> = Vec::new();
    for (idx, item) in session.items.iter().enumerate() {
        let Some(m) = fuzzy::match_query(query, &item.display) else {
            continue;
        };
        scored.push((m.score, idx));
    }
    scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    *filtered = scored
        .into_iter()
        .map(|(_, idx)| idx)
        .take(MAX_FILTERED)
        .collect();
}

fn render(session: &Session) {
    let snapshot = build_snapshot(session);
    session.overlay.set_snapshot(snapshot);
}

fn build_snapshot(session: &Session) -> MenuSnapshot {
    let (prompt_label, query) = match &session.phase {
        SessionPhase::Search { query, .. } => ("menu", query.clone()),
        SessionPhase::Code { typed } => ("code", typed.clone()),
    };

    let rows: Vec<MenuRow> = match &session.phase {
        SessionPhase::Search {
            filtered,
            selected,
            query,
        } => {
            let top = window_top(*selected, filtered.len());
            let end = (top + MAX_VISIBLE_ROWS).min(filtered.len());
            filtered[top..end]
                .iter()
                .enumerate()
                .map(|(vis, &item_idx)| {
                    let item = &session.items[item_idx];
                    let positions = if query.is_empty() {
                        Vec::new()
                    } else {
                        fuzzy::match_query(query, &item.display)
                            .map(|m| m.positions)
                            .unwrap_or_default()
                    };
                    MenuRow {
                        code: session.codes[item_idx].clone(),
                        display: item.display.clone(),
                        matched_positions: positions,
                        selected: vis == (*selected - top),
                        dim: false,
                        disabled: !item.enabled,
                    }
                })
                .collect()
        }
        SessionPhase::Code { typed } => session
            .codes
            .iter()
            .enumerate()
            .filter(|(_, c)| c.starts_with(typed.as_str()))
            .take(MAX_VISIBLE_ROWS)
            .enumerate()
            .map(|(vis, (item_idx, _))| {
                let item = &session.items[item_idx];
                MenuRow {
                    code: session.codes[item_idx].clone(),
                    display: item.display.clone(),
                    matched_positions: Vec::new(),
                    selected: vis == 0,
                    dim: false,
                    disabled: !item.enabled,
                }
            })
            .collect(),
    };

    MenuSnapshot {
        prompt_label: prompt_label.to_string(),
        query,
        cursor_visible: true,
        rows,
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn window_top_keeps_selection_centered() {
        assert_eq!(window_top(20, 100), 13);
        assert_eq!(window_top(1, 100), 0);
        assert_eq!(window_top(99, 100), 86);
    }

    #[test]
    fn window_top_returns_zero_for_short_lists() {
        assert_eq!(window_top(3, 5), 0);
    }

    #[test]
    fn arrow_keys_are_recognised() {
        assert!(is_arrow(0x7D));
        assert!(is_arrow(0x7E));
        assert!(!is_arrow(0x26));
    }

    #[test]
    fn ctrl_jk_navigate() {
        assert_eq!(nav_delta(0x26, true), Some(1));
        assert_eq!(nav_delta(0x28, true), Some(-1));
    }

    #[test]
    fn arrows_navigate_without_ctrl() {
        assert_eq!(nav_delta(0x7D, false), Some(1));
        assert_eq!(nav_delta(0x7E, false), Some(-1));
    }

    #[test]
    fn plain_letters_are_not_navigation_without_ctrl() {
        assert_eq!(nav_delta(0x26, false), None); // plain 'j' with no ctrl
    }
}
