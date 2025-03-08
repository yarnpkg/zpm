use std::io::{Read, Write};

use crate::{error::Error, tar_iter::TarIterator};

use super::Entry;

unsafe fn any_as_u8_slice<T: Sized>(p: &T) -> &[u8] {
    ::core::slice::from_raw_parts(
        (p as *const T) as *const u8,
        ::core::mem::size_of::<T>(),
    )
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
    TarIterator::new(buffer).collect()
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

pub fn unpack_tgz(buffer: &[u8]) -> Result<Vec<u8>, Error> {
    let mut gz = flate2::read::GzDecoder::new(buffer);

    let mut buffer = Vec::new();
    gz.read_to_end(&mut buffer)?;

    Ok(buffer)
}

pub fn craft_tgz(entries: &[Entry]) -> Result<Vec<u8>, Error> {
    let tar = craft_tar(entries);

    let mut gz
        = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());

    gz.write_all(&tar)?;

    Ok(gz.finish()?)
}
