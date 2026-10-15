# Barkley

[Website](https://nogiec.org/barkley/) ·
[Downloads](https://github.com/marek-nogiec/barkley/releases/latest) ·
[Source code](https://github.com/marek-nogiec/barkley)

[![Buy Me a Coffee](https://img.shields.io/badge/Buy_Me_a_Coffee-marocreates-FFDD00?logo=buymeacoffee&logoColor=000000)](https://www.buymeacoffee.com/marocreates)

A tiny Alfred-style launcher for macOS 26+ with a Liquid Glass UI, written in Rust (AppKit via
`objc2`). It sits in the menu bar. Double-tap ⌘ (or press ⌥Space), type, hit Enter.

Everything it can do comes from plugins: small programs that turn what you type into a list of rows.
Plugins can be shell scripts or Rust binaries. The bundled ones (web search, units, audio
devices, kill, empty trash) are ordinary plugins, built the same way you'd build your own.

## Install

Download the latest `Barkley-*-arm64.zip` from [Releases](https://github.com/marek-nogiec/barkley/releases/latest) (Apple Silicon, macOS 26+),
unzip it and move `Barkley.app` to `/Applications`. Releases are not notarized, so the first launch
needs one of these:

```sh
xattr -dr com.apple.quarantine /Applications/Barkley.app
```

or right-click → **Open** the first time.

Double-tap ⌘ needs Accessibility permission, and Barkley asks for it on first launch. ⌥Space works
without any permission.

To start Barkley when you log in, tick menu-bar icon → **Launch at Login**. It then shows up in
System Settings → General → Login Items.

## Using it

| Key | Does |
|---|---|
| double-tap ⌘ / ⌥Space | toggle the panel |
| ↑ ↓ or hover | move the selection |
| Enter / click | run the selected row |
| ⌘1 … ⌘9 | run that row (hold ⌘ to see the numbers) |
| Esc / click outside | hide |

Bundled plugins:

| Type | Plugin | Does |
|---|---|---|
| `g rust lang` | Web search | Opens a Google search. Any query nothing else matched also gets a "Search Google for …" row. |
| `10 km to mi`, `100 f in c`, `2 gb to mb` | Units | Converts length, mass, volume, temperature and data. Enter copies the number. |
| `audio`, `audio out`, `audio in airpods` | Audio | Lists input/output devices and switches the default. Also lists paired Bluetooth headphones that aren't connected; Enter connects them. Needs Bluetooth permission (asked on first use). |
| `kill slack` | Kill | Lists matching processes; Enter quits the selected one (SIGTERM). |
| `empty trash`, `trash` | Empty Trash | Shows how many items are in the Trash; Enter empties it. Asks Finder, so macOS asks for Automation permission once. |

## Config

`~/Library/Application Support/Barkley/config.toml` is created on first run. Open it from the
menu-bar icon → **Open Config File**. **Reload Config & Plugins** applies plugin changes. Hotkey
changes need a restart.

```toml
[hotkey]
double_cmd = true          # needs Accessibility
chord = "alt+space"        # "" disables; e.g. "cmd+shift+space"

[plugins]
dir = "~/Library/Application Support/Barkley/plugins"

# Per-plugin overrides, keyed by plugin folder name (bundled or yours)
[plugin.web-search]
keyword = "s"              # "" removes the keyword
env = { BARKLEY_SEARCH_URL = "https://duckduckgo.com/?q={query}", BARKLEY_SEARCH_NAME = "DuckDuckGo" }

[plugin.audio]
enabled = false
```

A folder in your plugins dir with the same name as a bundled plugin replaces the bundled one.

## Plugins

### What can be a plugin

A plugin gets the typed text and answers with up to a few rows. Each row has a title, subtitle,
icon, and one action: **open a URL**, **copy text**, or **run a command**. Anything that fits that
shape works:

- **Lookups:** convert, calculate, look up a word or a timezone, generate a UUID or password.
- **Search:** your repos (`gh repo list`), projects on disk, bookmarks, Homebrew formulae, docs.
- **Control:** quit a process, switch audio output, toggle dark mode, connect a VPN, run a script.
- **Launch:** open a project in your editor, open a URL with the query filled in.

Not a fit (yet): custom UI beyond rows, previews, rows that update on their own while you watch,
or work that keeps running in the background with nothing typed. A command started by `run` can do
anything once it's launched, though. If it fails, Barkley reopens with its last stderr line.

### Add one in two minutes

A plugin is a folder in `~/Library/Application Support/Barkley/plugins/` (menu bar → **Open
Plugins Folder**) with a `manifest.toml` and an executable. Here's the whole bundled
[`kill`](plugins/kill) plugin, which quits processes by name:

`kill/manifest.toml`:

```toml
name = "Kill"
keyword = "kill"
run = "./run.sh"
```

`kill/run.sh` (then `chmod +x run.sh`):

```sh
#!/bin/sh
# `kill <name>`: list matching processes; Enter quits the selected one (SIGTERM).
ps -axco pid=,comm= | LC_ALL=C awk '
  function json(s) { gsub(/\\/, "\\\\", s); gsub(/"/, "\\\"", s); return s }
  BEGIN { q = tolower(ENVIRON["BARKLEY_QUERY"]); printf "{\"items\":[" }
  { pid = $1; name = $0; sub(/^ *[0-9]+ /, "", name) }
  q != "" && index(tolower(name), q) {
    printf "%s{\"title\":\"%s\",\"subtitle\":\"PID %s\",\"verb\":\"Quit\",", sep, json(name), pid
    printf "\"icon\":{\"symbol\":\"xmark.octagon\"},\"action\":{\"type\":\"run\",\"cmd\":\"/bin/kill\",\"args\":[\"%s\"]}}", pid
    sep = ","
  }
  END {
    if (q == "") printf "{\"title\":\"Quit a process\",\"subtitle\":\"Type part of its name\",\"icon\":{\"symbol\":\"xmark.octagon\"}}"
    print "]}"
  }'
```

Save it under another folder name and keyword to make your own, then menu bar → **Reload Config &
Plugins**. Test it outside Barkley with `BARKLEY_QUERY=finder ./run.sh`. There's also a simpler starter in
[`plugins/example-echo`](plugins/example-echo).

### How a plugin is called

Barkley runs the executable with the query (keyword stripped) as `argv[1]` and as `$BARKLEY_QUERY`,
from inside the plugin folder, and reads JSON from stdout:

```json
{"items": [
  {"title": "Copy me",   "subtitle": "…", "action": {"type": "copy", "text": "hello"}},
  {"title": "Open site", "action": {"type": "open_url", "url": "https://example.com"}},
  {"title": "Run it",    "action": {"type": "run", "cmd": "/usr/bin/say", "args": ["hi"]}},
  {"title": "Info only", "score": 5, "icon": {"symbol": "info.circle"}, "tag": "Current"},
  {"title": "Custom art", "icon": {"path": "icon.png"}}
]}
```

| Field | |
|---|---|
| `title` | Required. |
| `subtitle` | Smaller second line. |
| `action` | `open_url` (`url`), `copy` (`text`) or `run` (`cmd`, `args`). Without it the row is info only. |
| `icon` | `{"symbol": "<SF Symbol name>"}` or `{"path": "<image relative to the plugin folder>"}`. |
| `score` | Higher ranks first. Without it, rows keep the order you printed them in. |
| `verb` | Label next to ↩ on the selected row (default: Open / Copy / Run). |
| `tag` | Short label at the right edge, e.g. "Current". |

A non-zero exit, invalid JSON or a timeout shows a red error row, and the process group gets killed
at `timeout_ms`.

### `manifest.toml`

```toml
name = "Echo"
keyword = "echo"     # optional; without it the plugin sees every keyword-less query
run = "./run.sh"     # relative to the plugin folder, exec'd directly (needs chmod +x)
timeout_ms = 1500    # optional, default 1500
fallback = false     # optional; true = also answers queries nothing else matched
persistent = false   # optional; see "Persistent mode"
[env]                # optional; config.toml's [plugin.<folder>.env] overrides it
GREETING = "hi"
```

Plugins without a keyword run on every query, so keep them fast and return no rows when the text
isn't for them (units does this). A plugin typed by its keyword alone (`audio`) gets an empty query.

### Rust plugins

Use Rust when there's real logic or a system API. The
[`barkley-plugin`](crates/barkley-plugin) crate has the protocol types and a `run()` that handles
both one-shot and persistent mode:

```toml
# Cargo.toml
[dependencies]
barkley-plugin = { git = "https://github.com/marek-nogiec/barkley" }
```

```rust
use barkley_plugin::Item;

fn main() {
    barkley_plugin::run(|query| {
        if query.is_empty() {
            return vec![Item::new("Type some text to convert")];
        }
        let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
        [
            ("UPPER", query.to_uppercase()),
            ("lower", query.to_lowercase()),
            ("snake_case", words.join("_")),
            ("kebab-case", words.join("-")),
        ]
        .into_iter()
        .map(|(style, text)| Item::new(&text).subtitle(style).symbol("textformat").copy(&text))
        .collect()
    });
}
```

`cargo build --release`, copy the binary into a plugin folder next to a manifest with
`run = "./your-binary"`, and reload.

**Shell or Rust?** Shell is fine for building a string or wrapping a CLI (web search starts in
~4 ms). Rust starts in ~2 ms and is the better choice for parsing, ranking or calling macOS APIs.

### Persistent mode

With `persistent = true`, Barkley starts the plugin once with `BARKLEY_PERSISTENT=1` and keeps it
running. Each query arrives as a line on stdin, `{"id": 7, "query": "…"}`, and the plugin answers
with one line, `{"id": 7, "items": […]}`. Use it when startup is expensive. The audio plugin does,
because CoreAudio's handshake costs ~80 ms per process. A timeout or crash kills the process, and it
restarts on the next query. `barkley_plugin::run` does all of this for you.

### Shipping a plugin with Barkley

Bundled plugins live in [`plugins/`](plugins). Add a folder there with its `manifest.toml`. For a
Rust plugin, also add its crate to `members` in the root `Cargo.toml`. `scripts/bundle.sh` copies
each folder (minus `src/`, `tests/` and `Cargo.toml`) into `Barkley.app/Contents/Resources/plugins/`,
builds Rust plugins and puts their binary next to the manifest, and signs them. Folders named
`example-*` are templates and aren't shipped.

## Development

```sh
scripts/bundle.sh          # release build → target/Barkley.app
open target/Barkley.app
```

`scripts/bundle.sh --debug` builds a debug bundle instead. To see logs, run
`target/Barkley.app/Contents/MacOS/Barkley` directly.

**Signing (one-time):** run `scripts/dev-cert.sh`. It creates a self-signed "Barkley Dev"
code-signing identity in its own keychain (`~/Library/Keychains/barkley-dev.keychain-db`), and
`bundle.sh` signs with it from then on. The signature stays the same across rebuilds, so macOS keeps
the Accessibility grant. To use a real identity instead, set `BARKLEY_SIGN_ID="Apple Development: …"`.
Without either, the app is signed ad-hoc and loses the grant on every rebuild.

**Icon:** `assets/AppIcon.svg` is the source. `scripts/make-icon.sh` renders
`AppIcon.png`/`AppIcon.icns` (needs librsvg and iconutil). Both are checked in, so normal builds need
neither.

**Layout:**

- `crates/barkley-core`: query parsing, dispatch and ranking (nucleo), settings, and the plugin runner (one-shot and persistent). No AppKit, fully unit-tested.
- `crates/barkley-plugin`: the SDK for writing plugins in Rust.
- `crates/barkley-app`: the macOS shell: status item, `NSPanel`, hotkeys, actions, and the background-query → main-queue bridge.
- `plugins/*`: the bundled plugins.

**Checks:**

```sh
cargo fmt --all --check && cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo deny check
```

Debug builds also take a smoke hook. After `scripts/bundle.sh --debug`, running
`BARKLEY_SMOKE="10 km to mi" target/Barkley.app/Contents/MacOS/Barkley` opens the panel, runs the
query through the real UI, logs the rows and exits. It takes keyboard focus, so don't type while it
runs.

CI runs the same checks on every pull request. Every merge to `main` publishes the app as a GitHub
release (`v<version>-r<run>`), and the website in [`docs/`](docs) is redeployed to GitHub Pages with the
new download link (`scripts/prepare-pages.sh`). `assets/og.html` is the source for the social preview
image `docs/og.png`.

## Contributing and security

Contributions, especially new plugins, are welcome. See [CONTRIBUTING.md](CONTRIBUTING.md). Report
suspected vulnerabilities privately as described in [SECURITY.md](SECURITY.md).

## License

GPL-3.0-or-later. See [LICENSE](LICENSE).
