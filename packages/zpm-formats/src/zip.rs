use std::{borrow::Cow, sync::LazyLock};

use pnp::fs::VPathInfo;
use zpm_utils::{Path, ToFileString};
use regex::Regex;

use crate::{error::Error, zip_iter::ZipIterator, zip_structs::{CentralDirectoryRecord, EndOfCentralDirectoryRecord, FileHeader, GeneralRecord}};

use super::Entry;

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
        general_capacity += std::mem::size_of::<GeneralRecord>() + entry.name.len() + entry.data.len();
        central_directory_capacity += std::mem::size_of::<CentralDirectoryRecord>() + entry.name.len();
    }

    let mut general_segment = Vec::with_capacity(general_capacity);
    let mut central_directory_segment = Vec::with_capacity(central_directory_capacity);

    for entry in entries {
        let offset = general_segment.len();

        inject_general_record(&mut general_segment, entry);
        inject_central_directory_record(&mut central_directory_segment, entry, offset);
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

fn inject_general_record(target: &mut Vec<u8>, entry: &Entry) {
    unsafe {
        target.extend_from_slice(
            any_as_u8_slice(&GeneralRecord {
                signature: [0x50, 0x4b, 0x03, 0x04],
                header: FileHeader {
                    version_needed_to_extract: 0x0A,
                    general_purpose_bit_flag: 0x00,
                    compression_method: 0x00,
                    last_mod_file_time: 0xae40,
                    last_mod_file_date: 0x08d6,
                    crc_32: entry.crc,
                    compressed_size: entry.data.len() as u32,
                    uncompressed_size: entry.data.len() as u32,
                    file_name_length: entry.name.len() as u16,
                    extra_field_length: 0x00,
                },
            }),
        );
    }

    // File name
    target.extend_from_slice(entry.name.as_bytes());

    // File data
    target.extend_from_slice(&entry.data);
}

fn inject_central_directory_record(target: &mut Vec<u8>, entry: &Entry, offset: usize) {
    unsafe {
        target.extend_from_slice(
            any_as_u8_slice(&CentralDirectoryRecord {
                signature: [0x50, 0x4b, 0x01, 0x02],
                version_made_by: 0x0314, // UNIX
                header: FileHeader {
                    version_needed_to_extract: 0x14,
                    general_purpose_bit_flag: 0x00,
                    compression_method: 0x00,
                    last_mod_file_time: 0xae40,
                    last_mod_file_date: 0x08d6,
                    crc_32: entry.crc,
                    compressed_size: entry.data.len() as u32,
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

static VIRTUAL_REGEX: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"/__virtual__/[^/]+/0/").unwrap());
static ZIP_REGEX: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(.*\.zip)/(?:__virtual__/[^/]+/0/)?(.*)").unwrap());

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
