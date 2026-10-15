//! Hidden main menu with the standard Edit items. As an accessory app Barkley
//! never shows a menu bar, but AppKit still routes key equivalents through the
//! main menu, and that is what gives text fields ⌘X/⌘C/⌘V/⌘Z.

use objc2::rc::Retained;
use objc2::runtime::Sel;
use objc2::{MainThreadMarker, MainThreadOnly, sel};
use objc2_app_kit::{NSMenu, NSMenuItem};
use objc2_foundation::NSString;

pub fn build(mtm: MainThreadMarker) -> Retained<NSMenu> {
    let edit = NSMenu::initWithTitle(NSMenu::alloc(mtm), &NSString::from_str("Edit"));
    // Target nil → first responder chain → the field editor. Uppercase key = with ⇧.
    let entries: [(&str, Option<Sel>, &str); 7] = [
        ("Undo", Some(sel!(undo:)), "z"),
        ("Redo", Some(sel!(redo:)), "Z"),
        ("", None, ""),
        ("Cut", Some(sel!(cut:)), "x"),
        ("Copy", Some(sel!(copy:)), "c"),
        ("Paste", Some(sel!(paste:)), "v"),
        ("Select All", Some(sel!(selectAll:)), "a"),
    ];
    for (title, action, key) in entries {
        let mi = match action {
            None => NSMenuItem::separatorItem(mtm),
            Some(action) => unsafe {
                NSMenuItem::initWithTitle_action_keyEquivalent(
                    NSMenuItem::alloc(mtm),
                    &NSString::from_str(title),
                    Some(action),
                    &NSString::from_str(key),
                )
            },
        };
        edit.addItem(&mi);
    }

    let edit_item = NSMenuItem::new(mtm);
    edit_item.setSubmenu(Some(&edit));
    let main = NSMenu::new(mtm);
    main.addItem(&edit_item);
    main
}
