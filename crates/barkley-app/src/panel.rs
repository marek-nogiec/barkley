//! The search panel: a transparent non-activating NSPanel holding one Liquid
//! Glass shape (macOS 26+). With no results it is a capsule search bar; with
//! results it grows downward into a rounded panel, Spotlight-style.

use std::cell::Cell;

use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2::{AnyThread, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSAnimatablePropertyContainer, NSBackingStoreType, NSBezierPath, NSBox, NSBoxType, NSColor, NSEvent,
    NSEventModifierFlags, NSEventType, NSFocusRingType, NSFont, NSFontWeightMedium, NSFontWeightRegular,
    NSGlassEffectView, NSGlassEffectViewStyle, NSImage, NSImageScaling, NSImageSymbolConfiguration, NSImageView,
    NSLineBreakMode, NSPanel, NSScreen, NSScrollView, NSTableCellView, NSTableColumn, NSTableRowView, NSTableView,
    NSTableViewStyle, NSTextAlignment, NSTextField, NSView, NSWindowCollectionBehavior, NSWindowStyleMask,
};
use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};
use objc2_quartz_core::CALayer;

use barkley_core::{Action, Icon, Item};

use crate::anim;
use crate::controller::{self, Controller};

/// Width of the glass panel.
pub const WIDTH: f64 = 680.0;
/// Transparent border around the glass so its edge highlight and shadow aren't clipped.
const MARGIN: f64 = 24.0;
const WINDOW_W: f64 = WIDTH + 2.0 * MARGIN;
const FIELD_H: f64 = 64.0;
/// Half the bar height: a capsule when collapsed, a soft rounded panel when open.
const CORNER: f64 = FIELD_H / 2.0;
const LIST_PAD: f64 = 8.0;
pub const ROW_H: f64 = 56.0;
const MAX_ROWS: usize = barkley_core::dispatch::MAX_RESULTS;
/// The window is always tall enough for a full list; everything outside the
/// glass is transparent and lets clicks through.
const WINDOW_H: f64 = 2.0 * MARGIN + FIELD_H + 1.0 + 2.0 * LIST_PAD + MAX_ROWS as f64 * ROW_H;

/// Horizontal inset of the selection pill inside the panel.
const PILL_INSET: f64 = 10.0;
const ICON: f64 = 30.0;

define_class!(
    // SAFETY: NSPanel has no subclassing requirements; no Drop impl.
    #[unsafe(super(NSPanel))]
    #[thread_kind = MainThreadOnly]
    #[name = "BarkleyPanel"]
    pub struct Panel;

    impl Panel {
        // Borderless windows refuse key status by default; we need typing.
        #[unsafe(method(canBecomeKeyWindow))]
        fn can_become_key_window(&self) -> bool {
            true
        }

        // ⌘1…⌘9 run the matching row.
        #[unsafe(method(performKeyEquivalent:))]
        fn perform_key_equivalent(&self, event: &NSEvent) -> bool {
            match command_digit(event) {
                Some(n) => {
                    controller::run_row(n - 1);
                    true
                }
                None => unsafe { msg_send![super(self), performKeyEquivalent: event] },
            }
        }

        // Holding ⌘ reveals the ⌘1…⌘9 hints.
        #[unsafe(method(flagsChanged:))]
        fn flags_changed(&self, event: &NSEvent) {
            let flags = event.modifierFlags() & NSEventModifierFlags::DeviceIndependentFlagsMask;
            controller::set_command_held(flags == NSEventModifierFlags::Command);
            unsafe { msg_send![super(self), flagsChanged: event] }
        }

        // Hover selects. Caught here because the field editor, as first
        // responder, would otherwise swallow mouse-moved events.
        #[unsafe(method(sendEvent:))]
        fn send_event(&self, event: &NSEvent) {
            if event.r#type() == NSEventType::MouseMoved {
                controller::hover(event.locationInWindow());
            }
            unsafe { msg_send![super(self), sendEvent: event] }
        }

        // Esc when the field editor isn't first responder.
        #[unsafe(method(cancelOperation:))]
        fn cancel_operation(&self, _sender: Option<&objc2::runtime::AnyObject>) {
            controller::hide_panel();
        }
    }
);

