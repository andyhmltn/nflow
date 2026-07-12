pub mod motions;
pub mod search;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use objc2_app_kit::{NSPasteboard, NSPasteboardTypeString};
use objc2_foundation::{MainThreadMarker, NSString};

use crate::hint::collect::{self, AxElement};
use crate::hint::geometry;
use crate::hint::labels;
use crate::hint::matcher::{self, MatchResult};
use crate::hint::overlay::{HighlightRect, HintBadge, Overlay};
use crate::types::Rect;
use search::TextMatch;

const PROMPT_LABEL_X: f64 = 24.0;
const PROMPT_LABEL_Y: f64 = 24.0;
const MAX_LIVE_HIGHLIGHTS: usize = 80;
const LINE_PROBE_RADIUS: usize = 600;

static ACTIVE: AtomicBool = AtomicBool::new(false);
static SESSION: Mutex<Option<Session>> = Mutex::new(None);

struct TextTarget {
    element: AxElement,
    chars: Vec<char>,
}

#[derive(Clone, Copy)]
enum Pending {
    None,
    Find,
    Till,
}

enum Phase {
    Search,
    Pick {
        matches: Vec<TextMatch>,
        labels: Vec<String>,
        typed: String,
    },
    Visual {
        target: usize,
        anchor: usize,
        head: usize,
        pending: Pending,
        last_find: Option<(bool, char)>,
    },
}

struct Session {
    targets: Vec<TextTarget>,
    query: String,
    phase: Phase,
    overlay: Overlay,
    screen_height: f64,
}

unsafe impl Send for Session {}

pub fn is_active() -> bool {
    ACTIVE.load(Ordering::SeqCst)
}

pub fn toggle(screen: Rect) {
    let _ = MainThreadMarker::new().expect("text select must run on the main thread");
    if is_active() {
        return;
    }
    let targets: Vec<TextTarget> = collect::collect_text_targets(screen)
        .into_iter()
        .filter_map(|t| {
            let element = t.element?;
            let value = element.value()?;
            let chars: Vec<char> = value.chars().collect();
            if chars.is_empty() {
                None
            } else {
                Some(TextTarget { element, chars })
            }
        })
        .collect();

    if targets.is_empty() {
        return;
    }

    let overlay = Overlay::show(vec![prompt_badge("")]);
    *SESSION.lock().unwrap_or_else(|e| e.into_inner()) = Some(Session {
        targets,
        query: String::new(),
        phase: Phase::Search,
        overlay,
        screen_height: screen.height,
    });
    ACTIVE.store(true, Ordering::SeqCst);
}

pub fn handle_key(
    keycode: u32,
    typed: Option<char>,
    is_escape: bool,
    is_backspace: bool,
    is_return: bool,
) {
    let _ = MainThreadMarker::new().expect("text select must run on the main thread");
    if is_escape {
        end_session();
        return;
    }
    let mut guard = SESSION.lock().unwrap_or_else(|e| e.into_inner());
    let Some(session) = guard.as_mut() else {
        return;
    };

    match &mut session.phase {
        Phase::Search => handle_search(session, typed, is_backspace, is_return),
        Phase::Pick { .. } => handle_pick(session, keycode, typed, is_backspace),
        Phase::Visual { .. } => {
            if let Some(text) = handle_visual(session, keycode, typed) {
                drop(guard);
                write_plain(&text);
                end_session();
                crate::toast::show("Copied");
            }
        }
    }
}

fn handle_search(session: &mut Session, typed: Option<char>, is_backspace: bool, is_return: bool) {
    if is_return {
        enter_pick(session);
        return;
    }
    if is_backspace {
        session.query.pop();
    } else if let Some(ch) = typed {
        session.query.push(ch);
    } else {
        return;
    }
    session
        .overlay
        .set_badges(vec![prompt_badge(&session.query)]);
    let highlights = live_highlights(session);
    session.overlay.set_highlights(highlights);
}

fn live_highlights(session: &Session) -> Vec<HighlightRect> {
    if session.query.is_empty() {
        return Vec::new();
    }
    let text_chars: Vec<Vec<char>> = session.targets.iter().map(|t| t.chars.clone()).collect();
    let matches = search::find_matches(&text_chars, &session.query);
    let mut rects = Vec::new();
    for m in matches.iter().take(MAX_LIVE_HIGHLIGHTS) {
        let target = &session.targets[m.target];
        rects.extend(selection_rects(
            target,
            m.start,
            m.start + m.len - 1,
            session.screen_height,
        ));
    }
    rects
}

fn enter_pick(session: &mut Session) {
    let text_chars: Vec<Vec<char>> = session.targets.iter().map(|t| t.chars.clone()).collect();
    let matches = search::find_matches(&text_chars, &session.query);
    if matches.is_empty() {
        return;
    }
    let labels = labels::generate(matches.len());
    let badges = match_badges(session, &matches, &labels);
    session.overlay.set_badges(badges);
    session.phase = Phase::Pick {
        matches,
        labels,
        typed: String::new(),
    };
}

