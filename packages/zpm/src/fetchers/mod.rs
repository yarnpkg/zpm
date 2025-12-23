use zpm_primitives::{Locator, Reference, RegistryReference};
use zpm_utils::{Hash64, Path, ToHumanString};
use serde::{Deserialize, Serialize};

use crate::{
    error::Error,
    install::{FetchResult, InstallContext, InstallOpResult},
};

pub mod builtin;
pub mod folder;
pub mod git;
pub mod link;
pub mod npm;
pub mod patch;
pub mod portal;
pub mod tarball;
pub mod url;
pub mod variants;
pub mod workspace;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum PackageLinking {
    Hard,
    Soft,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum PackageData {
    /** An abstract package must not be fetched; it'll be replaced in the tree by a concrete variant */
    Abstract,

    Local {
        /** Directory that contains the package.json file */
        package_directory: Path,

        /** Whether the package is a synthetic package, ie an arbitrary folder that is used as a package */
        is_synthetic_package: bool,
    },

    MissingZip {
        /** Path of the .zip file on disk */
        archive_path: Path,

        /** Directory from which relative links from link:/file:/portal: dependencies will be resolved */
        context_directory: Path,

        /** Directory that contains the package.json file */
        package_directory: Path,
    },

    Zip {
        /** Path of the .zip file on disk */
        archive_path: Path,

        /** Checksum of the archive; only present when the archive was newly cached */
        checksum: Option<Hash64>,

        /** Directory from which relative links from link:/file:/portal: dependencies will be resolved */
        context_directory: Path,

        /** Directory that contains the package.json file */
        package_directory: Path,
    },
}

impl PackageData {
    pub fn symlink_target(&self) -> Option<&Path> {
        if let PackageData::Local {package_directory, ..} = self {
            Some(package_directory)
        } else {
            None
        }
    }

    /** Top-most context directory of the package */
    pub fn data_root(&self) -> &Path {
        match self {
            PackageData::Abstract => panic!("Invalid package data"),
            PackageData::Local {package_directory, ..} => package_directory,
            PackageData::MissingZip {archive_path, ..} => archive_path,
            PackageData::Zip {archive_path, ..} => archive_path,
        }
    }

    /** Directory from which relative links from link:/file:/portal: dependencies will be resolved */
    pub fn context_directory(&self) -> &Path {
        match self {
            PackageData::Abstract => panic!("Invalid package data"),
            PackageData::Local {package_directory, ..} => package_directory,
            PackageData::MissingZip {context_directory, ..} => context_directory,
            PackageData::Zip {context_directory, ..} => context_directory,
        }
    }

    /** Directory that contains the package.json file */
    pub fn package_directory(&self) -> &Path {
        match self {
            PackageData::Abstract => panic!("Invalid package data"),
            PackageData::Local {package_directory, ..} => package_directory,
            PackageData::MissingZip {package_directory, ..} => package_directory,
            PackageData::Zip {package_directory, ..} => package_directory,
        }
    }

    pub fn package_subpath(&self) -> Path {
        self.package_directory()
            .relative_to(self.data_root())
    }

    pub fn checksum(&self) -> Option<Hash64> {
        match self {
            PackageData::Abstract => None,
            PackageData::Local {..} => None,
            PackageData::MissingZip {..} => None,
            PackageData::Zip {checksum, ..} => checksum.clone(),
        }
    }

    pub fn link_type(&self) -> PackageLinking {
        match self {
            PackageData::Abstract => panic!("Invalid package data"),
            PackageData::Local {..} => PackageLinking::Soft,
            PackageData::MissingZip {..} => PackageLinking::Hard,
            PackageData::Zip {..} => PackageLinking::Hard,
        }
    }
}

pub enum SyncFetchAttempt {
    Success(FetchResult),
    Failure(Vec<InstallOpResult>),
}

pub fn try_fetch_locator_sync(context: InstallContext, locator: &Locator, is_mock_request: bool, dependencies: Vec<InstallOpResult>) -> Result<SyncFetchAttempt, Error> {
    match &locator.reference {
        Reference::Shorthand(params)
            => match npm::try_fetch_locator_sync(&context, locator, &RegistryReference {ident: locator.ident.clone(), version: params.version.clone()}, is_mock_request)? {
                Some(fetch_result) => Ok(SyncFetchAttempt::Success(fetch_result)),
                None => Ok(SyncFetchAttempt::Failure(dependencies)),
            },

        Reference::Registry(params)
            => match npm::try_fetch_locator_sync(&context, locator, params, is_mock_request)? {
                Some(fetch_result) => Ok(SyncFetchAttempt::Success(fetch_result)),
                None => Ok(SyncFetchAttempt::Failure(dependencies)),
            },

        Reference::Link(params)
            => Ok(SyncFetchAttempt::Success(link::fetch_locator(&context, locator, params, dependencies)?)),

        Reference::Portal(params)
            => Ok(SyncFetchAttempt::Success(portal::fetch_locator(&context, locator, params, dependencies)?)),

        Reference::WorkspaceIdent(params)
            => Ok(SyncFetchAttempt::Success(workspace::fetch_locator_ident(&context, locator, params)?)),

        Reference::WorkspacePath(params)
            => Ok(SyncFetchAttempt::Success(workspace::fetch_locator_path(&context, locator, params)?)),

        _ => Ok(SyncFetchAttempt::Failure(dependencies)),
    }
}

pub async fn fetch_locator<'a>(context: InstallContext<'a>, locator: &Locator, is_mock_request: bool, dependencies: Vec<InstallOpResult>) -> Result<FetchResult, Error> {
    match &locator.reference {
        Reference::Builtin(params)
            => builtin::fetch_builtin_locator(&context, locator, params).await,

        Reference::Link(params)
            => link::fetch_locator(&context, locator, params, dependencies),

        Reference::Portal(params)
            => portal::fetch_locator(&context, locator, params, dependencies),

        Reference::Url(params)
            => url::fetch_locator(&context, locator, params).await,

        Reference::Tarball(params)
            => tarball::fetch_locator(&context, locator, params, dependencies).await,

        Reference::Folder(params)
            => folder::fetch_locator(&context, locator, params, dependencies).await,

        Reference::Git(params)
            => git::fetch_locator(&context, locator, params).await,

        Reference::Patch(params)
            => patch::fetch_locator(&context, locator, params, dependencies).await,

        Reference::Shorthand(params)
            => npm::fetch_locator(&context, locator, &RegistryReference {ident: locator.ident.clone(), version: params.version.clone()}, is_mock_request).await,

        Reference::Registry(params)
            => npm::fetch_locator(&context, locator, params, is_mock_request).await,

        Reference::WorkspaceIdent(params)
            => workspace::fetch_locator_ident(&context, locator, params),

        Reference::WorkspacePath(params)
            => workspace::fetch_locator_path(&context, locator, params),

        _ => panic!("This reference ({}) should never end up being passed to a fetcher", locator.reference.to_print_string()),
    }
}