define_class!(
    // SAFETY: NSView has no subclassing requirements; no Drop impl.
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "BarkleyFlippedView"]
    /// Top-left origin, so content stays pinned to the top while the glass
    /// around it changes height.
    struct FlippedView;

    impl FlippedView {
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }
    }
);

impl FlippedView {
    fn new(mtm: MainThreadMarker, frame: NSRect) -> Retained<Self> {
        unsafe { msg_send![super(Self::alloc(mtm).set_ivars(())), initWithFrame: frame] }
    }
}

define_class!(
    // SAFETY: NSView has no subclassing requirements; no Drop impl.
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "BarkleyHairline"]
    /// 1 px line in the system separator colour (re-resolved on appearance change).
    struct Hairline;

    impl Hairline {
        #[unsafe(method(drawRect:))]
        fn draw_rect(&self, _dirty: NSRect) {
            NSColor::separatorColor().setFill();
            NSBezierPath::fillRect(self.bounds());
        }
    }
);

impl Hairline {
    fn new(mtm: MainThreadMarker, frame: NSRect) -> Retained<Self> {
        unsafe { msg_send![super(Self::alloc(mtm).set_ivars(())), initWithFrame: frame] }
    }
}

define_class!(
    // SAFETY: NSTableRowView has no subclassing requirements; no Drop impl.
    #[unsafe(super(NSTableRowView, NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "BarkleyRowView"]
    pub struct RowView;

    impl RowView {
        // Keep normal text colours on the selected row; the pill is subtle.
        #[unsafe(method(isEmphasized))]
        fn is_emphasized(&self) -> bool {
            false
        }

        /// Soft translucent pill instead of the stock accent bar.
        #[unsafe(method(drawSelectionInRect:))]
        fn draw_selection(&self, _dirty: NSRect) {
            let b = self.bounds();
            let r = NSRect::new(
                NSPoint::new(b.origin.x + PILL_INSET, b.origin.y + 2.0),
                NSSize::new(b.size.width - 2.0 * PILL_INSET, b.size.height - 4.0),
            );
            NSColor::labelColor().colorWithAlphaComponent(0.11).setFill();
            NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(r, 16.0, 16.0).fill();
        }
    }
);

impl RowView {
    pub fn new(mtm: MainThreadMarker) -> Retained<Self> {
        unsafe { msg_send![super(Self::alloc(mtm).set_ivars(())), init] }
    }
}

/// ⌘1…⌘9 → Some(1…9)
fn command_digit(event: &NSEvent) -> Option<usize> {
    let flags = event.modifierFlags() & NSEventModifierFlags::DeviceIndependentFlagsMask;
    if flags != NSEventModifierFlags::Command {
        return None;
    }
    let n: usize = event.charactersIgnoringModifiers()?.to_string().parse().ok()?;
    (1..=9).contains(&n).then_some(n)
}

pub struct PanelUi {
    pub panel: Retained<Panel>,
    pub field: Retained<NSTextField>,
    pub table: Retained<NSTableView>,
    glass: Retained<NSGlassEffectView>,
    content: Retained<FlippedView>,
    separator: Retained<Hairline>,
    rows: Cell<usize>,
}

