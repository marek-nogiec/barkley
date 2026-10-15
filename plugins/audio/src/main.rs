//! Barkley plugin: list audio devices, switch the default input/output.
//!
//! Query mode (what Barkley runs): `barkley-audio "<filter>"` prints devices.
//! Each item's action re-runs this binary as `barkley-audio --set out <uid>`,
//! or `--connect <address>` for paired Bluetooth devices that aren't connected
//! (CoreAudio only knows about connected ones).

use barkley_plugin::Item;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Output,
    Input,
}

impl Direction {
    fn label(self) -> &'static str {
        match self {
            Direction::Output => "Output",
            Direction::Input => "Input",
        }
    }

    fn tag(self) -> &'static str {
        match self {
            Direction::Output => "out",
            Direction::Input => "in",
        }
    }
}

/// How the device is connected (CoreAudio transport type).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Transport {
    BuiltIn,
    Usb,
    Bluetooth,
    Hdmi,
    DisplayPort,
    Thunderbolt,
    AirPlay,
    Virtual,
    Aggregate,
    Other,
}

impl Transport {
    fn label(self) -> &'static str {
        match self {
            Transport::BuiltIn => "Built-in",
            Transport::Usb => "USB",
            Transport::Bluetooth => "Bluetooth",
            Transport::Hdmi => "HDMI",
            Transport::DisplayPort => "DisplayPort",
            Transport::Thunderbolt => "Thunderbolt",
            Transport::AirPlay => "AirPlay",
            Transport::Virtual => "Virtual",
            Transport::Aggregate => "Aggregate",
            Transport::Other => "",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Device {
    /// Stable across reboots, unlike CoreAudio's numeric device id.
    pub uid: String,
    pub name: String,
    pub direction: Direction,
    pub is_default: bool,
    pub transport: Transport,
    /// Has both input and output streams (typically a headset).
    pub duplex: bool,
}

impl Device {
    /// SF Symbol for this device: connection type first, name hints second.
    pub fn symbol(&self) -> &'static str {
        let name = self.name.to_lowercase();
        let has = |words: &[&str]| words.iter().any(|w| name.contains(w));
        let output = self.direction == Direction::Output;
        if has(&["airpods max"]) {
            return "airpods.max";
        }
        if has(&["airpods pro"]) {
            return "airpods.pro";
        }
        if has(&["airpods"]) {
            return "airpods";
        }
        if !output && has(&["camera", "cam", "brio", "facetime", "c920", "c922"]) {
            return "web.camera";
        }
        match self.transport {
            Transport::Hdmi | Transport::DisplayPort if output => "display",
            Transport::AirPlay => "airplay.audio",
            Transport::Virtual | Transport::Aggregate => "waveform",
            Transport::Bluetooth => "headphones",
            Transport::BuiltIn if output && has(&["headphone"]) => "headphones",
            Transport::BuiltIn if output => "speaker.wave.2",
            _ if has(&["headset", "headphone", "earphone", "buds"]) || self.duplex => "headphones",
            _ if output => "hifispeaker",
            _ => "mic",
        }
    }
}

/// "out airpods" → (Some(Output), "airpods")
fn parse_filter(args: &str) -> (Option<Direction>, &str) {
    let args = args.trim();
    let (first, rest) = args.split_once(' ').unwrap_or((args, ""));
    match first.to_lowercase().as_str() {
        "out" | "output" | "outputs" => (Some(Direction::Output), rest.trim()),
        "in" | "input" | "inputs" | "mic" => (Some(Direction::Input), rest.trim()),
        _ => (None, args),
    }
}

/// Case-insensitive subsequence match; tighter matches score higher.
fn fuzzy(needle: &str, hay: &str) -> Option<u32> {
    let hay = hay.to_lowercase();
    let mut chars = hay.char_indices();
    let (mut first, mut last) = (None, 0);
    for n in needle.to_lowercase().chars().filter(|c| !c.is_whitespace()) {
        let (i, _) = chars.find(|&(_, c)| c == n)?;
        first.get_or_insert(i);
        last = i;
    }
    let span = first.map_or(0, |f| last - f) as u32;
    Some(100u32.saturating_sub(span) + if first == Some(0) { 50 } else { 0 })
}

/// Comparable form of a Bluetooth address: 12 lowercase hex digits.
/// CoreAudio uids look like `0A-1B-2C-3D-4E-5F:output`, IOBluetooth like `0a-1b-2c-3d-4e-5f`.
fn bt_address(s: &str) -> Option<String> {
    let hex: String = s.split(':').next()?.chars().filter(char::is_ascii_hexdigit).collect();
    (hex.len() == 12).then(|| hex.to_lowercase())
}

/// A paired Bluetooth audio device, connected or not.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Paired {
    pub address: String,
    pub name: String,
    pub speaker: bool,
    /// Days since this Mac last connected to it; `None` = never.
    pub last_used_days: Option<u64>,
}

