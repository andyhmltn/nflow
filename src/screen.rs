use std::sync::atomic::{AtomicBool, Ordering};

use core_graphics::display::{CGDisplay, CGDisplayReconfigurationCallBack};

use crate::types::{NflowError, Rect, Result};

const MENU_BAR_HEIGHT: f64 = 25.0;
const K_CG_DISPLAY_BEGIN_CONFIGURATION_FLAG: u32 = 1 << 0;

static SCREEN_CHANGED: AtomicBool = AtomicBool::new(false);

unsafe extern "C" fn on_display_reconfigured(
    _display: u32,
    flags: u32,
    _user_info: *const std::ffi::c_void,
) {
    if flags & K_CG_DISPLAY_BEGIN_CONFIGURATION_FLAG != 0 {
        return;
    }
    SCREEN_CHANGED.store(true, Ordering::SeqCst);
}

pub fn get_screen_rect() -> Rect {
    let bounds = CGDisplay::main().bounds();
    Rect {
        x: bounds.origin.x,
        y: bounds.origin.y + MENU_BAR_HEIGHT,
        width: bounds.size.width,
        height: bounds.size.height - MENU_BAR_HEIGHT,
    }
}

pub fn get_full_screen_rect() -> Rect {
    let bounds = CGDisplay::main().bounds();
    Rect {
        x: bounds.origin.x,
        y: bounds.origin.y,
        width: bounds.size.width,
        height: bounds.size.height,
    }
}

pub fn get_screen_width() -> u32 {
    CGDisplay::main().bounds().size.width as u32
}

pub fn register_screen_change_callback() -> Result<()> {
    let callback: CGDisplayReconfigurationCallBack = on_display_reconfigured;
    let result = unsafe {
        core_graphics::display::CGDisplayRegisterReconfigurationCallback(callback, std::ptr::null())
    };
    if result == 0 {
        Ok(())
    } else {
        Err(NflowError::ScreenDetection(format!(
            "CGDisplayRegisterReconfigurationCallback returned {result}"
        )))
    }
}

pub fn check_screen_changed() -> bool {
    SCREEN_CHANGED.swap(false, Ordering::SeqCst)
}
