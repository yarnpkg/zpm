use std::{mem::take, sync::Arc};

use bincode::{Decode, Encode};
use serde::Deserialize;
use zpm_macros::parse_enum;
use zpm_utils::{impl_serialization_traits, FromFileString, OkMissing, Path, ToFileString, ToHumanString};

use crate::errors::Error;

use zpm_semver::Version;

#[parse_enum(or_else = |s| Err(Error::UnknownBinaryName(s.to_string())))]
#[derive(Clone, Copy, Debug, Decode, Encode, PartialEq, Eq)]
#[derive_variants(Clone, Copy, Debug, Decode, Encode, PartialEq, Eq)]
enum BinaryName {
    #[pattern(spec = r"yarn")]
    Yarn,
}

impl ToFileString for BinaryName {
    fn to_file_string(&self) -> String {
        match self {
            BinaryName::Yarn => "yarn".to_string(),
        }
    }
}

impl ToHumanString for BinaryName {
    fn to_print_string(&self) -> String {
        self.to_file_string()
    }
}

impl_serialization_traits!(BinaryName);

#[parse_enum(or_else = |s| Err(Error::InvalidPackageManagerReference(s.to_string())))]
#[derive(Clone, Debug, Decode, Encode, PartialEq, Eq)]
#[derive_variants(Clone, Debug, Decode, Encode, PartialEq, Eq)]
pub enum PackageManagerReference {
    #[pattern(spec = r"(?<version>.*)")]
    Version {
        version: Version,
    },

    #[pattern(spec = r"local:(?<path>.*)")]
    Local {
        path: Path,
    },
}

impl ToFileString for PackageManagerReference {
    fn to_file_string(&self) -> String {
        match self {
            PackageManagerReference::Version(params)
                => format!("{}", params.version.to_file_string()),

            PackageManagerReference::Local(params)
                => format!("local:{}", params.path.to_file_string()),
        }
    }
}

impl ToHumanString for PackageManagerReference {
    fn to_print_string(&self) -> String {
        match self {
            PackageManagerReference::Version(params)
                => format!("{}", params.version.to_print_string()),

            PackageManagerReference::Local(params)
                => format!("local:{}", params.path.to_print_string()),
        }
    }
}

impl_serialization_traits!(PackageManagerReference);

#[derive(Clone, Debug, Decode, Encode, PartialEq, Eq)]
pub struct PackageManagerField {
    pub name: String,
    pub reference: PackageManagerReference,
    pub checksum: Option<String>,
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

        Ok(PackageManagerField { name, reference, checksum: None })
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

impl_serialization_traits!(PackageManagerField);

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Manifest {
    package_manager: Option<PackageManagerField>,
}

#[derive(Debug)]
pub struct FindResult {
    pub detected_root_path: Option<Path>,
    pub detected_package_manager: Option<PackageManagerField>,
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

        if let Some(manifest) = manifest {
            let parsed_manifest: Manifest = sonic_rs::from_str(&manifest)
                .map_err(|err| Error::FailedToParseManifest(Arc::new(err)))?;

            if let Some(package_manager) = parsed_manifest.package_manager {
                if matches!(package_manager.reference, PackageManagerReference::Local(_)) {
                    return Err(Error::PackageManifestsCannotReferenceLocalBinaries(package_manager.to_print_string()));
                }

                return Ok(FindResult {
                    detected_root_path: Some(parent),
                    detected_package_manager: Some(package_manager),
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
    })
}

pub fn validate_package_manager(package_manager: PackageManagerField, expected: &str) -> Result<PackageManagerReference, Error> {
    if package_manager.name == expected {
        Ok(package_manager.reference)
    } else {
        Err(Error::UnsupportedProject(package_manager.name))
    }
}
