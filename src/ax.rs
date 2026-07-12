use std::collections::HashMap;
use std::ffi::CString;

use core_foundation::base::{CFTypeRef, TCFType};
use core_foundation::string::CFString;
use core_foundation_sys::base::{kCFAllocatorDefault, CFAllocatorRef};
use core_foundation_sys::string::{kCFStringEncodingUTF8, CFStringCreateWithCString, CFStringRef};
use core_graphics::geometry::{CGPoint, CGSize};
use objc2_app_kit::{NSApplicationActivationOptions, NSRunningApplication, NSWorkspace};

use crate::space::WindowBridge;
use crate::types::{Rect, Result, WindowId};

type AXUIElementRef = *mut std::ffi::c_void;
type AXError = i32;

const K_AX_VALUE_CG_POINT_TYPE: u32 = 1;
const K_AX_VALUE_CG_SIZE_TYPE: u32 = 2;

const FRAME_MATCH_TOLERANCE: f64 = 2.0;
const APPLY_FRAME_MAX_ATTEMPTS: u32 = 4;

extern "C" {
    fn AXIsProcessTrusted() -> bool;
    fn AXUIElementCreateApplication(pid: i32) -> AXUIElementRef;
    fn AXUIElementCopyAttributeValue(
        element: AXUIElementRef,
        attribute: CFStringRef,
        value: *mut CFTypeRef,
    ) -> AXError;
    fn AXUIElementSetAttributeValue(
        element: AXUIElementRef,
        attribute: CFStringRef,
        value: CFTypeRef,
    ) -> AXError;
    fn AXUIElementPerformAction(element: AXUIElementRef, action: CFStringRef) -> AXError;
    fn AXValueCreate(value_type: u32, value: *const std::ffi::c_void) -> CFTypeRef;
    fn AXValueGetValue(value: CFTypeRef, value_type: u32, out_ptr: *mut std::ffi::c_void) -> u8;
    fn CFRelease(cf: CFTypeRef);
    fn CFRetain(cf: CFTypeRef) -> CFTypeRef;
    fn _AXUIElementGetWindow(element: AXUIElementRef, window_id: *mut u32) -> AXError;
}

fn make_cf_string(s: &str) -> CFStringRef {
    let c_str = CString::new(s).expect("string has no interior nulls");
    unsafe {
        CFStringCreateWithCString(
            kCFAllocatorDefault as CFAllocatorRef,
            c_str.as_ptr(),
            kCFStringEncodingUTF8,
        )
    }
}

fn ax_set_position(element: AXUIElementRef, x: f64, y: f64) -> AXError {
    let point = CGPoint::new(x, y);
    unsafe {
        let ax_value = AXValueCreate(
            K_AX_VALUE_CG_POINT_TYPE,
            &raw const point as *const std::ffi::c_void,
        );
        if ax_value.is_null() {
            return -1;
        }
        let attr = make_cf_string("AXPosition");
        let result = AXUIElementSetAttributeValue(element, attr, ax_value);
        CFRelease(ax_value);
        CFRelease(attr as CFTypeRef);
        result
    }
}

fn ax_set_size(element: AXUIElementRef, width: f64, height: f64) -> AXError {
    let size = CGSize::new(width, height);
    unsafe {
        let ax_value = AXValueCreate(
            K_AX_VALUE_CG_SIZE_TYPE,
            &raw const size as *const std::ffi::c_void,
        );
        if ax_value.is_null() {
            return -1;
        }
        let attr = make_cf_string("AXSize");
        let result = AXUIElementSetAttributeValue(element, attr, ax_value);
        CFRelease(ax_value);
        CFRelease(attr as CFTypeRef);
        result
    }
}

fn ax_get_position(window: AXUIElementRef) -> Option<CGPoint> {
    unsafe {
        let attr = make_cf_string("AXPosition");
        let mut value: CFTypeRef = std::ptr::null();
        let err = AXUIElementCopyAttributeValue(window, attr, &mut value);
        CFRelease(attr as CFTypeRef);
        if err != 0 || value.is_null() {
            return None;
        }
        let mut point = CGPoint::new(0.0, 0.0);
        let ok = AXValueGetValue(
            value,
            K_AX_VALUE_CG_POINT_TYPE,
            &raw mut point as *mut std::ffi::c_void,
        );
        CFRelease(value);
        if ok != 0 {
            Some(point)
        } else {
            None
        }
    }
}

