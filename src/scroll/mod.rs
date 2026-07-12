pub mod wheel;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use objc2_foundation::MainThreadMarker;

use crate::hint::collect::{self, AxElement};
use crate::hint::geometry;
use crate::hint::labels;
use crate::hint::matcher::{self, MatchResult};
use crate::hint::overlay::{HintBadge, Overlay};
use crate::hotkey::{CONTROL_KEY, SHIFT_KEY};
use crate::types::Rect;

const PROMPT_LABEL_X: f64 = 24.0;
const PROMPT_LABEL_Y: f64 = 24.0;
const LINE_STEP: i32 = 80;
const HELD_STEP: i32 = 14;
const JUMP_PENDING: i32 = 30000;

static ACTIVE: AtomicBool = AtomicBool::new(false);
static SESSION: Mutex<Option<Session>> = Mutex::new(None);

struct ScrollTarget {
    frame: Rect,
    element: AxElement,
}

enum Phase {
    Pick {
        labels: Vec<String>,
        typed: String,
    },
    Scroll {
        target: usize,
        saved_cursor: (f64, f64),
        pending_g: bool,
    },
}

struct Session {
    targets: Vec<ScrollTarget>,
    phase: Phase,
    overlay: Overlay,
}

unsafe impl Send for Session {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Motion {
    Down,
    Up,
    Left,
    Right,
    HalfDown,
    HalfUp,
    Bottom,
    GPending,
}

pub fn is_active() -> bool {
    ACTIVE.load(Ordering::SeqCst)
}

pub fn toggle(screen: Rect) {
    let _ = MainThreadMarker::new().expect("scroll mode must run on the main thread");
    if is_active() {
        return;
    }
    wheel::start_engine();
    let targets: Vec<ScrollTarget> = collect::collect_scroll_targets(screen)
        .into_iter()
        .filter_map(|t| {
            Some(ScrollTarget {
                frame: t.frame,
                element: t.element?,
            })
        })
        .collect();

    if targets.is_empty() {
        crate::toast::show("No scroll areas");
        return;
    }

    let single = targets.len() == 1;
    let labels = if single {
        Vec::new()
    } else {
        labels::generate(targets.len())
    };
    let badges = if single {
        Vec::new()
    } else {
        build_badges(&labels, &targets, screen.height)
    };
    let overlay = Overlay::show(badges);

    *SESSION.lock().unwrap_or_else(|e| e.into_inner()) = Some(Session {
        targets,
        phase: Phase::Pick {
            labels,
            typed: String::new(),
        },
        overlay,
    });
    ACTIVE.store(true, Ordering::SeqCst);

    if single {
        let mut guard = SESSION.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(session) = guard.as_mut() {
            select(session, 0);
        }
    }
}

pub fn handle_key(keycode: u32, modifiers: u32, is_escape: bool, is_backspace: bool) {
    let _ = MainThreadMarker::new().expect("scroll mode must run on the main thread");
    if is_escape {
        end_session();
        return;
    }
    let mut guard = SESSION.lock().unwrap_or_else(|e| e.into_inner());
    let Some(session) = guard.as_mut() else {
        return;
    };
    match &session.phase {
        Phase::Pick { .. } => handle_pick(session, keycode, is_backspace),
        Phase::Scroll { .. } => handle_scroll(session, keycode, modifiers),
    }
}

fn handle_pick(session: &mut Session, keycode: u32, is_backspace: bool) {
    let hit = {
        let Phase::Pick { labels, typed } = &mut session.phase else {
            return;
        };
        if is_backspace {
            typed.pop();
            session.overlay.set_visible_labels(typed);
            return;
        }
        let Some(ch) = crate::hotkey::char_for_keycode(keycode) else {
            return;
        };
        let candidate = format!("{typed}{ch}");
        match matcher::classify(labels, &candidate) {
            MatchResult::NoMatch => return,
            MatchResult::Pending => {
                *typed = candidate;
                session.overlay.set_visible_labels(typed);
                return;
            }
            MatchResult::Hit(i) => i,
        }
    };
    select(session, hit);
}

pub fn handle_key_up(keycode: u32) {
    let _ = MainThreadMarker::new().expect("scroll mode must run on the main thread");
    let guard = SESSION.lock().unwrap_or_else(|e| e.into_inner());
    let in_scroll = matches!(guard.as_ref().map(|s| &s.phase), Some(Phase::Scroll { .. }));
    drop(guard);
    if !in_scroll {
        return;
    }
    match crate::hotkey::char_for_keycode(keycode) {
        Some('j' | 'k') => wheel::set_velocity_y(0),
        Some('h' | 'l') => wheel::set_velocity_x(0),
        _ => {}
    }
}

fn handle_scroll(session: &mut Session, keycode: u32, modifiers: u32) {
    let ctrl = modifiers & CONTROL_KEY != 0;
    let shift = modifiers & SHIFT_KEY != 0;
    let Some(ch) = crate::hotkey::char_for_keycode(keycode) else {
        clear_pending(session);
        return;
    };
    let Some(motion) = classify(ch, ctrl, shift) else {
        clear_pending(session);
        return;
    };
    apply_motion(session, motion);
}

fn classify(ch: char, ctrl: bool, shift: bool) -> Option<Motion> {
    if ctrl {
        return match ch {
            'd' => Some(Motion::HalfDown),
            'u' => Some(Motion::HalfUp),
            _ => None,
        };
    }
    match ch {
        'j' => Some(Motion::Down),
        'k' => Some(Motion::Up),
        'h' => Some(Motion::Left),
        'l' => Some(Motion::Right),
        'g' if shift => Some(Motion::Bottom),
        'g' => Some(Motion::GPending),
        _ => None,
    }
}

fn apply_motion(session: &mut Session, motion: Motion) {
    let Phase::Scroll {
        target, pending_g, ..
    } = &mut session.phase
    else {
        return;
    };
    let idx = *target;

    if motion == Motion::GPending {
        if *pending_g {
            *pending_g = false;
            jump(&session.targets[idx], false);
        } else {
            *pending_g = true;
        }
        return;
    }
    *pending_g = false;

    let half = (session.targets[idx].frame.height as i32 / 2).max(LINE_STEP);
    match motion {
        Motion::Down => wheel::set_velocity_y(-HELD_STEP),
        Motion::Up => wheel::set_velocity_y(HELD_STEP),
        Motion::Left => wheel::set_velocity_x(-HELD_STEP),
        Motion::Right => wheel::set_velocity_x(HELD_STEP),
        Motion::HalfDown => wheel::add_pending(0, -half),
        Motion::HalfUp => wheel::add_pending(0, half),
        Motion::Bottom => jump(&session.targets[idx], true),
        Motion::GPending => {}
    }
}

fn jump(target: &ScrollTarget, bottom: bool) {
    let fraction = if bottom { 1.0 } else { 0.0 };
    if target.element.set_vertical_fraction(fraction) {
        return;
    }
    let pending = if bottom { -JUMP_PENDING } else { JUMP_PENDING };
    wheel::add_pending(0, pending);
}

fn clear_pending(session: &mut Session) {
    if let Phase::Scroll { pending_g, .. } = &mut session.phase {
        *pending_g = false;
    }
}

fn select(session: &mut Session, index: usize) {
    let (cx, cy) = geometry::center(session.targets[index].frame);
    let saved = wheel::cursor_location();
    wheel::warp_cursor(cx, cy);
    session.overlay.set_badges(vec![prompt_badge()]);
    session.phase = Phase::Scroll {
        target: index,
        saved_cursor: saved,
        pending_g: false,
    };
}

fn build_badges(labels: &[String], targets: &[ScrollTarget], screen_height: f64) -> Vec<HintBadge> {
    labels
        .iter()
        .zip(targets.iter())
        .map(|(label, target)| HintBadge {
            label: label.clone(),
            x: target.frame.x,
            y: geometry::flip_y(target.frame.y, target.frame.height, screen_height),
        })
        .collect()
}

fn prompt_badge() -> HintBadge {
    HintBadge {
        label: "scroll  hjkl  gg G  ^u ^d".to_string(),
        x: PROMPT_LABEL_X,
        y: PROMPT_LABEL_Y,
    }
}

fn end_session() {
    let _ = MainThreadMarker::new().expect("scroll mode must run on the main thread");
    let session = SESSION.lock().unwrap_or_else(|e| e.into_inner()).take();
    ACTIVE.store(false, Ordering::SeqCst);
    wheel::stop();
    if let Some(mut session) = session {
        if let Phase::Scroll { saved_cursor, .. } = session.phase {
            wheel::warp_cursor(saved_cursor.0, saved_cursor.1);
        }
        session.overlay.close();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_keys_map_to_directions() {
        assert_eq!(classify('j', false, false), Some(Motion::Down));
        assert_eq!(classify('k', false, false), Some(Motion::Up));
        assert_eq!(classify('h', false, false), Some(Motion::Left));
        assert_eq!(classify('l', false, false), Some(Motion::Right));
    }

    #[test]
    fn ctrl_d_and_u_are_half_pages() {
        assert_eq!(classify('d', true, false), Some(Motion::HalfDown));
        assert_eq!(classify('u', true, false), Some(Motion::HalfUp));
    }

    #[test]
    fn plain_d_and_u_do_nothing() {
        assert_eq!(classify('d', false, false), None);
        assert_eq!(classify('u', false, false), None);
    }

    #[test]
    fn g_is_pending_and_shift_g_is_bottom() {
        assert_eq!(classify('g', false, false), Some(Motion::GPending));
        assert_eq!(classify('g', false, true), Some(Motion::Bottom));
    }

    #[test]
    fn ctrl_overrides_plain_direction_keys() {
        assert_eq!(classify('j', true, false), None);
        assert_eq!(classify('h', true, false), None);
    }
}
