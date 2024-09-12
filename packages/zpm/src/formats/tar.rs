use std::{borrow::Cow, io::Write, sync::LazyLock};

use itertools::Itertools;
use regex::Regex;

use crate::error::Error;

use super::Entry;

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

static ZIP_PATH_INVALID_PATTERNS: LazyLock<Regex> = LazyLock::new(|| {
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

#[allow(dead_code)]
#[repr(packed)]
struct FileHeader {
    file_name: [u8; 100],
    file_mode: [u8; 8],
    owner_id: [u8; 8],
    group_id: [u8; 8],
    file_size: [u8; 12],
    last_modification_time: [u8; 12],
    checksum: [u8; 8],
    file_type: u8,
    linked_file_name: [u8; 100],
    padding: [u8; 255],
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

pub fn craft_tar(entries: &[Entry]) -> Vec<u8> {
    let mut total_capacity = 1024;

    for entry in entries {
        total_capacity += 512 + ((entry.data.len() + 511) / 512) * 512;
    }

    let mut archive = Vec::with_capacity(total_capacity);

    for entry in entries {
        let file_name = {
            let mut file_name: [u8; 100] = [0; 100];
            file_name[..99][..entry.name.len()].copy_from_slice(entry.name.as_bytes());
            file_name
        };

        let file_mode = {
            let mut file_mode = [0; 8];
            let fmt = format!("{:o}", entry.mode);
            file_mode[..7][..fmt.len()].copy_from_slice(fmt.as_bytes());
            file_mode
        };

        let file_size = {
            let mut file_size = [0; 12];
            let fmt = format!("{:o}", entry.data.len());
            file_size[..11][..fmt.len()].copy_from_slice(fmt.as_bytes());
            file_size
        };

        let mut header = FileHeader {
            file_name,
            file_mode,
            owner_id: [0; 8],
            group_id: [0; 8],
            file_size,
            last_modification_time: *b"03316406010 ",
            checksum: [b' '; 8],
            file_type: b'0',
            linked_file_name: [0; 100],
            padding: [0; 255],
        };

        let header_slice = unsafe {
            any_as_u8_slice(&header)
        };

        let checksum_n = header_slice.iter()
            .fold(0, |acc, &x| acc + x as u32);

        let checksum = {
            let mut checksum = [0u8; 8];
            let fmt = format!("{:06o} ", checksum_n);
            checksum[..7][..fmt.len()].copy_from_slice(fmt.as_bytes());
            checksum
        };

        header.checksum = checksum;

        unsafe {
            archive.extend_from_slice(
                any_as_u8_slice(&header),
            );
        }

        let padded_size = ((entry.data.len() + 511) / 512) * 512;
        let padding = vec![0; padded_size - entry.data.len()];

        archive.extend_from_slice(entry.data.as_ref());
        archive.extend_from_slice(&padding);
    }

    let end = vec![0; 1024];
    archive.extend_from_slice(&end);

    archive
}

pub fn craft_tgz(entries: &[Entry]) -> Result<Vec<u8>, Error> {
    let tar = craft_tar(entries);

    let mut gz
        = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());

    gz.write_all(&tar)?;

    Ok(gz.finish()?)
}
