use std::ffi::c_void;
use std::sync::atomic::{AtomicI32, AtomicPtr, Ordering};
use std::sync::OnceLock;
use std::thread;
use std::time::Duration;

use core_graphics::geometry::CGPoint;

type CGEventRef = *mut c_void;
type CGEventSourceRef = *mut c_void;

const K_CG_SCROLL_EVENT_UNIT_PIXEL: u32 = 0;
const K_CG_HID_EVENT_TAP: u32 = 0;
const K_CG_EVENT_SOURCE_STATE_HID: u32 = 1;

extern "C" {
    fn CGEventSourceCreate(state_id: u32) -> CGEventSourceRef;
    fn CGEventSourceSetLocalEventsSuppressionInterval(source: CGEventSourceRef, seconds: f64);
    fn CGEventCreateScrollWheelEvent(
        source: CGEventSourceRef,
        units: u32,
        wheel_count: u32,
        wheel1: i32,
        ...
    ) -> CGEventRef;
    fn CGEventCreate(source: CGEventSourceRef) -> CGEventRef;
    fn CGEventGetLocation(event: CGEventRef) -> CGPoint;
    fn CGEventPost(tap: u32, event: CGEventRef);
    fn CGWarpMouseCursorPosition(new_position: CGPoint) -> i32;
    fn CFRelease(cf: *const c_void);
}

static SOURCE: AtomicPtr<c_void> = AtomicPtr::new(std::ptr::null_mut());

unsafe fn event_source() -> CGEventSourceRef {
    let existing = SOURCE.load(Ordering::SeqCst);
    if !existing.is_null() {
        return existing;
    }
    let source = CGEventSourceCreate(K_CG_EVENT_SOURCE_STATE_HID);
    if !source.is_null() {
        CGEventSourceSetLocalEventsSuppressionInterval(source, 0.0);
    }
    SOURCE.store(source, Ordering::SeqCst);
    source
}

pub fn scroll_by(dx: i32, dy: i32) {
    unsafe {
        let source = event_source();
        let event = CGEventCreateScrollWheelEvent(source, K_CG_SCROLL_EVENT_UNIT_PIXEL, 2, dy, dx);
        if event.is_null() {
            return;
        }
        CGEventPost(K_CG_HID_EVENT_TAP, event);
        CFRelease(event);
    }
}

const FRAME: Duration = Duration::from_millis(12);
const MAX_PENDING_STEP: i32 = 110;

static VEL_X: AtomicI32 = AtomicI32::new(0);
static VEL_Y: AtomicI32 = AtomicI32::new(0);
static PENDING_X: AtomicI32 = AtomicI32::new(0);
static PENDING_Y: AtomicI32 = AtomicI32::new(0);
static ENGINE: OnceLock<()> = OnceLock::new();

pub fn start_engine() {
    ENGINE.get_or_init(|| {
        thread::spawn(|| loop {
            let dx = VEL_X.load(Ordering::SeqCst) + drain(&PENDING_X);
            let dy = VEL_Y.load(Ordering::SeqCst) + drain(&PENDING_Y);
            if dx != 0 || dy != 0 {
                scroll_by(dx, dy);
            }
            thread::sleep(FRAME);
        });
    });
}

fn drain(pending: &AtomicI32) -> i32 {
    let remaining = pending.load(Ordering::SeqCst);
    if remaining == 0 {
        return 0;
    }
    let mut step = remaining / 3;
    if step == 0 {
        step = remaining;
    }
    step = step.clamp(-MAX_PENDING_STEP, MAX_PENDING_STEP);
    pending.fetch_sub(step, Ordering::SeqCst);
    step
}

pub fn set_velocity(dx: i32, dy: i32) {
    VEL_X.store(dx, Ordering::SeqCst);
    VEL_Y.store(dy, Ordering::SeqCst);
}

pub fn set_velocity_x(dx: i32) {
    VEL_X.store(dx, Ordering::SeqCst);
}

pub fn set_velocity_y(dy: i32) {
    VEL_Y.store(dy, Ordering::SeqCst);
}

pub fn add_pending(dx: i32, dy: i32) {
    PENDING_X.fetch_add(dx, Ordering::SeqCst);
    PENDING_Y.fetch_add(dy, Ordering::SeqCst);
}

pub fn stop() {
    VEL_X.store(0, Ordering::SeqCst);
    VEL_Y.store(0, Ordering::SeqCst);
    PENDING_X.store(0, Ordering::SeqCst);
    PENDING_Y.store(0, Ordering::SeqCst);
}

pub fn warp_cursor(x: f64, y: f64) {
    unsafe {
        CGWarpMouseCursorPosition(CGPoint::new(x, y));
    }
}

pub fn cursor_location() -> (f64, f64) {
    unsafe {
        let event = CGEventCreate(std::ptr::null_mut());
        if event.is_null() {
            return (0.0, 0.0);
        }
        let point = CGEventGetLocation(event);
        CFRelease(event);
        (point.x, point.y)
    }
}