impl PanelUi {
    pub fn build(mtm: MainThreadMarker, controller: &Controller) -> Self {
        let rect = NSRect::new(NSPoint::ZERO, NSSize::new(WINDOW_W, WINDOW_H));
        let style = NSWindowStyleMask::Borderless
            | NSWindowStyleMask::NonactivatingPanel
            | NSWindowStyleMask::FullSizeContentView;
        let panel: Retained<Panel> = unsafe {
            msg_send![
                super(Panel::alloc(mtm).set_ivars(())),
                initWithContentRect: rect,
                styleMask: style,
                backing: NSBackingStoreType::Buffered,
                defer: false,
            ]
        };
        unsafe { panel.setReleasedWhenClosed(false) };
        panel.setFloatingPanel(true);
        panel.setLevel(objc2_app_kit::NSFloatingWindowLevel);
        panel.setCollectionBehavior(
            NSWindowCollectionBehavior::CanJoinAllSpaces
                | NSWindowCollectionBehavior::FullScreenAuxiliary
                | NSWindowCollectionBehavior::Transient,
        );
        // Fully transparent window; the glass draws its own edge and shadow.
        panel.setOpaque(false);
        panel.setBackgroundColor(Some(&NSColor::clearColor()));
        panel.setHasShadow(false);
        panel.setHidesOnDeactivate(false);
        panel.setAcceptsMouseMovedEvents(true);
        panel.setDelegate(Some(ProtocolObject::from_ref(controller)));

        let root = NSView::initWithFrame(NSView::alloc(mtm), rect);
        root.setWantsLayer(true);
        panel.setContentView(Some(&root));

        let glass = NSGlassEffectView::new(mtm);
        glass.setStyle(NSGlassEffectViewStyle::Regular);
        glass.setCornerRadius(CORNER);
        // Frost it more than the default so text behind doesn't compete with results.
        glass.setTintColor(Some(&NSColor::windowBackgroundColor().colorWithAlphaComponent(0.55)));
        glass.setFrame(glass_frame(0));
        root.addSubview(&glass);

        let content = FlippedView::new(mtm, NSRect::new(NSPoint::ZERO, NSSize::new(WIDTH, FIELD_H)));
        content.setClipsToBounds(true);
        glass.setContentView(Some(&content));

        // Search row.
        if let Some(icon) = symbol("magnifyingglass", 22.0, unsafe { NSFontWeightMedium }) {
            let iv = NSImageView::imageViewWithImage(&icon, mtm);
            iv.setFrame(NSRect::new(NSPoint::new(24.0, (FIELD_H - 28.0) / 2.0), NSSize::new(28.0, 28.0)));
            iv.setContentTintColor(Some(&NSColor::secondaryLabelColor()));
            content.addSubview(&iv);
        }
        let text_h = 34.0;
        let field = NSTextField::initWithFrame(
            NSTextField::alloc(mtm),
            NSRect::new(NSPoint::new(64.0, (FIELD_H - text_h) / 2.0), NSSize::new(WIDTH - 64.0 - 24.0, text_h)),
        );
        field.setBezeled(false);
        field.setBordered(false);
        field.setDrawsBackground(false);
        field.setFocusRingType(NSFocusRingType::None);
        field.setFont(Some(&NSFont::systemFontOfSize(26.0)));
        field.setPlaceholderString(Some(&NSString::from_str("Woof?")));
        field.setLineBreakMode(NSLineBreakMode::ByTruncatingHead);
        field.setUsesSingleLineMode(true);
        // SAFETY: delegate is weak; the controller lives for the whole process.
        unsafe { field.setDelegate(Some(ProtocolObject::from_ref(controller))) };
        content.addSubview(&field);

        // Hairline between the query and the results.
        let separator = Hairline::new(mtm, NSRect::new(NSPoint::new(20.0, FIELD_H), NSSize::new(WIDTH - 40.0, 1.0)));
        separator.setAlphaValue(0.0);
        content.addSubview(&separator);

        // Results. Always laid out at full height; the glass reveals as much as needed.
        let table = NSTableView::initWithFrame(NSTableView::alloc(mtm), NSRect::ZERO);
        let column = NSTableColumn::initWithIdentifier(NSTableColumn::alloc(mtm), &NSString::from_str("main"));
        column.setWidth(WIDTH);
        table.addTableColumn(&column);
        table.setHeaderView(None);
        table.setStyle(NSTableViewStyle::FullWidth);
        table.setIntercellSpacing(NSSize::new(0.0, 0.0));
        table.setRowHeight(ROW_H);
        table.setBackgroundColor(&NSColor::clearColor());
        table.setRefusesFirstResponder(true);
        table.setFocusRingType(NSFocusRingType::None);
        unsafe {
            table.setDataSource(Some(ProtocolObject::from_ref(controller)));
            table.setDelegate(Some(ProtocolObject::from_ref(controller)));
            table.setTarget(Some(controller));
            table.setAction(Some(sel!(rowClicked:)));
        }
        let scroll = NSScrollView::initWithFrame(
            NSScrollView::alloc(mtm),
            NSRect::new(NSPoint::new(0.0, FIELD_H + 1.0 + LIST_PAD), NSSize::new(WIDTH, MAX_ROWS as f64 * ROW_H)),
        );
        scroll.setDocumentView(Some(&table));
        scroll.setDrawsBackground(false);
        scroll.setHasVerticalScroller(false);
        content.addSubview(&scroll);

        Self { panel, field, table, glass, content, separator, rows: Cell::new(0) }
    }

