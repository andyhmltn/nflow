use std::thread;
use std::time::Duration;

use core_graphics::geometry::CGPoint;

type CGEventRef = *mut std::ffi::c_void;
type CGEventSourceRef = *mut std::ffi::c_void;

const K_CG_EVENT_LEFT_MOUSE_DOWN: u32 = 1;
const K_CG_EVENT_LEFT_MOUSE_UP: u32 = 2;
const K_CG_EVENT_LEFT_MOUSE_DRAGGED: u32 = 6;
const K_CG_MOUSE_BUTTON_LEFT: u32 = 0;
const K_CG_HID_EVENT_TAP: u32 = 0;

const DRAG_STEP_PX: f64 = 12.0;
const DRAG_STEP_INTERVAL: Duration = Duration::from_millis(4);

extern "C" {
    fn CGEventCreateMouseEvent(
        source: CGEventSourceRef,
        mouse_type: u32,
        mouse_cursor_position: CGPoint,
        mouse_button: u32,
    ) -> CGEventRef;
    fn CGEventPost(tap: u32, event: CGEventRef);
    fn CGWarpMouseCursorPosition(new_position: CGPoint) -> i32;
    fn CFRelease(cf: *const std::ffi::c_void);
}

fn post(event: CGEventRef) {
    unsafe {
        if !event.is_null() {
            CGEventPost(K_CG_HID_EVENT_TAP, event);
            CFRelease(event);
        }
    }
}

fn post_mouse(kind: u32, x: f64, y: f64) {
    let point = CGPoint::new(x, y);
    unsafe {
        let event =
            CGEventCreateMouseEvent(std::ptr::null_mut(), kind, point, K_CG_MOUSE_BUTTON_LEFT);
        post(event);
    }
}

pub fn warp(x: f64, y: f64) {
    unsafe {
        CGWarpMouseCursorPosition(CGPoint::new(x, y));
    }
}

pub fn click(x: f64, y: f64) {
    warp(x, y);
    post_mouse(K_CG_EVENT_LEFT_MOUSE_DOWN, x, y);
    post_mouse(K_CG_EVENT_LEFT_MOUSE_UP, x, y);
}

pub fn mouse_down(x: f64, y: f64) {
    warp(x, y);
    post_mouse(K_CG_EVENT_LEFT_MOUSE_DOWN, x, y);
}

pub fn mouse_up(x: f64, y: f64) {
    warp(x, y);
    post_mouse(K_CG_EVENT_LEFT_MOUSE_UP, x, y);
}

pub fn drag_to(from: (f64, f64), to: (f64, f64)) {
    let (fx, fy) = from;
    let (tx, ty) = to;
    let dx = tx - fx;
    let dy = ty - fy;
    let distance = (dx * dx + dy * dy).sqrt();
    let steps = ((distance / DRAG_STEP_PX).ceil() as usize).max(1);
    thread::spawn(move || {
        for i in 1..=steps {
            let t = i as f64 / steps as f64;
            let x = fx + dx * t;
            let y = fy + dy * t;
            unsafe {
                CGWarpMouseCursorPosition(CGPoint::new(x, y));
            }
            post_mouse(K_CG_EVENT_LEFT_MOUSE_DRAGGED, x, y);
            if i != steps {
                thread::sleep(DRAG_STEP_INTERVAL);
            }
        }
        post_mouse(K_CG_EVENT_LEFT_MOUSE_UP, tx, ty);
    });
}
