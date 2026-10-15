//! Global hotkeys: double-tap ⌘ (needs Accessibility) and a chord fallback
//! via Carbon `RegisterEventHotKey` (needs no permission).

use std::cell::RefCell;
use std::ffi::c_void;
use std::ptr::NonNull;
use std::rc::Rc;
use std::str::FromStr;

use block2::RcBlock;
use global_hotkey::hotkey::HotKey;
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2_app_kit::{NSEvent, NSEventMask, NSEventModifierFlags, NSEventType};
use objc2_foundation::{NSDictionary, NSNumber, NSString, NSTimer};

/// Max gap between taps, and max length of each tap, in seconds.
const TAP_WINDOW: f64 = 0.3;

#[derive(Debug, Clone, Copy, PartialEq)]
enum TapState {
    Idle,
    FirstDown(f64),
    FirstUp(f64),
}

/// ⌘down → ⌘up → ⌘down, each step within `TAP_WINDOW`, no other key or
/// modifier in between.
#[derive(Debug)]
pub struct DoubleTap {
    state: TapState,
    cmd_down: bool,
}

impl Default for DoubleTap {
    fn default() -> Self {
        Self { state: TapState::Idle, cmd_down: false }
    }
}

impl DoubleTap {
    /// Feed a modifier change. Returns `true` when a double tap completes.
    pub fn flags_changed(&mut self, t: f64, cmd: bool, other_modifiers: bool) -> bool {
        let was_down = std::mem::replace(&mut self.cmd_down, cmd);
        if other_modifiers {
            self.state = TapState::Idle;
            return false;
        }
        match (was_down, cmd) {
            (false, true) => match self.state {
                TapState::FirstUp(up) if t - up <= TAP_WINDOW => {
                    self.state = TapState::Idle;
                    return true;
                }
                _ => self.state = TapState::FirstDown(t),
            },
            (true, false) => {
                self.state = match self.state {
                    TapState::FirstDown(down) if t - down <= TAP_WINDOW => TapState::FirstUp(t),
                    _ => TapState::Idle,
                }
            }
            _ => {}
        }
        false
    }

    pub fn key_down(&mut self) {
        self.state = TapState::Idle;
    }
}

#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    fn AXIsProcessTrusted() -> bool;
    fn AXIsProcessTrustedWithOptions(options: *const c_void) -> bool;
    static kAXTrustedCheckOptionPrompt: *const c_void;
}

/// Ask for Accessibility trust, showing the system prompt if not yet granted.
fn request_accessibility() -> bool {
    // SAFETY: kAXTrustedCheckOptionPrompt is a CFStringRef constant, toll-free
    // bridged to NSString; the dictionary outlives the call.
    unsafe {
        let key: &NSString = &*kAXTrustedCheckOptionPrompt.cast::<NSString>();
        let yes = NSNumber::new_bool(true);
        let opts = NSDictionary::from_slices(&[key], &[&*yes]);
        AXIsProcessTrustedWithOptions(Retained::as_ptr(&opts).cast())
    }
}

/// Keeps event monitors / timers / the hotkey manager alive.
#[derive(Default)]
pub struct Hotkeys {
    _monitors: Vec<Retained<AnyObject>>,
    _manager: Option<GlobalHotKeyManager>,
}

impl Hotkeys {
    pub fn install(
        double_cmd: bool,
        chord: &str,
        on_trigger: impl Fn() + Clone + 'static,
    ) -> Rc<RefCell<Self>> {
        let this = Rc::new(RefCell::new(Self::default()));

        if !chord.trim().is_empty() {
            match install_chord(chord) {
                Ok(m) => this.borrow_mut()._manager = Some(m),
                Err(e) => eprintln!("barkley: chord hotkey {chord:?} unavailable: {e}"),
            }
        }

        if double_cmd {
            if request_accessibility() {
                this.borrow_mut()._monitors = install_double_cmd(on_trigger);
            } else {
                // Monitors only receive events once trusted; poll until the
                // user flips the switch in System Settings.
                eprintln!("barkley: waiting for Accessibility permission for double-⌘");
                let this2 = Rc::clone(&this);
                let block = RcBlock::new(move |timer: NonNull<NSTimer>| {
                    if unsafe { AXIsProcessTrusted() } {
                        unsafe { timer.as_ref() }.invalidate();
                        this2.borrow_mut()._monitors = install_double_cmd(on_trigger.clone());
                        eprintln!("barkley: Accessibility granted, double-⌘ active");
                    }
                });
                unsafe { NSTimer::scheduledTimerWithTimeInterval_repeats_block(2.0, true, &block) };
            }
        }
        this
    }
}

