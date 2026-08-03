mod click;
pub mod collect;
pub mod copylink;
pub mod geometry;
pub mod labels;
pub mod matcher;
pub mod overlay;

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use matcher::MatchResult;
use objc2_foundation::MainThreadMarker;

use crate::types::Rect;

extern "C" {
    static _dispatch_main_q: std::ffi::c_void;
    fn dispatch_async_f(
        queue: *const std::ffi::c_void,
        context: *mut std::ffi::c_void,
        work: extern "C" fn(*mut std::ffi::c_void),
    );
}

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
static GENERATION: AtomicUsize = AtomicUsize::new(0);

struct Session {
    labels: Vec<String>,
    targets: Vec<HintTarget>,
    typed: String,
    overlay: overlay::Overlay,
    action: HintAction,
    allocator: labels::LabelAllocator,
    screen_height: f64,
    generation: usize,
    cancelled: Arc<AtomicBool>,
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
    let generation = GENERATION.fetch_add(1, Ordering::SeqCst).wrapping_add(1);
    let cancelled = Arc::new(AtomicBool::new(false));
    let overlay = overlay::Overlay::show(Vec::new());
    *SESSION.lock().unwrap_or_else(|e| e.into_inner()) = Some(Session {
        labels: Vec::new(),
        targets: Vec::new(),
        typed: String::new(),
        overlay,
        action,
        allocator: labels::LabelAllocator::new(),
        screen_height: screen.height,
        generation,
        cancelled: cancelled.clone(),
    });
    HINT_ACTIVE.store(true, Ordering::SeqCst);
    let link_only = matches!(action, HintAction::CopyLink);
    collect::stream_targets(
        screen,
        link_only,
        cancelled,
        Box::new(move |batch, done| dispatch_batch(generation, batch, done)),
    );
}

struct BatchDelivery {
    generation: usize,
    batch: Vec<HintTarget>,
    done: bool,
}

fn dispatch_batch(generation: usize, batch: Vec<HintTarget>, done: bool) {
    let context = Box::into_raw(Box::new(BatchDelivery {
        generation,
        batch,
        done,
    }));
    unsafe {
        dispatch_async_f(
            &_dispatch_main_q as *const _,
            context as *mut std::ffi::c_void,
            deliver_batch_main,
        );
    }
}

extern "C" fn deliver_batch_main(context: *mut std::ffi::c_void) {
    let delivery = unsafe { Box::from_raw(context as *mut BatchDelivery) };
    deliver_batch(*delivery);
}

fn deliver_batch(delivery: BatchDelivery) {
    let _ = MainThreadMarker::new().expect("hint batches must be delivered on the main thread");
    let mut guard = SESSION.lock().unwrap_or_else(|e| e.into_inner());
    let Some(session) = guard.as_mut() else {
        return;
    };
    if session.generation != delivery.generation {
        return;
    }
    if !delivery.batch.is_empty() {
        let mut badges = Vec::with_capacity(delivery.batch.len());
        for target in delivery.batch {
            let label = session.allocator.allocate();
            badges.push(overlay::HintBadge {
                label: label.clone(),
                x: target.frame.x,
                y: geometry::flip_y(target.frame.y, target.frame.height, session.screen_height),
            });
            session.labels.push(label);
            session.targets.push(target);
        }
        session.overlay.append_badges(badges);
    }
    if delivery.done && session.targets.is_empty() {
        drop(guard);
        end_session();
    }
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
    let session = SESSION.lock().unwrap_or_else(|e| e.into_inner()).take();
    HINT_ACTIVE.store(false, Ordering::SeqCst);
    if let Some(mut s) = session {
        s.cancelled.store(true, Ordering::SeqCst);
        s.overlay.close();
    }
}
