use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use objc2_foundation::MainThreadMarker;

use crate::hint::overlay::{HintBadge, Overlay};

type DispatchTime = u64;

extern "C" {
    static _dispatch_main_q: std::ffi::c_void;
    fn dispatch_time(when: DispatchTime, delta: i64) -> DispatchTime;
    fn dispatch_after_f(
        when: DispatchTime,
        queue: *const std::ffi::c_void,
        context: *mut std::ffi::c_void,
        work: extern "C" fn(*mut std::ffi::c_void),
    );
}

const DISPATCH_TIME_NOW: DispatchTime = 0;
const STEP_NANOS: i64 = 16_000_000;
const FONT_SIZE: f64 = 16.0;
const CHAR_WIDTH: f64 = 9.0;
const BOTTOM_MARGIN: f64 = 96.0;
const RISE: f64 = 16.0;

const FADE_IN_STEPS: u32 = 11;
const HOLD_STEPS: u32 = 62;
const FADE_OUT_STEPS: u32 = 12;
const TOTAL_STEPS: u32 = FADE_IN_STEPS + HOLD_STEPS + FADE_OUT_STEPS;

struct ToastState {
    overlay: Overlay,
    base_x: f64,
    base_y: f64,
    step: u32,
}

unsafe impl Send for ToastState {}

static TOAST: Mutex<Option<ToastState>> = Mutex::new(None);
static GENERATION: AtomicUsize = AtomicUsize::new(0);

pub fn show(text: &str) {
    let _ = MainThreadMarker::new().expect("toast must run on the main thread");
    let screen = crate::screen::get_full_screen_rect();
    let estimated_width = (text.chars().count() as f64) * CHAR_WIDTH + 28.0;
    let badge = HintBadge {
        label: text.to_string(),
        x: screen.x + screen.width / 2.0 - estimated_width / 2.0,
        y: BOTTOM_MARGIN,
    };
    let overlay = Overlay::show_toast(badge, FONT_SIZE);
    let (base_x, base_y) = overlay.origin();
    overlay.set_frame_origin(base_x, base_y - RISE);

    let mut guard = TOAST.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(mut old) = guard.take() {
        old.overlay.close();
    }
    *guard = Some(ToastState {
        overlay,
        base_x,
        base_y,
        step: 0,
    });
    drop(guard);

    let generation = GENERATION.fetch_add(1, Ordering::SeqCst).wrapping_add(1);
    schedule(generation);
}

fn schedule(generation: usize) {
    unsafe {
        let when = dispatch_time(DISPATCH_TIME_NOW, STEP_NANOS);
        dispatch_after_f(
            when,
            &_dispatch_main_q as *const _,
            generation as *mut std::ffi::c_void,
            tick,
        );
    }
}

fn ease_out(p: f64) -> f64 {
    1.0 - (1.0 - p) * (1.0 - p)
}

extern "C" fn tick(context: *mut std::ffi::c_void) {
    let generation = context as usize;
    if GENERATION.load(Ordering::SeqCst) != generation {
        return;
    }
    let mut guard = TOAST.lock().unwrap_or_else(|e| e.into_inner());
    let Some(state) = guard.as_mut() else {
        return;
    };

    let step = state.step;
    if step >= TOTAL_STEPS {
        if let Some(mut done) = guard.take() {
            done.overlay.close();
        }
        return;
    }

    let (alpha, offset) = if step < FADE_IN_STEPS {
        let p = ease_out((step + 1) as f64 / FADE_IN_STEPS as f64);
        (p, RISE * (1.0 - p))
    } else if step < FADE_IN_STEPS + HOLD_STEPS {
        (1.0, 0.0)
    } else {
        let p = ease_out((step - FADE_IN_STEPS - HOLD_STEPS + 1) as f64 / FADE_OUT_STEPS as f64);
        (1.0 - p, 0.0)
    };

    state.overlay.set_alpha(alpha);
    state
        .overlay
        .set_frame_origin(state.base_x, state.base_y - offset);
    state.step += 1;
    drop(guard);

    schedule(generation);
}
