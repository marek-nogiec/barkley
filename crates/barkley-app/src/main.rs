//! Barkley: menu-bar launcher. Double-tap ⌘ (or the chord) to search.

mod actions;
mod anim;
mod bridge;
mod controller;
mod edit_menu;
mod hotkey;
mod panel;
mod status_item;

use objc2::MainThreadMarker;
use objc2::runtime::ProtocolObject;
use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy};

fn main() {
    let Some(_instance) = single_instance() else {
        eprintln!("Barkley is already running; opening its panel");
        controller::show_running_instance();
        return;
    };
    let mtm = MainThreadMarker::new().expect("must run on the main thread");
    let app = NSApplication::sharedApplication(mtm);
    // No Dock icon, no menu bar of our own (also set via LSUIElement in the bundle).
    app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
    app.setMainMenu(Some(&edit_menu::build(mtm)));

    let controller = controller::Controller::new(mtm);
    app.setDelegate(Some(ProtocolObject::from_ref(&*controller)));
    controller::set_global(controller);
    app.run();
}

/// Holds an exclusive lock for the life of the process, so a second copy (another
/// bundle path, `open -n`, the bare binary) exits instead of fighting over the
/// hotkeys (it asks the running one to open its panel). The OS drops the lock if we crash. `None` = another instance has it.
/// Debug smoke runs skip it so they work next to an installed Barkley.
fn single_instance() -> Option<Option<std::fs::File>> {
    if cfg!(debug_assertions) && std::env::var_os("BARKLEY_SMOKE").is_some() {
        return Some(None);
    }
    let dir = barkley_core::settings::app_support_dir();
    let _ = std::fs::create_dir_all(&dir);
    let Ok(file) = std::fs::File::create(dir.join("barkley.lock")) else {
        return Some(None); // Can't lock: better two instances than none.
    };
    match file.try_lock() {
        Ok(()) => Some(Some(file)),
        Err(std::fs::TryLockError::WouldBlock) => None,
        Err(std::fs::TryLockError::Error(_)) => Some(None),
    }
}
