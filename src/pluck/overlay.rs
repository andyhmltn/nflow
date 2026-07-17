//! Centered palette overlay for pluck.
//!
//! A single borderless, transparent `NSWindow` covers the screen. Its content
//! view draws a centered rounded panel containing a search prompt (with the
//! current mode shown at the right edge), a list of result rows, and a footer
//! cheatsheet. Each row shows the token drawn character by character in a
//! monospaced font so matched characters can be brightened individually; the
//! selected row gets a background highlight and marked rows show a leading `●`.
//! The view is flipped (`isFlipped = true`) so layout uses top-left origin
//! coordinates.

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
pub struct PluckRow {
    pub display: String,
    /// Character indices within `display` that the query matched (bright).
    pub matched_positions: Vec<usize>,
    pub selected: bool,
    /// Toggled with `Tab` for multi-copy.
    pub marked: bool,
    /// The candidate has a markdown rendering (links). Shown as a small `md`
    /// marker at the row's right edge so `ctrl-m` is discoverable.
    pub md: bool,
}

#[derive(Debug, Clone, Default)]
pub struct PluckSnapshot {
    pub query: String,
    /// `words` / `lines`, shown at the right edge of the prompt row.
    pub mode: String,
    pub cursor_visible: bool,
    pub rows: Vec<PluckRow>,
    pub marked_count: usize,
}

const FONT_SIZE: f64 = 14.0;
const ROW_HEIGHT: f64 = 26.0;
const PROMPT_HEIGHT: f64 = 38.0;
const FOOTER_HEIGHT: f64 = 24.0;
const PADDING: f64 = 14.0;
const PANEL_WIDTH: f64 = 640.0;
const PANEL_CORNER: f64 = 10.0;
const MARK_WIDTH: f64 = 20.0;

fn to_any<T: ClassType<Super = NSObject> + 'static>(value: Retained<T>) -> Retained<AnyObject> {
    let object = Retained::into_super(value);
    Retained::into_super(object)
}

struct PluckOverlayIvars {
    state: RefCell<PluckSnapshot>,
    screen_size: RefCell<NSSize>,
}

