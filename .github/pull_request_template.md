## Summary

<!-- What changed, and why? -->

## Verification

<!-- List the checks you ran. -->

- [ ] `cargo fmt --all -- --check`
- [ ] `cargo clippy --workspace --all-targets --locked -- -D warnings`
- [ ] `cargo test --workspace --locked`
- [ ] Tried it in the app (`scripts/bundle.sh --debug`) if it changes UI or a plugin

## Checklist

- [ ] I added or updated tests for behavior changes.
- [ ] I updated user-facing documentation where needed.
- [ ] I considered permission, dependency, release, and licensing impact.
- [ ] This change contains no credentials or generated output.
