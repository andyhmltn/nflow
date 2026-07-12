use crate::hint::ClickKind;

type CGEventRef = *mut std::ffi::c_void;
type CGEventSourceRef = *mut std::ffi::c_void;

const K_CG_EVENT_LEFT_MOUSE_DOWN: u32 = 1;
const K_CG_EVENT_LEFT_MOUSE_UP: u32 = 2;
const K_CG_EVENT_RIGHT_MOUSE_DOWN: u32 = 3;
const K_CG_EVENT_RIGHT_MOUSE_UP: u32 = 4;
const K_CG_MOUSE_BUTTON_LEFT: u32 = 0;
const K_CG_MOUSE_BUTTON_RIGHT: u32 = 1;
const K_CG_HID_EVENT_TAP: u32 = 0;

extern "C" {
    fn CGEventCreateMouseEvent(
        source: CGEventSourceRef,
        mouse_type: u32,
        mouse_cursor_position: core_graphics::geometry::CGPoint,
        mouse_button: u32,
    ) -> CGEventRef;
    fn CGEventPost(tap: u32, event: CGEventRef);
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

pub fn click_at(x: f64, y: f64, kind: ClickKind) {
    let point = core_graphics::geometry::CGPoint::new(x, y);
    let (down_type, up_type, button) = match kind {
        ClickKind::Left => (
            K_CG_EVENT_LEFT_MOUSE_DOWN,
            K_CG_EVENT_LEFT_MOUSE_UP,
            K_CG_MOUSE_BUTTON_LEFT,
        ),
        ClickKind::Right => (
            K_CG_EVENT_RIGHT_MOUSE_DOWN,
            K_CG_EVENT_RIGHT_MOUSE_UP,
            K_CG_MOUSE_BUTTON_RIGHT,
        ),
    };
    unsafe {
        let down = CGEventCreateMouseEvent(std::ptr::null_mut(), down_type, point, button);
        let up = CGEventCreateMouseEvent(std::ptr::null_mut(), up_type, point, button);
        post(down);
        post(up);
    }
}