declare_class!(
    struct PluckOverlayView;

    unsafe impl ClassType for PluckOverlayView {
        type Super = NSView;
        type Mutability = mutability::MainThreadOnly;
        const NAME: &'static str = "nflowPluckOverlayView";
    }

    impl DeclaredClass for PluckOverlayView {
        type Ivars = PluckOverlayIvars;
    }

    unsafe impl NSObjectProtocol for PluckOverlayView {}

    unsafe impl PluckOverlayView {
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

impl PluckOverlayView {
    fn new(mtm: MainThreadMarker, frame: NSRect) -> Retained<Self> {
        let this = mtm.alloc().set_ivars(PluckOverlayIvars {
            state: RefCell::new(PluckSnapshot::default()),
            screen_size: RefCell::new(frame.size),
        });
        unsafe { msg_send_id![super(this), initWithFrame: frame] }
    }

    fn set_snapshot(&self, snapshot: PluckSnapshot) {
        *self.ivars().state.borrow_mut() = snapshot;
        unsafe { self.setNeedsDisplay(true) };
    }
}

impl PluckOverlayView {
    fn draw_panel(&self) {
        let state = self.ivars().state.borrow();
        let screen = *self.ivars().screen_size.borrow();

        let font = unsafe { NSFont::monospacedSystemFontOfSize_weight(FONT_SIZE, 0.0) };
        let advance = char_advance(&font);

        let panel_width = PANEL_WIDTH.min(screen.width - 80.0).max(320.0);
        let row_count = state.rows.len();
        let panel_height =
            PADDING * 2.0 + PROMPT_HEIGHT + row_count as f64 * ROW_HEIGHT + FOOTER_HEIGHT + 4.0;
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
            accent_attrs: accent_attrs.clone(),
        };

        // Prompt row.
        let prompt_y = panel_y + PADDING;
        let label = NSString::from_str("pluck");
        let label_origin = NSPoint::new(panel_x + PADDING, prompt_y + 9.0);
        unsafe { label.drawAtPoint_withAttributes(label_origin, Some(&dim_attrs)) };
        let label_width = unsafe { label.sizeWithAttributes(Some(&dim_attrs)) }.width;

        let sep = NSString::from_str("›");
        let sep_origin = NSPoint::new(label_origin.x + label_width + 8.0, prompt_y + 9.0);
        unsafe { sep.drawAtPoint_withAttributes(sep_origin, Some(&dim_attrs)) };
        let sep_width = unsafe { sep.sizeWithAttributes(Some(&dim_attrs)) }.width;

        let query_origin = NSPoint::new(sep_origin.x + sep_width + 8.0, prompt_y + 9.0);
        let query_ns = NSString::from_str(&state.query);
        unsafe { query_ns.drawAtPoint_withAttributes(query_origin, Some(&bright_attrs)) };
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

        // Mode indicator at the right edge of the prompt row.
        let mode_label = NSString::from_str(&format!("[{}]", state.mode));
        let mode_width = unsafe { mode_label.sizeWithAttributes(Some(&dim_attrs)) }.width;
        let mode_origin =
            NSPoint::new(panel_x + panel_width - PADDING - mode_width, prompt_y + 9.0);
        unsafe { mode_label.drawAtPoint_withAttributes(mode_origin, Some(&dim_attrs)) };

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

        // Rows.
        let text_x = panel_x + PADDING + MARK_WIDTH;
        let text_max_width = panel_x + panel_width - PADDING - text_x - MARK_WIDTH;
        for (i, row) in state.rows.iter().enumerate() {
            let row_y = sep_y + 8.0 + i as f64 * ROW_HEIGHT;
            draw_row(
                row,
                panel_x,
                panel_width,
                row_y,
                text_x,
                text_max_width,
                &style,
            );
        }

        // Footer cheatsheet.
        let footer_y = sep_y + 8.0 + row_count as f64 * ROW_HEIGHT + 6.0;
        let footer = if state.marked_count > 0 {
            format!(
                "enter=copy({})  ctrl-m=markdown  tab=mark  ctrl-f=mode  esc=cancel",
                state.marked_count
            )
        } else {
            "enter=copy  ctrl-m=markdown  tab=mark  ctrl-f=mode  esc=cancel".to_string()
        };
        let footer_ns = NSString::from_str(&footer);
        unsafe {
            footer_ns.drawAtPoint_withAttributes(
                NSPoint::new(panel_x + PADDING, footer_y),
                Some(&dim_attrs),
            )
        };
    }
}

struct RowStyle {
    font: Retained<NSFont>,
    advance: f64,
    bright_attrs: Retained<NSDictionary<NSString, AnyObject>>,
    accent_attrs: Retained<NSDictionary<NSString, AnyObject>>,
}

fn draw_row(
    row: &PluckRow,
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

    // Mark indicator.
    if row.marked {
        let mark = NSString::from_str("●");
        let mark_color =
            unsafe { NSColor::colorWithSRGBRed_green_blue_alpha(0.40, 0.90, 0.60, 1.0) };
        let keys = [unsafe { NSFontAttributeName }, unsafe {
            NSForegroundColorAttributeName
        }];
        let mark_attrs =
            NSDictionary::from_vec(&keys, vec![to_any(style.font.clone()), to_any(mark_color)]);
        unsafe {
            mark.drawAtPoint_withAttributes(
                NSPoint::new(
                    panel_x + PADDING,
                    row_y + (ROW_HEIGHT - FONT_SIZE) / 2.0 - 1.0,
                ),
                Some(&mark_attrs),
            )
        };
    }

    // Display text, char by char, highlighting matched positions.
    let matched: std::collections::HashSet<usize> = row.matched_positions.iter().copied().collect();
    let mut x = text_x;
    let baseline_y = row_y + (ROW_HEIGHT - FONT_SIZE) / 2.0 - 1.0;
    let mut emitted = 0.0f64;
    for (idx, ch) in row.display.chars().enumerate() {
        if emitted > text_max_width {
            break;
        }
        let attrs = if matched.contains(&idx) {
            &style.bright_attrs
        } else {
            &style.accent_attrs
        };
        let s = NSString::from_str(&ch.to_string());
        unsafe { s.drawAtPoint_withAttributes(NSPoint::new(x, baseline_y), Some(attrs)) };
        x += style.advance;
        emitted += style.advance;
    }

    // Markdown indicator at the row's right edge: shown when the candidate has
    // a markdown rendering (i.e. it's a reconstructed line containing links),
    // so `ctrl-m` is discoverable.
    if row.md {
        let md_color = unsafe { NSColor::colorWithSRGBRed_green_blue_alpha(0.55, 0.75, 0.95, 0.9) };
        let keys = [unsafe { NSFontAttributeName }, unsafe {
            NSForegroundColorAttributeName
        }];
        let md_attrs =
            NSDictionary::from_vec(&keys, vec![to_any(style.font.clone()), to_any(md_color)]);
        let label = NSString::from_str("md");
        let size = unsafe { label.sizeWithAttributes(Some(&md_attrs)) };
        let origin = NSPoint::new(
            panel_x + panel_width - PADDING - size.width,
            row_y + (ROW_HEIGHT - FONT_SIZE) / 2.0 - 1.0,
        );
        unsafe { label.drawAtPoint_withAttributes(origin, Some(&md_attrs)) };
    }
}

fn char_advance(font: &Retained<NSFont>) -> f64 {
    let attrs = NSDictionary::from_vec(
        &[unsafe { NSFontAttributeName }],
        vec![to_any(font.clone())],
    );
    let probe = NSString::from_str("M");
    let size = unsafe { probe.sizeWithAttributes(Some(&attrs)) };
    size.width.max(1.0)
}

pub struct PluckOverlay {
    window: Retained<NSWindow>,
    view: Retained<PluckOverlayView>,
}

impl PluckOverlay {
    pub fn show() -> PluckOverlay {
        let mtm = MainThreadMarker::new().expect("pluck overlay must be built on the main thread");

        let frame = match NSScreen::mainScreen(mtm) {
            Some(screen) => screen.frame(),
            None => NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(1440.0, 900.0)),
        };

        let view = PluckOverlayView::new(mtm, frame);

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

        PluckOverlay { window, view }
    }

    pub fn set_snapshot(&self, snapshot: PluckSnapshot) {
        self.view.set_snapshot(snapshot);
    }

    pub fn close(&mut self) {
        self.window.orderOut(None);
    }
}
