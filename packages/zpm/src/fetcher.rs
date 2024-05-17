use std::{fmt::{self, Display, Formatter}, io::{Cursor, Read, Write}, sync::Arc};

use arca::Path;
use bytes::Bytes;
use serde::{Deserialize, Serialize};
use tar::Archive;
use zip::{write::FileOptions, CompressionMethod, ZipWriter};

use crate::{config::registry_url_for, error::Error, hash::Sha256, http::http_client, install::InstallContext, primitives::{Ident, Locator, Reference}, semver};

#[derive(Clone, Debug, Deserialize, Serialize)]
pub enum PackageData {
    Local {
        path: Path,
        discard_from_lookup: bool,
    },

    Zip {
        path: Path,
        data: Vec<u8>,
        checksum: Sha256,
    },
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub enum PackageLinking {
    Hard,
    Soft,
}

impl Display for PackageLinking {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        match self {
            PackageLinking::Hard => write!(f, "hard"),
            PackageLinking::Soft => write!(f, "soft"),
        }
    }
}

impl PackageData {
    pub fn path(&self) -> &Path {
        match self {
            PackageData::Local {path, ..} => path,
            PackageData::Zip {path, ..} => path,
        }
    }

    pub fn source_dir(&self, locator: &Locator) -> Path {
        match self {
            PackageData::Local {..} => Path::from("."),
            PackageData::Zip {..} => Path::from(format!("node_modules/{}", locator.ident.as_str())),
        }
    }

    pub fn checksum(&self) -> Option<Sha256> {
        match self {
            PackageData::Local {..} => None,
            PackageData::Zip {checksum, ..} => Some(checksum.clone()),
        }
    }

    pub fn link_type(&self) -> PackageLinking {
        match self {
            PackageData::Local {..} => PackageLinking::Soft,
            PackageData::Zip {..} => PackageLinking::Hard,
        }
    }

    pub fn read_text(&self, p: &Path) -> Result<String, Error> {
        match self {
            PackageData::Local {path, ..} => {
                let path = path
                    .with_join(p);

                std::fs::read_to_string(path.to_path_buf())
                    .map_err(Arc::new)
                    .map_err(Error::IoError)
            },

            PackageData::Zip {data, ..} => {
                let reader = Cursor::new(data);
                let mut zip = zip::read::ZipArchive::new(reader)
                    .unwrap();

                let mut file_entry = zip.by_name(&p.to_string())
                    .expect("Failed to find the requested file");

                let mut text = String::new();
                file_entry.read_to_string(&mut text).unwrap();

                Ok(text)
            },
        }
    }
}

fn ignore_tar_entry(p: &str, mut strip_components: u8) -> Option<&str> {
    if p.starts_with('/') {
        return None;
    }

    let mut skip = 0;

    for segment in p.split('/') {
        if segment == ".." {
            return None;
        } else if strip_components > 0 {
            strip_components -= 1;
            skip += segment.len() + 1;
        }
    }

    if skip >= p.len() {
        return None;
    }

    Some(&p[skip..])
}

fn convert_tar_gz_to_zip(ident: &Ident, tar_gz_data: Bytes) -> Result<Vec<u8>, Box<dyn std::error::Error + Send + Sync>> {
    // Create a GzDecoder to decompress the tar.gz data.
    let tar_gz_cursor = Cursor::new(tar_gz_data);
    let gz_decoder = flate2::bufread::GzDecoder::new(tar_gz_cursor);

    // Create a new tar Archive with the decoder.
    let mut archive = Archive::new(gz_decoder);

    // Prepare to write the zip file into a byte buffer.
    let mut zip_buffer = Cursor::new(Vec::new());
    let mut zip = ZipWriter::new(&mut zip_buffer);

    // Iterate over each file in the tar archive.
    for file in archive.entries()? {
        let mut file = file?;

        let file_name = file.path()?.into_owned();
        let file_name_str = file_name.to_str().ok_or("Invalid UTF-8 in file name")?;

        if let Some(file_name_str) = ignore_tar_entry(file_name_str, 1) {
            let file_name_str = format!("node_modules/{}/{}", ident.as_str(), file_name_str);

            // Start a new file in the zip archive.
            zip.start_file(file_name_str, FileOptions::default()
                .compression_method(CompressionMethod::Deflated))?;

            // Copy contents from the tar file to the zip file.
            let mut contents = Vec::new();
            file.read_to_end(&mut contents)?;
            zip.write_all(&contents)?;
        }
    }

    // Finalize the zip archive.
    zip.finish()?;
    drop(zip);

    // Retrieve the internal buffer of the zip writer.
    let zip_data = zip_buffer.into_inner();

    // Convert Vec<u8> to Bytes and return.
    Ok(zip_data)
}

