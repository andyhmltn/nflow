use std::collections::{HashMap, HashSet};
use std::ffi::CString;
use std::time::{Duration, Instant};

use core_foundation::base::{CFTypeRef, TCFType};
use core_foundation::boolean::CFBoolean;
use core_foundation::number::CFNumber;
use core_foundation::string::CFString;
use core_foundation_sys::array::{
    CFArrayGetCount, CFArrayGetTypeID, CFArrayGetValueAtIndex, CFArrayRef,
};
use core_foundation_sys::base::{kCFAllocatorDefault, CFAllocatorRef, CFGetTypeID};
use core_foundation_sys::dictionary::{CFDictionaryGetValueIfPresent, CFDictionaryRef};
use core_foundation_sys::number::{
    kCFNumberSInt32Type, kCFNumberSInt64Type, CFNumberGetValue, CFNumberRef,
};
use core_foundation_sys::string::{
    kCFStringEncodingUTF8, CFStringCreateWithCString, CFStringGetTypeID, CFStringRef,
};
use core_graphics::geometry::{CGPoint, CGRect, CGSize};
use objc2_app_kit::NSWorkspace;

use crate::hint::geometry::is_usable;
use crate::hint::HintTarget;
use crate::types::Rect;

type AXUIElementRef = *mut std::ffi::c_void;
type AXError = i32;

const K_AX_VALUE_CG_POINT_TYPE: u32 = 1;
const K_AX_VALUE_CG_SIZE_TYPE: u32 = 2;
const K_AX_VALUE_CG_RECT_TYPE: u32 = 3;
const K_AX_VALUE_CF_RANGE_TYPE: u32 = 4;

const TEXT_ROLES: &[&str] = &[
    "AXStaticText",
    "AXTextField",
    "AXTextArea",
    "AXComboBox",
    "AXSearchField",
];

#[repr(C)]
struct CFRange {
    location: isize,
    length: isize,
}

const K_CG_WINDOW_LIST_OPTION_ON_SCREEN_ONLY: u32 = 1 << 0;
const K_CG_NULL_WINDOW_ID: u32 = 0;

const MAX_DEPTH: u32 = 120;
const MAX_TARGETS: usize = 20000;
const MENU_BUDGET: Duration = Duration::from_millis(150);
const WINDOW_BUDGET: Duration = Duration::from_millis(3000);
const MESSAGING_TIMEOUT: f32 = 0.25;
const EXTRAS_MESSAGING_TIMEOUT: f32 = 0.08;

const ACTIONABLE_NAMES: &[&str] = &[
    "AXPress",
    "AXOpen",
    "AXShowMenu",
    "AXPick",
    "AXConfirm",
    "AXIncrement",
    "AXDecrement",
];

const TARGET_ROLES: &[&str] = &[
    "AXTextField",
    "AXTextArea",
    "AXComboBox",
    "AXSearchField",
    "AXLink",
    "AXButton",
    "AXMenuButton",
    "AXPopUpButton",
    "AXMenuItem",
    "AXMenuBarItem",
    "AXCheckBox",
    "AXRadioButton",
    "AXTab",
    "AXDisclosureTriangle",
    "AXStepper",
    "AXSlider",
    "AXColorWell",
];

extern "C" {
    fn AXUIElementCreateApplication(pid: i32) -> AXUIElementRef;
    fn AXUIElementCopyAttributeValue(
        element: AXUIElementRef,
        attribute: CFStringRef,
        value: *mut CFTypeRef,
    ) -> AXError;
    fn AXUIElementCopyActionNames(element: AXUIElementRef, names: *mut CFArrayRef) -> AXError;
    fn AXUIElementSetMessagingTimeout(element: AXUIElementRef, timeout: f32) -> AXError;
    fn AXValueGetValue(value: CFTypeRef, value_type: u32, out_ptr: *mut std::ffi::c_void) -> u8;
    fn _AXUIElementGetWindow(element: AXUIElementRef, window_id: *mut u32) -> AXError;
    fn AXUIElementSetAttributeValue(
        element: AXUIElementRef,
        attribute: CFStringRef,
        value: CFTypeRef,
    ) -> AXError;
    fn AXUIElementCopyParameterizedAttributeValue(
        element: AXUIElementRef,
        parameter_attribute: CFStringRef,
        parameter: CFTypeRef,
        result: *mut CFTypeRef,
    ) -> AXError;
    fn AXValueCreate(the_type: u32, value_ptr: *const std::ffi::c_void) -> CFTypeRef;
    fn AXUIElementPerformAction(element: AXUIElementRef, action: CFStringRef) -> AXError;
    fn CFRelease(cf: CFTypeRef);
    fn CFRetain(cf: CFTypeRef) -> CFTypeRef;
    fn CFURLGetString(url: CFTypeRef) -> CFStringRef;
    fn CGWindowListCopyWindowInfo(option: u32, relative_to: u32) -> CFArrayRef;
}

