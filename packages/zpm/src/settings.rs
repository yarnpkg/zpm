use std::{collections::BTreeMap, io::IsTerminal, str::FromStr};

use serde::{Deserialize, Serialize};
use zpm_macros::{FromToSerialize, yarn_config};
use zpm_semver::RangeKind;
use zpm_utils::{FromFileString, Path, ToFileString};

use crate::{
    config::{ConfigPaths, Password},
    config_fields::{BoolField, DictField, EnumField, Glob, GlobField, OptionalStringField, PathField, StringField, UintField, VecField},
    primitives::{
        descriptor::{descriptor_map_deserializer, descriptor_map_serializer},
        Descriptor, Ident, PeerRange, SemverDescriptor
    }
};

#[derive(Clone, Debug, Default, Deserialize, Serialize, FromToSerialize)]
#[serde(rename_all = "camelCase")]
pub struct NetworkSettings {
    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enable_network: Option<bool>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, FromToSerialize)]
#[serde(rename_all = "camelCase")]
pub struct NpmRegistrySettings {
    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub npm_always_auth: Option<bool>,

    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub npm_auth_ident: Option<Password>,

    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub npm_auth_token: Option<Password>,
}

#[derive(Clone, Debug, Deserialize, Serialize, FromToSerialize)]
#[serde(rename_all = "camelCase")]
pub struct NpmScopeSettings {
    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub npm_registry_server: Option<String>,

    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub npm_publish_registry: Option<String>,

    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub npm_always_auth: Option<bool>,

    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub npm_auth_ident: Option<Password>,

    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub npm_auth_token: Option<Password>,
}

#[derive(Clone, Debug, Deserialize, Serialize, FromToSerialize)]
#[serde(rename_all = "camelCase")]
pub struct PackageExtension {
    #[serde(default)]
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    #[serde(serialize_with = "descriptor_map_serializer")]
    #[serde(deserialize_with = "descriptor_map_deserializer")]
    pub dependencies: BTreeMap<Ident, Descriptor>,

    #[serde(default)]
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub peer_dependencies: BTreeMap<Ident, PeerRange>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum PnpFallbackMode {
    #[serde(rename = "none")]
    None,

    #[serde(rename = "dependencies-only")]
    DependenciesOnly,

    #[serde(rename = "all")]
    All,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum NodeLinker {
    #[serde(rename = "pnp")]
    Pnp,

    #[serde(rename = "pnpm")]
    #[serde(alias = "node-modules")]
    Pnpm,

    #[serde(rename = "nm")]
    Nm,
}

/**
 * Configuration settings obtained from the environment variables only. Those
 * variables are extracted whenever the program starts and are never updated.
 *
 * In general you only want to use this for one-off debugging settings.
 */
#[yarn_config]
pub struct EnvConfig {
    #[default(false)]
    pub enable_timings: BoolField,

    #[default(9)]
    pub lockfile_version_override: UintField,
}

#[yarn_config]
pub struct UserConfig {
    #[default(true)]
    pub enable_network: BoolField,

    #[default(|_| !zpm_ci::is_ci().is_some() && std::io::stdout().is_terminal())]
    pub enable_progress_bars: BoolField,

    #[default(Path::home_dir().unwrap().unwrap().with_join_str(".yarn/zpm"))]
    pub global_folder: PathField,

    #[default(3)]
    pub http_retry: UintField,

    #[default(100)]
    pub network_concurrency: UintField,

    #[default(BTreeMap::new())]
    pub network_settings: DictField<Glob, NetworkSettings>,
}

fn check_tsconfig(config_paths: &ConfigPaths) -> bool {
    if let Some(project_cwd) = &config_paths.project_cwd {
        let root_has_tsconfig = project_cwd
            .with_join_str("tsconfig.json")
            .fs_exists();

        if root_has_tsconfig {
            return true;
        }
    }

    if let Some(package_cwd) = &config_paths.package_cwd {
        let package_has_tsconfig = package_cwd
            .with_join_str("tsconfig.json")
            .fs_exists();

        if package_has_tsconfig {
            return true;
        }
    }

    false
}

#[yarn_config]
pub struct ProjectConfig {
    #[default(RangeKind::Caret)]
    pub default_semver_range_prefix: EnumField<RangeKind>,

    #[default(|path| check_tsconfig(path))]
    #[alias(ts_enable_auto_types)]
    pub enable_auto_types: BoolField,

    #[default(true)]
    pub enable_global_cache: BoolField,

    #[default(true)]
    pub enable_local_cache_cleanup: BoolField,

    #[default(false)]
    pub enable_immutable_cache: BoolField,

    #[default(false)]
    pub enable_immutable_installs: BoolField,

    #[default(true)]
    pub enable_scripts: BoolField,

    #[default(true)]
    pub enable_transparent_workspaces: BoolField,

    #[default("cache".to_string())]
    pub local_cache_folder_name: StringField,

    #[default(NodeLinker::Pnp)]
    pub node_linker: EnumField<NodeLinker>,

    #[default(None)]
    pub npm_publish_registry: OptionalStringField,

    #[default(BTreeMap::new())]
    pub npm_registries: DictField<String, NpmRegistrySettings>,

    #[default("https://registry.npmjs.org".to_string())]
    pub npm_registry_server: StringField,

    #[default(BTreeMap::new())]
    pub npm_scopes: DictField<String, NpmScopeSettings>,

    #[default(true)]
    pub pnp_enable_inlining: BoolField,

    #[default(PnpFallbackMode::DependenciesOnly)]
    pub pnp_fallback_mode: EnumField<PnpFallbackMode>,

    #[default(vec![])]
    pub pnp_ignore_patterns: VecField<GlobField>,

    #[default("#!/usr/bin/env node".to_string())]
    pub pnp_shebang: StringField,

    #[default("node_modules/.store".to_string())]
    pub pnpm_store_folder: StringField,

    #[default(BTreeMap::new())]
    pub package_extensions: DictField<SemverDescriptor, PackageExtension>,

    #[default(vec![])]
    pub unsafe_http_whitelist: VecField<GlobField>,

    #[default(Path::from_str(".yarn/__virtual__").unwrap())]
    pub virtual_folder: PathField,
}
