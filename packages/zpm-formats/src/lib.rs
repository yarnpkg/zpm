use std::{borrow::Cow, io::Write, os::unix::fs::PermissionsExt};

use flate2::{write::DeflateEncoder, Compression as FlateCompression};
use zpm_utils::{Path, ToFileString};

pub(crate) mod zip_structs;

pub mod convert;
pub mod error;
pub mod iter_ext;
pub mod tar_iter;
pub mod tar;
pub mod zip_iter;
pub mod zip;

pub use error::Error;

#[derive(Debug, Clone, Copy)]
pub enum CompressionAlgorithm {
    Deflate(usize),
}

#[derive(Debug, Clone)]
pub struct Compression<'a> {
    pub data: Cow<'a, [u8]>,
    pub algorithm: CompressionAlgorithm,
}

#[derive(Debug, Clone)]
pub struct Entry<'a> {
    pub name: String,
    pub mode: u32,
    pub crc: u32,
    pub data: Cow<'a, [u8]>,
    pub compression: Option<Compression<'a>>,
}

impl<'a> Entry<'a> {
    pub fn into_compressed(mut self, compression: CompressionAlgorithm) -> Self {
        let compressed_data = match compression {
            CompressionAlgorithm::Deflate(level) => {
                let mut encoder
                    = DeflateEncoder::new(Vec::new(), FlateCompression::new(level as u32));

                encoder.write_all(&self.data).unwrap();
                encoder.finish().unwrap()
            },
        };

        self.compression = Some(Compression {
            data: Cow::Owned(compressed_data),
            algorithm: compression,
        });

        self
    }
}

pub fn entries_to_disk<'a>(entries: &[Entry<'a>], base: &Path) -> Result<(), Error> {
    for entry in entries {
        base.with_join_str(&entry.name)
            .fs_create_parent()?
            .fs_change(&entry.data, entry.mode & 0o111 == 0o111)?;
    }

    Ok(())
}

pub fn entries_from_folder<'a>(path: &Path) -> Result<Vec<Entry<'a>>, Error> {
    let mut entries = vec![];
    let mut process_queue = vec![path.clone()];

    while let Some(path) = process_queue.pop() {
        let listing = path.fs_read_dir()?;

        for entry in listing {
            let entry = entry?;
            let path = Path::try_from(entry.path())?;

            if path.fs_is_dir() {
                process_queue.push(path);
                continue;
            }

            let name = entry.file_name().into_string().unwrap();
            let data = path.fs_read()?;
            let metadata = path.fs_metadata()?;

            let is_exec = metadata.permissions().mode() & 0o111 != 0;
            let mode = if is_exec { 0o755 } else { 0o644 };

            entries.push(Entry {
                name,
                mode,
                crc: 0,
                data: Cow::Owned(data),
                compression: None,
            });
        }
    }

    Ok(entries)
}

pub fn entries_from_files<'a>(base: &Path, files: &[Path]) -> Result<Vec<Entry<'a>>, Error> {
    let mut entries = vec![];

    for rel_path in files {
        let abs_path = base
            .with_join(rel_path);

        let data = abs_path.fs_read()?;
        let metadata = abs_path.fs_metadata()?;

        let is_exec = metadata.permissions().mode() & 0o111 != 0;
        let mode = if is_exec { 0o755 } else { 0o644 };

        entries.push(Entry {
            name: rel_path.to_file_string(),
            mode,
            crc: 0,
            data: Cow::Owned(data),
            compression: None,
        });
    }

    Ok(entries)
}

pub fn normalize_entries(mut entries: Vec<Entry>) -> Vec<Entry> {
    entries.sort_by(|a, b| a.name.cmp(&b.name));

    if let Some(manifest_idx) = entries.iter().position(|entry| entry.name == "package.json") {
        let manifest_entry = entries.remove(manifest_idx);
        entries.insert(0, manifest_entry);
    }

    entries
}

pub fn prefix_entries<T: AsRef<str>>(mut entries: Vec<Entry>, prefix: T) -> Vec<Entry> {
    for entry in entries.iter_mut() {
        entry.name = format!("{}/{}", prefix.as_ref(), entry.name);
    }

    entries
}
pub fn strip_first_segment(entries: Vec<Entry>) -> Vec<Entry> {
    let mut next = vec![];

    for mut entry in entries.into_iter() {
        if let Some(slash_index) = entry.name.find('/') {
            entry.name = entry.name[slash_index + 1..].to_string();
            next.push(entry);
        }
    }

    next
}

pub fn strip_prefix<T: AsRef<str>>(mut entries: Vec<Entry>, prefix: T) -> Vec<Entry> {
    let prefix = prefix.as_ref();

    for entry in entries.iter_mut() {
        if entry.name.starts_with(prefix) {
            entry.name = entry.name[prefix.len() + 1..].to_string();
        }
    }

    entries
}

pub fn compute_crc32(mut entries: Vec<Entry>) -> Vec<Entry> {
    for entry in entries.iter_mut() {
        entry.crc = crc32fast::hash(&entry.data);
    }

    entries
}
