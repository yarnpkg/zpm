use std::io::{Cursor, Read};

use arca::Path;
use bytes::Bytes;

use crate::{error::Error, primitives::Ident};

use super::{entries_from_folder, normalize_entries, prefix_entries, strip_first_segment, tar, zip, Entry};

pub fn convert_entries_to_zip(ident: &Ident, entries: Vec<Entry>) -> Result<Vec<u8>, Error> {
    let entries = normalize_entries(entries);
    let entries = prefix_entries(entries, format!("node_modules/{}", ident.as_str()));

    Ok(zip::craft_zip(&entries))
}

pub fn convert_tar_gz_to_zip(ident: &Ident, tar_gz_data: Bytes) -> Result<Vec<u8>, Error> {
    let mut decompressed = vec![];

    if tar_gz_data.starts_with(&[0x1f, 0x8b]) {
        flate2::read::GzDecoder::new(Cursor::new(tar_gz_data)).read_to_end(&mut decompressed)?;
    } else {
        decompressed = tar_gz_data.to_vec();
    }

    let entries = tar::entries_from_tar(&decompressed)?;
    let entries = strip_first_segment(entries);

    convert_entries_to_zip(ident, entries)
}

pub fn convert_folder_to_zip(ident: &Ident, folder_path: &Path) -> Result<Vec<u8>, Error> {
    let entries = entries_from_folder(folder_path)?;

    convert_entries_to_zip(ident, entries)
}
