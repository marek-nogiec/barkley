# Contributing

Contributions are welcome through GitHub issues and pull requests. New plugins
are especially welcome; see [Plugins](README.md#plugins) for the protocol.

## Before opening a pull request

You need macOS 26+ with Xcode 26 (for the Liquid Glass SDK) and the Rust
toolchain pinned in `rust-toolchain.toml`. Optionally install
[Cocogitto](https://docs.cocogitto.io/) and run `cog install-hook commit-msg`
to enable the Conventional Commit check.

Run the same checks as CI:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo deny check
```

To try the app, build it with `scripts/bundle.sh --debug` and run
`target/Barkley.app/Contents/MacOS/Barkley`. Running `scripts/dev-cert.sh`
once keeps macOS permissions across rebuilds.

Keep changes focused, add tests for behavior changes, and update documentation
when user-visible behavior changes. Use a Conventional Commit subject such as
`fix(audio): skip hidden devices` or `feat: add a timezone plugin`.

## Pull requests

Describe the problem and solution, list the checks you ran, and call out any
new permissions (Info.plist usage strings), dependency, release, or licensing
impact. Do not commit build output, generated files, or credentials.

By contributing, you agree that your contribution is licensed under
GPL-3.0-or-later.

Report security concerns according to [SECURITY.md](SECURITY.md), not through a
public issue.
