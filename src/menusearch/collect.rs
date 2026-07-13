//! Collects the leaf menu items of the frontmost application's menu bar.
//!
//! macOS exposes the menu bar through the Accessibility API as a tree rooted at
//! the application's `AXMenuBar`. Top-level entries (Apple, File, Edit, ...)
//! are `AXMenuItem` elements whose `AXChildren` wrap a single `AXMenu`; that
//! menu's children are the leaf commands (`Save`, `Open`, ...) or further
//! submenus. We recurse depth-first, retaining every pressable, enabled leaf
//! and recording its breadcrumb path so the palette can render
//! "File > Save As…".

use std::time::{Duration, Instant};

use objc2_app_kit::NSWorkspace;

use crate::hint::collect::AxElement;

const COLLECT_BUDGET: Duration = Duration::from_millis(400);
const MAX_ITEMS: usize = 2000;
const MAX_DEPTH: u32 = 32;

#[allow(dead_code)]
pub struct MenuItem {
    pub title: String,
    /// Breadcrumb of submenu titles leading to this item, e.g. `["File"]`.
    pub path: Vec<String>,
    /// `path > title`, precomputed for matching and rendering.
    pub display: String,
    pub element: AxElement,
    pub enabled: bool,
}

pub fn collect_menu_items() -> Vec<MenuItem> {
    let Some(pid) = frontmost_pid() else {
        return Vec::new();
    };
    let app = match crate::hint::collect::make_app_element(pid) {
        Some(a) => a,
        None => return Vec::new(),
    };

    // The menu bar lives on the app element's `AXMenuBar` attribute. Its
    // children are the top-level menus (Apple, File, Edit, ...).
    let menu_bar = match app.attr_element("AXMenuBar") {
        Some(m) => m,
        None => return Vec::new(),
    };

    let deadline = Instant::now() + COLLECT_BUDGET;
    let mut out = Vec::new();
    walk(&menu_bar, &[], &mut out, 0, deadline);
    out
}

fn frontmost_pid() -> Option<i32> {
    unsafe {
        let app = NSWorkspace::sharedWorkspace().frontmostApplication()?;
        Some(app.processIdentifier())
    }
}

fn walk(
    element: &AxElement,
    path: &[String],
    out: &mut Vec<MenuItem>,
    depth: u32,
    deadline: Instant,
) {
    if depth > MAX_DEPTH || out.len() >= MAX_ITEMS || Instant::now() >= deadline {
        return;
    }
    for child in element.children() {
        if out.len() >= MAX_ITEMS || Instant::now() >= deadline {
            break;
        }
        let title = child
            .string_attr("AXTitle")
            .map(|t| t.trim().to_string())
            .unwrap_or_default();

        // Separator items and the like have no title and no press action.
        let sub = child.children();
        if sub.is_empty() {
            if title.is_empty() || !child.has_action("AXPress") {
                continue;
            }
            let enabled = child.bool_attr("AXEnabled").unwrap_or(true);
            let display = if path.is_empty() {
                title.clone()
            } else {
                format!("{} > {}", path.join(" > "), title)
            };
            out.push(MenuItem {
                title,
                path: path.to_vec(),
                display,
                element: child,
                enabled,
            });
        } else {
            let mut next_path = path.to_vec();
            if !title.is_empty() {
                next_path.push(title);
            }
            walk(&child, &next_path, out, depth + 1, deadline);
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn display_joins_path_and_title() {
        let path = vec!["File".to_string()];
        let title = "Save".to_string();
        let display = if path.is_empty() {
            title
        } else {
            format!("{} > {}", path.join(" > "), title)
        };
        assert_eq!(display, "File > Save");
    }
}
