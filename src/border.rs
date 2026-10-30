use std::cell::Cell;

use core_graphics::display::CGDisplay;
use objc2::rc::Retained;
use objc2::{declare_class, msg_send_id, mutability, ClassType, DeclaredClass};
use objc2_app_kit::{
    NSBackingStoreType, NSBezierPath, NSColor, NSFloatingWindowLevel, NSView, NSWindow,
    NSWindowCollectionBehavior, NSWindowStyleMask,
};
use objc2_foundation::{MainThreadMarker, NSObjectProtocol, NSPoint, NSRect, NSSize};

use crate::config::{BorderColor, BorderConfig};
use crate::types::Rect;

struct BorderState {
    color: BorderColor,
    width: f64,
    corner_radius: f64,
}

pub struct BorderViewIvars {
    state: Cell<BorderState>,
}

declare_class!(
    pub struct BorderView;

    unsafe impl ClassType for BorderView {
        type Super = NSView;
        type Mutability = mutability::MainThreadOnly;
        const NAME: &'static str = "nflowBorderView";
    }

    impl DeclaredClass for BorderView {
        type Ivars = BorderViewIvars;
    }

    unsafe impl NSObjectProtocol for BorderView {}

    unsafe impl BorderView {
        #[method(isFlipped)]
        fn is_flipped(&self) -> bool {
            false
        }

        #[method(drawRect:)]
        fn draw_rect(&self, _dirty: NSRect) {
            self.draw_border();
        }
    }
);

impl BorderView {
    fn new(mtm: MainThreadMarker, frame: NSRect, state: BorderState) -> Retained<Self> {
        let this = mtm.alloc().set_ivars(BorderViewIvars {
            state: Cell::new(state),
        });
        unsafe { msg_send_id![super(this), initWithFrame: frame] }
    }

    fn set_state(&self, state: BorderState) {
        self.ivars().state.set(state);
    }

    fn draw_border(&self) {
        let state = self.ivars().state.get();
        if state.width <= 0.0 {
            return;
        }
        let bounds = self.bounds();
        let inset = state.width / 2.0;
        let rect = NSRect::new(
            NSPoint::new(bounds.origin.x + inset, bounds.origin.y + inset),
            NSSize::new(
                (bounds.size.width - state.width).max(0.0),
                (bounds.size.height - state.width).max(0.0),
            ),
        );
        unsafe {
            let path = NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(
                rect,
                state.corner_radius,
                state.corner_radius,
            );
            path.setLineWidth(state.width);
            let color = NSColor::colorWithSRGBRed_green_blue_alpha(
                state.color.r,
                state.color.g,
                state.color.b,
                state.color.a,
            );
            color.set();
            path.stroke();
        }
    }
}

impl Copy for BorderState {}
impl Clone for BorderState {
    fn clone(&self) -> Self {
        *self
    }
}

pub struct BorderOverlay {
    window: Retained<NSWindow>,
    view: Retained<BorderView>,
    config: BorderConfig,
    current_frame: Option<Rect>,
}

impl BorderOverlay {
    pub fn new(config: BorderConfig) -> Self {
        let mtm = MainThreadMarker::new().expect("border overlay must be built on the main thread");
        let frame = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(10.0, 10.0));
        let view = BorderView::new(
            mtm,
            frame,
            BorderState {
                color: config.active_color,
                width: config.width,
                corner_radius: config.corner_radius,
            },
        );
        let window = unsafe {
            NSWindow::initWithContentRect_styleMask_backing_defer(
                mtm.alloc(),
                frame,
                NSWindowStyleMask::Borderless,
                NSBackingStoreType::NSBackingStoreBuffered,
                false,
            )
        };
        unsafe {
            window.setOpaque(false);
            window.setBackgroundColor(Some(&NSColor::clearColor()));
            window.setIgnoresMouseEvents(true);
            window.setLevel(NSFloatingWindowLevel);
            window.setCollectionBehavior(NSWindowCollectionBehavior::CanJoinAllSpaces);
            window.setContentView(Some(&view));
            window.setHasShadow(false);
            window.setAlphaValue(1.0);
        }
        Self {
            window,
            view,
            config,
            current_frame: None,
        }
    }

    pub fn set_config(&mut self, config: BorderConfig) {
        self.config = config;
        self.view.set_state(BorderState {
            color: config.active_color,
            width: config.width,
            corner_radius: config.corner_radius,
        });
        if !config.enabled {
            self.hide();
        } else if let Some(frame) = self.current_frame {
            self.show_at(frame);
        }
        unsafe { self.view.setNeedsDisplay(true) };
    }

    pub fn hide(&mut self) {
        self.current_frame = None;
        self.window.orderOut(None);
    }

    pub fn update(&mut self, frame: Option<Rect>) {
        if !self.config.enabled {
            if self.current_frame.is_some() {
                self.hide();
            }
            return;
        }
        match frame {
            None => {
                if self.current_frame.is_some() {
                    self.hide();
                }
            }
            Some(f) => self.show_at(f),
        }
    }

    fn show_at(&mut self, frame: Rect) {
        let pad = self.config.width;
        let total_h = CGDisplay::main().bounds().size.height;
        let outer_x = frame.x - pad;
        let outer_w = frame.width + pad * 2.0;
        let outer_h = frame.height + pad * 2.0;
        let outer_cg_y = frame.y - pad;
        let outer_ns_y = total_h - outer_cg_y - outer_h;
        let ns_frame = NSRect::new(
            NSPoint::new(outer_x, outer_ns_y),
            NSSize::new(outer_w, outer_h),
        );
        unsafe {
            self.window.setFrame_display(ns_frame, true);
            self.view.setNeedsDisplay(true);
            self.window.orderFrontRegardless();
        }
        self.current_frame = Some(frame);
    }
}