fn handle_pick(session: &mut Session, keycode: u32, typed: Option<char>, is_backspace: bool) {
    let Phase::Pick {
        matches,
        labels,
        typed: typed_label,
    } = &mut session.phase
    else {
        return;
    };

    if is_backspace {
        typed_label.pop();
        session.overlay.set_visible_labels(typed_label);
        return;
    }
    let Some(ch) = typed
        .map(|c| c.to_ascii_lowercase())
        .or_else(|| crate::hotkey::char_for_keycode(keycode))
    else {
        return;
    };
    let candidate = format!("{typed_label}{ch}");
    match matcher::classify(labels, &candidate) {
        MatchResult::NoMatch => {}
        MatchResult::Pending => {
            *typed_label = candidate;
            session.overlay.set_visible_labels(typed_label);
        }
        MatchResult::Hit(i) => {
            let hit = matches[i];
            let anchor = hit.start;
            let head = hit.start + hit.len - 1;
            session.phase = Phase::Visual {
                target: hit.target,
                anchor,
                head,
                pending: Pending::None,
                last_find: None,
            };
            session.overlay.set_badges(Vec::new());
            apply_selection(&session.targets[hit.target], anchor, head);
            update_highlight(
                &session.overlay,
                &session.targets[hit.target],
                anchor,
                head,
                session.screen_height,
            );
        }
    }
}

fn handle_visual(session: &mut Session, keycode: u32, typed: Option<char>) -> Option<String> {
    let Phase::Visual {
        target,
        anchor,
        head,
        pending,
        last_find,
    } = &mut session.phase
    else {
        return None;
    };
    let chars = &session.targets[*target].chars;

    if let Pending::Find | Pending::Till = pending {
        let want_till = matches!(pending, Pending::Till);
        *pending = Pending::None;
        if let Some(ch) = typed.or_else(|| key_char(keycode)) {
            *last_find = Some((want_till, ch));
            *head = apply_find(chars, *head, want_till, ch, false);
        }
        sync_selection(session);
        return None;
    }

    let ch = typed.or_else(|| key_char(keycode))?;
    match ch.to_ascii_lowercase() {
        'y' => {
            let lo = (*anchor).min(*head);
            let hi = (*anchor).max(*head);
            return Some(chars[lo..=hi].iter().collect());
        }
        'h' => *head = motions::char_left(*head),
        'l' => *head = motions::char_right(chars, *head),
        'w' => *head = motions::next_word_start(chars, *head),
        'e' => *head = motions::word_end(chars, *head),
        'b' => *head = motions::prev_word_start(chars, *head),
        '0' => *head = motions::line_start(chars, *head),
        '$' => *head = motions::line_end(chars, *head),
        'j' => *head = visual_line(&session.targets[*target], *head, true),
        'k' => *head = visual_line(&session.targets[*target], *head, false),
        ';' => {
            let (till, target_ch) = (*last_find)?;
            *head = apply_find(chars, *head, till, target_ch, true);
        }
        'f' => {
            *pending = Pending::Find;
            return None;
        }
        't' => {
            *pending = Pending::Till;
            return None;
        }
        _ => return None,
    }
    sync_selection(session);
    None
}

fn apply_find(chars: &[char], head: usize, till: bool, ch: char, repeat: bool) -> usize {
    if !till {
        return motions::find_forward(chars, head, ch);
    }
    if repeat {
        let next = motions::find_forward(chars, head + 1, ch);
        if next > head + 1 {
            next - 1
        } else {
            head
        }
    } else {
        motions::till_forward(chars, head, ch)
    }
}

fn sync_selection(session: &mut Session) {
    let Phase::Visual {
        target,
        anchor,
        head,
        ..
    } = &session.phase
    else {
        return;
    };
    apply_selection(&session.targets[*target], *anchor, *head);
    update_highlight(
        &session.overlay,
        &session.targets[*target],
        *anchor,
        *head,
        session.screen_height,
    );
}

fn apply_selection(target: &TextTarget, anchor: usize, head: usize) {
    let (location, length) = motions::selection_range(&target.chars, anchor, head);
    target.element.set_selected_range(location, length);
}

fn char_rect(target: &TextTarget, index: usize) -> Option<motions::CharRect> {
    let chars = &target.chars;
    if index >= chars.len() {
        return None;
    }
    let location = motions::char_index_to_utf16(chars, index);
    let length = motions::char_index_to_utf16(chars, index + 1) - location;
    let rect = target.element.bounds_for_range(location, length)?;
    Some(motions::CharRect {
        index,
        x: rect.x,
        y: rect.y,
        h: rect.height,
    })
}

