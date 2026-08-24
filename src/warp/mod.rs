pub mod grid;
pub mod motion;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use objc2_foundation::MainThreadMarker;

use crate::hint::geometry::flip_y;
use crate::hint::overlay::{HintBadge, Overlay};
use crate::types::Rect;

const PROMPT_X: f64 = 24.0;
const PROMPT_Y: f64 = 24.0;
const SPACE_KEYCODE: u32 = 0x31;
const MAX_PICK_LEVELS: usize = 3;

static ACTIVE: AtomicBool = AtomicBool::new(false);
static SESSION: Mutex<Option<Session>> = Mutex::new(None);

enum Phase {
    Pick,
    Drag { start: (f64, f64) },
}

struct Session {
    screen: Rect,
    stack: Vec<Rect>,
    labels: Vec<String>,
    cells: Vec<Rect>,
    overlay: Overlay,
    phase: Phase,
}

unsafe impl Send for Session {}

pub fn is_active() -> bool {
    ACTIVE.load(Ordering::SeqCst)
}

pub fn toggle(screen: Rect) {
    let _ = MainThreadMarker::new().expect("warp mode must run on the main thread");
    if is_active() {
        return;
    }
    let bounds = screen;
    let cells = grid::subdivide(bounds);
    let labels = grid::labels();
    let overlay = Overlay::show(build_badges(&labels, &cells, screen.height));
    let mut session = Session {
        screen,
        stack: vec![bounds],
        labels,
        cells,
        overlay,
        phase: Phase::Pick,
    };
    render(&mut session);
    *SESSION.lock().unwrap_or_else(|e| e.into_inner()) = Some(session);
    ACTIVE.store(true, Ordering::SeqCst);
}

pub fn handle_key(keycode: u32, is_escape: bool, is_backspace: bool, is_return: bool) {
    let _ = MainThreadMarker::new().expect("warp mode must run on the main thread");
    if is_escape {
        cancel();
        return;
    }
    if is_return {
        commit();
        return;
    }
    if keycode == SPACE_KEYCODE {
        toggle_drag();
        return;
    }
    if is_backspace {
        pop_level();
        return;
    }
    let Some(ch) = crate::hotkey::char_for_keycode(keycode) else {
        return;
    };
    let candidate = ch.to_string();
    let hit = {
        let guard = SESSION.lock().unwrap_or_else(|e| e.into_inner());
        let Some(session) = guard.as_ref() else {
            return;
        };
        session.labels.iter().position(|l| *l == candidate)
    };
    if let Some(index) = hit {
        descend(index);
    }
}

fn descend(index: usize) {
    let mut guard = SESSION.lock().unwrap_or_else(|e| e.into_inner());
    let Some(session) = guard.as_mut() else {
        return;
    };
    let picked = session.cells[index];
    if should_finish_after_pick(session.stack.len(), picked) {
        let (cx, cy) = grid::centre(picked);
        drop(guard);
        finish_at(cx, cy);
        return;
    }
    session.stack.push(picked);
    session.cells = grid::subdivide(picked);
    render(session);
}

fn should_finish_after_pick(current_level: usize, picked: Rect) -> bool {
    current_level >= MAX_PICK_LEVELS || grid::is_terminal(picked)
}

fn pop_level() {
    let mut guard = SESSION.lock().unwrap_or_else(|e| e.into_inner());
    let Some(session) = guard.as_mut() else {
        return;
    };
    if session.stack.len() <= 1 {
        return;
    }
    session.stack.pop();
    let bounds = *session
        .stack
        .last()
        .expect("stack always has at least the root bounds");
    session.cells = grid::subdivide(bounds);
    render(session);
}

fn commit() {
    let mut guard = SESSION.lock().unwrap_or_else(|e| e.into_inner());
    let Some(session) = guard.as_mut() else {
        return;
    };
    let bounds = *session
        .stack
        .last()
        .expect("stack always has at least the root bounds");
    let (cx, cy) = grid::centre(bounds);
    drop(guard);
    finish_at(cx, cy);
}

