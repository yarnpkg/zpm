pub mod config;
pub mod daemons;
mod errors;
mod http;
mod ipc;
mod manifest;
mod yarn_enums;
mod yarn;

pub use errors::{
  Error,
};

pub use ipc::YARNSW_PATH_ENV;

pub use manifest::{
    PackageManagerField,
    PackageManagerReference,
    VersionPackageManagerReference,
    find_closest_package_manager,
    resolve_detected_root,
};

pub use yarn_enums::{
    Channel,
    ReleaseLine,
    Selector,
};

pub use yarn::{
    BinMeta,
    extract_bin_meta,
    get_bin_version,
    get_default_yarn_version,
    resolve_channel_selector,
    resolve_semver_range,
    resolve_selector,
};
