//! Centered command-palette overlay for menu search.
//!
//! A single borderless, transparent `NSWindow` covers the screen. Its content
//! view draws a centered rounded panel containing a search prompt and a list of
//! result rows. Each row shows a hint-code badge on the left and the menu
//! item's breadcrumb title on the right, with the query-matched characters
//! highlighted. The selected row gets a background highlight. The view is
//! flipped (`isFlipped = true`) so layout uses top-left origin coordinates.

use std::cell::RefCell;

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObject};
use objc2::{declare_class, msg_send_id, mutability, ClassType, DeclaredClass};
use objc2_app_kit::{
    NSBackingStoreType, NSBezierPath, NSColor, NSFont, NSFontAttributeName,
    NSForegroundColorAttributeName, NSScreen, NSStatusWindowLevel, NSStringDrawing, NSView,
    NSWindow, NSWindowCollectionBehavior, NSWindowStyleMask,
};
use objc2_foundation::{
    MainThreadMarker, NSDictionary, NSObjectProtocol, NSPoint, NSRect, NSSize, NSString,
};

#[derive(Debug, Clone)]
pub struct MenuRow {
    pub code: String,
    pub display: String,
    pub matched_positions: Vec<usize>,
    pub selected: bool,
    pub dim: bool,
    pub disabled: bool,
}

#[derive(Debug, Clone, Default)]
pub struct MenuSnapshot {
    pub prompt_label: String,
    pub query: String,
    pub cursor_visible: bool,
    pub rows: Vec<MenuRow>,
}

const FONT_SIZE: f64 = 14.0;
const ROW_HEIGHT: f64 = 26.0;
const PROMPT_HEIGHT: f64 = 38.0;
const PADDING: f64 = 14.0;
const PANEL_WIDTH: f64 = 640.0;
const BADGE_WIDTH: f64 = 42.0;
const BADGE_HEIGHT: f64 = 20.0;
const BADGE_CORNER: f64 = 4.0;
const PANEL_CORNER: f64 = 10.0;
const TEXT_GAP: f64 = 10.0;

fn to_any<T: ClassType<Super = NSObject> + 'static>(value: Retained<T>) -> Retained<AnyObject> {
    let object = Retained::into_super(value);
    Retained::into_super(object)
}

struct MenuOverlayIvars {
    state: RefCell<MenuSnapshot>,
    screen_size: RefCell<NSSize>,
}

declare_class!(
    struct MenuOverlayView;

    unsafe impl ClassType for MenuOverlayView {
        type Super = NSView;
        type Mutability = mutability::MainThreadOnly;
        const NAME: &'static str = "nflowMenuOverlayView";
    }

    impl DeclaredClass for MenuOverlayView {
        type Ivars = MenuOverlayIvars;
    }

    unsafe impl NSObjectProtocol for MenuOverlayView {}

    unsafe impl MenuOverlayView {
        #[method(isFlipped)]
        fn is_flipped(&self) -> bool {
            true
        }

        #[method(drawRect:)]
        fn draw_rect(&self, _dirty: NSRect) {
            self.draw_panel();
        }
    }
);

impl MenuOverlayView {
    fn new(mtm: MainThreadMarker, frame: NSRect) -> Retained<Self> {
        let this = mtm.alloc().set_ivars(MenuOverlayIvars {
            state: RefCell::new(MenuSnapshot::default()),
            screen_size: RefCell::new(frame.size),
        });
        unsafe { msg_send_id![super(this), initWithFrame: frame] }
    }

    fn set_snapshot(&self, snapshot: MenuSnapshot) {
        *self.ivars().state.borrow_mut() = snapshot;
        unsafe { self.setNeedsDisplay(true) };
    }
}

