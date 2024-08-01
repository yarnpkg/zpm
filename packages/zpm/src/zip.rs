use std::{borrow::Cow, os::unix::fs::PermissionsExt, path::PathBuf, sync::Arc};

use arca::Path;
use itertools::Itertools;
use once_cell::sync::Lazy;
use regex::Regex;

use crate::error::Error;

#[cfg(test)]
#[path = "./zip.test.rs"]
mod zip_tests;

unsafe fn any_as_u8_slice<T: Sized>(p: &T) -> &[u8] {
    ::core::slice::from_raw_parts(
        (p as *const T) as *const u8,
        ::core::mem::size_of::<T>(),
    )
}

fn trim_zero(x: &[u8]) -> &[u8] {
    match x.iter().find_position(|c| c == &&0) {
        Some((i, _)) => &x[..i],
        None => x,
    }
}

fn from_oct(x: &[u8]) -> u64 {
    let mut result = 0;
    for i in x.iter().filter(|c| **c >= b'0' && **c <= b'7') {
        result = result * 8 + (i - b'0') as u64;
    }
    result
}

#[derive(Debug, PartialEq)]
pub struct Entry<'a> {
    pub name: String,
    pub mode: u64,
    pub crc: u32,
    pub data: Cow<'a, [u8]>,
}

#[repr(packed)]
struct FileHeader {
    version_needed_to_extract: u16,
    general_purpose_bit_flag: u16,
    compression_method: u16,
    last_mod_file_time: u16,
    last_mod_file_date: u16,
    crc_32: u32,
    compressed_size: u32,
    uncompressed_size: u32,
    file_name_length: u16,
    extra_field_length: u16,
}

#[repr(packed)]
struct GeneralRecord {
    signature: [u8; 4],
    header: FileHeader,
}

#[repr(packed)]
struct CentralDirectoryRecord {
    signature: [u8; 4],
    version_made_by: u16,
    header: FileHeader,
    file_comment_length: u16,
    disk_number_start: u16,
    internal_file_attributes: u16,
    external_file_attributes: u32,
    relative_offset_of_local_header: u32,
}

#[repr(packed)]
struct EndOfCentralDirectoryRecord {
    signature: [u8; 4],
    disk_number: u16,
    disk_with_central_directory: u16,
    number_of_files_on_this_disk: u16,
    number_of_files: u16,
    size_of_central_directory: u32,
    offset_of_central_directory: u32,
    comment_length: u16,
}

static ZIP_PATH_INVALID_PATTERNS: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"\\|/\.{0,2}/|^\.{0,2}/|/\.{0,2}$|^\.{0,2}$").unwrap()
});

fn clean_name(name: &str) -> Option<String> {
    if name.starts_with('/') {
        return None
    }

    let has_parent_specifier = name.split('/')
        .any(|part| part == "..");

    if has_parent_specifier {
        return None
    }

    let mut name = arca::Path::from(name).to_string();

    if name.ends_with('/') {
        name.pop();
    }

    Some(name)
}

pub fn entries_from_tar(buffer: &[u8]) -> Result<Vec<Entry>, Error> {
    let mut offset = 0;
    let mut entries = vec![];

    while offset < buffer.len() {
        let size = from_oct(&buffer[offset + 124..offset + 136]) as usize;

        if buffer[offset] != 0 {
            let name_slice = trim_zero(&buffer[offset..offset + 100]);

            let name = match std::str::from_utf8(name_slice) {
                Ok(v) => v,
                Err(e) => panic!("Invalid UTF-8 sequence: {}", e),
            }.to_string();

            let name = clean_name(&name)
                .ok_or(Error::InvalidTarFilePath(name))?;

            if ZIP_PATH_INVALID_PATTERNS.is_match(&name) {
                return Err(Error::InvalidTarFilePath(name));
            }

            if buffer[offset + 156] == b'0' {
                let mode = from_oct(&buffer[offset + 100..offset + 108]);
                let data = &buffer[offset + 512..offset + 512 + size];

                entries.push(Entry {
                    name,
                    mode,
                    crc: 0,
                    data: Cow::Borrowed(data),
                });
            }
        }

        // round up to the next multiple of 512
        offset += 512 + ((size + 511) / 512) * 512;
    }

    Ok(entries)
}

