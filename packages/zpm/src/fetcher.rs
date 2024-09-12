use std::{io::{Cursor, Read}, sync::Arc};

use arca::Path;
use bytes::Bytes;
use serde::{Deserialize, Serialize};

use crate::{error::Error, formats, git::{clone_repository, GitReference}, hash::Sha256, http::http_client, install::InstallContext, manifest::{Manifest, RemoteManifest}, prepare, primitives::{Ident, Locator, Reference}, resolver::Resolution, semver};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum PackageLinking {
    Hard,
    Soft,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
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
}

fn convert_tar_gz_to_zip(ident: &Ident, tar_gz_data: Bytes) -> Result<Vec<u8>, Error> {
    let mut decompressed = vec![];

    flate2::read::GzDecoder::new(Cursor::new(tar_gz_data))
        .read_to_end(&mut decompressed)?;

    let entries = formats::tar::entries_from_tar(&decompressed)?;
    let entries = formats::strip_first_segment(entries);
    let entries = formats::normalize_entries(entries);
    let entries = formats::prefix_entries(entries, format!("node_modules/{}", ident.as_str()));

    Ok(formats::zip::craft_zip(&entries))
}

fn convert_folder_to_zip(ident: &Ident, folder_path: &Path) -> Result<Vec<u8>, Error> {
    let entries = formats::entries_from_folder(folder_path)?;
    let entries = formats::normalize_entries(entries);
    let entries = formats::prefix_entries(entries, format!("node_modules/{}", ident.as_str()));

    Ok(formats::zip::craft_zip(&entries))
}

pub async fn fetch<'a>(context: InstallContext<'a>, locator: &Locator, is_mock_request: bool, parent_data: Option<PackageData>) -> Result<PackageData, Error> {
    match &locator.reference {
        Reference::Link(path)
            => fetch_link(path, parent_data),

        Reference::Portal(path)
            => fetch_portal(path, parent_data),

        Reference::Url(url)
            => Ok(fetch_remote_tarball_with_manifest(context, locator, url).await?.1),

        Reference::Tarball(path)
            => Ok(fetch_local_tarball_with_manifest(context, locator, path, parent_data).await?.1),

        Reference::Folder(path)
            => Ok(fetch_folder_with_manifest(context, locator, path, parent_data).await?.1),

        Reference::Git(reference)
            => Ok(fetch_repository_with_manifest(context, locator, reference).await?.1),

        Reference::Semver(version)
            => fetch_semver(context, locator, &locator.ident, version, is_mock_request).await,

        Reference::SemverAlias(ident, version)
            => fetch_semver(context, locator, ident, version, is_mock_request).await,

        Reference::Workspace(ident)
            => fetch_workspace(context, ident),

        _ => Err(Error::Unsupported),
    }
}

pub fn fetch_link(path: &str, parent_data: Option<PackageData>) -> Result<PackageData, Error> {
    let parent_data = parent_data
        .expect("The parent data is required for retrieving the path of a linked package");

    let package_directory = parent_data.context_directory()
        .with_join_str(path);

    Ok(PackageData::Local {
        package_directory,
        discard_from_lookup: true,
    })
}

pub fn fetch_portal(path: &str, parent_data: Option<PackageData>) -> Result<PackageData, Error> {
    let parent_data = parent_data
        .expect("The parent data is required for retrieving the path of a portal package");

    let package_directory = parent_data.context_directory()
        .with_join_str(path);

    Ok(PackageData::Local {
        package_directory,
        discard_from_lookup: false,
    })
}

pub async fn fetch_remote_tarball_with_manifest<'a>(context: InstallContext<'a>, locator: &Locator, url: &str) -> Result<(Resolution, PackageData), Error> {
    let (archive_path, data, checksum) = context.package_cache.unwrap().upsert_blob(locator.clone(), ".zip", || async {
        let client = http_client()?;
        let response = client.get(url).send().await
            .map_err(|err| Error::RemoteRegistryError(Arc::new(err)))?;

        let archive = response.bytes().await
            .map_err(|err| Error::RemoteRegistryError(Arc::new(err)))?;

        convert_tar_gz_to_zip(&locator.ident, archive)
    }).await?;

    let first_entry = formats::zip::first_entry_from_zip(&data);
    let manifest = first_entry
        .and_then(|entry|
            serde_json::from_slice::<Manifest>(&entry.data)
                .map_err(Arc::new)
                .map_err(Error::InvalidJsonData)
        )?;

    let resolution
        = Resolution::from_remote_manifest(locator.clone(), manifest.remote);

    let package_directory = archive_path
        .with_join_str(locator.ident.nm_subdir());

    Ok((resolution, PackageData::Zip {
        archive_path,
        context_directory: package_directory.clone(),
        package_directory,
        data,
        checksum,
    }))
}

