use std::mem::take;

use rkyv::Archive;
use serde::{de::IgnoredAny, Deserialize};
use zpm_macro_enum::zpm_enum;
use zpm_parsers::JsonDocument;
use zpm_utils::{impl_file_string_from_str, impl_file_string_serialization, FromFileString, IoResultExt, Path, ToFileString, ToHumanString};

use crate::errors::Error;

use zpm_semver::Version;

#[zpm_enum(or_else = |s| Err(Error::UnknownBinaryName(s.to_string())))]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Archive, rkyv::Serialize, rkyv::Deserialize)]
#[derive_variants(Clone, Copy, Debug, PartialEq, Eq, Archive, rkyv::Serialize, rkyv::Deserialize)]
#[variant_struct_attr(rkyv(derive(PartialEq, Eq)))]
enum BinaryName {
    #[pattern(r"yarn")]
    #[to_file_string(|| "yarn".to_string())]
    #[to_print_string(|| "yarn".to_string())]
    Yarn,
}


#[zpm_enum(or_else = |s| Err(Error::InvalidPackageManagerReference(s.to_string())))]
#[derive(Clone, Debug, PartialEq, Eq, Archive, rkyv::Serialize, rkyv::Deserialize)]
#[derive_variants(Clone, Debug, PartialEq, Eq, Archive, rkyv::Serialize, rkyv::Deserialize)]
#[variant_struct_attr(rkyv(derive(PartialEq, Eq)))]
pub enum PackageManagerReference {
    #[pattern(r"(?<version>.*)")]
    #[to_file_string(|params| params.version.to_file_string())]
    #[to_print_string(|params| params.version.to_print_string())]
    Version {
        version: Version,
    },

    #[no_pattern]
    #[to_file_string(|params| format!("local:{}", params.path.to_file_string()))]
    #[to_print_string(|params| params.path.to_print_string())]
    Local {
        path: Path,
    },
}


#[derive(Clone, Debug, PartialEq, Eq, Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct PackageManagerField {
    pub name: String,

    // Not public so we can force usage to use either `reference()` or `into_reference()`
    reference: PackageManagerReference,
}

impl PackageManagerField {
    pub fn new_yarn(reference: PackageManagerReference) -> PackageManagerField {
        PackageManagerField {
            name: "yarn".to_string(),
            reference,
        }
    }

    pub fn into_reference(self, expected_name: &'static str) -> Result<PackageManagerReference, Error> {
        if self.name == expected_name {
            Ok(self.reference)
        } else {
            Err(Error::UnsupportedProject {field: "packageManager", name: self.name})
        }
    }

    pub fn reference(&self, expected_name: &'static str) -> Result<&PackageManagerReference, Error> {
        if self.name == expected_name {
            Ok(&self.reference)
        } else {
            Err(Error::UnsupportedProject {field: "packageManager", name: self.name.clone()})
        }
    }
}

impl FromFileString for PackageManagerField {
    type Error = Error;

    fn from_file_string(s: &str) -> Result<Self, Error> {
        let at_index = s
            .find('@')
            .ok_or(Error::InvalidPackageManagerString)?;

        let name
            = s[..at_index].to_string();

        let reference
            = PackageManagerReference::from_file_string(&s[at_index + 1..])?;



        Ok(PackageManagerField {name, reference})
    }
}

impl ToFileString for PackageManagerField {
    fn to_file_string(&self) -> String {
        format!("{}@{}", self.name.to_file_string(), self.reference.to_file_string())
    }
}

impl ToHumanString for PackageManagerField {
    fn to_print_string(&self) -> String {
        format!("{}@{}", self.name.to_print_string(), self.reference.to_print_string())
    }
}

impl_file_string_from_str!(PackageManagerField);
impl_file_string_serialization!(PackageManagerField);

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Manifest {
    package_manager: Option<PackageManagerField>,
    package_manager_migration: Option<PackageManagerField>,
    dev_engines: Option<DevEngines>,
}