/// "used today", "used 3 days ago", "never used on this Mac"
fn last_used(days: Option<u64>) -> String {
    match days {
        None => "never used on this Mac".into(),
        Some(0) => "used today".into(),
        Some(1) => "used yesterday".into(),
        Some(d) if d < 60 => format!("used {d} days ago"),
        Some(d) => format!("used {} months ago", d / 30),
    }
}

fn items(devices: &[Device], paired: &[Paired], query: &str, exe: &str) -> Vec<Item> {
    let (only, needle) = parse_filter(query);
    // Paired devices CoreAudio doesn't list yet: offer to connect them.
    let known: Vec<String> = devices.iter().filter_map(|d| bt_address(&d.uid)).collect();
    let disconnected = paired
        .iter()
        .filter(|_| only.is_none_or(|o| o == Direction::Output))
        .filter(|p| bt_address(&p.address).is_none_or(|a| !known.contains(&a)))
        .enumerate()
        .filter_map(|(i, p)| {
            let fuzzy = fuzzy(needle, &p.name)?;
            // After the connected outputs, before the inputs.
            let score = if needle.is_empty() { 1800 - i as u32 } else { fuzzy * 10 };
            let as_device = Device {
                uid: String::new(),
                name: p.name.clone(),
                direction: Direction::Output,
                is_default: false,
                transport: Transport::Bluetooth,
                duplex: false,
            };
            let symbol = if p.speaker { "hifispeaker" } else { as_device.symbol() };
            Some(
                Item::new(&p.name)
                    .subtitle(format!("Not connected \u{b7} {}", last_used(p.last_used_days)))
                    .score(score)
                    .symbol(symbol)
                    .verb("Connect")
                    .run(exe, ["--connect", &p.address]),
            )
        });
    let items: Vec<Item> = devices
        .iter()
        .enumerate()
        .filter(|(_, d)| only.is_none_or(|o| d.direction == o))
        .filter_map(|(i, d)| {
            let fuzzy = fuzzy(needle, &d.name)?;
            // Empty filter: outputs first, current device first in each group.
            let base = match d.direction {
                Direction::Output => 2000,
                Direction::Input => 1000,
            } + if d.is_default { 500 } else { 0 };
            let score = if needle.is_empty() { base - i as u32 } else { fuzzy * 10 + base / 1000 };
            let subtitle = match d.transport.label() {
                "" => d.direction.label().to_string(),
                t => format!("{} \u{b7} {t}", d.direction.label()),
            };
            let item = Item::new(&d.name).subtitle(subtitle).score(score).symbol(d.symbol());
            Some(if d.is_default {
                item.tag("Current")
            } else {
                item.verb("Switch").run(exe, ["--set", d.direction.tag(), &d.uid])
            })
        })
        .chain(disconnected)
        .collect();
    if items.is_empty() {
        return vec![
            Item::new("No matching audio devices")
                .subtitle("Try \u{201c}audio out\u{201d} or \u{201c}audio in\u{201d}")
                .symbol("speaker.slash"),
        ];
    }
    items
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(String::as_str) == Some("--set") {
        let direction = match args.get(2).map(String::as_str) {
            Some("out") => Direction::Output,
            Some("in") => Direction::Input,
            _ => {
                eprintln!("usage: barkley-audio --set <in|out> <uid>");
                std::process::exit(2);
            }
        };
        let uid = args.get(3).map(String::as_str).unwrap_or_default();
        exit_on_err(coreaudio::set_default(uid, direction));
        return;
    }
    if args.get(1).map(String::as_str) == Some("--connect") {
        exit_on_err(connect(args.get(2).map(String::as_str).unwrap_or_default()));
        return;
    }

    let exe = std::env::current_exe().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default();
    // Persistent (see manifest): CoreAudio's ~80 ms client handshake happens once.
    barkley_plugin::run(|query| match coreaudio::devices() {
        Ok(devices) => items(&devices, &bluetooth::paired_audio(), query, &exe),
        Err(e) => vec![Item::new("Can't list audio devices").subtitle(e).symbol("exclamationmark.triangle.fill")],
    });
}