const TEXT_FALLBACK_DEPTH: u32 = 4;

const WEB_SCROLLER_ROLES: &[&str] = &["AXGroup", "AXOutline", "AXList", "AXTable"];
const WEB_SCROLLER_MIN_SIZE: f64 = 100.0;
const ELECTRON_ENABLE_DELAY: Duration = Duration::from_millis(300);

pub struct AxElement(AXUIElementRef);

unsafe impl Send for AxElement {}

impl AxElement {
    pub(crate) fn retain(raw: AXUIElementRef) -> Self {
        unsafe { CFRetain(raw as CFTypeRef) };
        AxElement(raw)
    }

    pub fn bool_attr(&self, attribute: &str) -> Option<bool> {
        let attr = make_cf_string(attribute);
        if attr.is_null() {
            return None;
        }
        unsafe {
            let mut value: CFTypeRef = std::ptr::null();
            let err = AXUIElementCopyAttributeValue(self.0, attr, &mut value);
            CFRelease(attr as CFTypeRef);
            if err != 0 || value.is_null() {
                return None;
            }
            let is_true = value == core_foundation_sys::number::kCFBooleanTrue as CFTypeRef;
            CFRelease(value);
            Some(is_true)
        }
    }

    pub fn string_attr(&self, attribute: &str) -> Option<String> {
        ax_get_string_attribute(self.0, attribute)
    }

    /// Read a single-valued element attribute (e.g. `AXMenuBar`, `AXFocusedWindow`)
    /// and retain it into a new `AxElement`.
    pub fn attr_element(&self, attribute: &str) -> Option<AxElement> {
        let attr = make_cf_string(attribute);
        if attr.is_null() {
            return None;
        }
        unsafe {
            let mut value: CFTypeRef = std::ptr::null();
            let err = AXUIElementCopyAttributeValue(self.0, attr, &mut value);
            CFRelease(attr as CFTypeRef);
            if err != 0 || value.is_null() {
                return None;
            }
            let child = AxElement::retain(value as AXUIElementRef);
            CFRelease(value);
            Some(child)
        }
    }

    /// Direct children (AXChildren). Each is retained into a new `AxElement`.
    pub fn children(&self) -> Vec<AxElement> {
        let attr = make_cf_string("AXChildren");
        if attr.is_null() {
            return Vec::new();
        }
        let children_value = unsafe {
            let mut value: CFTypeRef = std::ptr::null();
            let err = AXUIElementCopyAttributeValue(self.0, attr, &mut value);
            CFRelease(attr as CFTypeRef);
            if err != 0 || value.is_null() {
                return Vec::new();
            }
            value
        };
        let out = unsafe {
            let array = children_value as CFArrayRef;
            let count = CFArrayGetCount(array);
            let mut items = Vec::with_capacity(count as usize);
            for i in 0..count {
                let child = CFArrayGetValueAtIndex(array, i) as AXUIElementRef;
                if child.is_null() {
                    continue;
                }
                items.push(AxElement::retain(child));
            }
            items
        };
        unsafe { CFRelease(children_value) };
        out
    }

    pub fn has_action(&self, action: &str) -> bool {
        unsafe {
            let mut names: CFArrayRef = std::ptr::null();
            let err = AXUIElementCopyActionNames(self.0, &mut names);
            if err != 0 || names.is_null() {
                return false;
            }
            let count = CFArrayGetCount(names);
            let mut found = false;
            for i in 0..count {
                let name = CFArrayGetValueAtIndex(names, i) as CFStringRef;
                if name.is_null() {
                    continue;
                }
                let cf = CFString::wrap_under_get_rule(name);
                #[allow(clippy::cmp_owned)]
                if cf.to_string() == action {
                    found = true;
                    break;
                }
            }
            CFRelease(names as CFTypeRef);
            found
        }
    }

    pub fn press(&self) -> bool {
        let attr = make_cf_string("AXPress");
        if attr.is_null() {
            return false;
        }
        let err = unsafe { AXUIElementPerformAction(self.0, attr) };
        unsafe { CFRelease(attr as CFTypeRef) };
        err == 0
    }

