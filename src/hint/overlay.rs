use std::cell::RefCell;

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObject};
use objc2::{declare_class, msg_send_id, mutability, ClassType, DeclaredClass};
use objc2_app_kit::{
    NSBackingStoreType, NSBezierPath, NSColor, NSFont, NSFontAttributeName,
    NSForegroundColorAttributeName, NSScreen, NSShadow, NSStatusWindowLevel, NSStringDrawing,
    NSView, NSWindow, NSWindowCollectionBehavior, NSWindowStyleMask,
};
use objc2_foundation::{
    MainThreadMarker, NSDictionary, NSObjectProtocol, NSPoint, NSRect, NSSize, NSString,
};

pub struct HintBadge {
    pub label: String,
    pub x: f64,
    pub y: f64,
}

pub struct HighlightRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

struct ViewState {
    badges: Vec<HintBadge>,
    typed: String,
    highlights: Vec<HighlightRect>,
    font_size: f64,
}

const DEFAULT_FONT_SIZE: f64 = 13.0;

fn to_any<T: ClassType<Super = NSObject> + 'static>(value: Retained<T>) -> Retained<AnyObject> {
    let object = Retained::into_super(value);
    Retained::into_super(object)
}

pub struct OverlayViewIvars {
    state: RefCell<ViewState>,
}

declare_class!(
    pub struct OverlayView;

    unsafe impl ClassType for OverlayView {
        type Super = NSView;
        type Mutability = mutability::MainThreadOnly;
        const NAME: &'static str = "nflowOverlayView";
    }

    impl DeclaredClass for OverlayView {
        type Ivars = OverlayViewIvars;
    }

    unsafe impl NSObjectProtocol for OverlayView {}

    unsafe impl OverlayView {
        #[method(isFlipped)]
        fn is_flipped(&self) -> bool {
            false
        }

        #[method(drawRect:)]
        fn draw_rect(&self, _dirty: NSRect) {
            self.draw_badges();
        }
    }
);

impl OverlayView {
    fn new(
        mtm: MainThreadMarker,
        frame: NSRect,
        badges: Vec<HintBadge>,
        font_size: f64,
    ) -> Retained<Self> {
        let this = mtm.alloc().set_ivars(OverlayViewIvars {
            state: RefCell::new(ViewState {
                badges,
                typed: String::new(),
                highlights: Vec::new(),
                font_size,
            }),
        });
        unsafe { msg_send_id![super(this), initWithFrame: frame] }
    }

    fn set_typed(&self, typed: &str) {
        self.ivars().state.borrow_mut().typed = typed.to_string();
    }

    fn set_badges(&self, badges: Vec<HintBadge>) {
        let mut state = self.ivars().state.borrow_mut();
        state.badges = badges;
        state.typed = String::new();
    }

    fn set_highlights(&self, highlights: Vec<HighlightRect>) {
        self.ivars().state.borrow_mut().highlights = highlights;
    }

    fn draw_badges(&self) {
        self.draw_highlights();
        let state = self.ivars().state.borrow();

        let font = unsafe { NSFont::boldSystemFontOfSize(state.font_size) };
        let bright = unsafe { NSColor::colorWithSRGBRed_green_blue_alpha(0.96, 0.97, 1.0, 1.0) };
        let dim = unsafe { NSColor::colorWithSRGBRed_green_blue_alpha(0.45, 0.48, 0.55, 1.0) };
        let bg = unsafe { NSColor::colorWithSRGBRed_green_blue_alpha(0.06, 0.07, 0.10, 0.92) };
        let border = unsafe { NSColor::colorWithSRGBRed_green_blue_alpha(0.40, 0.80, 1.0, 0.55) };

        let keys = [unsafe { NSFontAttributeName }, unsafe {
            NSForegroundColorAttributeName
        }];
        let bright_attrs =
            NSDictionary::from_vec(&keys, vec![to_any(font.clone()), to_any(bright.clone())]);
        let dim_attrs =
            NSDictionary::from_vec(&keys, vec![to_any(font.clone()), to_any(dim.clone())]);

        let padding_x = 6.0;
        let padding_y = 3.0;
        let corner = 5.0;

        let shadow = unsafe { NSShadow::new() };
        let shadow_color =
            unsafe { NSColor::colorWithSRGBRed_green_blue_alpha(0.0, 0.0, 0.0, 0.55) };
        unsafe {
            shadow.setShadowOffset(NSSize::new(0.0, -1.0));
            shadow.setShadowBlurRadius(3.0);
            shadow.setShadowColor(Some(&shadow_color));
        }

        for badge in &state.badges {
            if !badge.label.starts_with(&state.typed) {
                continue;
            }

            let label = NSString::from_str(&badge.label);
            let text_size = unsafe { label.sizeWithAttributes(Some(&bright_attrs)) };
            let width = text_size.width + padding_x * 2.0;
            let height = text_size.height + padding_y * 2.0;
            let rect = NSRect::new(NSPoint::new(badge.x, badge.y), NSSize::new(width, height));

            unsafe {
                shadow.set();
                let path =
                    NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(rect, corner, corner);
                bg.set();
                path.fill();

                NSShadow::new().set();
                path.setLineWidth(1.0);
                border.set();
                path.stroke();

                let typed_count = state.typed.chars().count();
                let (prefix, suffix) = split_at_chars(&badge.label, typed_count);
                let origin = NSPoint::new(badge.x + padding_x, badge.y + padding_y);
                if prefix.is_empty() {
                    label.drawAtPoint_withAttributes(origin, Some(&bright_attrs));
                } else {
                    let prefix_ns = NSString::from_str(prefix);
                    prefix_ns.drawAtPoint_withAttributes(origin, Some(&dim_attrs));
                    let prefix_width = prefix_ns.sizeWithAttributes(Some(&dim_attrs)).width;
                    let suffix_origin = NSPoint::new(origin.x + prefix_width, origin.y);
                    NSString::from_str(suffix)
                        .drawAtPoint_withAttributes(suffix_origin, Some(&bright_attrs));
                }
            }
        }
    }
}