pub async fn fetch_local_tarball_with_manifest<'a>(context: InstallContext<'a>, locator: &Locator, path: &str, parent_data: Option<PackageData>) -> Result<(Resolution, PackageData), Error> {
    let parent_data = parent_data
        .expect("The parent data is required for retrieving the path of a tarball package");

    let tarball_path = parent_data.context_directory()
        .with_join_str(path);

    let (archive_path, data, checksum) = context.package_cache.unwrap().upsert_blob(locator.clone(), ".zip", || async {
        convert_tar_gz_to_zip(&locator.ident, Bytes::from(tarball_path.fs_read()?))
    }).await?;

    let first_entry = formats::zip::first_entry_from_zip(&data);
    let manifest = first_entry
        .and_then(|entry|
            serde_json::from_slice::<Manifest>(&entry.data)
                .map_err(Arc::new)
                .map_err(Error::InvalidJsonData)
        )?;

    let resolution
        = Resolution::from_remote_manifest(locator.clone(), manifest.remote);

    let package_directory = archive_path
        .with_join_str(locator.ident.nm_subdir());

    Ok((resolution, PackageData::Zip {
        archive_path,
        context_directory: package_directory.clone(),
        package_directory,
        data,
        checksum,
    }))
}

pub async fn fetch_folder_with_manifest<'a>(context: InstallContext<'a>, locator: &Locator, path: &str, parent_data: Option<PackageData>) -> Result<(Resolution, PackageData), Error> {
    let parent_data = parent_data
        .expect("The parent data is required for retrieving the path of a tarball package");

    let context_directory = parent_data.context_directory()
        .with_join_str(path);

    let (archive_path, data, checksum) = context.package_cache.unwrap().upsert_blob(locator.clone(), ".zip", || async {
        convert_folder_to_zip(&locator.ident, &context_directory)
    }).await?;

    let first_entry
        = formats::zip::first_entry_from_zip(&data);

    let remote_manifest = first_entry
        .and_then(|entry|
            serde_json::from_slice::<RemoteManifest>(&entry.data)
                .map_err(Arc::new)
                .map_err(Error::InvalidJsonData)
        )?;

    let resolution
        = Resolution::from_remote_manifest(locator.clone(), remote_manifest);

    let package_directory = archive_path
        .with_join_str(locator.ident.nm_subdir());

    Ok((resolution, PackageData::Zip {
        archive_path,
        context_directory,
        package_directory,
        data,
        checksum,
    }))
}

pub async fn fetch_repository_with_manifest<'a>(context: InstallContext<'a>, locator: &Locator, reference: &GitReference) -> Result<(Resolution, PackageData), Error> {
    let (archive_path, data, checksum) = context.package_cache.unwrap().upsert_blob(locator.clone(), ".zip", || async {
        let repository_path
            = clone_repository(&reference.repo, &reference.commit).await?;

        let pack_tgz = prepare::prepare_project(
            &repository_path,
            &reference.prepare_params,
        ).await?;

        convert_tar_gz_to_zip(&locator.ident, pack_tgz.into())
    }).await?;

    let first_entry
        = formats::zip::first_entry_from_zip(&data);

    let remote_manifest = first_entry
        .and_then(|entry|
            serde_json::from_slice::<RemoteManifest>(&entry.data)
                .map_err(Arc::new)
                .map_err(Error::InvalidJsonData)
        )?;

    let resolution
        = Resolution::from_remote_manifest(locator.clone(), remote_manifest);

    let package_directory = archive_path
        .with_join_str(locator.ident.nm_subdir());

    Ok((resolution, PackageData::Zip {
        archive_path,
        context_directory: package_directory.clone(),
        package_directory,
        data,
        checksum,
    }))
}

pub async fn fetch_semver<'a>(context: InstallContext<'a>, locator: &Locator, ident: &Ident, version: &semver::Version, is_mock_request: bool) -> Result<PackageData, Error> {
    let project = context.project
        .expect("The project is required for resolving a workspace package");

    if is_mock_request {
        let archive_path = context.package_cache.unwrap()
            .key_path(locator, ".zip")?;

        let package_directory = archive_path
            .with_join_str(ident.nm_subdir());

        return Ok(PackageData::MissingZip {
            archive_path,
            context_directory: package_directory.clone(),
            package_directory,
        });
    }

    let registry_url
        = project.config.registry_url_for_package_data(ident, version);

    let (archive_path, data, checksum) = context.package_cache.unwrap().upsert_blob_or_mock(is_mock_request, locator.clone(), ".zip", || async {
        let client = http_client()?;

        let response = client.get(registry_url.clone()).send().await
            .map_err(|err| Error::RemoteRegistryError(Arc::new(err)))?;

        let archive = response.bytes().await
            .map_err(|err| Error::RemoteRegistryError(Arc::new(err)))?;

        convert_tar_gz_to_zip(ident, archive)
    }).await?;

    let package_directory = archive_path
        .with_join_str(ident.nm_subdir());

    Ok(PackageData::Zip {
        archive_path,
        context_directory: package_directory.clone(),
        package_directory,
        data,
        checksum,
    })
}

pub fn fetch_workspace(context: InstallContext, ident: &Ident) -> Result<PackageData, Error> {
    let project = context.project
        .expect("The project is required for fetching a workspace package");

    let workspace = project.workspaces
        .get(ident)
        .ok_or_else(|| Error::WorkspaceNotFound(ident.clone()))?;

    Ok(PackageData::Local {
        package_directory: workspace.path.clone(),
        discard_from_lookup: false,
    })
}
