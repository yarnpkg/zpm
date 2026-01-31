use std::{borrow::Cow, io::{Read, Write}};

use zerocopy::{Immutable, IntoBytes, KnownLayout, Unaligned};

use crate::{error::Error, tar_iter::TarIterator};

use super::Entry;

#[derive(IntoBytes, Immutable, KnownLayout, Unaligned)]
#[repr(C)]
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

pub fn entries_from_tar(buffer: &[u8]) -> Result<Vec<Entry<'_>>, Error> {
    TarIterator::new(buffer).collect()
}

pub trait ToTar {
    fn to_tar(&self) -> Vec<u8>;
    fn to_tgz(&self) -> Result<Vec<u8>, Error>;
}

impl<'a> ToTar for Vec<Entry<'a>> {
    fn to_tar(&self) -> Vec<u8> {
        let mut total_capacity
            = 1024;

        for entry in self {
            total_capacity += 512 + ((entry.data.len() + 511) / 512) * 512;
        }

        let mut archive
            = Vec::with_capacity(total_capacity);

        for entry in self {
            let name_bytes
                = entry.name.as_str().as_bytes();

            let file_name = {
                let mut file_name: [u8; 100] = [0; 100];
                file_name[..99][..name_bytes.len()].copy_from_slice(name_bytes);
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

            let checksum_n = header.as_bytes().iter()
                .fold(0, |acc, &x| acc + x as u32);

            let checksum = {
                let mut checksum = [0u8; 8];
                let fmt = format!("{:06o} ", checksum_n);
                checksum[..7][..fmt.len()].copy_from_slice(fmt.as_bytes());
                checksum
            };

            header.checksum = checksum;

            archive.extend_from_slice(header.as_bytes());

            let padded_size
                = ((entry.data.len() + 511) / 512) * 512;
            let padding
                = vec![0; padded_size - entry.data.len()];

            archive.extend_from_slice(entry.data.as_ref());
            archive.extend_from_slice(&padding);
        }

        let end = vec![0; 1024];
        archive.extend_from_slice(&end);

        archive
    }

    fn to_tgz(&self) -> Result<Vec<u8>, Error> {
        let tar = self.to_tar();

        let mut gz
            = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());

        gz.write_all(&tar)?;

        Ok(gz.finish()?)
    }
}

pub fn unpack_tgz(buffer: &[u8]) -> Result<Cow<'_, [u8]>, Error> {
    if buffer.starts_with(&[0x1f, 0x8b]) {
        let mut gz = flate2::read::GzDecoder::new(buffer);

        let mut buffer = Vec::new();
        gz.read_to_end(&mut buffer)?;

        Ok(Cow::Owned(buffer))
    } else {
        Ok(Cow::Borrowed(buffer))
    }
}