fn install_chord(chord: &str) -> Result<GlobalHotKeyManager, String> {
    let hotkey = HotKey::from_str(chord).map_err(|e| e.to_string())?;
    let manager = GlobalHotKeyManager::new().map_err(|e| e.to_string())?;
    manager.register(hotkey).map_err(|e| e.to_string())?;
    GlobalHotKeyEvent::set_event_handler(Some(|e: GlobalHotKeyEvent| {
        if e.state() == HotKeyState::Pressed {
            dispatch2::DispatchQueue::main().exec_async(crate::controller::toggle_panel);
        }
    }));
    Ok(manager)
}

fn install_double_cmd(on_trigger: impl Fn() + 'static) -> Vec<Retained<AnyObject>> {
    let detector = Rc::new(RefCell::new(DoubleTap::default()));
    let handle = Rc::new(move |ev: &NSEvent| {
        let fire = match ev.r#type() {
            NSEventType::KeyDown => {
                detector.borrow_mut().key_down();
                false
            }
            NSEventType::FlagsChanged => {
                let flags = ev.modifierFlags();
                let others = flags.intersects(
                    NSEventModifierFlags::Shift | NSEventModifierFlags::Control | NSEventModifierFlags::Option,
                );
                detector.borrow_mut().flags_changed(
                    ev.timestamp(),
                    flags.contains(NSEventModifierFlags::Command),
                    others,
                )
            }
            _ => false,
        };
        if fire {
            on_trigger();
        }
    });

    let mask = NSEventMask::FlagsChanged | NSEventMask::KeyDown;
    let mut monitors = Vec::new();

    let h = Rc::clone(&handle);
    let global = RcBlock::new(move |ev: NonNull<NSEvent>| h(unsafe { ev.as_ref() }));
    monitors.extend(NSEvent::addGlobalMonitorForEventsMatchingMask_handler(mask, &global));

    // Our own panel's events never reach the global monitor.
    let h = Rc::clone(&handle);
    let local = RcBlock::new(move |ev: NonNull<NSEvent>| -> *mut NSEvent {
        h(unsafe { ev.as_ref() });
        ev.as_ptr()
    });
    monitors.extend(unsafe { NSEvent::addLocalMonitorForEventsMatchingMask_handler(mask, &local) });
    monitors
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tap(d: &mut DoubleTap, t: f64) -> bool {
        let fired = d.flags_changed(t, true, false);
        assert!(!d.flags_changed(t + 0.05, false, false));
        fired
    }

    #[test]
    fn double_tap_fires() {
        let mut d = DoubleTap::default();
        assert!(!tap(&mut d, 0.0));
        assert!(tap(&mut d, 0.2));
    }

    #[test]
    fn slow_second_tap_does_not_fire() {
        let mut d = DoubleTap::default();
        assert!(!tap(&mut d, 0.0));
        assert!(!tap(&mut d, 0.5));
        // ...but it starts a new sequence.
        assert!(tap(&mut d, 0.7));
    }

    #[test]
    fn long_hold_does_not_count_as_tap() {
        let mut d = DoubleTap::default();
        assert!(!d.flags_changed(0.0, true, false));
        assert!(!d.flags_changed(0.6, false, false));
        assert!(!tap(&mut d, 0.7));
    }

    #[test]
    fn key_between_taps_cancels() {
        let mut d = DoubleTap::default();
        assert!(!tap(&mut d, 0.0));
        d.key_down();
        assert!(!tap(&mut d, 0.15));
    }

    #[test]
    fn cmd_held_with_key_cancels() {
        // ⌘C then quick ⌘ tap must not fire.
        let mut d = DoubleTap::default();
        assert!(!d.flags_changed(0.0, true, false));
        d.key_down();
        assert!(!d.flags_changed(0.05, false, false));
        assert!(!tap(&mut d, 0.1));
    }

    #[test]
    fn other_modifier_cancels() {
        let mut d = DoubleTap::default();
        assert!(!tap(&mut d, 0.0));
        assert!(!d.flags_changed(0.1, true, true));
        assert!(!d.flags_changed(0.15, false, false));
        assert!(!tap(&mut d, 0.2));
    }

    #[test]
    fn triple_tap_fires_once() {
        let mut d = DoubleTap::default();
        assert!(!tap(&mut d, 0.0));
        assert!(tap(&mut d, 0.1));
        assert!(!tap(&mut d, 0.2));
    }
}