fn ax_get_size(window: AXUIElementRef) -> Option<CGSize> {
    unsafe {
        let attr = make_cf_string("AXSize");
        let mut value: CFTypeRef = std::ptr::null();
        let err = AXUIElementCopyAttributeValue(window, attr, &mut value);
        CFRelease(attr as CFTypeRef);
        if err != 0 || value.is_null() {
            return None;
        }
        let mut size = CGSize::new(0.0, 0.0);
        let ok = AXValueGetValue(
            value,
            K_AX_VALUE_CG_SIZE_TYPE,
            &raw mut size as *mut std::ffi::c_void,
        );
        CFRelease(value);
        if ok != 0 {
            Some(size)
        } else {
            None
        }
    }
}

fn ax_get_frame(window: AXUIElementRef) -> Option<Rect> {
    let pos = ax_get_position(window)?;
    let size = ax_get_size(window)?;
    Some(Rect {
        x: pos.x,
        y: pos.y,
        width: size.width,
        height: size.height,
    })
}

pub(crate) fn frames_match(a: Rect, b: Rect, tol: f64) -> bool {
    (a.x - b.x).abs() <= tol
        && (a.y - b.y).abs() <= tol
        && (a.width - b.width).abs() <= tol
        && (a.height - b.height).abs() <= tol
}

fn ax_perform_action(element: AXUIElementRef, action: &str) -> AXError {
    unsafe {
        let attr = make_cf_string(action);
        let result = AXUIElementPerformAction(element, attr);
        CFRelease(attr as CFTypeRef);
        result
    }
}

fn ax_get_bool_attribute(element: AXUIElementRef, attribute: &str) -> Option<bool> {
    unsafe {
        let attr = make_cf_string(attribute);
        let mut value: CFTypeRef = std::ptr::null();
        let err = AXUIElementCopyAttributeValue(element, attr, &mut value);
        CFRelease(attr as CFTypeRef);
        if err != 0 || value.is_null() {
            return None;
        }
        let is_true = value == core_foundation_sys::number::kCFBooleanTrue as CFTypeRef;
        CFRelease(value);
        Some(is_true)
    }
}

fn hidden_state_satisfied(actual: Option<bool>, cached: Option<bool>, desired: bool) -> bool {
    match actual {
        Some(state) => state == desired,
        None => cached == Some(desired),
    }
}

fn ax_set_bool_attribute(element: AXUIElementRef, attribute: &str, value: bool) -> AXError {
    unsafe {
        let cf_bool: CFTypeRef = if value {
            core_foundation_sys::number::kCFBooleanTrue as CFTypeRef
        } else {
            core_foundation_sys::number::kCFBooleanFalse as CFTypeRef
        };
        let attr = make_cf_string(attribute);
        let result = AXUIElementSetAttributeValue(element, attr, cf_bool);
        CFRelease(attr as CFTypeRef);
        result
    }
}

fn ax_get_string_attribute(element: AXUIElementRef, attribute: &str) -> Option<String> {
    unsafe {
        let attr = make_cf_string(attribute);
        let mut value: CFTypeRef = std::ptr::null();
        let err = AXUIElementCopyAttributeValue(element, attr, &mut value);
        CFRelease(attr as CFTypeRef);
        if err != 0 || value.is_null() {
            return None;
        }
        let cf = CFString::wrap_under_get_rule(value as CFStringRef);
        let result = cf.to_string();
        CFRelease(value);
        Some(result)
    }
}

fn title_matches(title: &str, patterns: &[String]) -> bool {
    patterns.iter().any(|p| title.contains(p.as_str()))
}

