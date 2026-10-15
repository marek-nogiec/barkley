# Barkley

A tiny Alfred-style launcher for macOS 26+ (Liquid Glass UI), written in Rust with AppKit via `objc2`.
It sits in the menu bar. Double-tap ⌘ (or press ⌥Space) to open a search panel.

## Build & run

```sh
scripts/bundle.sh          # release build → target/Barkley.app
open target/Barkley.app
```

`scripts/bundle.sh --debug` builds a debug bundle instead. To see logs, run the binary directly:
`target/Barkley.app/Contents/MacOS/Barkley`.

**Signing (one-time):** run `scripts/dev-cert.sh`. It creates a self-signed "Barkley Dev" code-signing
identity in its own keychain (`~/Library/Keychains/barkley-dev.keychain-db`), and `bundle.sh` signs
with it from then on. The signature stays the same across rebuilds, so macOS keeps the Accessibility
grant. To use a real identity instead, set `BARKLEY_SIGN_ID="Apple Development: …"`. Without either,
the app is signed ad-hoc and loses the grant on every rebuild.

**Accessibility:** double-tap ⌘ needs it, and Barkley asks for it on first launch. The ⌥Space chord
needs no permission.

**Icon:** `assets/AppIcon.svg` is the source; `scripts/make-icon.sh` renders `AppIcon.png`/`AppIcon.icns` (needs librsvg + iconutil). Both are checked in, so
normal builds need neither.

## Using it

| Key | Does |
|---|---|
| double-tap ⌘ / ⌥Space | toggle panel |
| ↑ ↓ | move selection |
| Enter / click | run selected item |
| ⌘1 … ⌘9 | run that row |
| Esc / click outside | hide |

Bundled plugins (ordinary plugins in `plugins/`, shipped inside the app):

- **Web search** (`plugins/web-search`, sh): `g rust lang` opens a search. Any query that nothing else matched also gets a "Search Google for …" row.
- **Audio** (`plugins/audio`, Rust): `audio` lists output and input devices, with the current ones tagged "Current". Enter switches to the selected device. Narrow the list with `audio out …` or `audio in airpods`. Paired Bluetooth headphones that aren't connected show up too; Enter connects them and makes them the output. Listing them needs Bluetooth permission, which macOS asks for the first time you type `audio`.
- **Units** (`plugins/units`, Rust): `10 km to mi`, `100 f in c`, `2 gb to mb`, `1 GiB as MB`. Covers length, mass, volume, temperature and data. Enter copies the number.

## Config

`~/Library/Application Support/Barkley/config.toml` is created on first run. Open it from the menu-bar
icon → **Open Config File**. **Reload Config & Plugins** applies plugin changes. Hotkey changes need a
restart.

```toml
[hotkey]
double_cmd = true          # needs Accessibility
chord = "alt+space"        # "" disables; e.g. "cmd+shift+space"

[plugins]
dir = "~/Library/Application Support/Barkley/plugins"

# Per-plugin overrides, keyed by folder name (bundled or yours)
[plugin.web-search]
keyword = "s"              # "" removes the keyword
env = { BARKLEY_SEARCH_URL = "https://duckduckgo.com/?q={query}", BARKLEY_SEARCH_NAME = "DuckDuckGo" }

[plugin.audio]
enabled = false
```

A folder in your plugins dir with the same name as a bundled plugin replaces it.

## Writing plugins

Every plugin, bundled or yours, is a folder with a `manifest.toml` and an executable:

```toml
name = "Echo"
keyword = "echo"     # optional; without it the plugin sees every keyword-less query
run = "./run.sh"     # relative to the plugin folder, exec'd directly (needs chmod +x)
timeout_ms = 1500    # optional, default 1500
fallback = false     # optional; true = also answers queries nothing else matched
persistent = false   # optional; see below
[env]                # optional; config.toml's [plugin.<name>.env] overrides it
GREETING = "hi"
```

The query, with the keyword stripped, is passed as `argv[1]` and as `$BARKLEY_QUERY`. The working
directory is the plugin folder. Print JSON to stdout:

```json
{"items": [
  {"title": "Copy me",   "subtitle": "…", "action": {"type": "copy", "text": "hello"}},
  {"title": "Open site", "action": {"type": "open_url", "url": "https://example.com"}},
  {"title": "Run it",    "action": {"type": "run", "cmd": "/usr/bin/say", "args": ["hi"]}},
  {"title": "Info only", "score": 5, "icon": {"symbol": "info.circle"}},
  {"title": "Custom art", "icon": {"path": "icon.png"}}
]}
```

- `subtitle`, `action`, `score`, `icon` and `verb` are optional. `icon` is either an SF Symbol name or an image path relative to the plugin folder. Without `score`, items keep the order you printed them in. `verb` labels the ↩ hint (default: Open / Copy / Run).
- A non-zero exit, invalid JSON or a timeout shows a red error row. The process group gets killed at `timeout_ms`.
- A broken `manifest.toml` shows up as an error row when the panel opens with an empty query.

**Persistent mode.** With `persistent = true`, Barkley starts the plugin once with
`BARKLEY_PERSISTENT=1` and keeps it running. Each query arrives as a line on stdin,
`{"id": 7, "query": "…"}`, and the plugin answers with one line, `{"id": 7, "items": […]}`. Use it
when startup is expensive. The audio plugin does, because CoreAudio's handshake costs ~80 ms per
process. A timeout or crash kills the process, and it restarts on the next query.

**Rust or shell?** Shell is fine for formatting a string (web search starts in ~4 ms). Use Rust
when there's real logic or a system API: units (~2 ms) and audio (CoreAudio, IOBluetooth). Rust
plugins depend on `crates/barkley-plugin`, where `barkley_plugin::run(|query| items)` handles both
modes. Add the crate to the workspace `members`, and `bundle.sh` builds it and ships the binary
next to its manifest.

Try the example: `cp -R plugins/example-echo ~/Library/Application\ Support/Barkley/plugins/`, then
**Reload Config & Plugins**, then type `echo hi`.

## Layout

- `crates/barkley-core` contains query parsing, dispatch and ranking (nucleo), settings, and the plugin runner (one-shot and persistent). It has no AppKit and is fully unit-tested.
- `crates/barkley-plugin` is the small SDK for writing plugins in Rust: protocol types plus `run()`.
- `plugins/*` holds the bundled plugins. `example-echo` is a template and isn't shipped.
- `crates/barkley-app` is the macOS shell: status item, `NSPanel`, hotkeys, actions, and the background-query → main-queue bridge.

## Checks

```sh
cargo test --workspace && cargo clippy --workspace --all-targets && cargo deny check
```

Debug builds also take a smoke hook. After `scripts/bundle.sh --debug`, running
`BARKLEY_SMOKE="10 km to mi" target/Barkley.app/Contents/MacOS/Barkley` opens the
panel, runs the query through the real UI, logs the rows and exits. It takes keyboard focus, so don't
type while it runs.

CI runs the same checks on every pull request. Every merge to `main` publishes the app bundle as a
GitHub release (`v<version>-r<run>`, Apple Silicon, ad-hoc signed).

## License

GPL-3.0-or-later. See [LICENSE](LICENSE).