fn visual_line(target: &TextTarget, head: usize, down: bool) -> usize {
    let Some(head_rect) = char_rect(target, head) else {
        return if down {
            motions::line_down(&target.chars, head)
        } else {
            motions::line_up(&target.chars, head)
        };
    };
    let n = target.chars.len();
    let lo = head.saturating_sub(LINE_PROBE_RADIUS);
    let hi = (head + LINE_PROBE_RADIUS + 1).min(n);
    let rects: Vec<motions::CharRect> = (lo..hi).filter_map(|i| char_rect(target, i)).collect();
    motions::nearest_in_band(&rects, &head_rect, down).unwrap_or(head)
}

fn update_highlight(
    overlay: &Overlay,
    target: &TextTarget,
    anchor: usize,
    head: usize,
    screen_height: f64,
) {
    overlay.set_highlights(selection_rects(target, anchor, head, screen_height));
}

fn selection_rects(
    target: &TextTarget,
    anchor: usize,
    head: usize,
    screen_height: f64,
) -> Vec<HighlightRect> {
    let chars = &target.chars;
    let lo = anchor.min(head);
    let hi = anchor.max(head);
    let mut rects = Vec::new();
    let mut seg_start = lo;
    let mut push = |start: usize, end: usize| {
        if end <= start {
            return;
        }
        let location = motions::char_index_to_utf16(chars, start);
        let length = motions::char_index_to_utf16(chars, end) - location;
        if let Some(r) = target.element.bounds_for_range(location, length) {
            rects.push(HighlightRect {
                x: r.x,
                y: geometry::flip_y(r.y, r.height, screen_height),
                width: r.width,
                height: r.height,
            });
        }
    };
    for (i, ch) in chars.iter().enumerate().take(hi + 1).skip(lo) {
        if *ch == '\n' {
            push(seg_start, i);
            seg_start = i + 1;
        }
    }
    push(seg_start, hi + 1);
    rects
}

fn match_badges(session: &Session, matches: &[TextMatch], labels: &[String]) -> Vec<HintBadge> {
    matches
        .iter()
        .zip(labels.iter())
        .map(|(m, label)| {
            let target = &session.targets[m.target];
            let location = motions::char_index_to_utf16(&target.chars, m.start);
            let length = motions::char_index_to_utf16(&target.chars, m.start + m.len) - location;
            let rect = target.element.bounds_for_range(location, length);
            let (x, y) = match rect {
                Some(r) => (r.x, geometry::flip_y(r.y, r.height, session.screen_height)),
                None => (PROMPT_LABEL_X, PROMPT_LABEL_Y),
            };
            HintBadge {
                label: label.clone(),
                x,
                y,
            }
        })
        .collect()
}

fn prompt_badge(query: &str) -> HintBadge {
    HintBadge {
        label: format!("/{query}"),
        x: PROMPT_LABEL_X,
        y: PROMPT_LABEL_Y,
    }
}

fn write_plain(text: &str) {
    unsafe {
        let pb = NSPasteboard::generalPasteboard();
        pb.clearContents();
        pb.setString_forType(&NSString::from_str(text), NSPasteboardTypeString);
    }
}

fn end_session() {
    let _ = MainThreadMarker::new().expect("text select must run on the main thread");
    let overlay = SESSION
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .take()
        .map(|s| s.overlay);
    ACTIVE.store(false, Ordering::SeqCst);
    if let Some(mut o) = overlay {
        o.close();
    }
}

fn key_char(keycode: u32) -> Option<char> {
    if let Some(ch) = crate::hotkey::char_for_keycode(keycode) {
        return Some(ch);
    }
    let ch = match keycode {
        0x31 => ' ',
        0x1D => '0',
        0x12 => '1',
        0x13 => '2',
        0x14 => '3',
        0x15 => '4',
        0x17 => '5',
        0x16 => '6',
        0x1A => '7',
        0x1C => '8',
        0x19 => '9',
        0x1B => '-',
        0x2F => '.',
        0x2C => '/',
        0x29 => ';',
        0x27 => '\'',
        0x2B => ',',
        0x21 => '[',
        0x1E => ']',
        0x2A => '\\',
        0x32 => '`',
        0x18 => '=',
        _ => return None,
    };
    Some(ch)
}

#[cfg(test)]
mod tests {
    use super::apply_find;

    fn cv(s: &str) -> Vec<char> {
        s.chars().collect()
    }

    #[test]
    fn find_lands_on_target() {
        let c = cv("hello world");
        assert_eq!(apply_find(&c, 0, false, 'o', false), 4);
    }

    #[test]
    fn find_repeat_advances_to_next() {
        let c = cv("hello world");
        assert_eq!(apply_find(&c, 4, false, 'o', true), 7);
    }

    #[test]
    fn till_lands_before_target() {
        let c = cv("a b c");
        assert_eq!(apply_find(&c, 0, true, ' ', false), 0);
    }

    #[test]
    fn till_repeat_skips_blocking_char_to_next() {
        let c = cv("a b c");
        assert_eq!(apply_find(&c, 0, true, ' ', true), 2);
    }

    #[test]
    fn till_repeat_with_no_further_target_stays_put() {
        let c = cv("hello world");
        assert_eq!(apply_find(&c, 4, true, ' ', true), 4);
    }
}