fn find_window_by_id(app_element: AXUIElementRef, target_id: WindowId) -> Option<AXUIElementRef> {
    unsafe {
        let windows_attr = make_cf_string("AXWindows");
        let mut windows_value: CFTypeRef = std::ptr::null();
        let err = AXUIElementCopyAttributeValue(
            app_element,
            windows_attr,
            &mut windows_value as *mut CFTypeRef,
        );
        CFRelease(windows_attr as CFTypeRef);

        if err != 0 || windows_value.is_null() {
            return None;
        }

        let windows_array = windows_value as core_foundation_sys::array::CFArrayRef;
        let count = core_foundation_sys::array::CFArrayGetCount(windows_array);

        for i in 0..count {
            let window_elem = core_foundation_sys::array::CFArrayGetValueAtIndex(windows_array, i)
                as AXUIElementRef;

            if window_elem.is_null() {
                continue;
            }

            let mut cg_id: u32 = 0;
            let id_err = _AXUIElementGetWindow(window_elem, &mut cg_id);
            if id_err == 0 && cg_id == target_id {
                CFRetain(window_elem as CFTypeRef);
                CFRelease(windows_value);
                return Some(window_elem);
            }
        }

        CFRelease(windows_value);
        None
    }
}

pub fn is_accessibility_enabled() -> bool {
    unsafe { AXIsProcessTrusted() }
}

pub fn frontmost_app() -> Option<(i32, String)> {
    unsafe {
        let app = NSWorkspace::sharedWorkspace().frontmostApplication()?;
        let pid = app.processIdentifier();
        let name = app
            .localizedName()
            .map(|n| n.to_string())
            .unwrap_or_default();
        Some((pid, name))
    }
}

fn activate_app(pid: i32) {
    unsafe {
        let Some(app) = NSRunningApplication::runningApplicationWithProcessIdentifier(pid) else {
            return;
        };
        let options = NSApplicationActivationOptions::NSApplicationActivateAllWindows;
        let _ = app.activateWithOptions(options);
    }
}

pub struct MacOSBridge {
    app_elements: HashMap<WindowId, AXUIElementRef>,
    last_attempted: HashMap<WindowId, (Rect, u32)>,
    window_to_pid: HashMap<WindowId, i32>,
    app_hidden: HashMap<i32, bool>,
}

impl Default for MacOSBridge {
    fn default() -> Self {
        Self::new()
    }
}

impl MacOSBridge {
    pub fn new() -> Self {
        Self {
            app_elements: HashMap::new(),
            last_attempted: HashMap::new(),
            window_to_pid: HashMap::new(),
            app_hidden: HashMap::new(),
        }
    }

    fn get_window_element(&self, window_id: WindowId) -> Option<AXUIElementRef> {
        let app_element = self.app_elements.get(&window_id)?;
        find_window_by_id(*app_element, window_id)
    }

    fn set_app_hidden(&mut self, window_id: WindowId, hidden: bool) {
        let pid = match self.window_to_pid.get(&window_id) {
            Some(&p) => p,
            None => return,
        };
        let Some(&app_element) = self.app_elements.get(&window_id) else {
            return;
        };
        let actual = ax_get_bool_attribute(app_element, "AXHidden");
        let cached = self.app_hidden.get(&pid).copied();
        if hidden_state_satisfied(actual, cached, hidden) {
            self.app_hidden.insert(pid, hidden);
            return;
        }
        if hidden {
            let _ = ax_set_bool_attribute(app_element, "AXHidden", true);
        } else if let Some(running) =
            unsafe { NSRunningApplication::runningApplicationWithProcessIdentifier(pid) }
        {
            unsafe { running.unhide() };
        }
        self.app_hidden.insert(pid, hidden);
    }

