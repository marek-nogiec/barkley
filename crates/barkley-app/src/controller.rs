//! App delegate + panel/table/field delegate. Lives on the main thread only;
//! other modules reach it through the free functions at the bottom.

use std::cell::{Cell, OnceCell, RefCell};
use std::rc::Rc;
use std::sync::Arc;
use std::time::Instant;

use barkley_core::{Dispatcher, Item, Settings};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObject, NSObjectProtocol, Sel};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSApplicationDelegate, NSControl, NSControlTextEditingDelegate, NSStatusItem, NSTableColumn, NSTableRowView,
    NSTableView, NSTableViewDataSource, NSTableViewDelegate, NSTextFieldDelegate, NSTextView, NSView, NSWindowDelegate,
};
use objc2_foundation::{NSIndexSet, NSInteger, NSMutableIndexSet, NSNotification};

use crate::hotkey::Hotkeys;
use crate::panel::{self, Accessory, PanelUi, RowView};
use crate::{actions, bridge, status_item};

pub struct Ivars {
    ui: OnceCell<PanelUi>,
    status: OnceCell<Retained<NSStatusItem>>,
    hotkeys: OnceCell<Rc<RefCell<Hotkeys>>>,
    settings: RefCell<Settings>,
    dispatcher: RefCell<Arc<Dispatcher>>,
    /// Config / plugin load problems, shown as rows while the query is empty.
    problems: RefCell<Vec<Item>>,
    items: RefCell<Vec<Item>>,
    /// Logical visibility; the window stays on screen while it fades out.
    shown: Cell<bool>,
    /// Bumped on every show/hide so a stale fade-out can't close a reopened panel.
    visibility: Cell<u64>,
    /// Row whose cell currently shows the ↩ accessory.
    selected: Cell<Option<usize>>,
    command_held: Cell<bool>,
}

define_class!(
    // SAFETY: NSObject has no subclassing requirements; no Drop impl.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "BarkleyController"]
    #[ivars = Ivars]
    pub struct Controller;

    unsafe impl NSObjectProtocol for Controller {}

    unsafe impl NSApplicationDelegate for Controller {
        #[unsafe(method(applicationDidFinishLaunching:))]
        fn did_finish_launching(&self, _n: &NSNotification) {
            let mtm = self.mtm();
            let _ = self.ivars().ui.set(PanelUi::build(mtm, self));
            let _ = self.ivars().status.set(status_item::install(mtm, self));
            let hk = self.ivars().settings.borrow().hotkey.clone();
            let _ = self.ivars().hotkeys.set(Hotkeys::install(hk.double_cmd, &hk.chord, toggle_panel));
            if !self.ivars().problems.borrow().is_empty() {
                self.show();
            }
            #[cfg(debug_assertions)]
            if let Ok(q) = std::env::var("BARKLEY_SMOKE") {
                self.smoke_test(q);
            }
        }
    }

    unsafe impl NSWindowDelegate for Controller {
        #[unsafe(method(windowDidResignKey:))]
        fn window_did_resign_key(&self, _n: &NSNotification) {
            self.hide();
        }
    }

    unsafe impl NSControlTextEditingDelegate for Controller {
        #[unsafe(method(controlTextDidChange:))]
        fn control_text_did_change(&self, _n: &NSNotification) {
            self.query_changed();
        }

        #[unsafe(method(control:textView:doCommandBySelector:))]
        fn do_command(&self, _control: &NSControl, _tv: &NSTextView, cmd: Sel) -> bool {
            self.handle_command(cmd)
        }
    }

    unsafe impl NSTextFieldDelegate for Controller {}

    unsafe impl NSTableViewDataSource for Controller {
        #[unsafe(method(numberOfRowsInTableView:))]
        fn number_of_rows(&self, _t: &NSTableView) -> NSInteger {
            self.ivars().items.borrow().len() as NSInteger
        }
    }

    unsafe impl NSTableViewDelegate for Controller {
        #[unsafe(method_id(tableView:viewForTableColumn:row:))]
        fn view_for_row(&self, table: &NSTableView, _col: Option<&NSTableColumn>, row: NSInteger) -> Option<Retained<NSView>> {
            self.cell_for_row(table, row as usize)
        }

        #[unsafe(method_id(tableView:rowViewForRow:))]
        fn row_view(&self, _table: &NSTableView, _row: NSInteger) -> Option<Retained<NSTableRowView>> {
            Some(Retained::into_super(RowView::new(self.mtm())))
        }
    }

    impl Controller {
        #[unsafe(method(rowClicked:))]
        fn row_clicked(&self, _sender: Option<&AnyObject>) {
            let row = self.ui().table.clickedRow();
            if row >= 0 {
                self.run_row(row as usize);
            }
        }

        #[unsafe(method(togglePanel:))]
        fn toggle_panel_action(&self, _sender: Option<&AnyObject>) {
            self.toggle();
        }

        #[unsafe(method(openConfig:))]
        fn open_config(&self, _sender: Option<&AnyObject>) {
            let path = barkley_core::settings::config_path();
            let _ = Settings::load_or_create(&path);
            actions::run("/usr/bin/open", &["-t".into(), path.to_string_lossy().into_owned()], "Open config".into());
        }

        #[unsafe(method(openPluginsFolder:))]
        fn open_plugins_folder(&self, _sender: Option<&AnyObject>) {
            let dir = self.ivars().settings.borrow().plugins_dir();
            let _ = std::fs::create_dir_all(&dir);
            actions::run("/usr/bin/open", &[dir.to_string_lossy().into_owned()], "Open plugins folder".into());
        }

        #[unsafe(method(reloadPlugins:))]
        fn reload_plugins(&self, _sender: Option<&AnyObject>) {
            self.reload();
            let n = self.ivars().dispatcher.borrow().plugins().len();
            eprintln!("barkley: reloaded, {n} plugins");
            self.show();
        }
    }
);