pub async fn fetch<'a>(context: InstallContext<'a>, locator: &Locator, parent_data: Option<PackageData>) -> Result<PackageData, Error> {
    match &locator.reference {
        Reference::Link(path)
            => fetch_link(path, &locator.parent, parent_data),

        Reference::Portal(path)
            => fetch_portal(path, &locator.parent, parent_data),

        Reference::Semver(version)
            => fetch_semver(context, &locator, &locator.ident, &version).await,

        Reference::SemverAlias(ident, version)
            => fetch_semver(context, &locator, &ident, &version).await,

        Reference::Workspace(ident)
            => fetch_workspace(context, &ident),

        _ => Err(Error::Unsupported),
    }
}

pub fn fetch_link(path: &String, parent: &Option<Arc<Locator>>, parent_data: Option<PackageData>) -> Result<PackageData, Error> {
    let parent = parent.as_ref()
        .expect("The parent locator is required for resolving a linked package");
    let parent_data = parent_data
        .expect("The parent data is required for retrieving the path of a linked package");

    let link_path = parent_data.path()
        .with_join(&parent_data.source_dir(parent))
        .with_join_str(&path);

    Ok(PackageData::Local {
        path: link_path,
        discard_from_lookup: true,
    })
}

pub fn fetch_portal(path: &String, parent: &Option<Arc<Locator>>, parent_data: Option<PackageData>) -> Result<PackageData, Error> {
    let parent = parent.as_ref()
        .expect("The parent locator is required for resolving a portal package");
    let parent_data = parent_data
        .expect("The parent data is required for retrieving the path of a portal package");

    let portal_path = parent_data.path()
        .with_join(&parent_data.source_dir(parent))
        .with_join_str(&path);

    Ok(PackageData::Local {
        path: portal_path,
        discard_from_lookup: false,
    })
}

pub async fn fetch_tarball<'a>(context: InstallContext<'a>, locator: &Locator, path: &String, parent: &Option<Arc<Locator>>, parent_data: Option<PackageData>) -> Result<PackageData, Error> {
    let parent = parent.as_ref()
        .expect("The parent locator is required for resolving a tarball package");
    let parent_data = parent_data
        .expect("The parent data is required for retrieving the path of a tarball package");

    let tarball_path = parent_data.path()
        .with_join(&parent_data.source_dir(parent))
        .with_join_str(&path);

    let (path, data, checksum) = context.package_cache.unwrap().upsert_blob(locator.clone(), &".zip", || async {
        let archive = std::fs::read(tarball_path.to_path_buf())
            .map_err(Arc::new)?;

        convert_tar_gz_to_zip(&locator.ident, Bytes::from(archive))
            .map_err(|err| Error::PackageConversionError(Arc::new(err)))
    }).await?;

    Ok(PackageData::Zip {
        path,
        data,
        checksum,
    })
}

pub async fn fetch_directory<'a>(context: InstallContext<'a>, locator: &Locator, path: &String, parent: &Option<Arc<Locator>>, parent_data: Option<PackageData>) -> Result<PackageData, Error> {
    let parent = parent.as_ref()
        .expect("The parent locator is required for resolving a tarball package");
    let parent_data = parent_data
        .expect("The parent data is required for retrieving the path of a tarball package");

    let tarball_path = parent_data.path()
        .with_join(&parent_data.source_dir(parent))
        .with_join_str(&path);

    let (path, data, checksum) = context.package_cache.unwrap().upsert_blob(locator.clone(), &".zip", || async {
        let archive = std::fs::read(tarball_path.to_path_buf())
            .map_err(Arc::new)?;

        convert_tar_gz_to_zip(&locator.ident, Bytes::from(archive))
            .map_err(|err| Error::PackageConversionError(Arc::new(err)))
    }).await?;

    Ok(PackageData::Zip {
        path,
        data,
        checksum,
    })
}

pub async fn fetch_semver<'a>(context: InstallContext<'a>, locator: &Locator, ident: &Ident, version: &semver::Version) -> Result<PackageData, Error> {
    let (path, data, checksum) = context.package_cache.unwrap().upsert_blob(locator.clone(), &".zip", || async {
        let client = http_client()?;
        let url = format!("{}/{}/-/{}-{}.tgz", registry_url_for(ident), ident, ident.name(), version.to_string());

        let response = client.get(url.clone()).send().await
            .map_err(|err| Error::RemoteRegistryError(Arc::new(err)))?;

        let archive = response.bytes().await
            .map_err(|err| Error::RemoteRegistryError(Arc::new(err)))?;

        convert_tar_gz_to_zip(ident, archive)
            .map_err(|err| Error::PackageConversionError(Arc::new(err)))
    }).await?;

    Ok(PackageData::Zip {
        path,
        data,
        checksum,
    })
}

pub fn fetch_workspace(context: InstallContext, ident: &Ident) -> Result<PackageData, Error> {
    let project = context.project
        .expect("The project is required for fetching a workspace package");

    let workspace = project.workspaces
        .get(ident)
        .ok_or(Error::WorkspaceNotFound(ident.clone()))?;

    Ok(PackageData::Local {
        path: workspace.path.clone(),
        discard_from_lookup: false,
    })
}