    pub fn url(&self) -> Option<String> {
        let attr = make_cf_string("AXURL");
        if attr.is_null() {
            return None;
        }
        unsafe {
            let mut value: CFTypeRef = std::ptr::null();
            let err = AXUIElementCopyAttributeValue(self.0, attr, &mut value);
            CFRelease(attr as CFTypeRef);
            if err != 0 || value.is_null() {
                return None;
            }
            let cf_str = CFURLGetString(value);
            if cf_str.is_null() {
                CFRelease(value);
                return None;
            }
            let result = CFString::wrap_under_get_rule(cf_str).to_string();
            CFRelease(value);
            Some(result)
        }
    }

    pub fn link_text(&self) -> Option<String> {
        for attr in ["AXTitle", "AXDescription"] {
            if let Some(text) = nonempty_string(self.0, attr) {
                return Some(text);
            }
        }
        descendant_text(self.0, 0)
    }

    pub fn value(&self) -> Option<String> {
        ax_get_string_attribute(self.0, "AXValue")
    }

    /// The CG window id this element belongs to, used to group pieces that
    /// belong to the same window (so line reconstruction doesn't merge text
    /// across adjacent tiled windows).
    pub fn window_id(&self) -> Option<u32> {
        let mut id: u32 = 0;
        let err = unsafe { _AXUIElementGetWindow(self.0, &mut id) };
        if err == 0 && id != 0 {
            Some(id)
        } else {
            None
        }
    }

    pub fn set_vertical_fraction(&self, fraction: f64) -> bool {
        self.set_scroll_bar_fraction("AXVerticalScrollBar", fraction)
    }

    fn set_scroll_bar_fraction(&self, bar_attr: &str, fraction: f64) -> bool {
        let attr = make_cf_string(bar_attr);
        if attr.is_null() {
            return false;
        }
        unsafe {
            let mut bar: CFTypeRef = std::ptr::null();
            let err = AXUIElementCopyAttributeValue(self.0, attr, &mut bar);
            CFRelease(attr as CFTypeRef);
            if err != 0 || bar.is_null() {
                return false;
            }
            let value_attr = make_cf_string("AXValue");
            if value_attr.is_null() {
                CFRelease(bar);
                return false;
            }
            let number = CFNumber::from(fraction.clamp(0.0, 1.0));
            let set_err = AXUIElementSetAttributeValue(
                bar as AXUIElementRef,
                value_attr,
                number.as_concrete_TypeRef() as CFTypeRef,
            );
            CFRelease(value_attr as CFTypeRef);
            CFRelease(bar);
            set_err == 0
        }
    }

    pub fn set_selected_range(&self, location: usize, length: usize) {
        let range = CFRange {
            location: location as isize,
            length: length as isize,
        };
        unsafe {
            let ax_value = AXValueCreate(K_AX_VALUE_CF_RANGE_TYPE, &range as *const _ as *const _);
            if ax_value.is_null() {
                return;
            }
            let attr = make_cf_string("AXSelectedTextRange");
            if !attr.is_null() {
                AXUIElementSetAttributeValue(self.0, attr, ax_value);
                CFRelease(attr as CFTypeRef);
            }
            CFRelease(ax_value);
        }
    }

    pub fn bounds_for_range(&self, location: usize, length: usize) -> Option<Rect> {
        let range = CFRange {
            location: location as isize,
            length: length as isize,
        };
        unsafe {
            let param = AXValueCreate(K_AX_VALUE_CF_RANGE_TYPE, &range as *const _ as *const _);
            if param.is_null() {
                return None;
            }
            let attr = make_cf_string("AXBoundsForRange");
            if attr.is_null() {
                CFRelease(param);
                return None;
            }
            let mut value: CFTypeRef = std::ptr::null();
            let err = AXUIElementCopyParameterizedAttributeValue(self.0, attr, param, &mut value);
            CFRelease(attr as CFTypeRef);
            CFRelease(param);
            if err != 0 || value.is_null() {
                return None;
            }
            let mut rect = CGRect::new(&CGPoint::new(0.0, 0.0), &CGSize::new(0.0, 0.0));
            let ok = AXValueGetValue(
                value,
                K_AX_VALUE_CG_RECT_TYPE,
                &mut rect as *mut _ as *mut std::ffi::c_void,
            );
            CFRelease(value);
            if ok == 0 {
                return None;
            }
            Some(Rect {
                x: rect.origin.x,
                y: rect.origin.y,
                width: rect.size.width,
                height: rect.size.height,
            })
        }
    }
}

impl Drop for AxElement {
    fn drop(&mut self) {
        unsafe { CFRelease(self.0 as CFTypeRef) };
    }
}

