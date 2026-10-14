# Configuration

The configuration is handled in the [`zpm-config` crate](../../packages/zpm-config). It's generated from the [`schema.json`](../../packages/zpm-config/schema.json) file via the [`build.rs`](../../packages/zpm-config/build.rs) file ([Cargo documentation](https://doc.rust-lang.org/cargo/reference/build-scripts.html)).

All values are wrapped in a `Setting<T>` container type which lets you know where the setting comes from.

## Install config hash

Every top-level setting contributes to the install config hash stored in the install state. When the hash changes, the next command runs a full install instead of taking the up-to-date fast path.

A setting can opt out with `"affectsInstall": false` in `schema.json`. Only do so if changing the setting is guaranteed to have no effect on the install (output formatting, validation-only checks, daemon tuning, ...). When in doubt, leave it in: an extra install is cheap, a stale install isn't.
