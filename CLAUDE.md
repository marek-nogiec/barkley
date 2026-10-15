# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

Barkley is an Alfred-style menu-bar launcher for macOS 26+ (Liquid Glass UI), written in Rust with
AppKit via `objc2`. Every feature comes from a plugin. Users read `README.md`; this file covers
what isn't obvious from it.

## Commands

```sh
cargo test --workspace                                      # all tests (macOS only: CoreAudio, AppKit)
cargo test -p barkley-core dispatch::tests::bare_keyword    # single test (path filter)
cargo test -p barkley-audio                                 # one crate
cargo fmt --all                                             # rustfmt.toml: max_width 120; CI checks it
cargo clippy --workspace --all-targets -- -D warnings       # CI fails on any warning
cargo deny check                                            # licenses, advisories, duplicate versions
scripts/bundle.sh [--debug]                                 # → target/Barkley.app (builds app + Rust plugins, stages, signs)
```

Run the real app only from the bundle. `target/debug/barkley` finds no plugins, because bundled
plugins are resolved from `<exe>/../Resources/plugins` (`bundled_plugin_dirs` in `controller.rs`).

**Smoke hook (debug builds):** `BARKLEY_SMOKE="<query>" target/Barkley.app/Contents/MacOS/Barkley`
shows the panel, runs the query through the real UI, prints rows to stderr and exits. It never runs
an action. It takes keyboard focus. `BARKLEY_SMOKE_PNG=<path>` also writes a snapshot.

**Privacy-gated plugins** (Bluetooth in audio, Apple Events in empty-trash) get killed by TCC when
launched from a shell, because the responsible process is the terminal, which has no usage string.
Test them through LaunchServices so Barkley's `Info.plist` applies:

```sh
open -W -n --env BARKLEY_SMOKE=audio --stderr /tmp/out.txt target/Barkley.app
```

Never trigger destructive plugin actions while testing (emptying the Trash, killing processes,
switching audio devices). They act on the user's real machine.

## Architecture

```
crates/barkley-plugin   SDK + wire protocol types (serde). Plugins depend on it; core parses with it.
crates/barkley-core     Query parsing, Dispatcher (routing + ranking), Settings, plugin runner. No AppKit.
crates/barkley-app      `barkley` binary: AppKit shell (objc2). Depends on core.
plugins/*               Bundled plugins: folder + manifest.toml (+ Cargo crate for Rust ones).
```

**There are no built-in plugins.** Everything, including what ships in the app, goes through
`script_plugin.rs`: spawn the executable with the query as `argv[1]` + `$BARKLEY_QUERY`, parse JSON
stdout, kill the process group on timeout. `persistent = true` keeps one process alive
(`BARKLEY_PERSISTENT=1`, line-delimited `{"id","query"}` → `{"id","items"}`), restarting it on
timeout or exit. `load_all` merges plugin dirs in order: bundled first, then the user dir. A later
folder with the same name wins, and `[plugin.<folder>]` overrides in config.toml are applied at load.

**Dispatch** (`dispatch.rs`):
- A query whose first word is a plugin keyword goes only to that plugin. The bare keyword alone
  also routes there, with empty args.
- Otherwise every keyword-less, non-fallback plugin runs in parallel (`thread::scope`). Fallback
  plugins run only if those return nothing.
- Ranking: plugin score descending, then nucleo fuzzy match on title. Capped at `MAX_RESULTS` (9).

**Adding a protocol field** touches four places: `barkley_plugin::Item` (wire format + builder),
`barkley_core::plugin::Item`, `convert()` in `script_plugin.rs`, and rendering in `panel.rs`
(`make_cell`). `verb` and `tag` are examples.

**App threading:**
- UI is main-thread only.
- `bridge.rs` runs each search on a thread and hops results back with
  `dispatch2::DispatchQueue::main()`. A generation counter in `Dispatcher` drops stale results.
- `actions::run` spawns commands detached. On non-zero exit it posts the last stderr line back to
  the main queue → `controller::action_failed`, which reopens the panel with an error row.
- The `Controller` lives in a `thread_local`, reached through free functions (`toggle_panel`,
  `run_row`, `hover`, …) that `Panel` overrides and callbacks call.

**Hotkeys** (`hotkey.rs`): double-⌘ is a pure state machine (`DoubleTap`, unit-tested) fed by NSEvent
global and local monitors. It needs Accessibility, which is polled until granted. The ⌥Space chord
uses the `global-hotkey` crate and needs no permission.

## objc2 / AppKit gotchas

- Inside `define_class!` methods, an explicit `return` breaks type inference (Bool vs bool). Put
  logic in plain helper methods and call them.
- Init subclasses with
  `msg_send![super(Self::alloc(mtm).set_ivars(...)), init...]`.
- `objc2-app-kit`, `objc2-foundation` and `objc2-quartz-core` use trimmed feature lists in
  `crates/barkley-app/Cargo.toml`. A missing method or type usually means a feature to add there,
  not an API that doesn't exist. New objc2-family deps must match the existing versions, because
  cargo-deny bans duplicate crate versions.
- Animations live in `anim.rs` and respect Reduce Motion. The panel is one `NSGlassEffectView`.
  The window stays a fixed size, and only the glass frame animates (`fit_rows`).
- Keep `[profile.release.build-override] strip = false`. Stripping proc-macro dylibs breaks release
  builds ("mis-aligned LINKEDIT").

## Signing and permissions

`scripts/dev-cert.sh` (one-time) creates a self-signed "Barkley Dev" identity in
`~/Library/Keychains/barkley-dev.keychain-db`. `bundle.sh` signs nested plugin binaries, then the
app, with it. A stable identity is what keeps the Accessibility / Bluetooth / Automation grants
across rebuilds. CI has no identity and signs ad-hoc. New privacy-sensitive APIs need a usage
string in the `Info.plist` heredoc in `bundle.sh`; without one, TCC kills the process.

## Conventions and release

- Conventional Commits (`feat(scope): …`), checked by cocogitto in CI (`cog.toml`). `main` is
  protected by a ruleset: PR only, squash or rebase, checks "Test", "Deny" and
  "Check conventional commits" must pass.
- `ci.yml` runs on PRs: fmt, clippy and tests on `macos-26`; cargo-deny and the commit check on
  Linux.
- `release.yml` runs on every push to `main` and publishes `Barkley-v<version>-r<run>-arm64.zip`.
  `pages.yml` then redeploys `docs/` to https://nogiec.org/barkley/ with the new download link
  (`scripts/prepare-pages.sh`, which expects exactly 3 `data-app-download` links).
- `docs/og.png` is rendered from `assets/og.html`; the regenerate command is in that file.
- Shell vs Rust for plugins: shell for string formatting or wrapping a CLI (~4 ms spawn); Rust for
  real logic or system APIs (~2 ms spawn). Keyword-less plugins run on every keystroke, so they must
  return `{"items":[]}` fast when the query isn't for them.
- License: GPL-3.0-or-later. Workspace crates are `publish = false`, which lets cargo-deny skip them.