impl MenuOverlayView {
    fn draw_panel(&self) {
        let state = self.ivars().state.borrow();
        let screen = *self.ivars().screen_size.borrow();

        let font = unsafe { NSFont::monospacedSystemFontOfSize_weight(FONT_SIZE, 0.0) };
        let advance = char_advance(&font);

        let panel_width = PANEL_WIDTH.min(screen.width - 80.0).max(320.0);
        let row_count = state.rows.len();
        let panel_height = PADDING * 2.0 + PROMPT_HEIGHT + row_count as f64 * ROW_HEIGHT + 4.0;
        let panel_x = (screen.width - panel_width) / 2.0;
        let panel_y = (screen.height - panel_height) / 2.0;
        let panel_rect = NSRect::new(
            NSPoint::new(panel_x, panel_y),
            NSSize::new(panel_width, panel_height),
        );

        let bg = unsafe { NSColor::colorWithSRGBRed_green_blue_alpha(0.07, 0.08, 0.11, 0.96) };
        let border = unsafe { NSColor::colorWithSRGBRed_green_blue_alpha(0.35, 0.40, 0.50, 0.55) };
        unsafe {
            let path = NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(
                panel_rect,
                PANEL_CORNER,
                PANEL_CORNER,
            );
            bg.set();
            path.fill();
            path.setLineWidth(1.0);
            border.set();
            path.stroke();
        }

        let prompt_y = panel_y + PADDING;
        let bright = unsafe { NSColor::colorWithSRGBRed_green_blue_alpha(0.96, 0.97, 1.0, 1.0) };
        let dim = unsafe { NSColor::colorWithSRGBRed_green_blue_alpha(0.45, 0.48, 0.55, 1.0) };
        let accent = unsafe { NSColor::colorWithSRGBRed_green_blue_alpha(0.40, 0.80, 1.0, 1.0) };

        let keys = [unsafe { NSFontAttributeName }, unsafe {
            NSForegroundColorAttributeName
        }];
        let bright_attrs =
            NSDictionary::from_vec(&keys, vec![to_any(font.clone()), to_any(bright.clone())]);
        let dim_attrs =
            NSDictionary::from_vec(&keys, vec![to_any(font.clone()), to_any(dim.clone())]);
        let accent_attrs =
            NSDictionary::from_vec(&keys, vec![to_any(font.clone()), to_any(accent.clone())]);

        let style = RowStyle {
            font: font.clone(),
            advance,
            bright_attrs: bright_attrs.clone(),
            dim_attrs: dim_attrs.clone(),
            accent_attrs: accent_attrs.clone(),
        };

        let label = NSString::from_str(&state.prompt_label);
        let label_origin = NSPoint::new(panel_x + PADDING, prompt_y + 9.0);
        unsafe {
            label.drawAtPoint_withAttributes(label_origin, Some(&dim_attrs));
        }
        let label_width = unsafe { label.sizeWithAttributes(Some(&dim_attrs)) }.width;

        let sep = NSString::from_str("›");
        let sep_origin = NSPoint::new(label_origin.x + label_width + 8.0, prompt_y + 9.0);
        unsafe { sep.drawAtPoint_withAttributes(sep_origin, Some(&dim_attrs)) };
        let sep_width = unsafe { sep.sizeWithAttributes(Some(&dim_attrs)) }.width;

        let query_origin = NSPoint::new(sep_origin.x + sep_width + 8.0, prompt_y + 9.0);
        let query_ns = NSString::from_str(&state.query);
        unsafe {
            query_ns.drawAtPoint_withAttributes(query_origin, Some(&bright_attrs));
        }
        let query_width = unsafe { query_ns.sizeWithAttributes(Some(&bright_attrs)) }.width;

        if state.cursor_visible {
            let cursor_rect = NSRect::new(
                NSPoint::new(query_origin.x + query_width + 1.0, prompt_y + 6.0),
                NSSize::new(advance.max(7.0), FONT_SIZE + 4.0),
            );
            unsafe {
                let cursor_color =
                    NSColor::colorWithSRGBRed_green_blue_alpha(0.40, 0.80, 1.0, 0.85);
                cursor_color.set();
                NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(cursor_rect, 2.0, 2.0)
                    .fill();
            }
        }

        let sep_y = prompt_y + PROMPT_HEIGHT - 6.0;
        unsafe {
            let line_color = NSColor::colorWithSRGBRed_green_blue_alpha(0.30, 0.34, 0.42, 0.55);
            line_color.set();
            let line = NSBezierPath::bezierPath();
            line.moveToPoint(NSPoint::new(panel_x + PADDING, sep_y));
            line.lineToPoint(NSPoint::new(panel_x + panel_width - PADDING, sep_y));
            line.setLineWidth(1.0);
            line.stroke();
        }

        let text_x = panel_x + PADDING + BADGE_WIDTH + TEXT_GAP;
        let text_max_width = panel_x + panel_width - PADDING - text_x;

        for (i, row) in state.rows.iter().enumerate() {
            let row_y = sep_y + 8.0 + i as f64 * ROW_HEIGHT;
            draw_row(row, panel_x, panel_width, row_y, text_x, text_max_width, &style);
        }
    }
}

struct RowStyle {
    font: Retained<NSFont>,
    advance: f64,
    bright_attrs: Retained<NSDictionary<NSString, AnyObject>>,
    dim_attrs: Retained<NSDictionary<NSString, AnyObject>>,
    accent_attrs: Retained<NSDictionary<NSString, AnyObject>>,
}

