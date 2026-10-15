//! Barkley: menu-bar launcher. Double-tap ⌘ (or the chord) to search.

mod actions;
mod anim;
mod bridge;
mod controller;
mod hotkey;
mod panel;
mod status_item;

use objc2::MainThreadMarker;
use objc2::runtime::ProtocolObject;
use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy};

fn main() {
    let mtm = MainThreadMarker::new().expect("must run on the main thread");
    let app = NSApplication::sharedApplication(mtm);
    // No Dock icon, no menu bar of our own (also set via LSUIElement in the bundle).
    app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);

    let controller = controller::Controller::new(mtm);
    app.setDelegate(Some(ProtocolObject::from_ref(&*controller)));
    controller::set_global(controller);
    app.run();
}
