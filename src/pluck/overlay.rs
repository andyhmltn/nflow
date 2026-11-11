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
    pub matched_positions: Vec<usize>,
    pub selected: bool,
    pub marked: bool,
    pub md: bool,
}

#[derive(Debug, Clone, Default)]
pub struct PluckSnapshot {
    pub query: String,
    pub mode: String,
    pub cursor_visible: bool,
    pub rows: Vec<PluckRow>,
    pub marked_count: usize,
}

const FONT_SIZE: f64 = 14.0;
const HINT_SIZE: f64 = 11.0;
const ROW_HEIGHT: f64 = 32.0;
const PROMPT_HEIGHT: f64 = 38.0;
const FOOTER_HEIGHT: f64 = 30.0;
const PAD_X: f64 = 22.0;
const PAD_TOP: f64 = 18.0;
const PAD_BOTTOM: f64 = 14.0;
const SECTION_GAP: f64 = 8.0;
const PANEL_WIDTH: f64 = 560.0;
const PANEL_CORNER: f64 = 14.0;
const MARK_WIDTH: f64 = 20.0;
const SELECT_BAR_WIDTH: f64 = 2.5;

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
        let panel_height = PAD_TOP
            + PROMPT_HEIGHT
            + SECTION_GAP
            + row_count as f64 * ROW_HEIGHT
            + SECTION_GAP
            + FOOTER_HEIGHT
            + PAD_BOTTOM;
        let panel_x = (screen.width - panel_width) / 2.0;
        let panel_y = (screen.height - panel_height) / 2.0;
        let panel_rect = NSRect::new(
            NSPoint::new(panel_x, panel_y),
            NSSize::new(panel_width, panel_height),
        );

        let bg = unsafe { srgb(0x16, 0x16, 0x16, 0.98) };
        let border = unsafe { srgb(0x26, 0x26, 0x26, 1.0) };
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

        let hint_font = unsafe { NSFont::monospacedSystemFontOfSize_weight(HINT_SIZE, 0.0) };

        let fg = unsafe { srgb(0xf2, 0xf4, 0xf8, 1.0) };
        let fg_dim = unsafe { srgb(0x9a, 0x9a, 0x9a, 1.0) };
        let fg_muted = unsafe { srgb(0x6f, 0x6f, 0x6f, 1.0) };
        let keys = [unsafe { NSFontAttributeName }, unsafe {
            NSForegroundColorAttributeName
        }];
        let fg_attrs =
            NSDictionary::from_vec(&keys, vec![to_any(font.clone()), to_any(fg.clone())]);
        let dim_attrs =
            NSDictionary::from_vec(&keys, vec![to_any(font.clone()), to_any(fg_dim.clone())]);
        let muted_attrs =
            NSDictionary::from_vec(&keys, vec![to_any(font.clone()), to_any(fg_muted.clone())]);
        let hint_attrs = NSDictionary::from_vec(
            &keys,
            vec![to_any(hint_font.clone()), to_any(fg_muted.clone())],
        );
        let style = RowStyle {
            font: font.clone(),
            advance,
            fg_attrs: fg_attrs.clone(),
            dim_attrs: dim_attrs.clone(),
        };

        let prompt_y = panel_y + PAD_TOP;
        let label = NSString::from_str("pluck");
        let label_origin = NSPoint::new(panel_x + PAD_X, prompt_y + 9.0);
        unsafe { label.drawAtPoint_withAttributes(label_origin, Some(&muted_attrs)) };
        let label_width = unsafe { label.sizeWithAttributes(Some(&muted_attrs)) }.width;

        let sep = NSString::from_str("›");
        let sep_origin = NSPoint::new(label_origin.x + label_width + 8.0, prompt_y + 9.0);
        unsafe { sep.drawAtPoint_withAttributes(sep_origin, Some(&muted_attrs)) };
        let sep_width = unsafe { sep.sizeWithAttributes(Some(&muted_attrs)) }.width;

        let query_origin = NSPoint::new(sep_origin.x + sep_width + 8.0, prompt_y + 9.0);
        let query_ns = NSString::from_str(&state.query);
        unsafe { query_ns.drawAtPoint_withAttributes(query_origin, Some(&fg_attrs)) };
        let query_width = unsafe { query_ns.sizeWithAttributes(Some(&fg_attrs)) }.width;

        if state.cursor_visible {
            let cursor_rect = NSRect::new(
                NSPoint::new(query_origin.x + query_width + 1.0, prompt_y + 6.0),
                NSSize::new(advance.max(7.0), FONT_SIZE + 4.0),
            );
            unsafe {
                let cursor_color = srgb(0x00, 0xb4, 0xff, 0.9);
                cursor_color.set();
                NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(cursor_rect, 2.0, 2.0)
                    .fill();
            }
        }

        let mode_label = NSString::from_str(&state.mode.to_uppercase());
        let mode_width = unsafe { mode_label.sizeWithAttributes(Some(&hint_attrs)) }.width;
        let mode_pad_x = 8.0;
        let mode_pad_y = 3.0;
        let mode_bg_rect = NSRect::new(
            NSPoint::new(
                panel_x + panel_width - PAD_X - mode_width - mode_pad_x * 2.0,
                prompt_y + 8.0,
            ),
            NSSize::new(
                mode_width + mode_pad_x * 2.0,
                HINT_SIZE + mode_pad_y * 2.0 + 2.0,
            ),
        );
        unsafe {
            let chip_bg = srgb(0x26, 0x26, 0x26, 1.0);
            chip_bg.set();
            NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(mode_bg_rect, 4.0, 4.0).fill();
            mode_label.drawAtPoint_withAttributes(
                NSPoint::new(mode_bg_rect.origin.x + mode_pad_x, prompt_y + 10.0),
                Some(&hint_attrs),
            );
        }

        let rows_top = prompt_y + PROMPT_HEIGHT + SECTION_GAP;

        let text_x = panel_x + PAD_X + MARK_WIDTH;
        let text_max_width = panel_x + panel_width - PAD_X - text_x - MARK_WIDTH;
        for (i, row) in state.rows.iter().enumerate() {
            let row_y = rows_top + i as f64 * ROW_HEIGHT;
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

        let footer_y = rows_top + row_count as f64 * ROW_HEIGHT + SECTION_GAP + 8.0;
        let footer = if state.marked_count > 0 {
            format!(
                "enter copy·{}    ⌃m markdown    tab mark    ⌃f mode    esc cancel",
                state.marked_count
            )
        } else {
            "enter copy    ⌃m markdown    tab mark    ⌃f mode    esc cancel".to_string()
        };
        let footer_ns = NSString::from_str(&footer);
        let footer_size = unsafe { footer_ns.sizeWithAttributes(Some(&hint_attrs)) };
        let footer_x = panel_x + (panel_width - footer_size.width) / 2.0;
        unsafe {
            footer_ns
                .drawAtPoint_withAttributes(NSPoint::new(footer_x, footer_y), Some(&hint_attrs))
        };
    }
}

