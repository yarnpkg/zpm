use std::io::{self, ErrorKind};

use zpm_utils::Path;

use crate::error::Error;

use super::Manifest;

fn wrap_error<T>(result: Result<T, io::Error>) -> Result<T, Error> {
    result.map_err(|err| match err.kind() {
        ErrorKind::NotFound | ErrorKind::NotADirectory => Error::ManifestNotFound,
        _ => err.into(),
    })
}

pub fn read_manifest(abs_path: &Path) -> Result<Manifest, Error> {
    let metadata = wrap_error(abs_path.fs_metadata())?;

    Ok(read_manifest_with_size(abs_path, metadata.len())?)
}

pub fn read_manifest_with_size(abs_path: &Path, size: u64) -> Result<Manifest, Error> {
    let manifest_text = wrap_error(abs_path.fs_read_text_with_size(size))?;

    parse_manifest(&manifest_text)
}

pub fn parse_manifest_from_bytes(bytes: &[u8]) -> Result<Manifest, Error> {
    if bytes.len() > 0 {
        Ok(sonic_rs::from_slice(bytes)?)
    } else {
        Ok(Manifest::default())
    }
}

pub fn parse_manifest(manifest_text: &str) -> Result<Manifest, Error> {
    if manifest_text.len() > 0 {
        Ok(sonic_rs::from_str(&manifest_text)?)
    } else {
        Ok(Manifest::default())
    }
}