/// Create a retained `AxElement` for an application by PID.
pub(crate) fn make_app_element(pid: i32) -> Option<AxElement> {
    let raw = unsafe { AXUIElementCreateApplication(pid) };
    if raw.is_null() {
        return None;
    }
    Some(AxElement::retain(raw))
}

fn nonempty_string(element: AXUIElementRef, attribute: &str) -> Option<String> {
    let value = ax_get_string_attribute(element, attribute)?;
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn descendant_text(element: AXUIElementRef, depth: u32) -> Option<String> {
    if depth > TEXT_FALLBACK_DEPTH {
        return None;
    }
    if let Some(role) = ax_get_string_attribute(element, "AXRole") {
        if role == "AXStaticText" {
            if let Some(text) = nonempty_string(element, "AXValue") {
                return Some(text);
            }
        }
    }
    let attr = make_cf_string("AXChildren");
    if attr.is_null() {
        return None;
    }
    let children_value = unsafe {
        let mut value: CFTypeRef = std::ptr::null();
        let err = AXUIElementCopyAttributeValue(element, attr, &mut value);
        CFRelease(attr as CFTypeRef);
        if err != 0 || value.is_null() {
            return None;
        }
        value
    };
    let mut found = None;
    unsafe {
        let children = children_value as CFArrayRef;
        let count = CFArrayGetCount(children);
        for i in 0..count {
            let child = CFArrayGetValueAtIndex(children, i) as AXUIElementRef;
            if child.is_null() {
                continue;
            }
            if let Some(text) = descendant_text(child, depth + 1) {
                found = Some(text);
                break;
            }
        }
        CFRelease(children_value);
    }
    found
}

pub(crate) fn make_cf_string(s: &str) -> CFStringRef {
    let Ok(c_str) = CString::new(s) else {
        return std::ptr::null();
    };
    unsafe {
        CFStringCreateWithCString(
            kCFAllocatorDefault as CFAllocatorRef,
            c_str.as_ptr(),
            kCFStringEncodingUTF8,
        )
    }
}

fn dict_get_value(dict: CFDictionaryRef, key: &str) -> Option<*const std::ffi::c_void> {
    let cf_key = make_cf_string(key);
    if cf_key.is_null() {
        return None;
    }
    unsafe {
        let mut value: *const std::ffi::c_void = std::ptr::null();
        let present = CFDictionaryGetValueIfPresent(
            dict,
            cf_key as *const std::ffi::c_void,
            &mut value as *mut *const std::ffi::c_void,
        );
        CFRelease(cf_key as CFTypeRef);
        if present == 0 || value.is_null() {
            return None;
        }
        Some(value)
    }
}

fn dict_get_i32(dict: CFDictionaryRef, key: &str) -> Option<i32> {
    let value = dict_get_value(dict, key)?;
    unsafe {
        let mut out: i32 = 0;
        let ok = CFNumberGetValue(
            value as CFNumberRef,
            kCFNumberSInt32Type,
            &raw mut out as *mut std::ffi::c_void,
        );
        if ok {
            Some(out)
        } else {
            None
        }
    }
}

fn dict_get_window_id(dict: CFDictionaryRef, key: &str) -> Option<u32> {
    let value = dict_get_value(dict, key)?;
    unsafe {
        let mut out: i64 = 0;
        let ok = CFNumberGetValue(
            value as CFNumberRef,
            kCFNumberSInt64Type,
            &raw mut out as *mut std::ffi::c_void,
        );
        if ok {
            Some(out as u32)
        } else {
            None
        }
    }
}

fn on_screen_windows() -> HashMap<i32, HashSet<u32>> {
    let mut map: HashMap<i32, HashSet<u32>> = HashMap::new();
    unsafe {
        let list =
            CGWindowListCopyWindowInfo(K_CG_WINDOW_LIST_OPTION_ON_SCREEN_ONLY, K_CG_NULL_WINDOW_ID);
        if list.is_null() {
            return map;
        }
        let count = CFArrayGetCount(list);
        for i in 0..count {
            let dict = CFArrayGetValueAtIndex(list, i) as CFDictionaryRef;
            if dict.is_null() {
                continue;
            }
            let Some(pid) = dict_get_i32(dict, "kCGWindowOwnerPID") else {
                continue;
            };
            let Some(window_number) = dict_get_window_id(dict, "kCGWindowNumber") else {
                continue;
            };
            map.entry(pid).or_default().insert(window_number);
        }
        CFRelease(list as CFTypeRef);
    }
    map
}

fn ax_get_position(element: AXUIElementRef) -> Option<CGPoint> {
    let attr = make_cf_string("AXPosition");
    if attr.is_null() {
        return None;
    }
    unsafe {
        let mut value: CFTypeRef = std::ptr::null();
        let err = AXUIElementCopyAttributeValue(element, attr, &mut value);
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

fn ax_get_size(element: AXUIElementRef) -> Option<CGSize> {
    let attr = make_cf_string("AXSize");
    if attr.is_null() {
        return None;
    }
    unsafe {
        let mut value: CFTypeRef = std::ptr::null();
        let err = AXUIElementCopyAttributeValue(element, attr, &mut value);
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

fn ax_get_frame(element: AXUIElementRef) -> Option<Rect> {
    let pos = ax_get_position(element)?;
    let size = ax_get_size(element)?;
    Some(Rect {
        x: pos.x,
        y: pos.y,
        width: size.width,
        height: size.height,
    })
}

pub(crate) fn ax_get_string_attribute(element: AXUIElementRef, attribute: &str) -> Option<String> {
    let attr = make_cf_string(attribute);
    if attr.is_null() {
        return None;
    }
    unsafe {
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

fn has_actionable(element: AXUIElementRef) -> bool {
    unsafe {
        let mut names: CFArrayRef = std::ptr::null();
        let err = AXUIElementCopyActionNames(element, &mut names);
        if err != 0 || names.is_null() {
            return false;
        }
        let count = CFArrayGetCount(names);
        let mut found = false;
        for i in 0..count {
            let name = CFArrayGetValueAtIndex(names, i) as CFStringRef;
            if name.is_null() {
                continue;
            }
            let cf = CFString::wrap_under_get_rule(name);
            let action = cf.to_string();
            if ACTIONABLE_NAMES.contains(&action.as_str()) {
                found = true;
                break;
            }
        }
        CFRelease(names as CFTypeRef);
        found
    }
}

fn is_clickable(element: AXUIElementRef) -> bool {
    if has_actionable(element) {
        return true;
    }
    match ax_get_string_attribute(element, "AXRole") {
        Some(role) => TARGET_ROLES.contains(&role.as_str()),
        None => false,
    }
}

fn is_link(element: AXUIElementRef) -> bool {
    matches!(
        ax_get_string_attribute(element, "AXRole").as_deref(),
        Some("AXLink")
    )
}

fn is_text(element: AXUIElementRef) -> bool {
    match ax_get_string_attribute(element, "AXRole") {
        Some(role) => TEXT_ROLES.contains(&role.as_str()),
        None => false,
    }
}

fn ax_get_class_list(element: AXUIElementRef) -> Vec<String> {
    let attr = make_cf_string("AXDOMClassList");
    if attr.is_null() {
        return Vec::new();
    }
    unsafe {
        let mut value: CFTypeRef = std::ptr::null();
        let err = AXUIElementCopyAttributeValue(element, attr, &mut value);
        CFRelease(attr as CFTypeRef);
        if err != 0 || value.is_null() {
            return Vec::new();
        }
        if CFGetTypeID(value) != CFArrayGetTypeID() {
            CFRelease(value);
            return Vec::new();
        }
        let array = value as CFArrayRef;
        let count = CFArrayGetCount(array);
        let mut out = Vec::with_capacity(count as usize);
        for i in 0..count {
            let item = CFArrayGetValueAtIndex(array, i);
            if item.is_null() || CFGetTypeID(item as CFTypeRef) != CFStringGetTypeID() {
                continue;
            }
            out.push(CFString::wrap_under_get_rule(item as CFStringRef).to_string());
        }
        CFRelease(value);
        out
    }
}

fn has_scroll_class(classes: &[String]) -> bool {
    classes
        .iter()
        .any(|class| class.to_ascii_lowercase().contains("scroll"))
}

fn is_scroll_target(element: AXUIElementRef) -> bool {
    let Some(role) = ax_get_string_attribute(element, "AXRole") else {
        return false;
    };
    if role == "AXScrollArea" || role == "AXWebArea" {
        return true;
    }
    if !WEB_SCROLLER_ROLES.contains(&role.as_str()) {
        return false;
    }
    let Some(frame) = ax_get_frame(element) else {
        return false;
    };
    if frame.width < WEB_SCROLLER_MIN_SIZE || frame.height < WEB_SCROLLER_MIN_SIZE {
        return false;
    }
    has_scroll_class(&ax_get_class_list(element))
}

struct Walker {
    is_target: fn(AXUIElementRef) -> bool,
    capture: bool,
    out: Vec<HintTarget>,
}

impl Walker {
    fn new(is_target: fn(AXUIElementRef) -> bool, capture: bool) -> Self {
        Walker {
            is_target,
            capture,
            out: Vec::new(),
        }
    }

    fn done(&self, deadline: Instant) -> bool {
        self.out.len() >= MAX_TARGETS || Instant::now() >= deadline
    }

    fn push_if_target(&mut self, element: AXUIElementRef) {
        if !(self.is_target)(element) {
            return;
        }
        let Some(frame) = ax_get_frame(element) else {
            return;
        };
        let captured = if self.capture {
            Some(AxElement::retain(element))
        } else {
            None
        };
        self.out.push(HintTarget {
            frame,
            element: captured,
        });
    }

    fn recurse(&mut self, element: AXUIElementRef, depth: u32, deadline: Instant) {
        if depth > MAX_DEPTH || self.done(deadline) {
            return;
        }

        self.push_if_target(element);

        let attr = make_cf_string("AXChildren");
        if attr.is_null() {
            return;
        }
        let children_value = unsafe {
            let mut value: CFTypeRef = std::ptr::null();
            let err = AXUIElementCopyAttributeValue(element, attr, &mut value);
            CFRelease(attr as CFTypeRef);
            if err != 0 || value.is_null() {
                return;
            }
            value
        };

        unsafe {
            let children = children_value as CFArrayRef;
            let count = CFArrayGetCount(children);
            for i in 0..count {
                if self.done(deadline) {
                    break;
                }
                let child = CFArrayGetValueAtIndex(children, i) as AXUIElementRef;
                if child.is_null() {
                    continue;
                }
                self.recurse(child, depth + 1, deadline);
            }
            CFRelease(children_value);
        }
    }

    fn walk_app(&mut self, pid: i32, on_screen: &HashSet<u32>, deadline: Instant) {
        let app = unsafe { AXUIElementCreateApplication(pid) };
        if app.is_null() {
            return;
        }
        unsafe {
            AXUIElementSetMessagingTimeout(app, MESSAGING_TIMEOUT);
        }

        let attr = make_cf_string("AXWindows");
        if attr.is_null() {
            unsafe { CFRelease(app as CFTypeRef) };
            return;
        }
        let windows_value = unsafe {
            let mut value: CFTypeRef = std::ptr::null();
            let err = AXUIElementCopyAttributeValue(app, attr, &mut value);
            CFRelease(attr as CFTypeRef);
            if err != 0 || value.is_null() {
                CFRelease(app as CFTypeRef);
                return;
            }
            value
        };

        unsafe {
            let windows = windows_value as CFArrayRef;
            let count = CFArrayGetCount(windows);
            for i in 0..count {
                if self.done(deadline) {
                    break;
                }
                let window = CFArrayGetValueAtIndex(windows, i) as AXUIElementRef;
                if window.is_null() {
                    continue;
                }
                let mut cg_id: u32 = 0;
                if _AXUIElementGetWindow(window, &mut cg_id) != 0 {
                    continue;
                }
                if !on_screen.contains(&cg_id) {
                    continue;
                }
                self.recurse(window, 0, deadline);
            }
            CFRelease(windows_value);
            CFRelease(app as CFTypeRef);
        }
    }

    fn collect_child_targets(&mut self, parent: AXUIElementRef, deadline: Instant) {
        let attr = make_cf_string("AXChildren");
        if attr.is_null() {
            return;
        }
        let children_value = unsafe {
            let mut value: CFTypeRef = std::ptr::null();
            let err = AXUIElementCopyAttributeValue(parent, attr, &mut value);
            CFRelease(attr as CFTypeRef);
            if err != 0 || value.is_null() {
                return;
            }
            value
        };

        unsafe {
            let children = children_value as CFArrayRef;
            let count = CFArrayGetCount(children);
            for i in 0..count {
                if self.done(deadline) {
                    break;
                }
                let child = CFArrayGetValueAtIndex(children, i) as AXUIElementRef;
                if child.is_null() {
                    continue;
                }
                self.push_if_target(child);
            }
            CFRelease(children_value);
        }
    }

    fn walk_menu_bar(&mut self, app: AXUIElementRef, attribute: &str, deadline: Instant) {
        let attr = make_cf_string(attribute);
        if attr.is_null() {
            return;
        }
        let menu_value = unsafe {
            let mut value: CFTypeRef = std::ptr::null();
            let err = AXUIElementCopyAttributeValue(app, attr, &mut value);
            CFRelease(attr as CFTypeRef);
            if err != 0 || value.is_null() {
                return;
            }
            value
        };
        self.collect_child_targets(menu_value as AXUIElementRef, deadline);
        unsafe { CFRelease(menu_value) };
    }

    fn collect_menu_bars(&mut self, deadline: Instant, front: Option<i32>) {
        if let Some(pid) = front {
            let app = unsafe { AXUIElementCreateApplication(pid) };
            if !app.is_null() {
                unsafe { AXUIElementSetMessagingTimeout(app, MESSAGING_TIMEOUT) };
                self.walk_menu_bar(app, "AXMenuBar", deadline);
                unsafe { CFRelease(app as CFTypeRef) };
            }
        }

        for pid in running_app_pids() {
            if self.done(deadline) {
                break;
            }
            let app = unsafe { AXUIElementCreateApplication(pid) };
            if app.is_null() {
                continue;
            }
            unsafe { AXUIElementSetMessagingTimeout(app, EXTRAS_MESSAGING_TIMEOUT) };
            self.walk_menu_bar(app, "AXExtrasMenuBar", deadline);
            unsafe { CFRelease(app as CFTypeRef) };
        }
    }

    fn walk_focused_window(&mut self, pid: i32, deadline: Instant) {
        let app = unsafe { AXUIElementCreateApplication(pid) };
        if app.is_null() {
            return;
        }
        unsafe {
            AXUIElementSetMessagingTimeout(app, MESSAGING_TIMEOUT);
        }

        let attr = make_cf_string("AXFocusedWindow");
        if attr.is_null() {
            unsafe { CFRelease(app as CFTypeRef) };
            return;
        }
        let window_value = unsafe {
            let mut value: CFTypeRef = std::ptr::null();
            let err = AXUIElementCopyAttributeValue(app, attr, &mut value);
            CFRelease(attr as CFTypeRef);
            if err != 0 || value.is_null() {
                CFRelease(app as CFTypeRef);
                return;
            }
            value
        };

        self.recurse(window_value as AXUIElementRef, 0, deadline);
        unsafe {
            CFRelease(window_value);
            CFRelease(app as CFTypeRef);
        }
    }

    fn walk_windows(&mut self, front: Option<i32>) {
        let windows_by_pid = on_screen_windows();
        let mut pids: Vec<i32> = windows_by_pid.keys().copied().collect();
        pids.sort_by_key(|pid| if Some(*pid) == front { 0 } else { 1 });

        let window_deadline = Instant::now() + WINDOW_BUDGET;
        for pid in &pids {
            if self.done(window_deadline) {
                break;
            }
            if let Some(ids) = windows_by_pid.get(pid) {
                self.walk_app(*pid, ids, window_deadline);
            }
        }
    }
}

fn frontmost_pid() -> Option<i32> {
    unsafe {
        let app = NSWorkspace::sharedWorkspace().frontmostApplication()?;
        Some(app.processIdentifier())
    }
}

fn running_app_pids() -> Vec<i32> {
    unsafe {
        let apps = NSWorkspace::sharedWorkspace().runningApplications();
        let count = apps.count();
        let mut pids = Vec::with_capacity(count);
        for i in 0..count {
            pids.push(apps.objectAtIndex(i).processIdentifier());
        }
        pids
    }
}

fn dedup_targets(targets: Vec<HintTarget>) -> Vec<HintTarget> {
    let mut seen: HashSet<(i64, i64)> = HashSet::new();
    let mut out = Vec::new();
    for target in targets {
        let (cx, cy) = crate::hint::geometry::center(target.frame);
        let key = (cx.round() as i64, cy.round() as i64);
        if seen.insert(key) {
            out.push(target);
        }
    }
    out
}

fn finish(walker: Walker, screen: Rect, start: Instant, kind: &str) -> Vec<HintTarget> {
    let raw_count = walker.out.len();
    let elapsed = start.elapsed();

    let usable: Vec<HintTarget> = walker
        .out
        .into_iter()
        .filter(|t| is_usable(t.frame, screen))
        .collect();
    let result = dedup_targets(usable);

    log::info!(
        "hint: collected {} {kind} targets (raw {raw_count}, elapsed={elapsed:?})",
        result.len()
    );
    result
}

pub fn collect_targets(screen: Rect) -> Vec<HintTarget> {
    let start = Instant::now();
    let front = frontmost_pid();

    let mut walker = Walker::new(is_clickable, false);
    walker.collect_menu_bars(Instant::now() + MENU_BUDGET, front);
    walker.walk_windows(front);

    finish(walker, screen, start, "clickable")
}

pub fn collect_link_targets(screen: Rect) -> Vec<HintTarget> {
    let start = Instant::now();
    let front = frontmost_pid();

    let mut walker = Walker::new(is_link, true);
    walker.walk_windows(front);

    finish(walker, screen, start, "link")
}

pub fn collect_text_targets(screen: Rect) -> Vec<HintTarget> {
    let start = Instant::now();
    let front = frontmost_pid();

    let mut walker = Walker::new(is_text, true);
    walker.walk_windows(front);

    finish(walker, screen, start, "text")
}

/// Collect both text and link elements in a single walk. Used by pluck, which
/// needs links (their text and URL) alongside static text so it can reconstruct
/// visual lines that mix the two -- e.g. a Wikipedia hatnote where "Patty" and
/// "Hamburger (disambiguation)" are links inside running prose.
pub fn collect_text_and_link_targets(screen: Rect) -> Vec<HintTarget> {
    let start = Instant::now();
    let front = frontmost_pid();

    let mut walker = Walker::new(is_text_or_link, true);
    walker.walk_windows(front);

    finish(walker, screen, start, "text+link")
}

fn is_text_or_link(element: AXUIElementRef) -> bool {
    is_text(element) || is_link(element)
}

fn enable_manual_accessibility(pid: i32) -> bool {
    let app = unsafe { AXUIElementCreateApplication(pid) };
    if app.is_null() {
        return false;
    }
    let attr = make_cf_string("AXManualAccessibility");
    if attr.is_null() {
        unsafe { CFRelease(app as CFTypeRef) };
        return false;
    }
    let value = CFBoolean::true_value();
    let err = unsafe {
        AXUIElementSetAttributeValue(app, attr, value.as_concrete_TypeRef() as CFTypeRef)
    };
    unsafe {
        CFRelease(attr as CFTypeRef);
        CFRelease(app as CFTypeRef);
    }
    err == 0
}

fn focused_window_target(pid: i32) -> Option<HintTarget> {
    let app = unsafe { AXUIElementCreateApplication(pid) };
    if app.is_null() {
        return None;
    }
    let attr = make_cf_string("AXFocusedWindow");
    if attr.is_null() {
        unsafe { CFRelease(app as CFTypeRef) };
        return None;
    }
    let window = unsafe {
        let mut value: CFTypeRef = std::ptr::null();
        let err = AXUIElementCopyAttributeValue(app, attr, &mut value);
        CFRelease(attr as CFTypeRef);
        CFRelease(app as CFTypeRef);
        if err != 0 || value.is_null() {
            return None;
        }
        value as AXUIElementRef
    };
    let frame = ax_get_frame(window);
    let target = frame.map(|frame| HintTarget {
        frame,
        element: Some(AxElement::retain(window)),
    });
    unsafe { CFRelease(window as CFTypeRef) };
    target
}

pub fn collect_scroll_targets(screen: Rect) -> Vec<HintTarget> {
    let start = Instant::now();
    let Some(front) = frontmost_pid() else {
        return Vec::new();
    };

    let mut walker = Walker::new(is_scroll_target, true);
    walker.walk_focused_window(front, Instant::now() + WINDOW_BUDGET);

    if walker.out.is_empty() && enable_manual_accessibility(front) {
        log::info!("hint: no scroll targets, enabled AXManualAccessibility and retrying");
        std::thread::sleep(ELECTRON_ENABLE_DELAY);
        walker.walk_focused_window(front, Instant::now() + WINDOW_BUDGET);
    }

    if walker.out.is_empty() {
        if let Some(target) = focused_window_target(front) {
            log::info!("hint: no scroll targets, falling back to the focused window");
            walker.out.push(target);
        }
    }

    finish(walker, screen, start, "scroll")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn classes(names: &[&str]) -> Vec<String> {
        names.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn matches_web_app_scroller_classes() {
        assert!(has_scroll_class(&classes(&["c-scrollbar__hider"])));
        assert!(has_scroll_class(&classes(&[
            "x6ikm8r",
            "notion-scroller",
            "vertical"
        ])));
        assert!(has_scroll_class(&classes(&["monaco-scrollable-element"])));
    }

    #[test]
    fn matching_ignores_case() {
        assert!(has_scroll_class(&classes(&["ScrollContainer"])));
    }

    #[test]
    fn rejects_non_scroll_classes() {
        assert!(!has_scroll_class(&classes(&[
            "p-message_pane__top_banners",
            "c-virtual_list__item"
        ])));
        assert!(!has_scroll_class(&[]));
    }
}
