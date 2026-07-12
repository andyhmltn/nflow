use std::sync::{mpsc, Mutex};

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Bool, ProtocolObject};
use objc2::{declare_class, msg_send_id, mutability, sel, ClassType, DeclaredClass};
use objc2_app_kit::{
    NSApplication, NSApplicationActivationPolicy, NSBezierPath, NSColor, NSControlStateValueOff,
    NSControlStateValueOn, NSEventModifierFlags, NSImage, NSMenu, NSMenuDelegate, NSMenuItem,
    NSStatusBar, NSStatusItem,
};
use objc2_foundation::{
    ns_string, MainThreadMarker, NSObject, NSObjectProtocol, NSPoint, NSRect, NSSize, NSString,
};

use crate::types::Command;

#[derive(Default, Clone)]
struct MenuState {
    total_windows: usize,
    space_count: usize,
    active_space: usize,
    active_scene: usize,
    scenes: Vec<(usize, String)>,
}

static MENU_STATE: Mutex<Option<MenuState>> = Mutex::new(None);
static SHORTCUTS: Mutex<Vec<MenuShortcutEntry>> = Mutex::new(Vec::new());

#[derive(Debug, Clone, PartialEq)]
pub struct MenuShortcutEntry {
    pub title: String,
    pub pattern: String,
    pub command: Command,
}

pub fn update_shortcuts(shortcuts: Vec<MenuShortcutEntry>) {
    *SHORTCUTS.lock().unwrap() = shortcuts;
}

pub fn update_menu_state(
    total_windows: usize,
    space_count: usize,
    active_space: usize,
    active_scene: usize,
    scenes: Vec<(usize, String)>,
) {
    let mut state = MENU_STATE.lock().unwrap();
    *state = Some(MenuState {
        total_windows,
        space_count,
        active_space,
        active_scene,
        scenes,
    });
}

pub struct Ivars {
    tx: mpsc::Sender<Command>,
}

declare_class!(
    pub struct nflowStatusController;

    unsafe impl ClassType for nflowStatusController {
        type Super = NSObject;
        type Mutability = mutability::MainThreadOnly;
        const NAME: &'static str = "nflowStatusController";
    }

    impl DeclaredClass for nflowStatusController {
        type Ivars = Ivars;
    }

    unsafe impl NSObjectProtocol for nflowStatusController {}

    unsafe impl nflowStatusController {
        #[method(applyScene:)]
        fn apply_scene(&self, sender: &NSMenuItem) {
            let tag = unsafe { sender.tag() };
            if tag >= 0 {
                let _ = self.ivars().tx.send(Command::ApplyScene(tag as usize));
            }
        }

        #[method(openConfig:)]
        fn open_config(&self, _sender: &NSMenuItem) {
            let _ = self.ivars().tx.send(Command::OpenConfig);
        }

        #[method(runShortcut:)]
        fn run_shortcut(&self, sender: &NSMenuItem) {
            let tag = unsafe { sender.tag() };
            if tag < 0 {
                return;
            }
            let command = SHORTCUTS
                .lock()
                .unwrap()
                .get(tag as usize)
                .map(|entry| entry.command.clone());
            if let Some(command) = command {
                let _ = self.ivars().tx.send(command);
            }
        }

        #[method(quit:)]
        fn quit(&self, _sender: &NSMenuItem) {
            let mtm = MainThreadMarker::from(self);
            let app = NSApplication::sharedApplication(mtm);
            unsafe { app.terminate(None) };
        }
    }

    unsafe impl NSMenuDelegate for nflowStatusController {
        #[method(menuNeedsUpdate:)]
        unsafe fn menu_needs_update(&self, menu: &NSMenu) {
            self.rebuild_menu(menu);
        }
    }
);

impl nflowStatusController {
    fn new(mtm: MainThreadMarker, tx: mpsc::Sender<Command>) -> Retained<Self> {
        let this = mtm.alloc().set_ivars(Ivars { tx });
        unsafe { msg_send_id![super(this), init] }
    }

    fn rebuild_menu(&self, menu: &NSMenu) {
        let mtm = MainThreadMarker::from(self);
        unsafe { menu.removeAllItems() };

        let state = MENU_STATE.lock().unwrap().clone().unwrap_or_default();

        add_label(
            menu,
            mtm,
            &format!(
                "Apps: {} across {} spaces",
                state.total_windows, state.space_count
            ),
        );
        add_label(menu, mtm, &format!("Active space: {}", state.active_space));

        menu.addItem(&NSMenuItem::separatorItem(mtm));

        let target: &AnyObject = self;
        for (number, label) in &state.scenes {
            let item = unsafe {
                NSMenuItem::initWithTitle_action_keyEquivalent(
                    mtm.alloc(),
                    &NSString::from_str(label),
                    Some(sel!(applyScene:)),
                    ns_string!(""),
                )
            };
            unsafe {
                item.setTarget(Some(target));
                item.setTag(*number as isize);
                let state_value = if *number == state.active_scene {
                    NSControlStateValueOn
                } else {
                    NSControlStateValueOff
                };
                item.setState(state_value);
            }
            menu.addItem(&item);
        }

        let shortcuts = SHORTCUTS.lock().unwrap().clone();
        if !shortcuts.is_empty() {
            menu.addItem(&NSMenuItem::separatorItem(mtm));
            add_label(menu, mtm, "Accessibility");
            for (index, entry) in shortcuts.iter().enumerate() {
                add_shortcut_item(menu, mtm, target, entry, index);
            }
        }

        menu.addItem(&NSMenuItem::separatorItem(mtm));
        add_action(menu, mtm, target, "Open config", sel!(openConfig:), "");
        add_action(menu, mtm, target, "Quit nflow", sel!(quit:), "q");
    }
}