unsafe fn srgb(r: u8, g: u8, b: u8, a: f64) -> Retained<NSColor> {
    NSColor::colorWithSRGBRed_green_blue_alpha(
        r as f64 / 255.0,
        g as f64 / 255.0,
        b as f64 / 255.0,
        a,
    )
}

struct RowStyle {
    font: Retained<NSFont>,
    advance: f64,
    fg_attrs: Retained<NSDictionary<NSString, AnyObject>>,
    dim_attrs: Retained<NSDictionary<NSString, AnyObject>>,
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
    if row.selected {
        let inset = PAD_X - 6.0;
        let row_rect = NSRect::new(
            NSPoint::new(panel_x + inset, row_y + 1.0),
            NSSize::new(panel_width - inset * 2.0, ROW_HEIGHT - 2.0),
        );
        unsafe {
            let fill = srgb(0x26, 0x26, 0x26, 1.0);
            let path = NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(row_rect, 7.0, 7.0);
            fill.set();
            path.fill();

            let bar_rect = NSRect::new(
                NSPoint::new(panel_x + inset + 1.0, row_y + 5.0),
                NSSize::new(SELECT_BAR_WIDTH, ROW_HEIGHT - 10.0),
            );
            let bar_color = srgb(0x00, 0xb4, 0xff, 1.0);
            bar_color.set();
            NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(bar_rect, 1.25, 1.25).fill();
        }
    }

    if row.marked {
        let mark = NSString::from_str("●");
        let mark_color = unsafe { srgb(0x00, 0xc1, 0x5a, 1.0) };
        let keys = [unsafe { NSFontAttributeName }, unsafe {
            NSForegroundColorAttributeName
        }];
        let mark_attrs =
            NSDictionary::from_vec(&keys, vec![to_any(style.font.clone()), to_any(mark_color)]);
        unsafe {
            mark.drawAtPoint_withAttributes(
                NSPoint::new(
                    panel_x + PAD_X,
                    row_y + (ROW_HEIGHT - FONT_SIZE) / 2.0 - 1.0,
                ),
                Some(&mark_attrs),
            )
        };
    }

    let matched: std::collections::HashSet<usize> = row.matched_positions.iter().copied().collect();
    let mut x = text_x;
    let baseline_y = row_y + (ROW_HEIGHT - FONT_SIZE) / 2.0 - 1.0;
    let mut emitted = 0.0f64;
    for (idx, ch) in row.display.chars().enumerate() {
        if emitted > text_max_width {
            break;
        }
        let attrs = if matched.contains(&idx) {
            &style.fg_attrs
        } else {
            &style.dim_attrs
        };
        let s = NSString::from_str(&ch.to_string());
        unsafe { s.drawAtPoint_withAttributes(NSPoint::new(x, baseline_y), Some(attrs)) };
        x += style.advance;
        emitted += style.advance;
    }

    if row.md {
        let md_color = unsafe { srgb(0x00, 0xdf, 0xdb, 1.0) };
        let keys = [unsafe { NSFontAttributeName }, unsafe {
            NSForegroundColorAttributeName
        }];
        let md_attrs =
            NSDictionary::from_vec(&keys, vec![to_any(style.font.clone()), to_any(md_color)]);
        let label = NSString::from_str("md");
        let size = unsafe { label.sizeWithAttributes(Some(&md_attrs)) };
        let origin = NSPoint::new(
            panel_x + panel_width - PAD_X - size.width,
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
