use std::{borrow::Cow, io::Read};

use zerocopy::{Immutable, IntoBytes, KnownLayout, Unaligned};

use crate::{error::Error, gzip_compress, tar_iter::TarIterator};

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

// The V7 name field holds 99 bytes plus a NUL terminator
const MAX_HEADER_NAME_LEN: usize = 99;

fn octal_field<const N: usize>(value: u64) -> [u8; N] {
    let mut field = [0; N];
    let fmt = format!("{:o}", value);
    field[..N - 1][..fmt.len()].copy_from_slice(fmt.as_bytes());
    field
}

fn truncate_name(name: &[u8]) -> [u8; 100] {
    let mut file_name: [u8; 100] = [0; 100];
    let len = name.len().min(MAX_HEADER_NAME_LEN);
    file_name[..len].copy_from_slice(&name[..len]);
    file_name
}

// A PAX record is "<len> <key>=<value>\n", where <len> counts the whole
// record including its own digits.
fn pax_record(key: &str, value: &str) -> Vec<u8> {
    let payload_len = key.len() + value.len() + 3;
    let mut len = payload_len + payload_len.to_string().len();

    if len.to_string().len() + payload_len != len {
        len = payload_len + len.to_string().len();
    }

    format!("{} {}={}\n", len, key, value).into_bytes()
}

fn write_record(archive: &mut Vec<u8>, file_name: [u8; 100], file_mode: u32, file_type: u8, data: &[u8]) {
    let mut header = FileHeader {
        file_name,
        file_mode: octal_field(file_mode as u64),
        owner_id: [0; 8],
        group_id: [0; 8],
        file_size: octal_field(data.len() as u64),
        last_modification_time: *b"03316406010 ",
        checksum: [b' '; 8],
        file_type,
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
        = ((data.len() + 511) / 512) * 512;

    archive.extend_from_slice(data);
    archive.resize(archive.len() + padded_size - data.len(), 0);
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
            let name
                = entry.name.as_str();

            // Names that don't fit the header are stored in a PAX extended
            // header preceding the entry, like npm's tar does
            if name.len() > MAX_HEADER_NAME_LEN {
                let pax_name
                    = format!("PaxHeader/{}", name);

                write_record(&mut archive, truncate_name(pax_name.as_bytes()), 0o644, b'x', &pax_record("path", name));
            }

            write_record(&mut archive, truncate_name(name.as_bytes()), entry.mode, b'0', entry.data.as_ref());
        }

        let end = vec![0; 1024];
        archive.extend_from_slice(&end);

        archive
    }

    fn to_tgz(&self) -> Result<Vec<u8>, Error> {
        let tar
            = self.to_tar();

        Ok(gzip_compress(&tar, 6))
    }
}

pub fn unpack_tgz(buffer: &[u8]) -> Result<Cow<'_, [u8]>, Error> {
    if buffer.starts_with(&[0x1f, 0x8b]) {
        let mut gz
            = flate2::read::GzDecoder::new(buffer);

        let mut out
            = Vec::with_capacity(gzip_isize_hint(buffer).unwrap_or(0));

        gz.read_to_end(&mut out)?;

        Ok(Cow::Owned(out))
    } else {
        Ok(Cow::Borrowed(buffer))
    }
}

fn gzip_isize_hint(buffer: &[u8]) -> Option<usize> {
    // ISIZE is the uncompressed size modulo 2^32, stored in the last 4 bytes (little-endian).
    const MAX_GZIP_ISIZE: usize
        = 64 * 1024 * 1024;

    if buffer.len() < 4 {
        return None;
    }

    let tail
        = buffer.get(buffer.len().saturating_sub(4)..)?;

    let raw: [u8; 4]
        = tail.try_into().ok()?;

    let isize
        = u32::from_le_bytes(raw) as usize;

    if isize > MAX_GZIP_ISIZE {
        return None;
    }

    Some(isize)
}

#[cfg(test)]
mod tests {
    use zpm_utils::Path;

    use super::*;

    fn entry(name: &str, data: &'static [u8]) -> Entry<'static> {
        Entry {
            name: Path::try_from(name).unwrap(),
            mode: 0o644,
            crc: 0,
            data: Cow::Borrowed(data),
            compression: None,
        }
    }

    #[test]
    fn test_pax_record_length_counts_itself() {
        for value_len in [1, 80, 90, 91, 92, 200, 995, 996, 997, 2000] {
            let value = "a".repeat(value_len);
            let record = pax_record("path", &value);
            let (len, _) = std::str::from_utf8(&record).unwrap().split_once(' ').unwrap();

            assert_eq!(len.parse::<usize>().unwrap(), record.len(), "value length {}", value_len);
        }
    }

    #[test]
    fn test_long_names_round_trip() {
        let long_name = format!("package/{}/index.js", "nested-directory".repeat(10));
        let entries = vec![
            entry("package/package.json", b"{}"),
            entry(&long_name, b"module.exports = 42;"),
        ];

        let tar = entries.to_tar();
        let read_back = entries_from_tar(&tar).unwrap();

        assert_eq!(read_back.len(), 2);
        assert_eq!(read_back[0].name.as_str(), "package/package.json");
        assert_eq!(read_back[1].name.as_str(), long_name);
        assert_eq!(read_back[1].data.as_ref(), b"module.exports = 42;");
    }
}