    /// Grow or shrink the glass to fit `rows`, top edge fixed.
    pub fn fit_rows(&self, rows: usize, animate: bool) {
        let before = self.rows.replace(rows);
        let frame = glass_frame(rows);
        let bounds = NSRect::new(NSPoint::ZERO, frame.size);
        let separator_alpha = if rows == 0 { 0.0 } else { 1.0 };
        if !animate || anim::reduce_motion() || before == rows {
            self.glass.setFrame(frame);
            self.content.setFrame(bounds);
            self.separator.setAlphaValue(separator_alpha);
            return;
        }
        let (glass, content, separator) = (self.glass.clone(), self.content.clone(), self.separator.clone());
        let duration = if rows < before { 0.18 } else { 0.26 };
        anim::group(
            duration,
            move || {
                glass.animator().setFrame(frame);
                content.animator().setFrame(bounds);
                separator.animator().setAlphaValue(separator_alpha);
            },
            || {},
        );
    }

    /// The point the panel pops out of: the centre of the search bar.
    fn pop_anchor(&self) -> (f64, f64) {
        (MARGIN + WIDTH / 2.0, WINDOW_H - MARGIN - FIELD_H / 2.0)
    }

    fn root_layer(&self) -> Option<Retained<CALayer>> {
        self.panel.contentView()?.layer()
    }

    /// Order front with a springy pop + fade.
    pub fn present(&self) {
        let panel = &self.panel;
        panel.setAlphaValue(0.0);
        panel.makeKeyAndOrderFront(None);
        if !anim::reduce_motion()
            && let Some(layer) = self.root_layer()
        {
            let (x, y) = self.pop_anchor();
            anim::pop_in(&layer, 0.9, x, y);
        }
        let p = panel.clone();
        anim::group(0.14, move || p.animator().setAlphaValue(1.0), || {});
    }

    /// Fade and shrink slightly, then call `done` (which orders the panel out).
    pub fn dismiss(&self, done: impl Fn() + 'static) {
        if !anim::reduce_motion()
            && let Some(layer) = self.root_layer()
        {
            let (x, y) = self.pop_anchor();
            anim::shrink_out(&layer, 0.96, x, y, 0.12);
        }
        let p = self.panel.clone();
        anim::group(0.12, move || p.animator().setAlphaValue(0.0), done);
    }

    /// Fade/slide visible rows in one after another.
    pub fn cascade_rows(&self) {
        if anim::reduce_motion() {
            return;
        }
        for row in 0..self.table.numberOfRows() {
            if let Some(layer) = self.table.rowViewAtRow_makeIfNecessary(row, false).and_then(|v| v.layer()) {
                anim::reveal(&layer, 0.04 + row as f64 * 0.025, -6.0);
            }
        }
    }

    /// Debug: render the panel's view tree (minus the glass blur) to a PNG.
    #[cfg(debug_assertions)]
    pub fn snapshot(&self, path: &str) {
        let view: &NSView = &self.glass;
        let b = view.bounds();
        let Some(rep) = view.bitmapImageRepForCachingDisplayInRect(b) else { return };
        view.cacheDisplayInRect_toBitmapImageRep(b, &rep);
        let props = objc2_foundation::NSDictionary::new();
        if let Some(data) =
            unsafe { rep.representationUsingType_properties(objc2_app_kit::NSBitmapImageFileType::PNG, &props) }
        {
            data.writeToFile_atomically(&NSString::from_str(path), true);
        }
    }

