use pnp::fs::VPathInfo;
use zpm_utils::{Path, ToFileString};

use crate::{error::Error, zip_iter::ZipIterator, zip_structs::{CentralDirectoryRecord, EndOfCentralDirectoryRecord, FileHeader, GeneralRecord}, CompressionAlgorithm};

use super::Entry;

#[derive(Debug, Clone)]
pub struct CraftZipOptions {
    pub compression: Option<CompressionAlgorithm>,
}

unsafe fn any_as_u8_slice<T: Sized>(p: &T) -> &[u8] {
    ::core::slice::from_raw_parts(
        (p as *const T) as *const u8,
        ::core::mem::size_of::<T>(),
    )
}

pub fn entries_from_zip(buffer: &[u8]) -> Result<Vec<Entry>, Error> {
    ZipIterator::new(buffer)?.collect()
}

pub fn first_entry_from_zip(buffer: &[u8]) -> Result<Entry, Error> {
    ZipIterator::new(buffer)?.next()
        .unwrap_or_else(|| Err(Error::InvalidZipFile("Empty".to_string())))
}

pub fn craft_zip(entries: &[Entry]) -> Vec<u8> {
    let mut general_capacity = 0;
    let mut central_directory_capacity = std::mem::size_of::<EndOfCentralDirectoryRecord>();

    for entry in entries {
        let compressed_data = entry.compression
            .as_ref()
            .map_or(&entry.data, |compressed_data| &compressed_data.data);

        general_capacity
            += std::mem::size_of::<GeneralRecord>() + entry.name.len() + compressed_data.len();

        central_directory_capacity
            += std::mem::size_of::<CentralDirectoryRecord>() + entry.name.len();
    }

    let mut general_segment
        = Vec::with_capacity(general_capacity);
    let mut central_directory_segment
        = Vec::with_capacity(central_directory_capacity);

    for entry in entries {
        let compressed_data = entry.compression
            .as_ref()
            .map_or(&entry.data, |compressed_data| &compressed_data.data);

        let compression = entry.compression
            .as_ref()
            .map(|compressed_data| compressed_data.algorithm);

        let offset = general_segment.len();

        inject_general_record(&mut general_segment, entry, compressed_data, compression);
        inject_central_directory_record(&mut central_directory_segment, entry, compressed_data, offset, compression);
    }

    unsafe {
        central_directory_segment.extend_from_slice(
            any_as_u8_slice(&EndOfCentralDirectoryRecord {
                signature: [0x50, 0x4b, 0x05, 0x06],
                disk_number: 0x00,
                disk_with_central_directory: 0x00,
                number_of_files_on_this_disk: entries.len() as u16,
                number_of_files: entries.len() as u16,
                size_of_central_directory: central_directory_segment.len() as u32,
                offset_of_central_directory: general_segment.len() as u32,
                comment_length: 0x00,
            }),
        );
    }

    assert_eq!(general_segment.len(), general_capacity);
    assert_eq!(central_directory_segment.len(), central_directory_capacity);

    [general_segment, central_directory_segment].concat()
}

fn inject_general_record(target: &mut Vec<u8>, entry: &Entry, compressed_data: &[u8], compression: Option<CompressionAlgorithm>) {
    let compression_method = match compression {
        Some(CompressionAlgorithm::Deflate(_)) => 0x08, // Deflate compression
        None => 0x00, // No compression
    };

    unsafe {
        target.extend_from_slice(
            any_as_u8_slice(&GeneralRecord {
                signature: [0x50, 0x4b, 0x03, 0x04],
                header: FileHeader {
                    version_needed_to_extract: if compression_method == 0x08 { 0x14 } else { 0x0A },
                    general_purpose_bit_flag: 0x00,
                    compression_method,
                    last_mod_file_time: 0xae40,
                    last_mod_file_date: 0x08d6,
                    crc_32: entry.crc,
                    compressed_size: compressed_data.len() as u32,
                    uncompressed_size: entry.data.len() as u32,
                    file_name_length: entry.name.len() as u16,
                    extra_field_length: 0x00,
                },
            }),
        );
    }

    // File name
    target.extend_from_slice(entry.name.as_bytes());

    // File data (compressed or uncompressed)
    target.extend_from_slice(compressed_data);
}

fn inject_central_directory_record(target: &mut Vec<u8>, entry: &Entry, compressed_data: &[u8], offset: usize, compression: Option<CompressionAlgorithm>) {
    let compression_method = match compression {
        Some(CompressionAlgorithm::Deflate(_)) => 0x08, // Deflate compression
        None => 0x00, // No compression
    };

    unsafe {
        target.extend_from_slice(
            any_as_u8_slice(&CentralDirectoryRecord {
                signature: [0x50, 0x4b, 0x01, 0x02],
                version_made_by: 0x0314, // UNIX
                header: FileHeader {
                    version_needed_to_extract: if compression_method == 0x08 { 0x14 } else { 0x14 },
                    general_purpose_bit_flag: 0x00,
                    compression_method,
                    last_mod_file_time: 0xae40,
                    last_mod_file_date: 0x08d6,
                    crc_32: entry.crc,
                    compressed_size: compressed_data.len() as u32,
                    uncompressed_size: entry.data.len() as u32,
                    file_name_length: entry.name.len() as u16,
                    extra_field_length: 0x00,
                },
                file_comment_length: 0x00,
                disk_number_start: 0x00,
                internal_file_attributes: 0x00,
                external_file_attributes: (entry.mode as u32) << 16,
                relative_offset_of_local_header: offset as u32,
            }),
        );
    }

    // File name
    target.extend_from_slice(entry.name.as_bytes());
}

pub trait ZipSupport {
    fn fs_read_text_from_zip_buffer(&self, buf: &[u8]) -> Result<String, Error>;
    fn fs_read_text_with_zip(&self) -> Result<String, Error>;
}

impl ZipSupport for Path {
    fn fs_read_text_from_zip_buffer(&self, zip_data: &[u8]) -> Result<String, Error> {
        let path_as_string
            = self.to_file_string();

        let entries
            = entries_from_zip(zip_data)?;

        let entry = entries.iter()
            .find(|entry| entry.name == path_as_string)
            .ok_or(std::io::Error::from(std::io::ErrorKind::NotFound))?;

        Ok(String::from_utf8_lossy(&entry.data).to_string())
    }

    fn fs_read_text_with_zip(&self) -> Result<String, Error> {
        let path_str
            = self.to_path_buf();

        let parsed
            = pnp::fs::vpath(&path_str)?;

        match parsed {
            pnp::fs::VPath::Native(_) => {
                Ok(self.fs_read_text_prealloc()?)
            },

            pnp::fs::VPath::Virtual(info) => {
                let file_data
                    = Path::try_from(info.physical_base_path()).unwrap()
                        .fs_read_text_prealloc()?;

                Ok(file_data)
            },

            pnp::fs::VPath::Zip(info) => {
                let zip_data
                    = Path::try_from(info.physical_base_path()).unwrap()
                        .fs_read_prealloc()?;

                let file_data = Path::try_from(info.zip_path)?
                    .fs_read_text_from_zip_buffer(&zip_data)?;

                Ok(file_data)
            },
        }
    }
}