fn finish_at(cx: f64, cy: f64) {
    let phase = {
        let guard = SESSION.lock().unwrap_or_else(|e| e.into_inner());
        guard.as_ref().map(|s| match s.phase {
            Phase::Pick => Phase::Pick,
            Phase::Drag { start } => Phase::Drag { start },
        })
    };
    end_session();
    match phase {
        Some(Phase::Pick) => motion::click(cx, cy),
        Some(Phase::Drag { start }) => motion::drag_to(start, (cx, cy)),
        None => {}
    }
}

fn toggle_drag() {
    let mut guard = SESSION.lock().unwrap_or_else(|e| e.into_inner());
    let Some(session) = guard.as_mut() else {
        return;
    };
    if matches!(session.phase, Phase::Drag { .. }) {
        return;
    }
    let bounds = *session
        .stack
        .last()
        .expect("stack always has at least the root bounds");
    let (cx, cy) = grid::centre(bounds);
    motion::mouse_down(cx, cy);
    session.phase = Phase::Drag { start: (cx, cy) };
    let full = session.screen;
    session.stack = vec![full];
    session.cells = grid::subdivide(full);
    render(session);
}

fn cancel() {
    let phase = {
        let guard = SESSION.lock().unwrap_or_else(|e| e.into_inner());
        guard.as_ref().map(|s| match s.phase {
            Phase::Pick => Phase::Pick,
            Phase::Drag { start } => Phase::Drag { start },
        })
    };
    end_session();
    if let Some(Phase::Drag { start }) = phase {
        motion::mouse_up(start.0, start.1);
    }
}

fn end_session() {
    let _ = MainThreadMarker::new().expect("warp mode must run on the main thread");
    let session = SESSION.lock().unwrap_or_else(|e| e.into_inner()).take();
    ACTIVE.store(false, Ordering::SeqCst);
    if let Some(mut s) = session {
        s.overlay.close();
    }
}

fn render(session: &mut Session) {
    let mut badges = build_badges(&session.labels, &session.cells, session.screen.height);
    badges.push(prompt_badge(&session.phase));
    session.overlay.set_badges(badges);
}

fn build_badges(labels: &[String], cells: &[Rect], screen_height: f64) -> Vec<HintBadge> {
    labels
        .iter()
        .zip(cells.iter())
        .map(|(label, cell)| {
            let (cx, cy) = grid::centre(*cell);
            HintBadge {
                label: label.clone(),
                x: cx,
                y: flip_y(cy, 0.0, screen_height),
            }
        })
        .collect()
}

fn prompt_badge(phase: &Phase) -> HintBadge {
    let text = match phase {
        Phase::Pick => "warp  letter to pick  space to drag  enter to click",
        Phase::Drag { .. } => "drag  letter to pick drop  enter to release",
    };
    HintBadge {
        label: text.to_string(),
        x: PROMPT_X,
        y: PROMPT_Y,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(x: f64, y: f64, w: f64, h: f64) -> Rect {
        Rect {
            x,
            y,
            width: w,
            height: h,
        }
    }

    #[test]
    fn build_badges_places_labels_at_cell_centres() {
        let cells = grid::subdivide(r(0.0, 0.0, 400.0, 400.0));
        let labels = grid::labels();
        let screen_height = 400.0;
        let badges = build_badges(&labels, &cells, screen_height);
        assert_eq!(badges.len(), grid::CELL_COUNT);
        assert_eq!(badges[0].label, "a");
        assert_eq!(badges[0].x, 50.0);
        assert_eq!(badges[0].y, screen_height - 50.0);
    }

    #[test]
    fn third_picker_selection_is_final() {
        let picked = r(0.0, 0.0, 30.0, 30.0);

        assert!(!should_finish_after_pick(1, picked));
        assert!(!should_finish_after_pick(2, picked));
        assert!(should_finish_after_pick(3, picked));
    }
}