fn draw_row(
    row: &MenuRow,
    panel_x: f64,
    panel_width: f64,
    row_y: f64,
    text_x: f64,
    text_max_width: f64,
    style: &RowStyle,
) {
    let bg = if row.selected {
        Some(unsafe { NSColor::colorWithSRGBRed_green_blue_alpha(0.18, 0.32, 0.55, 0.85) })
    } else {
        None
    };
    if let Some(bg) = bg {
        let row_rect = NSRect::new(
            NSPoint::new(panel_x + 4.0, row_y - 2.0),
            NSSize::new(panel_width - 8.0, ROW_HEIGHT - 2.0),
        );
        unsafe {
            let path = NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(row_rect, 5.0, 5.0);
            bg.set();
            path.fill();
        }
    }

    let badge_rect = NSRect::new(
        NSPoint::new(
            panel_x + PADDING,
            row_y + (ROW_HEIGHT - BADGE_HEIGHT) / 2.0 - 2.0,
        ),
        NSSize::new(BADGE_WIDTH, BADGE_HEIGHT),
    );
    let badge_bg = if row.dim {
        unsafe { NSColor::colorWithSRGBRed_green_blue_alpha(0.14, 0.16, 0.22, 0.9) }
    } else {
        unsafe { NSColor::colorWithSRGBRed_green_blue_alpha(0.12, 0.20, 0.34, 0.95) }
    };
    let badge_border = if row.dim {
        unsafe { NSColor::colorWithSRGBRed_green_blue_alpha(0.30, 0.33, 0.40, 0.5) }
    } else {
        unsafe { NSColor::colorWithSRGBRed_green_blue_alpha(0.40, 0.80, 1.0, 0.7) }
    };
    unsafe {
        let path = NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(
            badge_rect,
            BADGE_CORNER,
            BADGE_CORNER,
        );
        badge_bg.set();
        path.fill();
        path.setLineWidth(1.0);
        badge_border.set();
        path.stroke();
    }

    let badge_text_color = if row.dim {
        unsafe { NSColor::colorWithSRGBRed_green_blue_alpha(0.45, 0.48, 0.55, 1.0) }
    } else {
        unsafe { NSColor::colorWithSRGBRed_green_blue_alpha(0.96, 0.97, 1.0, 1.0) }
    };
    let keys = [unsafe { NSFontAttributeName }, unsafe {
        NSForegroundColorAttributeName
    }];
    let badge_attrs = NSDictionary::from_vec(
        &keys,
        vec![to_any(style.font.clone()), to_any(badge_text_color)],
    );
    let badge_label = NSString::from_str(&row.code);
    let badge_text_size = unsafe { badge_label.sizeWithAttributes(Some(&badge_attrs)) };
    let badge_text_origin = NSPoint::new(
        badge_rect.origin.x + (BADGE_WIDTH - badge_text_size.width) / 2.0,
        badge_rect.origin.y + (BADGE_HEIGHT - badge_text_size.height) / 2.0,
    );
    unsafe {
        badge_label.drawAtPoint_withAttributes(badge_text_origin, Some(&badge_attrs));
    }

    let matched: std::collections::HashSet<usize> = row.matched_positions.iter().copied().collect();
    let mut x = text_x;
    let baseline_y = row_y + (ROW_HEIGHT - FONT_SIZE) / 2.0 - 1.0;
    let mut emitted = 0.0f64;
    for (idx, ch) in row.display.chars().enumerate() {
        if emitted > text_max_width {
            break;
        }
        let attrs = if row.dim || row.disabled {
            &style.dim_attrs
        } else if matched.contains(&idx) {
            &style.bright_attrs
        } else {
            &style.accent_attrs
        };
        let s = NSString::from_str(&ch.to_string());
        unsafe { s.drawAtPoint_withAttributes(NSPoint::new(x, baseline_y), Some(attrs)) };
        x += style.advance;
        emitted += style.advance;
    }
}

fn char_advance(font: &Retained<NSFont>) -> f64 {
    let attrs = {
        NSDictionary::from_vec(
            &[unsafe { NSFontAttributeName }],
            vec![to_any(font.clone())],
        )
    };
    let probe = NSString::from_str("M");
    let size = unsafe { probe.sizeWithAttributes(Some(&attrs)) };
    (size.width).max(1.0)
}

pub struct MenuOverlay {
    window: Retained<NSWindow>,
    view: Retained<MenuOverlayView>,
}

impl MenuOverlay {
    pub fn show() -> MenuOverlay {
        let mtm = MainThreadMarker::new().expect("menu overlay must be built on the main thread");

        let frame = match NSScreen::mainScreen(mtm) {
            Some(screen) => screen.frame(),
            None => NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(1440.0, 900.0)),
        };

        let view = MenuOverlayView::new(mtm, frame);

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
            window.setAlphaValue(1.0);
            window.orderFrontRegardless();
        }

        MenuOverlay { window, view }
    }

    pub fn set_snapshot(&self, snapshot: MenuSnapshot) {
        self.view.set_snapshot(snapshot);
    }

    pub fn close(&mut self) {
        self.window.orderOut(None);
    }
}