fn exit_on_err(r: Result<(), String>) {
    if let Err(e) = r {
        eprintln!("barkley-audio: {e}");
        std::process::exit(1);
    }
}

/// Connect a paired Bluetooth device, wait for its audio output, make it the default.
fn connect(address: &str) -> Result<(), String> {
    let want = bt_address(address).ok_or_else(|| format!("bad Bluetooth address {address:?}"))?;
    bluetooth::connect(address)?;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        let found = coreaudio::devices()?
            .into_iter()
            .find(|d| d.direction == Direction::Output && bt_address(&d.uid).as_ref() == Some(&want));
        if let Some(d) = found {
            return coreaudio::set_default(&d.uid, Direction::Output);
        }
        if std::time::Instant::now() >= deadline {
            return Err(format!("{address} connected, but no audio output appeared"));
        }
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
}

mod bluetooth {
    //! Paired Bluetooth devices via IOBluetooth. Needs Bluetooth permission,
    //! which macOS asks for on behalf of Barkley (the responsible app).

    use objc2_foundation::NSString;
    use objc2_io_bluetooth::IOBluetoothDevice;

    use super::Paired;

    const CLASS_MAJOR_AUDIO: u32 = 0x04;
    const MINOR_LOUDSPEAKER: u32 = 0x05;
    const MINOR_HIFI: u32 = 0x0A;

    /// Paired audio devices that aren't connected right now, most recently used first.
    /// Nothing public says which are in range, so recency is the best hint.
    pub fn paired_audio() -> Vec<Paired> {
        // SAFETY: plain IOBluetooth getters; the array holds IOBluetoothDevice objects.
        let Some(all) = (unsafe { IOBluetoothDevice::pairedDevices() }) else { return Vec::new() };
        let mut out: Vec<Paired> = all
            .iter()
            .filter_map(|o| o.downcast::<IOBluetoothDevice>().ok())
            .filter(|d| unsafe { d.deviceClassMajor() } == CLASS_MAJOR_AUDIO && !unsafe { d.isConnected() })
            .filter_map(|d| {
                let address = unsafe { d.addressString() }?.to_string();
                let minor = unsafe { d.deviceClassMinor() };
                let last_used_days =
                    unsafe { d.recentAccessDate() }.map(|t| (-t.timeIntervalSinceNow()).max(0.0) as u64 / 86_400);
                Some(Paired {
                    address,
                    name: unsafe { d.name() }.to_string(),
                    speaker: minor == MINOR_LOUDSPEAKER || minor == MINOR_HIFI,
                    last_used_days,
                })
            })
            .collect();
        out.sort_by_key(|p| (p.last_used_days.unwrap_or(u64::MAX), p.name.to_lowercase()));
        out
    }

    /// Blocks until the baseband connection is up (or fails); audio profiles follow on their own.
    pub fn connect(address: &str) -> Result<(), String> {
        let device = unsafe { IOBluetoothDevice::deviceWithAddressString(Some(&NSString::from_str(address))) }
            .ok_or_else(|| format!("no Bluetooth device {address}"))?;
        if unsafe { device.isConnected() } {
            return Ok(());
        }
        match unsafe { device.openConnection() } {
            0 => Ok(()),
            code => Err(format!("couldn't connect {} (IOReturn {code:#x}); is it nearby and awake?", unsafe {
                device.name()
            })),
        }
    }
}

mod coreaudio {
    //! Minimal CoreAudio HAL bindings: enumerate devices, read/set defaults.

    use std::ffi::c_void;
    use std::mem::size_of;
    use std::ptr;

    use super::{Device, Direction, Transport};

    type AudioObjectId = u32;
    type CFStringRef = *const c_void;