    fn apply_frame_to_window(
        &mut self,
        app_element: AXUIElementRef,
        window: AXUIElementRef,
        window_id: WindowId,
        frame: Rect,
    ) {
        if let Some(current) = ax_get_frame(window) {
            if frames_match(current, frame, FRAME_MATCH_TOLERANCE) {
                self.last_attempted
                    .insert(window_id, (frame, APPLY_FRAME_MAX_ATTEMPTS));
                return;
            }
        }

        let attempts_remaining = match self.last_attempted.get(&window_id) {
            Some(&(last_frame, n)) if frames_match(last_frame, frame, FRAME_MATCH_TOLERANCE) => n,
            _ => APPLY_FRAME_MAX_ATTEMPTS,
        };
        if attempts_remaining == 0 {
            log::info!(
                "DIAG wid={window_id} GIVE UP (attempts exhausted) current={:?} target={frame:?}",
                ax_get_frame(window)
            );
            return;
        }

        let before = ax_get_frame(window);
        let enh_before = ax_get_bool_attribute(app_element, "AXEnhancedUserInterface");
        let e_off = ax_set_bool_attribute(app_element, "AXEnhancedUserInterface", false);
        let e_pos1 = ax_set_position(window, frame.x, frame.y);
        let e_size = ax_set_size(window, frame.width, frame.height);
        let e_pos2 = ax_set_position(window, frame.x, frame.y);
        let e_on = ax_set_bool_attribute(app_element, "AXEnhancedUserInterface", true);
        let after = ax_get_frame(window);
        log::info!(
            "DIAG wid={window_id} attempt {attempts_remaining} enh_before={enh_before:?} \
             before={before:?} target={frame:?} after={after:?} \
             err[enh_off={e_off} pos1={e_pos1} size={e_size} pos2={e_pos2} enh_on={e_on}]"
        );

        self.last_attempted
            .insert(window_id, (frame, attempts_remaining - 1));
    }
}

impl WindowBridge for MacOSBridge {
    fn register_window(&mut self, window_id: WindowId, pid: i32) {
        let app_element = unsafe { AXUIElementCreateApplication(pid) };
        if app_element.is_null() {
            log::debug!("failed to create AX element for pid {pid}");
            return;
        }
        let _ = ax_set_bool_attribute(app_element, "AXEnhancedUserInterface", true);
        log::debug!("registered app element for window {window_id}, pid {pid}");
        self.app_elements.insert(window_id, app_element);
        self.window_to_pid.insert(window_id, pid);
        self.app_hidden.entry(pid).or_insert(false);
    }

    fn apply_frame(&mut self, window_id: WindowId, frame: Rect) -> Result<()> {
        self.set_app_hidden(window_id, false);

        let Some(app_element) = self.app_elements.get(&window_id).copied() else {
            log::debug!("apply_frame: no app element for {window_id}, skipping");
            return Ok(());
        };

        let window = match self.get_window_element(window_id) {
            Some(w) => w,
            None => {
                log::debug!("apply_frame: no AX window for {window_id}, skipping");
                return Ok(());
            }
        };

        self.apply_frame_to_window(app_element, window, window_id, frame);

        unsafe { CFRelease(window as CFTypeRef) };
        Ok(())
    }

    fn hide(&mut self, window_id: WindowId) -> Result<()> {
        self.set_app_hidden(window_id, true);
        Ok(())
    }

    fn apply_frame_hiding(
        &mut self,
        window_id: WindowId,
        frame: Rect,
        hide_titles: &[String],
    ) -> Result<()> {
        self.set_app_hidden(window_id, false);

        let Some(app_element) = self.app_elements.get(&window_id).copied() else {
            return Ok(());
        };

        let windows_value = unsafe {
            let windows_attr = make_cf_string("AXWindows");
            let mut value: CFTypeRef = std::ptr::null();
            let err = AXUIElementCopyAttributeValue(app_element, windows_attr, &mut value);
            CFRelease(windows_attr as CFTypeRef);
            if err != 0 || value.is_null() {
                return Ok(());
            }
            value
        };

        let mut windows: Vec<(WindowId, AXUIElementRef, bool)> = Vec::new();
        let mut frontmost_id: Option<WindowId> = None;
        unsafe {
            let windows_array = windows_value as core_foundation_sys::array::CFArrayRef;
            let count = core_foundation_sys::array::CFArrayGetCount(windows_array);
            for i in 0..count {
                let elem = core_foundation_sys::array::CFArrayGetValueAtIndex(windows_array, i)
                    as AXUIElementRef;
                if elem.is_null() {
                    continue;
                }
                let mut cg_id: u32 = 0;
                if _AXUIElementGetWindow(elem, &mut cg_id) != 0 {
                    continue;
                }
                if frontmost_id.is_none() {
                    frontmost_id = Some(cg_id);
                }
                let title = ax_get_string_attribute(elem, "AXTitle").unwrap_or_default();
                windows.push((cg_id, elem, title_matches(&title, hide_titles)));
            }
        }

        let primary = windows
            .iter()
            .position(|&(id, _, hidden)| !hidden && id == window_id)
            .or_else(|| windows.iter().position(|&(_, _, hidden)| !hidden))
            .or_else(|| windows.iter().position(|&(id, _, _)| id == window_id));

        if let Some(primary_idx) = primary {
            let (primary_id, primary_elem, _) = windows[primary_idx];
            self.apply_frame_to_window(app_element, primary_elem, primary_id, frame);

            let mut covered_any = false;
            for (i, &(id, elem, hidden)) in windows.iter().enumerate() {
                if i == primary_idx || !hidden {
                    continue;
                }
                self.apply_frame_to_window(app_element, elem, id, frame);
                covered_any = true;
            }

            if covered_any && frontmost_id != Some(primary_id) {
                let _ = ax_set_bool_attribute(primary_elem, "AXMain", true);
                ax_perform_action(primary_elem, "AXRaise");
            }
        }

        unsafe { CFRelease(windows_value) };
        Ok(())
    }

