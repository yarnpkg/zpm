use zpm_utils::Path;
use serde::{Deserialize, Serialize};
use zpm_macros::yarn_config;
use zpm_semver::RangeKind;
use zpm_utils::{FromFileString, ToFileString};
use crate::{config::ConfigPaths, config_fields::{BoolField, EnumField, GlobField, PathField, StringField, UintField, VecField}};

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

    #[default(false)]
    pub enable_global_cache: BoolField,

    #[default(false)]
    pub enable_immutable_cache: BoolField,

    #[default(false)]
    pub enable_immutable_installs: BoolField,

    #[default(true)]
    pub enable_scripts: BoolField,

    #[default(true)]
    pub enable_transparent_workspaces: BoolField,

    #[default(Path::home_dir().unwrap().unwrap().with_join_str(".yarn/zpm"))]
    pub global_folder: PathField,

    #[default("cache".to_string())]
    pub local_cache_folder_name: StringField,

    #[default("https://registry.npmjs.org".to_string())]
    pub npm_registry_server: StringField,

    #[default(true)]
    pub pnp_enable_inlining: BoolField,

    #[default(PnpFallbackMode::All)]
    pub pnp_fallback_mode: EnumField<PnpFallbackMode>,

    #[default(vec![])]
    pub pnp_ignore_patterns: VecField<GlobField>,

    #[default("#!/usr/bin/env node".to_string())]
    pub pnp_shebang: StringField,
}
