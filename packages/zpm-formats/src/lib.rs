use std::borrow::Cow;

use libdeflater::{CompressionLvl, Compressor};
use zpm_utils::{FromFileString, impl_file_string_from_str, Path, ToFileString, ToHumanString};

pub(crate) mod zip_structs;

pub mod error;
pub mod iter_ext;
pub mod tar_iter;
pub mod tar;
pub mod zip_iter;
pub mod zip;

pub use error::Error;

/// Compresses `input` as a raw DEFLATE stream at `level`. We use
/// libdeflate (not flate2's `zlib-rs` backend) because zlib-rs's
/// per-arch SIMD paths produce bit-different — still valid — output
/// on x86_64 vs aarch64. The cache packer hashes the resulting zip,
/// so drift would surface as lockfile checksum mismatches under
/// `--refresh-lockfile` in hardened-mode CI.
pub(crate) fn deflate_compress(input: &[u8], level: usize) -> Vec<u8> {
    let mut compressor
        = Compressor::new(CompressionLvl::new(level as i32).expect("compression level out of range"));

    let bound
        = compressor.deflate_compress_bound(input.len());

    let mut out = vec![0u8; bound];
    let n = compressor.deflate_compress(input, &mut out)
        .expect("output buffer sized via deflate_compress_bound");

    out.truncate(n);
    out
}

pub(crate) fn gzip_compress(input: &[u8], level: usize) -> Vec<u8> {
    let mut compressor
        = Compressor::new(CompressionLvl::new(level as i32).expect("compression level out of range"));

    let bound
        = compressor.gzip_compress_bound(input.len());

    let mut out = vec![0u8; bound];
    let n = compressor.gzip_compress(input, &mut out)
        .expect("output buffer sized via gzip_compress_bound");

    out.truncate(n);
    out
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum CompressionAlgorithm {
    Deflate(usize),
}

impl FromFileString for CompressionAlgorithm {
    type Error = Error;

    fn from_file_string(src: &str) -> Result<Self, Self::Error> {
        if src == "mixed" {
            return Err(Error::MixedValueDeprecated);
        }

        let level
            = src.parse::<usize>()
                .map_err(|_| Error::InvalidCompressionLevel)?;

        if level > 9 {
            return Err(Error::InvalidCompressionLevel);
        }

        Ok(CompressionAlgorithm::Deflate(level))
    }
}

impl ToFileString for CompressionAlgorithm {
    fn to_file_string(&self) -> String {
        match self {
            CompressionAlgorithm::Deflate(level) => level.to_string(),
        }
    }
}

impl ToHumanString for CompressionAlgorithm {
    fn to_print_string(&self) -> String {
        match self {
            CompressionAlgorithm::Deflate(level) => level.to_string(),
        }
    }
}

impl<'de> serde::Deserialize<'de> for CompressionAlgorithm {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error> where D: serde::Deserializer<'de> {
        let s
            = usize::deserialize(deserializer)?;

        Ok(CompressionAlgorithm::Deflate(s))
    }
}

impl serde::Serialize for CompressionAlgorithm {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error> where S: serde::Serializer {
        match self {
            CompressionAlgorithm::Deflate(level)
                => serializer.serialize_u32(*level as u32),
        }
    }
}

impl_file_string_from_str!(CompressionAlgorithm);

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct Compression<'a> {
    pub data: Cow<'a, [u8]>,
    pub algorithm: CompressionAlgorithm,
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct Entry<'a> {
    pub name: Path,
    pub mode: u32,
    pub crc: u32,
    pub data: Cow<'a, [u8]>,
    pub compression: Option<Compression<'a>>,
}

impl<'a> Entry<'a> {
    pub fn new(name: Path) -> Self {
        Entry {
            name,
            mode: 0o644,
            crc: 0,
            data: Cow::Borrowed(b""),
            compression: None,
        }
    }

    pub fn new_file(name: Path, data: Cow<'a, [u8]>) -> Self {
        Entry {
            name,
            mode: 0o644,
            crc: 0,
            data,
            compression: None,
        }
    }

    pub fn compress_in_place(&mut self, algorithm: CompressionAlgorithm) {
        let compressed_data = match algorithm {
            CompressionAlgorithm::Deflate(level) => {
                deflate_compress(&self.data, level)
            },
        };

        self.compression = Some(Compression {
            data: Cow::Owned(compressed_data),
            algorithm,
        });
    }
}

pub fn entries_to_disk<'a>(entries: &[Entry<'a>], base: &Path) -> Result<(), Error> {
    for entry in entries {
        base.with_join(&entry.name)
            .fs_create_parent()?
            .fs_change(&entry.data, entry.mode & 0o111 == 0o111)?;
    }

    Ok(())
}

pub fn entries_from_folder<'a>(path: &Path) -> Result<Vec<Entry<'a>>, Error> {
    let base = path.clone();
    let mut entries = vec![];
    let mut process_queue = vec![path.clone()];

    while let Some(path) = process_queue.pop() {
        let listing = path.fs_read_dir()?;

        for entry in listing {
            let entry = entry?;
            let entry_path = Path::try_from(entry.path())?;

            if entry_path.fs_is_dir() {
                process_queue.push(entry_path);
                continue;
            }

            let rel_path = entry_path.relative_to(&base);
            let data = entry_path.fs_read()?;
            let is_exec = entry_path.fs_is_executable()?;
            let mode = if is_exec { 0o755 } else { 0o644 };

            entries.push(Entry {
                name: rel_path,
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
        let is_exec = abs_path.fs_is_executable()?;
        let mode = if is_exec { 0o755 } else { 0o644 };

        entries.push(Entry {
            name: rel_path.clone(),
            mode,
            crc: 0,
            data: Cow::Owned(data),
            compression: None,
        });
    }

    Ok(entries)
}
