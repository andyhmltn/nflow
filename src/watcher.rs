use std::collections::{HashMap, HashSet};

use core_foundation::base::{CFType, TCFType};
use core_foundation::dictionary::CFDictionary;
use core_foundation::number::CFNumber;
use core_foundation::string::CFString;
use core_graphics::window::{
    copy_window_info, kCGNullWindowID, kCGWindowListExcludeDesktopElements, kCGWindowListOptionAll,
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

fn iter_window_dicts<F: FnMut(&CFDictionary<CFString, CFType>)>(options: u32, mut f: F) {
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
    let mut windows_by_pid: HashMap<i32, DiscoveredWindow> = HashMap::new();

    iter_window_dicts(
        kCGWindowListOptionAll | kCGWindowListExcludeDesktopElements,
        |dict| {
            let Some(window) = extract_window(dict, &keys) else {
                return;
            };
            let area = window.width * window.height;
            let replace = windows_by_pid
                .get(&window.pid)
                .is_none_or(|current| area > current.width * current.height);
            if replace {
                windows_by_pid.insert(window.pid, window);
            }
        },
    );

    let mut results: Vec<DiscoveredWindow> = windows_by_pid.into_values().collect();
    results.sort_by_key(|window| window.pid);
    for window in &results {
        log::debug!(
            "discovered: {} (pid={}, wid={}, {}x{})",
            window.app_name,
            window.pid,
            window.window_id,
            window.width,
            window.height,
        );
    }
    results
}

pub fn frontmost_window() -> Option<DiscoveredWindow> {
    let keys = WindowKeys::new();
    let mut frontmost = None;

    iter_window_dicts(
        kCGWindowListOptionOnScreenOnly | kCGWindowListExcludeDesktopElements,
        |dict| {
            if frontmost.is_none() {
                frontmost = extract_window(dict, &keys);
            }
        },
    );

    frontmost
}

fn visible_window_pids() -> HashSet<i32> {
    let keys = WindowKeys::new();
    let mut pids = HashSet::new();
    iter_window_dicts(
        kCGWindowListOptionOnScreenOnly | kCGWindowListExcludeDesktopElements,
        |dict| {
            if let Some(window) = extract_window(dict, &keys) {
                pids.insert(window.pid);
            }
        },
    );
    pids
}

pub struct WindowWatcher {
    known_windows: HashMap<i32, WindowId>,
    ignored_apps: HashSet<String>,
    configured_apps: HashSet<String>,
}

impl WindowWatcher {
    pub fn new(ignored_apps: Vec<String>, configured_apps: Vec<String>) -> Self {
        Self {
            known_windows: HashMap::new(),
            ignored_apps: ignored_apps.into_iter().collect(),
            configured_apps: configured_apps.into_iter().collect(),
        }
    }

    pub fn set_ignored_apps(&mut self, ignored_apps: Vec<String>) {
        self.ignored_apps = ignored_apps.into_iter().collect();
    }

    pub fn set_configured_apps(&mut self, configured_apps: Vec<String>) {
        self.configured_apps = configured_apps.into_iter().collect();
    }

    pub fn poll(&mut self) -> (Vec<DiscoveredWindow>, Vec<i32>) {
        self.reconcile(discover_windows(), &visible_window_pids())
    }

    fn reconcile(
        &mut self,
        windows: Vec<DiscoveredWindow>,
        visible_pids: &HashSet<i32>,
    ) -> (Vec<DiscoveredWindow>, Vec<i32>) {
        let is_managed = |window: &DiscoveredWindow| {
            !self.ignored_apps.contains(&window.app_name)
                && (self.known_windows.contains_key(&window.pid)
                    || self.configured_apps.contains(&window.app_name)
                    || visible_pids.contains(&window.pid))
        };
        let current_windows: HashMap<i32, WindowId> = windows
            .iter()
            .filter(|window| is_managed(window))
            .map(|window| (window.pid, window.window_id))
            .collect();

        let gone_pids: Vec<i32> = self
            .known_windows
            .iter()
            .filter(|(pid, window_id)| current_windows.get(pid) != Some(window_id))
            .map(|(pid, _)| *pid)
            .collect();

        let new_windows: Vec<DiscoveredWindow> = windows
            .into_iter()
            .filter(is_managed)
            .filter(|window| self.known_windows.get(&window.pid) != Some(&window.window_id))
            .collect();

        self.known_windows = current_windows;

        (new_windows, gone_pids)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn window(window_id: WindowId, pid: i32, app_name: &str) -> DiscoveredWindow {
        DiscoveredWindow {
            window_id,
            pid,
            app_name: app_name.to_string(),
            width: 1000.0,
            height: 800.0,
        }
    }

    #[test]
    fn stable_inventory_is_not_reported_twice() {
        let mut watcher = WindowWatcher::new(Vec::new(), vec!["One".to_string()]);
        let visible = HashSet::new();
        let (new, gone) = watcher.reconcile(vec![window(10, 1, "One")], &visible);
        assert_eq!(new.len(), 1);
        assert!(gone.is_empty());

        let (new, gone) = watcher.reconcile(vec![window(10, 1, "One")], &visible);
        assert!(new.is_empty());
        assert!(gone.is_empty());
    }

    #[test]
    fn replaced_window_is_removed_and_registered_atomically() {
        let mut watcher = WindowWatcher::new(Vec::new(), vec!["One".to_string()]);
        let visible = HashSet::new();
        watcher.reconcile(vec![window(10, 1, "One")], &visible);

        let (new, gone) = watcher.reconcile(vec![window(11, 1, "One")], &visible);
        assert_eq!(gone, vec![1]);
        assert_eq!(new.len(), 1);
        assert_eq!(new[0].window_id, 11);
    }

    #[test]
    fn closed_window_is_removed_even_when_process_identity_is_unchanged() {
        let mut watcher = WindowWatcher::new(Vec::new(), vec!["One".to_string()]);
        let visible = HashSet::new();
        watcher.reconcile(vec![window(10, 1, "One")], &visible);

        let (new, gone) = watcher.reconcile(Vec::new(), &visible);
        assert!(new.is_empty());
        assert_eq!(gone, vec![1]);
    }

    #[test]
    fn hidden_unconfigured_app_is_deferred_until_first_visible() {
        let mut watcher = WindowWatcher::new(Vec::new(), Vec::new());
        let mut visible = HashSet::new();
        let (new, gone) = watcher.reconcile(vec![window(10, 1, "One")], &visible);
        assert!(new.is_empty());
        assert!(gone.is_empty());

        visible.insert(1);
        let (new, gone) = watcher.reconcile(vec![window(10, 1, "One")], &visible);
        assert_eq!(new.len(), 1);
        assert!(gone.is_empty());

        visible.clear();
        let (new, gone) = watcher.reconcile(vec![window(10, 1, "One")], &visible);
        assert!(new.is_empty());
        assert!(gone.is_empty());
    }
}
