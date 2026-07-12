use std::collections::HashSet;

use core_foundation::base::{CFType, TCFType};
use core_foundation::dictionary::CFDictionary;
use core_foundation::number::CFNumber;
use core_foundation::string::CFString;
use core_graphics::window::{
    copy_window_info, kCGNullWindowID, kCGWindowListExcludeDesktopElements,
    kCGWindowListOptionOnScreenOnly,
};

use crate::types::WindowId;

const MIN_WINDOW_SIZE: f64 = 100.0;
const NORMAL_WINDOW_LAYER: i32 = 0;

pub struct DiscoveredWindow {
    pub window_id: WindowId,
    pub pid: i32,
    pub app_name: String,
    pub width: f64,
    pub height: f64,
}

struct WindowKeys {
    layer: CFString,
    number: CFString,
    pid: CFString,
    name: CFString,
    bounds: CFString,
    width: CFString,
    height: CFString,
}

impl WindowKeys {
    fn new() -> Self {
        Self {
            layer: CFString::from_static_string("kCGWindowLayer"),
            number: CFString::from_static_string("kCGWindowNumber"),
            pid: CFString::from_static_string("kCGWindowOwnerPID"),
            name: CFString::from_static_string("kCGWindowOwnerName"),
            bounds: CFString::from_static_string("kCGWindowBounds"),
            width: CFString::from_static_string("Width"),
            height: CFString::from_static_string("Height"),
        }
    }
}

fn extract_window(
    dict: &CFDictionary<CFString, CFType>,
    keys: &WindowKeys,
) -> Option<DiscoveredWindow> {
    let layer = dict
        .find(&keys.layer)
        .as_deref()
        .and_then(|v| v.downcast::<CFNumber>())
        .and_then(|n| n.to_i32())?;
    if layer != NORMAL_WINDOW_LAYER {
        return None;
    }

    let bounds_value = dict.find(&keys.bounds)?;
    let bounds_dict: CFDictionary<CFString, CFType> =
        unsafe { CFDictionary::wrap_under_get_rule(bounds_value.as_CFTypeRef() as *const _) };
    let width = bounds_dict
        .find(&keys.width)
        .as_deref()
        .and_then(|v| v.downcast::<CFNumber>())
        .and_then(|n| n.to_f64())
        .unwrap_or(0.0);
    let height = bounds_dict
        .find(&keys.height)
        .as_deref()
        .and_then(|v| v.downcast::<CFNumber>())
        .and_then(|n| n.to_f64())
        .unwrap_or(0.0);
    if width < MIN_WINDOW_SIZE || height < MIN_WINDOW_SIZE {
        return None;
    }

    let window_id = dict
        .find(&keys.number)
        .as_deref()
        .and_then(|v| v.downcast::<CFNumber>())
        .and_then(|n| n.to_i32())
        .map(|v| v as WindowId)?;

    let pid = dict
        .find(&keys.pid)
        .as_deref()
        .and_then(|v| v.downcast::<CFNumber>())
        .and_then(|n| n.to_i32())?;

    let app_name = dict
        .find(&keys.name)
        .as_deref()
        .and_then(|v| v.downcast::<CFString>())
        .map(|s| s.to_string())?;

    Some(DiscoveredWindow {
        window_id,
        pid,
        app_name,
        width,
        height,
    })
}

fn iter_window_dicts<F: FnMut(&CFDictionary<CFString, CFType>)>(mut f: F) {
    let options = kCGWindowListOptionOnScreenOnly | kCGWindowListExcludeDesktopElements;
    let Some(array) = copy_window_info(options, kCGNullWindowID) else {
        return;
    };
    for i in 0..array.len() {
        let item_ptr = unsafe { *array.get_unchecked(i) };
        let dict: CFDictionary<CFString, CFType> =
            unsafe { CFDictionary::wrap_under_get_rule(item_ptr as _) };
        f(&dict);
    }
}

pub fn discover_windows() -> Vec<DiscoveredWindow> {
    let keys = WindowKeys::new();
    let mut results = Vec::new();
    let mut seen_pids: HashSet<i32> = HashSet::new();

    iter_window_dicts(|dict| {
        let Some(window) = extract_window(dict, &keys) else {
            return;
        };
        if seen_pids.insert(window.pid) {
            log::debug!(
                "discovered: {} (pid={}, wid={}, {}x{})",
                window.app_name,
                window.pid,
                window.window_id,
                window.width,
                window.height,
            );
            results.push(window);
        }
    });

    results
}

fn pid_is_running(pid: i32) -> bool {
    unsafe { libc::kill(pid, 0) == 0 }
}

pub struct WindowWatcher {
    known_pids: HashSet<i32>,
    ignored_apps: HashSet<String>,
}

impl WindowWatcher {
    pub fn new(ignored_apps: Vec<String>) -> Self {
        Self {
            known_pids: HashSet::new(),
            ignored_apps: ignored_apps.into_iter().collect(),
        }
    }

    pub fn set_ignored_apps(&mut self, ignored_apps: Vec<String>) {
        self.ignored_apps = ignored_apps.into_iter().collect();
    }

    pub fn poll(&mut self) -> (Vec<DiscoveredWindow>, Vec<i32>) {
        let windows = discover_windows();

        let new_windows: Vec<DiscoveredWindow> = windows
            .into_iter()
            .filter(|w| !self.ignored_apps.contains(&w.app_name))
            .filter(|w| !self.known_pids.contains(&w.pid))
            .collect();

        for w in &new_windows {
            self.known_pids.insert(w.pid);
        }

        let gone_pids: Vec<i32> = self
            .known_pids
            .iter()
            .filter(|&&pid| !pid_is_running(pid))
            .copied()
            .collect();

        for &pid in &gone_pids {
            self.known_pids.remove(&pid);
        }

        (new_windows, gone_pids)
    }
}
