mod click;
pub mod collect;
pub mod copylink;
pub mod geometry;
pub mod labels;
pub mod matcher;
pub mod overlay;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use matcher::MatchResult;
use objc2_foundation::MainThreadMarker;

use crate::types::Rect;

pub struct HintTarget {
    pub frame: Rect,
    pub element: Option<collect::AxElement>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClickKind {
    Left,
    Right,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HintAction {
    Click(ClickKind),
    CopyLink,
}

static HINT_ACTIVE: AtomicBool = AtomicBool::new(false);
static SESSION: Mutex<Option<Session>> = Mutex::new(None);

struct Session {
    labels: Vec<String>,
    targets: Vec<HintTarget>,
    typed: String,
    overlay: overlay::Overlay,
    action: HintAction,
}

unsafe impl Send for Session {}

pub fn is_active() -> bool {
    HINT_ACTIVE.load(Ordering::SeqCst)
}

pub fn toggle(screen: Rect, action: HintAction) {
    let _ = MainThreadMarker::new().expect("hint mode must run on the main thread");
    if is_active() {
        return;
    }
    let targets = match action {
        HintAction::Click(_) => collect::collect_targets(screen),
        HintAction::CopyLink => collect::collect_link_targets(screen),
    };
    if targets.is_empty() {
        return;
    }
    let labels = labels::generate(targets.len());
    let badges = build_badges(&labels, &targets, screen.height);
    let overlay = overlay::Overlay::show(badges);
    *SESSION.lock().unwrap_or_else(|e| e.into_inner()) = Some(Session {
        labels,
        targets,
        typed: String::new(),
        overlay,
        action,
    });
    HINT_ACTIVE.store(true, Ordering::SeqCst);
}

pub fn handle_key(keycode: u32, is_escape: bool, is_backspace: bool) {
    let _ = MainThreadMarker::new().expect("hint mode must run on the main thread");
    if is_escape {
        end_session();
        return;
    }
    let mut guard = SESSION.lock().unwrap_or_else(|e| e.into_inner());
    let Some(session) = guard.as_mut() else {
        return;
    };
    if is_backspace {
        session.typed.pop();
        session.overlay.set_visible_labels(&session.typed);
        return;
    }
    let Some(ch) = crate::hotkey::char_for_keycode(keycode) else {
        return;
    };
    let candidate = format!("{}{ch}", session.typed);
    match matcher::classify(&session.labels, &candidate) {
        MatchResult::NoMatch => {}
        MatchResult::Pending => {
            session.typed = candidate;
            session.overlay.set_visible_labels(&session.typed);
        }
        MatchResult::Hit(i) => match session.action {
            HintAction::Click(click_kind) => {
                let (cx, cy) = geometry::center(session.targets[i].frame);
                drop(guard);
                end_session();
                click::click_at(cx, cy, click_kind);
            }
            HintAction::CopyLink => {
                let copied = session.targets[i]
                    .element
                    .as_ref()
                    .map(copylink::copy_link)
                    .unwrap_or(false);
                drop(guard);
                end_session();
                if copied {
                    crate::toast::show("Link copied");
                } else {
                    log::warn!("hint: matched link had no copyable URL");
                }
            }
        },
    }
}

fn end_session() {
    let _ = MainThreadMarker::new().expect("hint mode must run on the main thread");
    let overlay = SESSION
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .take()
        .map(|s| s.overlay);
    HINT_ACTIVE.store(false, Ordering::SeqCst);
    if let Some(mut o) = overlay {
        o.close();
    }
}

fn build_badges(
    labels: &[String],
    targets: &[HintTarget],
    screen_height: f64,
) -> Vec<overlay::HintBadge> {
    labels
        .iter()
        .zip(targets.iter())
        .map(|(label, target)| overlay::HintBadge {
            label: label.clone(),
            x: target.frame.x,
            y: geometry::flip_y(target.frame.y, target.frame.height, screen_height),
        })
        .collect()
}
