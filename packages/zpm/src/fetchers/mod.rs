use arca::Path;
use serde::{Deserialize, Serialize};

use crate::{error::Error, hash::Sha256, install::{FetchResult, InstallContext, InstallOpResult}, primitives::{reference, Locator, Reference}};

pub mod folder;
pub mod git;
pub mod link;
pub mod npm;
pub mod patch;
pub mod portal;
pub mod tarball;
pub mod url;
pub mod workspace;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum PackageLinking {
    Hard,
    Soft,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum PackageData {
    Local {
        /** Directory that contains the package.json file */
        package_directory: Path,

        discard_from_lookup: bool,
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
        checksum: Option<Sha256>,

        /** Directory from which relative links from link:/file:/portal: dependencies will be resolved */
        context_directory: Path,

        /** Directory that contains the package.json file */
        package_directory: Path,
    },
}

impl PackageData {
    /** Top-most context directory of the package */
    pub fn data_root(&self) -> &Path {
        match self {
            PackageData::Local {package_directory, ..} => package_directory,
            PackageData::MissingZip {archive_path, ..} => archive_path,
            PackageData::Zip {archive_path, ..} => archive_path,
        }
    }

    /** Directory from which relative links from link:/file:/portal: dependencies will be resolved */
    pub fn context_directory(&self) -> &Path {
        match self {
            PackageData::Local {package_directory, ..} => package_directory,
            PackageData::MissingZip {context_directory, ..} => context_directory,
            PackageData::Zip {context_directory, ..} => context_directory,
        }
    }

    /** Directory that contains the package.json file */
    pub fn package_directory(&self) -> &Path {
        match self {
            PackageData::Local {package_directory, ..} => package_directory,
            PackageData::MissingZip {package_directory, ..} => package_directory,
            PackageData::Zip {package_directory, ..} => package_directory,
        }
    }

    pub fn package_subpath(&self) -> Path {
        self.package_directory()
            .relative_to(self.data_root())
    }

    pub fn checksum(&self) -> Option<Sha256> {
        match self {
            PackageData::Local {..} => None,
            PackageData::MissingZip {..} => None,
            PackageData::Zip {checksum, ..} => checksum.clone(),
        }
    }

    pub fn link_type(&self) -> PackageLinking {
        match self {
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
            => match npm::try_fetch_locator_sync(&context, locator, &reference::RegistryReference {ident: locator.ident.clone(), version: params.version.clone()}, is_mock_request)? {
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

        Reference::Workspace(params)
            => Ok(SyncFetchAttempt::Success(workspace::fetch_locator(&context, locator, params)?)),

        _ => Ok(SyncFetchAttempt::Failure(dependencies)),
    }
}

pub async fn fetch_locator<'a>(context: InstallContext<'a>, locator: &Locator, is_mock_request: bool, dependencies: Vec<InstallOpResult>) -> Result<FetchResult, Error> {
    match &locator.reference {
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
            => npm::fetch_locator(&context, locator, &reference::RegistryReference {ident: locator.ident.clone(), version: params.version.clone()}, is_mock_request).await,

        Reference::Registry(params)
            => npm::fetch_locator(&context, locator, params, is_mock_request).await,

        Reference::Workspace(params)
            => workspace::fetch_locator(&context, locator, params),

        _ => Err(Error::Unsupported),
    }
}