    /// Centre horizontally on the screen under the mouse; search bar top at 25 % height.
    pub fn position(&self, mtm: MainThreadMarker) {
        let mouse = NSEvent::mouseLocation();
        let screens = NSScreen::screens(mtm);
        let screen = screens.iter().find(|s| point_in_rect(mouse, s.frame())).or_else(|| NSScreen::mainScreen(mtm));
        let Some(screen) = screen else { return };
        let vf = screen.visibleFrame();
        let top = vf.origin.y + vf.size.height * 0.75 + MARGIN;
        let x = vf.origin.x + (vf.size.width - WINDOW_W) / 2.0;
        self.panel
            .setFrame_display(NSRect::new(NSPoint::new(x, top - WINDOW_H), NSSize::new(WINDOW_W, WINDOW_H)), true);
    }
}

fn glass_height(rows: usize) -> f64 {
    if rows == 0 { FIELD_H } else { FIELD_H + 1.0 + 2.0 * LIST_PAD + rows as f64 * ROW_H }
}

/// Glass frame in window coordinates, top edge fixed.
fn glass_frame(rows: usize) -> NSRect {
    let h = glass_height(rows);
    NSRect::new(NSPoint::new(MARGIN, WINDOW_H - MARGIN - h), NSSize::new(WIDTH, h))
}

fn point_in_rect(p: NSPoint, r: NSRect) -> bool {
    p.x >= r.origin.x && p.x < r.origin.x + r.size.width && p.y >= r.origin.y && p.y < r.origin.y + r.size.height
}

fn symbol(name: &str, point_size: f64, weight: f64) -> Option<Retained<NSImage>> {
    let img = NSImage::imageWithSystemSymbolName_accessibilityDescription(&NSString::from_str(name), None)?;
    let cfg = NSImageSymbolConfiguration::configurationWithPointSize_weight(point_size, weight);
    img.imageWithSymbolConfiguration(&cfg)
}

/// What the right edge of a row shows.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Accessory {
    None,
    /// Selected row: action verb + ↩ badge.
    Enter,
    /// ⌘ held: ⌘N shortcut.
    Shortcut,
}

pub fn verb(item: &Item) -> Option<&str> {
    match &item.action {
        Action::Nothing => None,
        _ if item.verb.is_some() => item.verb.as_deref(),
        Action::OpenUrl(_) => Some("Open"),
        Action::Copy(_) => Some("Copy"),
        Action::Run { .. } => Some("Run"),
    }
}

