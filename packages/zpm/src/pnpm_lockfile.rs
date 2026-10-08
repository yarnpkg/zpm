use std::{collections::{BTreeMap, BTreeSet}, sync::Arc};

use serde::Deserialize;
use zpm_primitives::Ident;
use zpm_semver::Version;
use zpm_utils::FromFileString;

use crate::{
    error::Error,
    preferred_versions::{LockedDependent, PreferredVersions},
};

pub const PNPM_LOCKFILE_NAME: &str = "pnpm-lock.yaml";

#[derive(Debug, Deserialize)]
struct PnpmImporterDependency {
    specifier: String,
    version: String,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PnpmImporter {
    #[serde(default)]
    dependencies: BTreeMap<String, PnpmImporterDependency>,

    #[serde(default)]
    dev_dependencies: BTreeMap<String, PnpmImporterDependency>,

    #[serde(default)]
    optional_dependencies: BTreeMap<String, PnpmImporterDependency>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PnpmSnapshot {
    #[serde(default)]
    dependencies: BTreeMap<String, String>,

    #[serde(default)]
    optional_dependencies: BTreeMap<String, String>,
}

#[derive(Debug, Deserialize)]
struct PnpmCatalogEntry {
    specifier: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PnpmLockfile {
    lockfile_version: serde_yaml::Value,

    #[serde(default)]
    catalogs: BTreeMap<String, BTreeMap<String, PnpmCatalogEntry>>,

    #[serde(default)]
    importers: BTreeMap<String, PnpmImporter>,

    /// Lockfile v9 keeps the package metadata here and their dependencies
    /// in `snapshots`; v6 keeps everything here.
    #[serde(default)]
    packages: BTreeMap<String, PnpmSnapshot>,

    #[serde(default)]
    snapshots: BTreeMap<String, PnpmSnapshot>,
}

/**
 * Splits a pnpm package key (`foo@1.0.0`, `@scope/foo@1.0.0(react@19.0.0)`,
 * `/foo@1.0.0` in v6) into the package it references. Returns `None` for
 * keys that don't point to a registry version (tarballs, git, folders).
 */
fn parse_package_key(key: &str) -> Option<(Ident, Version)> {
    let key
        = key.strip_prefix('/').unwrap_or(key);

    let key = match key.find('(') {
        Some(index) => &key[..index],
        None => key,
    };

    // The name of scoped packages starts with a `@`, so we skip it
    let separator
        = key[1..].rfind('@')? + 1;

    let ident
        = Ident::from_file_string(&key[..separator]).ok()?;
    let version
        = Version::from_file_string(&key[separator + 1..]).ok()?;

    Some((ident, version))
}

/**
 * Parses the version pnpm locked a dependency to. Regular dependencies only
 * store the version (with the resolved peer dependencies as suffix), while
 * aliases store the full key of the package they point to.
 */
fn parse_dependency_version(name: &Ident, value: &str) -> Option<(Ident, Version)> {
    let without_peers = match value.find('(') {
        Some(index) => &value[..index],
        None => value,
    };

    if let Ok(version) = Version::from_file_string(without_peers) {
        return Some((name.clone(), version));
    }

    parse_package_key(value)
}

/**
 * Turns the content of a `pnpm-lock.yaml` into the versions the resolver
 * should prefer. Overrides and patches don't need any special treatment:
 * the versions pnpm stored are the ones it got after applying them, and
 * the Yarn resolver checks the preferred versions against the ranges it
 * gets after applying its own `resolutions`.
 */
pub fn preferred_versions_from_pnpm_lockfile(src: &str) -> Result<PreferredVersions, Error> {
    let lockfile: PnpmLockfile
        = serde_yaml::from_str(src)
            .map_err(|err| Error::PnpmLockfileParseError(Arc::new(err)))?;

    let lockfile_version = match &lockfile.lockfile_version {
        serde_yaml::Value::String(version) => version.clone(),
        serde_yaml::Value::Number(version) => version.to_string(),
        _ => String::new(),
    };

    // v5 used a different key format (`/foo/1.0.0`) and is long gone; v6
    // and v9 (used by pnpm 8 to 10) share everything we care about.
    if !lockfile_version.starts_with("6.") && !lockfile_version.starts_with("9.") {
        return Err(Error::UnsupportedPnpmLockfileVersion(lockfile_version));
    }

    let mut preferred_versions
        = PreferredVersions::new();

    for importer in lockfile.importers.values() {
        let dependencies
            = importer.dependencies.iter()
                .chain(importer.dev_dependencies.iter())
                .chain(importer.optional_dependencies.iter());

        for (name, dependency) in dependencies {
            let Ok(name) = Ident::from_file_string(name) else {
                continue;
            };

            let Some((mut ident, version)) = parse_dependency_version(&name, &dependency.version) else {
                continue;
            };

            let specifier = match dependency.specifier.strip_prefix("catalog:") {
                Some(catalog) => {
                    let catalog_name = match catalog {
                        "" => "default",
                        catalog => catalog,
                    };

                    lockfile.catalogs.get(catalog_name)
                        .and_then(|catalog| catalog.get(name.as_str()))
                        .map(|entry| entry.specifier.as_str())
                        .unwrap_or(dependency.specifier.as_str())
                },

                None => {
                    dependency.specifier.as_str()
                },
            };

            // Catalog entries only store the version, even for aliases
            if let Some(aliased_ident) = aliased_ident(specifier) {
                ident = aliased_ident;
            }

            match declared_range(specifier, &ident) {
                Some(range) => preferred_versions.add_declared_range(ident, range, version),
                None => preferred_versions.add_locked_version(ident, version),
            }
        }
    }

    let mut seen_dependents
        = BTreeSet::new();

    let packages
        = lockfile.packages.iter()
            .chain(lockfile.snapshots.iter());

    for (key, snapshot) in packages {
        let Some((parent_ident, parent_version)) = parse_package_key(key) else {
            continue;
        };

        preferred_versions.add_locked_version(parent_ident.clone(), parent_version.clone());

        let dependencies
            = snapshot.dependencies.iter()
                .chain(snapshot.optional_dependencies.iter());

        for (name, value) in dependencies {
            let Ok(dependency_name) = Ident::from_file_string(name) else {
                continue;
            };

            let Some((ident, version)) = parse_dependency_version(&dependency_name, value) else {
                continue;
            };

            // The same package is listed once per set of peer dependencies
            let dependent_key
                = (parent_ident.clone(), parent_version.clone(), dependency_name.clone(), version.clone());

            if !seen_dependents.insert(dependent_key) {
                continue;
            }

            preferred_versions.add_dependent(ident, LockedDependent {
                parent_ident: parent_ident.clone(),
                parent_version: parent_version.clone(),
                dependency_name,
                version,
            });
        }
    }

    Ok(preferred_versions)
}

fn aliased_ident(specifier: &str) -> Option<Ident> {
    let aliased
        = specifier.strip_prefix("npm:")?;

    let separator
        = aliased.get(1..)?.rfind('@')? + 1;

    Ident::from_file_string(&aliased[..separator]).ok()
}

fn declared_range(specifier: &str, ident: &Ident) -> Option<zpm_semver::Range> {
    let range = match specifier.strip_prefix("npm:") {
        Some(aliased) => {
            let separator
                = aliased.get(1..)?.rfind('@')? + 1;

            if aliased[..separator] != *ident.as_str() {
                return None;
            }

            &aliased[separator + 1..]
        },

        None => {
            specifier
        },
    };

    zpm_semver::Range::from_file_string(range).ok()
}

#[cfg(test)]
mod tests {
    use zpm_primitives::Ident;
    use zpm_semver::Version;
    use zpm_utils::FromFileString;

    use super::{parse_dependency_version, parse_package_key};

    fn version(src: &str) -> Version {
        Version::from_file_string(src).unwrap()
    }

    #[test]
    fn should_parse_package_keys() {
        assert_eq!(parse_package_key("foo@1.0.0"), Some((Ident::new("foo"), version("1.0.0"))));
        assert_eq!(parse_package_key("/foo@1.0.0"), Some((Ident::new("foo"), version("1.0.0"))));
        assert_eq!(parse_package_key("@scope/foo@1.0.0-rc.1(react@19.0.0)(@types/react@19.0.0)"), Some((Ident::new("@scope/foo"), version("1.0.0-rc.1"))));
        assert_eq!(parse_package_key("foo@5.100.14(patch_hash=cb9eeca8)(eslint@9.39.4(jiti@2.7.0))"), Some((Ident::new("foo"), version("5.100.14"))));
        assert_eq!(parse_package_key("foo@https://example.com/foo.tgz"), None);
        assert_eq!(parse_package_key("foo@file:../foo"), None);
    }

    #[test]
    fn should_parse_dependency_versions() {
        let name
            = Ident::new("react-native");

        assert_eq!(parse_dependency_version(&name, "0.86.0(react@19.2.7)"), Some((name.clone(), version("0.86.0"))));
        assert_eq!(parse_dependency_version(&name, "@acme/react-native@0.86.0-3170b6c(react@19.2.7)"), Some((Ident::new("@acme/react-native"), version("0.86.0-3170b6c"))));
        assert_eq!(parse_dependency_version(&name, "link:../react-native"), None);
    }
}