impl Controller {
    pub fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(Ivars {
            ui: OnceCell::new(),
            status: OnceCell::new(),
            hotkeys: OnceCell::new(),
            settings: RefCell::new(Settings::default()),
            dispatcher: RefCell::new(Arc::new(Dispatcher::default())),
            problems: RefCell::new(Vec::new()),
            items: RefCell::new(Vec::new()),
            shown: Cell::new(false),
            visibility: Cell::new(0),
            selected: Cell::new(None),
            command_held: Cell::new(false),
        });
        let this: Retained<Self> = unsafe { msg_send![super(this), init] };
        this.reload();
        this
    }

    /// (Re)read config.toml and rebuild plugins. Hotkey changes need a restart.
    fn reload(&self) {
        let (settings, config_err) = Settings::load_or_create(&barkley_core::settings::config_path());
        let (dispatcher, plugin_errs) = barkley_core::build_dispatcher(&settings, &bundled_plugin_dirs());
        let problems = config_err
            .into_iter()
            .map(|e| Item::error("Config error (using defaults)", e))
            .chain(plugin_errs.into_iter().map(|e| Item::error("Plugin failed to load", e)))
            .collect();
        *self.ivars().settings.borrow_mut() = settings;
        *self.ivars().dispatcher.borrow_mut() = Arc::new(dispatcher);
        *self.ivars().problems.borrow_mut() = problems;
        if self.ivars().ui.get().is_some() {
            self.query_changed();
        }
    }

    /// Keyboard commands from the query field's editor.
    fn handle_command(&self, cmd: Sel) -> bool {
        if cmd == sel!(moveUp:) {
            self.move_selection(-1);
        } else if cmd == sel!(moveDown:) {
            self.move_selection(1);
        } else if cmd == sel!(insertNewline:) {
            self.run_selected();
        } else if cmd == sel!(cancelOperation:) {
            self.hide();
        } else {
            return false;
        }
        true
    }

    fn cell_for_row(&self, table: &NSTableView, row: usize) -> Option<Retained<NSView>> {
        let items = self.ivars().items.borrow();
        let accessory = if self.ivars().command_held.get() {
            Accessory::Shortcut
        } else if self.ivars().selected.get() == Some(row) {
            Accessory::Enter
        } else {
            Accessory::None
        };
        let cell = panel::make_cell(self.mtm(), items.get(row)?, row, table.bounds().size.width, accessory);
        Some(Retained::into_super(cell))
    }

    /// Rebuild just these rows' cells (accessories changed).
    fn refresh_rows(&self, rows: &[usize]) {
        let table = &self.ui().table;
        let n = table.numberOfRows().max(0) as usize;
        let set = NSMutableIndexSet::new();
        for &r in rows.iter().filter(|&&r| r < n) {
            set.addIndex(r);
        }
        table.reloadDataForRowIndexes_columnIndexes(&set, &NSIndexSet::indexSetWithIndex(0));
    }

    pub fn set_command_held(&self, held: bool) {
        if self.ivars().command_held.replace(held) != held {
            let n = self.ivars().items.borrow().len();
            self.refresh_rows(&(0..n).collect::<Vec<_>>());
        }
    }

    fn ui(&self) -> &PanelUi {
        self.ivars().ui.get().expect("panel built in applicationDidFinishLaunching")
    }

    fn toggle(&self) {
        if self.ivars().shown.get() { self.hide() } else { self.show() }
    }

    fn show(&self) {
        let ui = self.ui();
        let started = Instant::now();
        self.ivars().shown.set(true);
        self.ivars().visibility.set(self.ivars().visibility.get() + 1);
        ui.position(self.mtm());
        ui.present();
        ui.panel.makeFirstResponder(Some(&ui.field));
        // Keep the last query, selected, so typing replaces it.
        unsafe { ui.field.selectText(None) };
        eprintln!("barkley: panel shown in {:.2} ms", started.elapsed().as_secs_f64() * 1000.0);
        if ui.field.stringValue().length() == 0 {
            self.query_changed();
        }
    }

    fn hide(&self) {
        let Some(ui) = self.ivars().ui.get() else { return };
        if !self.ivars().shown.replace(false) {
            return;
        }
        self.ivars().command_held.set(false);
        let token = self.ivars().visibility.get() + 1;
        self.ivars().visibility.set(token);
        ui.dismiss(move || {
            with(|c| {
                if c.ivars().visibility.get() == token {
                    c.ui().panel.orderOut(None);
                }
            });
        });
    }

    fn query_changed(&self) {
        let text = self.ui().field.stringValue().to_string();
        let dispatcher = Arc::clone(&self.ivars().dispatcher.borrow());
        if text.trim().is_empty() {
            dispatcher.next_generation(); // drop in-flight results
            let problems = self.ivars().problems.borrow().clone();
            self.set_items(problems);
            return;
        }
        bridge::search(dispatcher, text);
    }

    /// Called on the main thread with results for `generation`.
    pub fn results_ready(&self, dispatcher: &Arc<Dispatcher>, generation: u64, items: Vec<Item>) {
        let current = Arc::ptr_eq(dispatcher, &self.ivars().dispatcher.borrow());
        if current && dispatcher.is_current(generation) {
            self.set_items(items);
        }
    }

    fn set_items(&self, items: Vec<Item>) {
        let ui = self.ui();
        let n = items.len();
        let was_empty = self.ivars().items.borrow().is_empty();
        *self.ivars().items.borrow_mut() = items;
        self.ivars().selected.set(None);
        ui.table.reloadData();
        ui.fit_rows(n, self.ivars().shown.get());
        if n > 0 {
            self.select_row(0);
            if was_empty && self.ivars().shown.get() {
                ui.cascade_rows();
            }
        }
    }

    fn select_row(&self, row: usize) {
        let table = &self.ui().table;
        table.selectRowIndexes_byExtendingSelection(&NSIndexSet::indexSetWithIndex(row), false);
        table.scrollRowToVisible(row as NSInteger);
        let old = self.ivars().selected.replace(Some(row));
        self.refresh_rows(&old.into_iter().chain([row]).collect::<Vec<_>>());
    }

    fn move_selection(&self, delta: isize) {
        let n = self.ivars().items.borrow().len() as isize;
        if n == 0 {
            return;
        }
        let cur = self.ui().table.selectedRow();
        let next = (cur + delta).rem_euclid(n);
        self.select_row(next as usize);
    }

    fn run_selected(&self) {
        let row = self.ui().table.selectedRow();
        if row >= 0 {
            self.run_row(row as usize);
        }
    }

    pub fn run_row(&self, row: usize) {
        let Some(item) = self.ivars().items.borrow().get(row).cloned() else { return };
        if item.action != barkley_core::Action::Nothing {
            self.hide();
            let verb = crate::panel::verb(&item).unwrap_or("Run");
            actions::perform(&item.action, format!("{verb} {}", item.title));
        }
    }

    /// A background action failed: reopen the panel with the reason.
    pub fn action_failed(&self, what: &str, detail: &str) {
        if !self.ivars().shown.get() {
            self.show();
        }
        self.ivars().dispatcher.borrow().next_generation(); // drop in-flight results
        self.set_items(vec![Item::error(format!("{what} failed"), detail)]);
    }

    /// Mouse moved over the results: select the row under it, Spotlight-style.
    pub fn hover(&self, window_point: objc2_foundation::NSPoint) {
        let table = &self.ui().table;
        let row = table.rowAtPoint(table.convertPoint_fromView(window_point, None));
        if row >= 0 && self.ivars().selected.get() != Some(row as usize) {
            self.select_row(row as usize);
        }
    }
}