    fn focus(&mut self, window_id: WindowId) -> Result<()> {
        log::info!("bridge.focus({window_id})");
        if let Some(&pid) = self.window_to_pid.get(&window_id) {
            activate_app(pid);
        }

        let window = match self.get_window_element(window_id) {
            Some(w) => w,
            None => return Ok(()),
        };

        let _ = ax_perform_action(window, "AXRaise");
        let _ = ax_set_bool_attribute(window, "AXMain", true);
        let _ = ax_set_bool_attribute(window, "AXFocused", true);
        unsafe { CFRelease(window as CFTypeRef) };
        Ok(())
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
    fn identical_frames_match() {
        assert!(frames_match(
            r(0.0, 0.0, 100.0, 100.0),
            r(0.0, 0.0, 100.0, 100.0),
            2.0
        ));
    }

    #[test]
    fn frames_within_tolerance_match() {
        assert!(frames_match(
            r(0.5, 0.0, 100.0, 100.0),
            r(0.0, 0.0, 100.0, 100.0),
            2.0
        ));
        assert!(frames_match(
            r(0.0, 1.5, 100.0, 100.0),
            r(0.0, 0.0, 100.0, 100.0),
            2.0
        ));
        assert!(frames_match(
            r(0.0, 0.0, 101.5, 99.0),
            r(0.0, 0.0, 100.0, 100.0),
            2.0
        ));
    }

    #[test]
    fn frames_at_tolerance_boundary_match() {
        assert!(frames_match(
            r(2.0, 0.0, 100.0, 100.0),
            r(0.0, 0.0, 100.0, 100.0),
            2.0
        ));
    }

    #[test]
    fn external_unhide_forces_reissue_when_cache_is_stale() {
        assert!(!hidden_state_satisfied(Some(false), Some(true), true));
    }

    #[test]
    fn actual_state_overrides_cache() {
        assert!(hidden_state_satisfied(Some(true), Some(false), true));
        assert!(hidden_state_satisfied(Some(false), Some(true), false));
    }

    #[test]
    fn falls_back_to_cache_when_actual_unreadable() {
        assert!(hidden_state_satisfied(None, Some(true), true));
        assert!(!hidden_state_satisfied(None, Some(false), true));
        assert!(!hidden_state_satisfied(None, None, true));
    }

    #[test]
    fn frames_outside_tolerance_do_not_match() {
        assert!(!frames_match(
            r(3.0, 0.0, 100.0, 100.0),
            r(0.0, 0.0, 100.0, 100.0),
            2.0
        ));
        assert!(!frames_match(
            r(0.0, 0.0, 110.0, 100.0),
            r(0.0, 0.0, 100.0, 100.0),
            2.0
        ));
        assert!(!frames_match(
            r(0.0, 0.0, 100.0, 90.0),
            r(0.0, 0.0, 100.0, 100.0),
            2.0
        ));
    }
}