impl OverlayView {
    fn draw_highlights(&self) {
        let state = self.ivars().state.borrow();
        if state.highlights.is_empty() {
            return;
        }
        let fill = unsafe { NSColor::colorWithSRGBRed_green_blue_alpha(0.30, 0.58, 1.0, 0.38) };
        let border = unsafe { NSColor::colorWithSRGBRed_green_blue_alpha(0.45, 0.70, 1.0, 0.85) };
        for hl in &state.highlights {
            let rect = NSRect::new(NSPoint::new(hl.x, hl.y), NSSize::new(hl.width, hl.height));
            unsafe {
                let path = NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(rect, 2.0, 2.0);
                fill.set();
                path.fill();
                path.setLineWidth(1.0);
                border.set();
                path.stroke();
            }
        }
    }
}

fn split_at_chars(s: &str, n: usize) -> (&str, &str) {
    match s.char_indices().nth(n) {
        Some((idx, _)) => s.split_at(idx),
        None => (s, ""),
    }
}

pub struct Overlay {
    window: Retained<NSWindow>,
    view: Retained<OverlayView>,
}

impl Overlay {
    pub fn show(badges: Vec<HintBadge>) -> Overlay {
        Overlay::build(badges, DEFAULT_FONT_SIZE, 1.0)
    }

    pub fn show_toast(badge: HintBadge, font_size: f64) -> Overlay {
        Overlay::build(vec![badge], font_size, 0.0)
    }

    fn build(badges: Vec<HintBadge>, font_size: f64, alpha: f64) -> Overlay {
        let mtm = MainThreadMarker::new().expect("overlay must be built on the main thread");

        let frame = match NSScreen::mainScreen(mtm) {
            Some(screen) => screen.frame(),
            None => NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(1440.0, 900.0)),
        };

        let view = OverlayView::new(mtm, frame, badges, font_size);

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
            window.setLevel(NSStatusWindowLevel);
            window.setCollectionBehavior(NSWindowCollectionBehavior::CanJoinAllSpaces);
            window.setContentView(Some(&view));
            window.setAlphaValue(alpha);
            window.orderFrontRegardless();
        }

        Overlay { window, view }
    }

    pub fn origin(&self) -> (f64, f64) {
        let frame = self.window.frame();
        (frame.origin.x, frame.origin.y)
    }

    pub fn set_alpha(&self, alpha: f64) {
        unsafe { self.window.setAlphaValue(alpha) };
    }

    pub fn set_frame_origin(&self, x: f64, y: f64) {
        unsafe { self.window.setFrameOrigin(NSPoint::new(x, y)) };
    }

    pub fn set_visible_labels(&self, typed: &str) {
        self.view.set_typed(typed);
        unsafe { self.view.setNeedsDisplay(true) };
    }

    pub fn set_badges(&self, badges: Vec<HintBadge>) {
        self.view.set_badges(badges);
        unsafe { self.view.setNeedsDisplay(true) };
    }

    pub fn set_highlights(&self, highlights: Vec<HighlightRect>) {
        self.view.set_highlights(highlights);
        unsafe { self.view.setNeedsDisplay(true) };
    }

    pub fn close(&mut self) {
        self.window.orderOut(None);
    }
}