/// The `devEngines` field, as described by
/// https://docs.npmjs.com/cli/configuring-npm/package-json#devengines.
///
/// We only care about the package manager names it lists, so anything that
/// doesn't match the expected shape is ignored rather than turned into a
/// manifest parse error; validating `devEngines` is the job of the tools that
/// implement it.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum DevEngines {
    Valid {
        #[serde(rename = "packageManager")]
        package_manager: Option<DevPackageManagers>,
    },

    Invalid(IgnoredAny),
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum DevPackageManagers {
    // Must come first, as `DevPackageManager::Invalid` would match an array
    Multiple(Vec<DevPackageManager>),
    Single(DevPackageManager),
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum DevPackageManager {
    Named {
        name: String,
    },

    Invalid(IgnoredAny),
}

impl DevEngines {
    fn into_package_manager_names(self) -> Vec<String> {
        let entries = match self {
            DevEngines::Valid {package_manager: Some(DevPackageManagers::Single(entry))}
                => vec![entry],

            DevEngines::Valid {package_manager: Some(DevPackageManagers::Multiple(entries))}
                => entries,

            _ => vec![],
        };

        entries.into_iter()
            .filter_map(|entry| match entry {
                DevPackageManager::Named {name} => Some(name),
                DevPackageManager::Invalid(_) => None,
            })
            .collect()
    }
}

#[derive(Debug)]
pub struct FindResult {
    pub detected_root_path: Option<Path>,
    pub detected_package_manager: Option<PackageManagerField>,
    pub detected_package_manager_migration: Option<PackageManagerField>,
    /// Names listed in `devEngines.packageManager` (a single entry or an
    /// array of alternatives); entries without a string `name` are skipped.
    pub detected_dev_package_manager_names: Vec<String>,
}

impl FindResult {
    /// Checks that the detected project may be run with Yarn as far as
    /// `devEngines` is concerned. Every command that picks the Yarn version
    /// of the project must call this before looking at links or falling back
    /// to the default Yarn version, so that a nested `devEngines` claim is
    /// never silently treated as an unconfigured project.
    ///
    /// Other package managers may declare themselves in `devEngines`, so we
    /// recognize those declarations to avoid running Yarn on their projects.
    /// Yarn itself is only ever selected through the `packageManager` field,
    /// so a Yarn entry in `devEngines` is an error rather than a declaration.
    pub fn check_dev_engines(&self) -> Result<(), Error> {
        let dev_package_manager_names
            = &self.detected_dev_package_manager_names;

        if dev_package_manager_names.iter().any(|name| name == "yarn") {
            // If the project is explicitly configured for another package manager,
            // report that rather than suggesting to switch it over to Yarn.
            if let Some(package_manager) = &self.detected_package_manager {
                package_manager.reference("yarn")?;
            }

            let manifest_path = self.detected_root_path
                .as_ref()
                .map(|detected_root_path| detected_root_path.with_join_str("package.json"))
                .expect("A devEngines declaration always comes from a detected manifest");

            return Err(Error::DevEnginesYarnUnsupported(manifest_path));
        }

        if !dev_package_manager_names.is_empty() {
            return Err(Error::UnsupportedProject {field: "devEngines.packageManager.name", name: dev_package_manager_names.join(", ")});
        }

        Ok(())
    }
}

const ROOT_FILES: &[&'static str] = &[
    "yarn.lock",
];

/// Resolves the detected-root path: prefers `YARNSW_DETECTED_ROOT` when set
/// (the switch binary stashes it before delegating to a package-manager
/// version), otherwise walks up from `cwd` looking for the closest manifest
/// with a `packageManager` field or a named `devEngines.packageManager` entry.
pub fn resolve_detected_root(cwd: &Path) -> Result<Path, Error> {
    if let Ok(env_root) = std::env::var("YARNSW_DETECTED_ROOT") {
        return Ok(Path::try_from(&env_root)?);
    }

    let find_result = find_closest_package_manager(cwd)?;

    find_result.detected_root_path
        .ok_or(Error::NoProjectFound)
}

pub fn find_closest_package_manager(path: &Path) -> Result<FindResult, Error> {
    let mut last_package_folder = None;

    for mut parent in path.iter_path().rev() {
        let manifest_path = parent
            .with_join_str("package.json");

        let manifest = manifest_path
            .fs_read_text()
            .ok_missing()?;

        if let Some(manifest) = &manifest {
            let parsed_manifest: Manifest = JsonDocument::hydrate_from_str(&manifest)
                .map_err(|err| Error::FailedToParseManifest(err))?;

            let dev_package_manager_names = parsed_manifest.dev_engines
                .map(|dev_engines| dev_engines.into_package_manager_names())
                .unwrap_or_default();

            if parsed_manifest.package_manager.is_some() || !dev_package_manager_names.is_empty() {
                return Ok(FindResult {
                    detected_root_path: Some(parent),
                    detected_package_manager: parsed_manifest.package_manager,
                    detected_package_manager_migration: parsed_manifest.package_manager_migration,
                    detected_dev_package_manager_names: dev_package_manager_names,
                });
            }
        }

        for root_file in ROOT_FILES {
            let root_file_path = parent
                .with_join_str(root_file);

            if root_file_path.fs_exists() {
                return Ok(FindResult {
                    detected_root_path: Some(parent),
                    detected_package_manager: None,
                    detected_package_manager_migration: None,
                    detected_dev_package_manager_names: vec![],
                });
            }
        }

        if manifest.is_some() {
            last_package_folder = Some(take(&mut parent));
        }
    }

    Ok(FindResult {
        detected_root_path: last_package_folder,
        detected_package_manager: None,
        detected_package_manager_migration: None,
        detected_dev_package_manager_names: vec![],
    })
}
