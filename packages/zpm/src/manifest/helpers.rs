use zpm_parsers::JsonDocument;
use zpm_utils::{IoResultExt, Path};

use crate::error::Error;

use super::Manifest;

pub fn read_manifest(abs_path: &Path) -> Result<Manifest, Error> {
    let metadata = abs_path.fs_metadata()
        .ok_missing()?
        .ok_or(Error::ManifestNotFound)?;

    Ok(read_manifest_with_size(abs_path, metadata.len())?)
}

pub fn read_manifest_with_size(abs_path: &Path, size: u64) -> Result<Manifest, Error> {
    let manifest_text = abs_path.fs_read_text_with_size(size)
        .ok_missing()?
        .ok_or(Error::ManifestNotFound)?;

    parse_manifest(&manifest_text)
        .map_err(|_| Error::ManifestParseError(abs_path.clone()))
}

pub fn parse_manifest_from_bytes(bytes: &[u8]) -> Result<Manifest, Error> {
    if bytes.len() > 0 {
        Ok(JsonDocument::hydrate_from_slice(bytes)?)
    } else {
        Ok(Manifest::default())
    }
}

pub fn parse_manifest(manifest_text: &str) -> Result<Manifest, Error> {
    if manifest_text.len() > 0 {
        Ok(JsonDocument::hydrate_from_str(&manifest_text)?)
    } else {
        Ok(Manifest::default())
    }
}