    #[repr(C)]
    struct Address {
        selector: u32,
        scope: u32,
        element: u32,
    }

    const fn fourcc(s: &[u8; 4]) -> u32 {
        u32::from_be_bytes(*s)
    }

    const SYSTEM_OBJECT: AudioObjectId = 1;
    const DEVICES: u32 = fourcc(b"dev#");
    const DEFAULT_OUTPUT: u32 = fourcc(b"dOut");
    const DEFAULT_INPUT: u32 = fourcc(b"dIn ");
    const NAME: u32 = fourcc(b"lnam");
    const UID: u32 = fourcc(b"uid ");
    const STREAMS: u32 = fourcc(b"stm#");
    const IS_HIDDEN: u32 = fourcc(b"hidn");
    const TRANSLATE_UID: u32 = fourcc(b"uidd");
    const TRANSPORT: u32 = fourcc(b"tran");
    const SCOPE_GLOBAL: u32 = fourcc(b"glob");
    const SCOPE_INPUT: u32 = fourcc(b"inpt");
    const SCOPE_OUTPUT: u32 = fourcc(b"outp");
    const ELEMENT_MAIN: u32 = 0;
    const UTF8: u32 = 0x0800_0100;

    #[link(name = "CoreAudio", kind = "framework")]
    unsafe extern "C" {
        fn AudioObjectGetPropertyDataSize(
            id: AudioObjectId,
            addr: *const Address,
            qualifier_size: u32,
            qualifier: *const c_void,
            out_size: *mut u32,
        ) -> i32;
        fn AudioObjectGetPropertyData(
            id: AudioObjectId,
            addr: *const Address,
            qualifier_size: u32,
            qualifier: *const c_void,
            io_size: *mut u32,
            out: *mut c_void,
        ) -> i32;
        fn AudioObjectSetPropertyData(
            id: AudioObjectId,
            addr: *const Address,
            qualifier_size: u32,
            qualifier: *const c_void,
            size: u32,
            data: *const c_void,
        ) -> i32;
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        fn CFStringGetLength(s: CFStringRef) -> isize;
        fn CFStringGetMaximumSizeForEncoding(len: isize, enc: u32) -> isize;
        fn CFStringGetCString(s: CFStringRef, buf: *mut u8, size: isize, enc: u32) -> bool;
        fn CFStringCreateWithBytes(
            alloc: *const c_void,
            bytes: *const u8,
            len: isize,
            enc: u32,
            external: bool,
        ) -> CFStringRef;
        fn CFRelease(obj: *const c_void);
    }

    fn addr(selector: u32, scope: u32) -> Address {
        Address { selector, scope, element: ELEMENT_MAIN }
    }

    fn check(status: i32, what: &str) -> Result<(), String> {
        if status == 0 { Ok(()) } else { Err(format!("{what}: OSStatus {status}")) }
    }

    fn get_u32(id: AudioObjectId, a: &Address) -> Option<u32> {
        let mut v = 0u32;
        let mut size = size_of::<u32>() as u32;
        // SAFETY: out buffer is a u32 and size says so.
        let st = unsafe { AudioObjectGetPropertyData(id, a, 0, ptr::null(), &mut size, (&raw mut v).cast()) };
        (st == 0).then_some(v)
    }

    fn get_string(id: AudioObjectId, a: &Address) -> Option<String> {
        let mut s: CFStringRef = ptr::null();
        let mut size = size_of::<CFStringRef>() as u32;
        // SAFETY: the property yields a +1 CFStringRef, released below.
        let st = unsafe { AudioObjectGetPropertyData(id, a, 0, ptr::null(), &mut size, (&raw mut s).cast()) };
        if st != 0 || s.is_null() {
            return None;
        }
        let out = unsafe { cfstring_to_string(s) };
        unsafe { CFRelease(s) };
        out
    }

    unsafe fn cfstring_to_string(s: CFStringRef) -> Option<String> {
        unsafe {
            let cap = CFStringGetMaximumSizeForEncoding(CFStringGetLength(s), UTF8) + 1;
            let mut buf = vec![0u8; cap as usize];
            if !CFStringGetCString(s, buf.as_mut_ptr(), cap, UTF8) {
                return None;
            }
            let end = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
            buf.truncate(end);
            String::from_utf8(buf).ok()
        }
    }

