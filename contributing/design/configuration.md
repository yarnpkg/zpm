# Configuration

The configuration is handled in the [`zpm-config` crate](../../packages/zpm-config). It's generated from the [`schema.json`](../../packages/zpm-config/schema.json) file via the [`build.rs`](../../packages/zpm-config/build.rs) file ([Cargo documentation](https://doc.rust-lang.org/cargo/reference/build-scripts.html)).

All values are wrapped in a `Setting<T>` container type which lets you know where the setting comes from.