pub fn entries_from_folder<'a>(path: PathBuf) -> Result<Vec<Entry<'a>>, Error> {
    let mut entries = vec![];
    let mut process_queue = vec![path];

    while let Some(path) = process_queue.pop() {
        let listing = std::fs::read_dir(&path)
            .map_err(Arc::new)
            .map_err(Error::IoError)?;

        for entry in listing {
            let entry = entry
                .map_err(Arc::new)
                .map_err(Error::IoError)?;

            let path = entry.path();

            if path.is_dir() {
                process_queue.push(path);
                continue;
            }

            let name = entry.file_name().into_string().unwrap();
            let data = std::fs::read(&path)
                .map_err(Arc::new)
                .map_err(Error::IoError)?;

            let metadata = path.metadata()
                .map_err(Arc::new)
                .map_err(Error::IoError)?;

            let is_exec = metadata.permissions().mode() & 0o111 != 0;
            let mode = if is_exec { 0o755 } else { 0o644 };

            entries.push(Entry {
                name,
                mode,
                crc: 0,
                data: Cow::Owned(data),
            });
        }
    }

    Ok(entries)
}

pub fn entries_from_zip(buffer: &[u8]) -> Result<Vec<Entry>, Error> {
    let end = buffer.len() - std::mem::size_of::<EndOfCentralDirectoryRecord>();
    let end_of_central_directory_record = unsafe {
        &*(buffer[end..].as_ptr() as *const EndOfCentralDirectoryRecord)
    };

    let mut offset = 0;
    let mut entries = vec![];

    while offset < end_of_central_directory_record.offset_of_central_directory as usize {
        let general_record = unsafe {
            &*(buffer[offset..].as_ptr() as *const GeneralRecord)
        };

        let name = std::str::from_utf8(&buffer[offset + 30..offset + 30 + general_record.header.file_name_length as usize])
            .map_err(Arc::new)
            .map_err(Error::Utf8Error)?;

        let size = general_record.header.compressed_size as usize;
        let data = &buffer[offset + 30 + general_record.header.file_name_length as usize..offset + 30 + general_record.header.file_name_length as usize + size];

        entries.push(Entry {
            name: name.to_string(),
            mode: 0o644,
            crc: general_record.header.crc_32,
            data: Cow::Borrowed(data),
        });

        offset += 30 + general_record.header.file_name_length as usize + size;
    }

    Ok(entries)
}

pub fn first_entry_from_zip(buffer: &[u8]) -> Result<Entry, Error> {
    unsafe {
        let general_record = &*(buffer.as_ptr() as *const GeneralRecord);
        let name = std::str::from_utf8(&buffer[30..30 + general_record.header.file_name_length as usize])
            .map_err(Arc::new)
            .map_err(Error::Utf8Error)?;

        let size = general_record.header.compressed_size as usize;
        let data = &buffer[30 + general_record.header.file_name_length as usize..30 + general_record.header.file_name_length as usize + size];

        Ok(Entry {
            name: name.to_string(),
            mode: 0o644,
            crc: general_record.header.crc_32,
            data: Cow::Borrowed(data),
        })    
    }
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

pub fn compute_crc32(mut entries: Vec<Entry>) -> Vec<Entry> {
    for entry in entries.iter_mut() {
        entry.crc = crc32fast::hash(&entry.data);
    }

    entries
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
                version_made_by: 0x08,
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
                external_file_attributes: 0x00,
                relative_offset_of_local_header: offset as u32,
            }),
        );
    }

    // File name
    target.extend_from_slice(entry.name.as_bytes());
}

const VIRTUAL_REGEX: Lazy<Regex> = Lazy::new(|| Regex::new(r"/__virtual__/[^/]+/0/").unwrap());
const ZIP_REGEX: Lazy<Regex> = Lazy::new(|| Regex::new(r"(.*\.zip)/(?:__virtual__/[^/]+/0/)?(.*)").unwrap());

pub trait ZipSupport {
    fn fs_read_text_with_zip(&self) -> Result<String, Error>;
}

impl ZipSupport for Path {
    fn fs_read_text_with_zip(&self) -> Result<String, Error> {
        let path_str = self.to_string();

        if let Some(captures) = ZIP_REGEX.captures(&path_str) {
            let zip_path = captures.get(1).unwrap().as_str();
            let subpath = captures.get(2).unwrap().as_str();

            let zip_data = std::fs::read(zip_path)
                .map_err(Arc::new)
                .map_err(Error::IoError)?;

            let entries = entries_from_zip(&zip_data)?;

            let entry = entries.iter()
                .find(|entry| entry.name == subpath)
                .ok_or(std::io::Error::from(std::io::ErrorKind::NotFound))
                .map_err(Arc::new)?;

            Ok(String::from_utf8_lossy(&entry.data).to_string())
        } else {
            Ok(match VIRTUAL_REGEX.replace(&path_str, "/") {
                Cow::Borrowed(_) => self.fs_read_text().map_err(Arc::new)?,
                Cow::Owned(path_str) => Path::from(path_str).fs_read_text().map_err(Arc::new)?,
            })
        }
    }
}