    fn has_streams(id: AudioObjectId, scope: u32) -> bool {
        let mut size = 0u32;
        // SAFETY: size query only.
        let st = unsafe { AudioObjectGetPropertyDataSize(id, &addr(STREAMS, scope), 0, ptr::null(), &mut size) };
        st == 0 && size > 0
    }

    fn device_ids() -> Result<Vec<AudioObjectId>, String> {
        let a = addr(DEVICES, SCOPE_GLOBAL);
        let mut size = 0u32;
        // SAFETY: size query, then a buffer of exactly that many bytes.
        check(unsafe { AudioObjectGetPropertyDataSize(SYSTEM_OBJECT, &a, 0, ptr::null(), &mut size) }, "list devices")?;
        let mut ids = vec![0 as AudioObjectId; size as usize / size_of::<AudioObjectId>()];
        check(
            unsafe {
                AudioObjectGetPropertyData(SYSTEM_OBJECT, &a, 0, ptr::null(), &mut size, ids.as_mut_ptr().cast())
            },
            "list devices",
        )?;
        ids.truncate(size as usize / size_of::<AudioObjectId>());
        Ok(ids)
    }

    pub fn device_for_uid(uid: &str) -> Result<AudioObjectId, String> {
        // SAFETY: CFString built from a valid UTF-8 slice, released after use.
        let cf = unsafe { CFStringCreateWithBytes(ptr::null(), uid.as_ptr(), uid.len() as isize, UTF8, false) };
        if cf.is_null() {
            return Err("bad device uid".into());
        }
        let mut id: AudioObjectId = 0;
        let mut size = size_of::<AudioObjectId>() as u32;
        let st = unsafe {
            AudioObjectGetPropertyData(
                SYSTEM_OBJECT,
                &addr(TRANSLATE_UID, SCOPE_GLOBAL),
                size_of::<CFStringRef>() as u32,
                (&raw const cf).cast(),
                &mut size,
                (&raw mut id).cast(),
            )
        };
        unsafe { CFRelease(cf) };
        check(st, "find device")?;
        if id == 0 { Err(format!("device {uid} is gone")) } else { Ok(id) }
    }

    fn transport(code: Option<u32>) -> Transport {
        match code.map(u32::to_be_bytes).as_ref() {
            Some(b"bltn") => Transport::BuiltIn,
            Some(b"usb ") => Transport::Usb,
            Some(b"blue" | b"blea") => Transport::Bluetooth,
            Some(b"hdmi") => Transport::Hdmi,
            Some(b"dprt") => Transport::DisplayPort,
            Some(b"thun") => Transport::Thunderbolt,
            Some(b"airp") => Transport::AirPlay,
            Some(b"virt") => Transport::Virtual,
            Some(b"grup") => Transport::Aggregate,
            _ => Transport::Other,
        }
    }

    pub fn devices() -> Result<Vec<Device>, String> {
        let global = |sel| addr(sel, SCOPE_GLOBAL);
        let default_out = get_u32(SYSTEM_OBJECT, &global(DEFAULT_OUTPUT));
        let default_in = get_u32(SYSTEM_OBJECT, &global(DEFAULT_INPUT));
        let mut out = Vec::new();
        for id in device_ids()? {
            if get_u32(id, &global(IS_HIDDEN)) == Some(1) {
                continue;
            }
            let (Some(uid), Some(name)) = (get_string(id, &global(UID)), get_string(id, &global(NAME))) else {
                continue;
            };
            // Private aggregates CoreAudio creates for apps.
            if uid.starts_with("CADefaultDeviceAggregate") {
                continue;
            }
            let transport = transport(get_u32(id, &global(TRANSPORT)));
            let (has_out, has_in) = (has_streams(id, SCOPE_OUTPUT), has_streams(id, SCOPE_INPUT));
            for (direction, present, default) in
                [(Direction::Output, has_out, default_out), (Direction::Input, has_in, default_in)]
            {
                if present {
                    out.push(Device {
                        uid: uid.clone(),
                        name: name.clone(),
                        direction,
                        is_default: default == Some(id),
                        transport,
                        duplex: has_out && has_in,
                    });
                }
            }
        }
        Ok(out)
    }