/// Plugins shipped inside the app: `Barkley.app/Contents/Resources/plugins`.
fn bundled_plugin_dirs() -> Vec<std::path::PathBuf> {
    std::env::current_exe()
        .ok()
        .and_then(|exe| Some(exe.parent()?.parent()?.join("Resources/plugins")))
        .into_iter()
        .collect()
}

#[cfg(debug_assertions)]
impl Controller {
    /// Debug-only: `BARKLEY_SMOKE="10 km to mi"` shows the panel, runs the query
    /// through the real UI path, logs what rendered, and exits.
    fn smoke_test(&self, query: String) {
        self.show();
        self.ui().field.setStringValue(&objc2_foundation::NSString::from_str(&query));
        self.query_changed();
        let block = block2::RcBlock::new(|_t: std::ptr::NonNull<objc2_foundation::NSTimer>| {
            with(|c| {
                let ui = c.ui();
                let f = ui.panel.frame();
                eprintln!(
                    "smoke: key={} visible={} rows={} selected={} frame=({:.0},{:.0} {:.0}x{:.0})",
                    ui.panel.isKeyWindow(),
                    ui.panel.isVisible(),
                    ui.table.numberOfRows(),
                    ui.table.selectedRow(),
                    f.origin.x,
                    f.origin.y,
                    f.size.width,
                    f.size.height
                );
                for it in c.ivars().items.borrow().iter() {
                    eprintln!("smoke:   {} | {} | {:?} | {:?}", it.title, it.subtitle, it.icon, it.action);
                }
                c.move_selection(1);
                eprintln!("smoke: after down, selected={}", ui.table.selectedRow());
                if let Ok(path) = std::env::var("BARKLEY_SMOKE_PNG") {
                    ui.snapshot(&path);
                }
                c.hide();
                let after_hide = block2::RcBlock::new(|_t: std::ptr::NonNull<objc2_foundation::NSTimer>| {
                    with(|c| eprintln!("smoke: after hide, visible={}", c.ui().panel.isVisible()));
                    std::process::exit(0);
                });
                unsafe { objc2_foundation::NSTimer::scheduledTimerWithTimeInterval_repeats_block(0.4, false, &after_hide) };
            });
        });
        unsafe { objc2_foundation::NSTimer::scheduledTimerWithTimeInterval_repeats_block(0.5, false, &block) };
    }
}

thread_local! {
    static CONTROLLER: OnceCell<Retained<Controller>> = const { OnceCell::new() };
}

pub fn set_global(c: Retained<Controller>) {
    CONTROLLER.with(|cell| {
        let _ = cell.set(c);
    });
}

fn with(f: impl FnOnce(&Controller)) {
    CONTROLLER.with(|cell| {
        if let Some(c) = cell.get() {
            f(c);
        }
    });
}

pub fn toggle_panel() {
    with(Controller::toggle);
}

pub fn hide_panel() {
    with(Controller::hide);
}

pub fn set_command_held(held: bool) {
    with(|c| c.set_command_held(held));
}

pub fn run_row(row: usize) {
    with(|c| c.run_row(row));
}

pub fn action_failed(what: &str, detail: &str) {
    with(|c| c.action_failed(what, detail));
}

pub fn hover(window_point: objc2_foundation::NSPoint) {
    with(|c| c.hover(window_point));
}

pub fn results_ready(dispatcher: &Arc<Dispatcher>, generation: u64, items: Vec<Item>) {
    with(|c| c.results_ready(dispatcher, generation, items));
}
