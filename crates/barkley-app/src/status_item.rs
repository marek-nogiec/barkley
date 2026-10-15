//! Menu-bar icon and its menu.

use objc2::rc::Retained;
use objc2::runtime::Sel;
use objc2::{MainThreadMarker, MainThreadOnly, sel};
use objc2_app_kit::{NSImage, NSMenu, NSMenuItem, NSStatusBar, NSStatusItem, NSVariableStatusItemLength};
use objc2_foundation::NSString;

use crate::controller::Controller;

pub fn install(mtm: MainThreadMarker, controller: &Controller) -> Retained<NSStatusItem> {
    let item = NSStatusBar::systemStatusBar().statusItemWithLength(NSVariableStatusItemLength);
    if let Some(button) = item.button(mtm) {
        let symbol = ["dog", "magnifyingglass"].iter().find_map(|name| {
            NSImage::imageWithSystemSymbolName_accessibilityDescription(
                &NSString::from_str(name),
                Some(&NSString::from_str("Barkley")),
            )
        });
        match symbol {
            Some(img) => {
                img.setTemplate(true);
                button.setImage(Some(&img));
            }
            None => button.setTitle(&NSString::from_str("B")),
        }
    }

    let menu = NSMenu::new(mtm);
    let entries: [(&str, Option<Sel>, &str); 8] = [
        ("Show Barkley", Some(sel!(togglePanel:)), ""),
        ("", None, ""),
        ("Open Config File", Some(sel!(openConfig:)), ","),
        ("Open Plugins Folder", Some(sel!(openPluginsFolder:)), ""),
        ("Reload Config & Plugins", Some(sel!(reloadPlugins:)), "r"),
        ("", None, ""),
        // Checkmark set in `validateMenuItem:` each time the menu opens.
        ("Launch at Login", Some(sel!(toggleLaunchAtLogin:)), ""),
        ("", None, ""),
    ];
    for (title, action, key) in entries {
        let mi = match action {
            None => NSMenuItem::separatorItem(mtm),
            Some(action) => {
                let mi = unsafe {
                    NSMenuItem::initWithTitle_action_keyEquivalent(
                        NSMenuItem::alloc(mtm),
                        &NSString::from_str(title),
                        Some(action),
                        &NSString::from_str(key),
                    )
                };
                unsafe { mi.setTarget(Some(controller)) };
                mi
            }
        };
        menu.addItem(&mi);
    }
    // Target nil → first responder chain → NSApplication.
    let quit = unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(
            NSMenuItem::alloc(mtm),
            &NSString::from_str("Quit Barkley"),
            Some(sel!(terminate:)),
            &NSString::from_str("q"),
        )
    };
    menu.addItem(&quit);
    item.setMenu(Some(&menu));
    item
}