    pub fn set_default(uid: &str, direction: Direction) -> Result<(), String> {
        let id = device_for_uid(uid)?;
        let selector = match direction {
            Direction::Output => DEFAULT_OUTPUT,
            Direction::Input => DEFAULT_INPUT,
        };
        // SAFETY: the property takes one AudioObjectID.
        let st = unsafe {
            AudioObjectSetPropertyData(
                SYSTEM_OBJECT,
                &addr(selector, SCOPE_GLOBAL),
                0,
                ptr::null(),
                size_of::<AudioObjectId>() as u32,
                (&raw const id).cast(),
            )
        };
        check(st, "set default device")
    }
}

#[cfg(test)]
mod tests {
    use barkley_plugin::Action;

    use super::*;

    fn dev(uid: &str, name: &str, direction: Direction, is_default: bool) -> Device {
        Device { uid: uid.into(), name: name.into(), direction, is_default, transport: Transport::Other, duplex: false }
    }

    fn symbol(name: &str, direction: Direction, transport: Transport, duplex: bool) -> &'static str {
        Device { duplex, transport, ..dev("u", name, direction, false) }.symbol()
    }

    #[test]
    fn device_symbols() {
        use Direction::*;
        use Transport::*;
        assert_eq!(symbol("Razer Nari Essential", Output, Usb, true), "headphones");
        assert_eq!(symbol("Razer Nari Essential", Input, Usb, true), "headphones");
        assert_eq!(symbol("Brio 500", Input, Usb, false), "web.camera");
        assert_eq!(symbol("LG ULTRAFINE", Output, DisplayPort, false), "display");
        assert_eq!(symbol("LG HDR 4K", Output, Hdmi, false), "display");
        assert_eq!(symbol("MacBook Air Speakers", Output, BuiltIn, false), "speaker.wave.2");
        assert_eq!(symbol("External Headphones", Output, BuiltIn, false), "headphones");
        assert_eq!(symbol("MacBook Air Microphone", Input, BuiltIn, false), "mic");
        assert_eq!(symbol("Sam's AirPods Pro", Output, Bluetooth, true), "airpods.pro");
        assert_eq!(symbol("Sony WH-1000XM5", Output, Bluetooth, true), "headphones");
        assert_eq!(symbol("Living Room", Output, AirPlay, false), "airplay.audio");
        assert_eq!(symbol("BlackHole 2ch", Output, Virtual, true), "waveform");
        assert_eq!(symbol("Focusrite Scarlett", Output, Usb, false), "hifispeaker");
        assert_eq!(symbol("Yeti", Input, Usb, false), "mic");
    }

    fn fake() -> Vec<Device> {
        vec![
            dev("spk", "MacBook Pro Speakers", Direction::Output, true),
            dev("pods", "AirPods Pro", Direction::Output, false),
            dev("pods", "AirPods Pro", Direction::Input, false),
            dev("mic", "MacBook Pro Microphone", Direction::Input, true),
        ]
    }

    fn sorted(mut items: Vec<Item>) -> Vec<Item> {
        items.sort_by_key(|i| std::cmp::Reverse(i.score));
        items
    }

    fn titles(items: &[Item]) -> Vec<&str> {
        items.iter().map(|i| i.title.as_str()).collect()
    }

    #[test]
    fn lists_outputs_then_inputs_current_first() {
        let items = sorted(items(&fake(), &[], "", "/x/barkley-audio"));
        assert_eq!(titles(&items), ["MacBook Pro Speakers", "AirPods Pro", "MacBook Pro Microphone", "AirPods Pro"]);
        assert_eq!(items[0].action, None);
        assert_eq!(items[0].tag.as_deref(), Some("Current"));
        assert_eq!(items[1].tag, None);
        assert!(items.iter().all(|i| i.icon.is_some()));
        let set = |dir: &str| {
            Some(Action::Run { cmd: "/x/barkley-audio".into(), args: vec!["--set".into(), dir.into(), "pods".into()] })
        };
        assert_eq!(items[1].action, set("out"));
        assert_eq!(items[3].action, set("in"));
    }

    fn paired(address: &str, name: &str) -> Paired {
        Paired { address: address.into(), name: name.into(), speaker: false, last_used_days: Some(3) }
    }

    #[test]
    fn offers_to_connect_paired_devices_coreaudio_lacks() {
        let mut devices = fake();
        devices.push(dev("0A-1B-2C-3D-4E-5F:output", "Studio Buds", Direction::Output, false));
        let bt = [paired("0a-1b-2c-3d-4e-5f", "Studio Buds"), paired("f0-e1-d2-c3-b4-a5", "Sam's AirPods Max")];
        let list = sorted(items(&devices, &bt, "", "/x/barkley-audio"));
        let offline: Vec<_> = list.iter().filter(|i| i.subtitle.starts_with("Not connected")).collect();
        assert_eq!(offline.len(), 1, "connected Studio Buds isn't offered twice: {list:?}");
        assert_eq!(offline[0].title, "Sam's AirPods Max");
        assert_eq!(offline[0].verb.as_deref(), Some("Connect"));
        assert_eq!(offline[0].subtitle, "Not connected \u{b7} used 3 days ago");
        assert_eq!(offline[0].icon, Some(barkley_plugin::Icon::Symbol { symbol: "airpods.max".into() }));
        assert_eq!(
            offline[0].action,
            Some(Action::Run {
                cmd: "/x/barkley-audio".into(),
                args: vec!["--connect".into(), "f0-e1-d2-c3-b4-a5".into()]
            })
        );
        // Below connected outputs, above inputs.
        let pos = |t: &str| list.iter().position(|i| i.title == t).unwrap();
        assert!(pos("Studio Buds") < pos("Sam's AirPods Max"));
        assert!(pos("Sam's AirPods Max") < pos("MacBook Pro Microphone"));
        // Only outputs can be connected this way.
        assert!(!titles(&items(&devices, &bt, "in", "x")).contains(&"Sam's AirPods Max"));
        assert_eq!(titles(&items(&devices, &bt, "max", "x")), ["Sam's AirPods Max"]);
    }

    #[test]
    fn last_used_labels() {
        assert_eq!(last_used(None), "never used on this Mac");
        assert_eq!(last_used(Some(0)), "used today");
        assert_eq!(last_used(Some(1)), "used yesterday");
        assert_eq!(last_used(Some(12)), "used 12 days ago");
        assert_eq!(last_used(Some(400)), "used 13 months ago");
    }

    #[test]
    fn bluetooth_addresses_compare_across_formats() {
        assert_eq!(bt_address("0A-1B-2C-3D-4E-5F:output"), bt_address("0a-1b-2c-3d-4e-5f"));
        assert_eq!(bt_address("00:f3:9f:6d:41:89"), None, "colon form isn't what either API returns");
        assert_eq!(bt_address("BuiltInSpeakerDevice"), None);
    }

    #[test]
    fn direction_filter() {
        let items = items(&fake(), &[], "in", "x");
        assert!(items.iter().all(|i| i.subtitle.starts_with("Input")), "{items:?}");
        assert_eq!(items.len(), 2);
    }

    #[test]
    fn fuzzy_filter() {
        assert_eq!(titles(&items(&fake(), &[], "out pods", "x")), ["AirPods Pro"]);
        assert_eq!(items(&fake(), &[], "zzz", "x")[0].title, "No matching audio devices");
    }

    #[test]
    fn fuzzy_prefers_tight_prefix_matches() {
        assert!(fuzzy("air", "AirPods Pro") > fuzzy("air", "MacBook Air Speakers"));
        assert_eq!(fuzzy("xyz", "AirPods"), None);
        assert!(fuzzy("", "anything").is_some());
    }

    /// Read-only: talks to the real HAL but never changes the default device.
    #[test]
    fn coreaudio_lists_devices() {
        let devices = coreaudio::devices().expect("CoreAudio device list");
        for d in &devices {
            assert!(!d.uid.is_empty() && !d.name.is_empty(), "{d:?}");
            // The lookup `--set` uses before switching.
            assert!(coreaudio::device_for_uid(&d.uid).is_ok(), "{d:?}");
        }
        assert!(coreaudio::device_for_uid("no-such-device").is_err());
        for dir in [Direction::Output, Direction::Input] {
            let defaults = devices.iter().filter(|d| d.direction == dir && d.is_default).count();
            assert!(defaults <= 1, "{dir:?}: {devices:?}");
        }
    }
}