fn add_label(menu: &NSMenu, mtm: MainThreadMarker, title: &str) {
    let item = unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(
            mtm.alloc(),
            &NSString::from_str(title),
            None,
            ns_string!(""),
        )
    };
    unsafe { item.setEnabled(false) };
    menu.addItem(&item);
}

fn add_shortcut_item(
    menu: &NSMenu,
    mtm: MainThreadMarker,
    target: &AnyObject,
    entry: &MenuShortcutEntry,
    index: usize,
) {
    let (key, mask) = match crate::hotkey::menu_shortcut(&entry.pattern) {
        Some(shortcut) => {
            let key = if shortcut.key == "space" {
                " ".to_string()
            } else {
                shortcut.key.clone()
            };
            let mut mask = NSEventModifierFlags::empty();
            if shortcut.command {
                mask |= NSEventModifierFlags::NSEventModifierFlagCommand;
            }
            if shortcut.option {
                mask |= NSEventModifierFlags::NSEventModifierFlagOption;
            }
            if shortcut.control {
                mask |= NSEventModifierFlags::NSEventModifierFlagControl;
            }
            if shortcut.shift {
                mask |= NSEventModifierFlags::NSEventModifierFlagShift;
            }
            (key, mask)
        }
        None => (String::new(), NSEventModifierFlags::empty()),
    };
    let item = unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(
            mtm.alloc(),
            &NSString::from_str(&entry.title),
            Some(sel!(runShortcut:)),
            &NSString::from_str(&key),
        )
    };
    unsafe {
        item.setKeyEquivalentModifierMask(mask);
        item.setTarget(Some(target));
        item.setTag(index as isize);
    }
    menu.addItem(&item);
}

fn add_action(
    menu: &NSMenu,
    mtm: MainThreadMarker,
    target: &AnyObject,
    title: &str,
    action: objc2::runtime::Sel,
    key: &str,
) {
    let item = unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(
            mtm.alloc(),
            &NSString::from_str(title),
            Some(action),
            &NSString::from_str(key),
        )
    };
    unsafe { item.setTarget(Some(target)) };
    menu.addItem(&item);
}

fn draw_glyph() {
    let springline = 9.2;
    let baseline = 2.5;
    let outer = NSPoint::new(9.0, springline);
    let outer_radius = 5.8;
    let inner_radius = 2.8;

    unsafe {
        let path = NSBezierPath::bezierPath();

        path.moveToPoint(NSPoint::new(9.0 - outer_radius, baseline));
        path.lineToPoint(NSPoint::new(9.0 - outer_radius, springline));
        path.appendBezierPathWithArcWithCenter_radius_startAngle_endAngle_clockwise(
            outer,
            outer_radius,
            180.0,
            0.0,
            true,
        );
        path.lineToPoint(NSPoint::new(9.0 + outer_radius, baseline));
        path.lineToPoint(NSPoint::new(9.0 + inner_radius, baseline));
        path.lineToPoint(NSPoint::new(9.0 + inner_radius, springline));
        path.appendBezierPathWithArcWithCenter_radius_startAngle_endAngle_clockwise(
            outer,
            inner_radius,
            0.0,
            180.0,
            false,
        );
        path.lineToPoint(NSPoint::new(9.0 - inner_radius, baseline));
        path.closePath();

        NSColor::blackColor().set();
        path.fill();
    }
}

fn make_icon() -> Retained<NSImage> {
    let handler = RcBlock::new(|_rect: NSRect| {
        draw_glyph();
        Bool::YES
    });
    let image = unsafe {
        NSImage::imageWithSize_flipped_drawingHandler(NSSize::new(18.0, 18.0), false, &handler)
    };
    unsafe { image.setTemplate(true) };
    image
}

pub struct StatusBarHandle {
    _status_item: Retained<NSStatusItem>,
    _controller: Retained<nflowStatusController>,
    _menu: Retained<NSMenu>,
}

pub fn install(mtm: MainThreadMarker, tx: mpsc::Sender<Command>) -> StatusBarHandle {
    let app = NSApplication::sharedApplication(mtm);
    app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);

    let controller = nflowStatusController::new(mtm, tx);

    let status_bar = unsafe { NSStatusBar::systemStatusBar() };
    let status_item = unsafe { status_bar.statusItemWithLength(-1.0) };
    if let Some(button) = unsafe { status_item.button(mtm) } {
        unsafe { button.setImage(Some(&make_icon())) };
    }

    let menu = NSMenu::new(mtm);
    let delegate = ProtocolObject::from_ref(&*controller);
    unsafe {
        menu.setDelegate(Some(delegate));
        status_item.setMenu(Some(&menu));
    }

    StatusBarHandle {
        _status_item: status_item,
        _controller: controller,
        _menu: menu,
    }
}
