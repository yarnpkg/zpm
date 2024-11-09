use arca::Path;
use serde::{Deserialize, Serialize};

use crate::{error::Error, formats, hash::Sha256, install::{FetchResult, InstallContext, InstallOpResult}, primitives::{Locator, Reference}};

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

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq, Hash)]
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

        /** Directory from which relative links from link:/file:/portal: dependencies will be resolved */
        context_directory: Path,

        /** Directory that contains the package.json file */
        package_directory: Path,

        data: Vec<u8>,
        checksum: Sha256,
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
            PackageData::Zip {checksum, ..} => Some(checksum.clone()),
        }
    }

    pub fn link_type(&self) -> PackageLinking {
        match self {
            PackageData::Local {..} => PackageLinking::Soft,
            PackageData::MissingZip {..} => PackageLinking::Hard,
            PackageData::Zip {..} => PackageLinking::Hard,
        }
    }

    pub fn file_entries(&self) -> Result<Vec<formats::Entry>, Error> {
        match self {
            PackageData::Local {package_directory, ..} => {
                formats::entries_from_folder(package_directory)
            },

            PackageData::Zip {data, ..} => {
                let entries
                    = formats::zip::entries_from_zip(data)?;

                let package_subpath
                    = self.package_subpath();

                Ok(formats::strip_prefix(entries, package_subpath.as_str()))
            },

            PackageData::MissingZip {..} => {
                Err(Error::Unsupported)
            },
        }
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

        Reference::Registry(params)
            => npm::fetch_locator(&context, locator, params, is_mock_request).await,

        Reference::Workspace(params)
            => workspace::fetch_locator(&context, locator, params),

        _ => Err(Error::Unsupported),
    }
}