/// Result row: icon, title over subtitle, and an accessory on the right.
pub fn make_cell(
    mtm: MainThreadMarker,
    item: &Item,
    row: usize,
    width: f64,
    accessory: Accessory,
) -> Retained<NSTableCellView> {
    let cell = NSTableCellView::initWithFrame(
        NSTableCellView::alloc(mtm),
        NSRect::new(NSPoint::ZERO, NSSize::new(width, ROW_H)),
    );
    let icon_x = PILL_INSET + 14.0;
    // Icon column is always reserved so titles line up across rows.
    let text_x = icon_x + ICON + 14.0;
    let right = width - PILL_INSET - 14.0;

    if let Some(image) = item.icon.as_ref().and_then(icon_image) {
        let iv = NSImageView::imageViewWithImage(&image, mtm);
        iv.setFrame(NSRect::new(NSPoint::new(icon_x, (ROW_H - ICON) / 2.0), NSSize::new(ICON, ICON)));
        iv.setImageScaling(NSImageScaling::ScaleProportionallyUpOrDown);
        let tint = if item.is_error { NSColor::systemRedColor() } else { NSColor::labelColor() };
        iv.setContentTintColor(Some(&tint));
        cell.addSubview(&iv);
    }

    // Accessory first, so the text knows how much room is left.
    let mut text_right = right;
    match (accessory, verb(item)) {
        (Accessory::Enter, Some(verb)) => {
            let badge = key_badge(mtm, "\u{21a9}", right);
            let badge_x = badge.frame().origin.x;
            cell.addSubview(&badge);
            let l = label(mtm, verb, 13.0, NSRect::new(NSPoint::new(badge_x - 108.0, 19.0), NSSize::new(100.0, 18.0)));
            l.setTextColor(Some(&NSColor::secondaryLabelColor()));
            l.setAlignment(NSTextAlignment::Right);
            cell.addSubview(&l);
            text_right = badge_x - 60.0;
        }
        (Accessory::Shortcut, _) if row < 9 => {
            let badge = key_badge(mtm, &format!("\u{2318}{}", row + 1), right);
            text_right = badge.frame().origin.x - 12.0;
            cell.addSubview(&badge);
        }
        _ => {
            if let Some(tag) = &item.tag {
                let badge = key_badge(mtm, tag, right);
                text_right = badge.frame().origin.x - 12.0;
                cell.addSubview(&badge);
            }
        }
    }
    let text_w = (text_right - text_x).max(40.0);

    let title = label(mtm, &item.title, 17.0, NSRect::new(NSPoint::new(text_x, 28.0), NSSize::new(text_w, 22.0)));
    if item.is_error {
        title.setTextColor(Some(&NSColor::systemRedColor()));
    }
    cell.addSubview(&title);
    unsafe { cell.setTextField(Some(&title)) };

    if !item.subtitle.is_empty() {
        let subtitle =
            label(mtm, &item.subtitle, 13.0, NSRect::new(NSPoint::new(text_x, 9.0), NSSize::new(text_w, 18.0)));
        subtitle.setTextColor(Some(&NSColor::secondaryLabelColor()));
        cell.addSubview(&subtitle);
    } else {
        title.setFrameOrigin(NSPoint::new(text_x, (ROW_H - 22.0) / 2.0));
    }
    cell
}

/// Small rounded key cap, right-aligned at `right`.
fn key_badge(mtm: MainThreadMarker, text: &str, right: f64) -> Retained<NSBox> {
    let w = 14.0 + 9.0 * text.chars().count() as f64;
    let h = 22.0;
    let b = NSBox::initWithFrame(
        NSBox::alloc(mtm),
        NSRect::new(NSPoint::new(right - w, (ROW_H - h) / 2.0), NSSize::new(w, h)),
    );
    b.setBoxType(NSBoxType::Custom);
    b.setBorderWidth(0.0);
    b.setCornerRadius(6.0);
    b.setFillColor(&NSColor::labelColor().colorWithAlphaComponent(0.1));
    b.setContentViewMargins(NSSize::new(0.0, 0.0));
    b.setTitlePosition(objc2_app_kit::NSTitlePosition::NoTitle);
    let l = label(mtm, text, 12.5, NSRect::new(NSPoint::new(0.0, 3.0), NSSize::new(w, 16.0)));
    l.setFont(Some(&NSFont::systemFontOfSize_weight(12.5, unsafe { NSFontWeightMedium })));
    l.setTextColor(Some(&NSColor::secondaryLabelColor()));
    l.setAlignment(NSTextAlignment::Center);
    b.addSubview(&l);
    b
}

fn icon_image(icon: &Icon) -> Option<Retained<NSImage>> {
    match icon {
        Icon::Symbol(name) => symbol(name, 20.0, unsafe { NSFontWeightRegular }),
        Icon::File(path) => {
            let path = NSString::from_str(&path.to_string_lossy());
            NSImage::initWithContentsOfFile(NSImage::alloc(), &path)
        }
    }
}

fn label(mtm: MainThreadMarker, text: &str, size: f64, frame: NSRect) -> Retained<NSTextField> {
    let l = NSTextField::labelWithString(&NSString::from_str(text), mtm);
    l.setFrame(frame);
    l.setFont(Some(&NSFont::systemFontOfSize(size)));
    l.setLineBreakMode(NSLineBreakMode::ByTruncatingTail);
    l
}
