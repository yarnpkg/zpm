use std::mem::take;

use bincode::{Decode, Encode};
use rkyv::Archive;
use serde::Deserialize;
use zpm_macro_enum::zpm_enum;
use zpm_parsers::JsonDocument;
use zpm_utils::{impl_file_string_from_str, impl_file_string_serialization, FromFileString, IoResultExt, Path, ToFileString, ToHumanString};

use crate::errors::Error;

use zpm_semver::Version;

#[zpm_enum(or_else = |s| Err(Error::UnknownBinaryName(s.to_string())))]
#[derive(Clone, Copy, Debug, Decode, Encode, PartialEq, Eq, Archive, rkyv::Serialize, rkyv::Deserialize)]
#[derive_variants(Clone, Copy, Debug, Decode, Encode, PartialEq, Eq, Archive, rkyv::Serialize, rkyv::Deserialize)]
enum BinaryName {
    #[pattern(r"yarn")]
    #[to_file_string(|| "yarn".to_string())]
    #[to_print_string(|| "yarn".to_string())]
    Yarn,
}


#[zpm_enum(or_else = |s| Err(Error::InvalidPackageManagerReference(s.to_string())))]
#[derive(Clone, Debug, Decode, Encode, PartialEq, Eq, Archive, rkyv::Serialize, rkyv::Deserialize)]
#[derive_variants(Clone, Debug, Decode, Encode, PartialEq, Eq, Archive, rkyv::Serialize, rkyv::Deserialize)]
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


#[derive(Clone, Debug, Decode, Encode, PartialEq, Eq, Archive, rkyv::Serialize, rkyv::Deserialize)]
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
            Err(Error::UnsupportedProject(expected_name))
        }
    }

    pub fn reference(&self, expected_name: &'static str) -> Result<&PackageManagerReference, Error> {
        if self.name == expected_name {
            Ok(&self.reference)
        } else {
            Err(Error::UnsupportedProject(expected_name))
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
}

#[derive(Debug)]
pub struct FindResult {
    pub detected_root_path: Option<Path>,
    pub detected_package_manager: Option<PackageManagerField>,
    pub detected_package_manager_migration: Option<PackageManagerField>,
}

const ROOT_FILES: &[&'static str] = &[
    "yarn.lock",
];

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

            if let Some(package_manager) = parsed_manifest.package_manager {
                return Ok(FindResult {
                    detected_root_path: Some(parent),
                    detected_package_manager: Some(package_manager),
                    detected_package_manager_migration: parsed_manifest.package_manager_migration,
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
    })
}
