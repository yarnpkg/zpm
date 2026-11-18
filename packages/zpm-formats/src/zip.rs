use std::borrow::Cow;
use std::io::{Read, Seek, SeekFrom};

use pnp::fs::VPathInfo;
use zerocopy::{FromBytes, IntoBytes};
use zerocopy::little_endian::{U16, U32};
use zpm_utils::Path;

use crate::{error::Error, zip_iter::{unpack_deflate, ZipIterator}, zip_structs::{CentralDirectoryRecord, EndOfCentralDirectoryRecord, FileHeader, GeneralRecord}, Compression, CompressionAlgorithm};

use super::Entry;

#[derive(Debug, Clone)]
pub struct CraftZipOptions {
    pub compression: Option<CompressionAlgorithm>,
}

pub fn entries_from_zip(buffer: &[u8]) -> Result<Vec<Entry<'_>>, Error> {
    ZipIterator::new(buffer)?.collect()
}

pub fn first_entry_from_zip(buffer: &[u8]) -> Result<Entry<'_>, Error> {
    ZipIterator::new(buffer)?.next()
        .unwrap_or_else(|| Err(Error::InvalidZipFile("Empty".to_string())))
}

/// The parts of an archive [`read_zip_index`] loads.
pub struct ZipIndex {
    /// The names of all the entries, in central directory order.
    pub names: Vec<Path>,

    /// The first entry of the archive (with its data), if any.
    pub first_entry: Option<Entry<'static>>,
}

/// Reads the entry names and the first entry of a zip archive without
/// loading the rest of it: only the end of central directory record, the
/// central directory, and the first entry are read from disk. Our cache
/// archives store the package manifest first, so this is enough to inspect
/// a package for a few kilobytes of IO, however large its files are.
pub fn read_zip_index(path: &Path) -> Result<ZipIndex, Error> {
    let mut file
        = std::fs::File::open(path.to_path_buf())?;

    let file_len
        = file.metadata()?.len() as usize;

    let end_of_central_directory_record_size
        = std::mem::size_of::<EndOfCentralDirectoryRecord>();

    if end_of_central_directory_record_size > file_len {
        return Err(Error::InvalidZipFile("Too small to contain the end of central directory record".to_string()));
    }

    let end_of_central_directory_record_offset
        = file_len - end_of_central_directory_record_size;

    let end_of_central_directory_record_bytes
        = read_file_range(&mut file, end_of_central_directory_record_offset, end_of_central_directory_record_size)?;

    let end_of_central_directory_record = EndOfCentralDirectoryRecord::read_from_bytes(&end_of_central_directory_record_bytes)
        .map_err(|_| Error::InvalidZipFile("Failed to parse end of central directory record".to_string()))?;

    let central_directory_offset
        = end_of_central_directory_record.offset_of_central_directory.get() as usize;

    if central_directory_offset > end_of_central_directory_record_offset {
        return Err(Error::InvalidZipFile("Central directory offset out of bounds".to_string()));
    }

    let central_directory
        = read_file_range(&mut file, central_directory_offset, end_of_central_directory_record_offset - central_directory_offset)?;

    let mut names
        = Vec::new();
    let mut first_entry
        = None;

    let mut offset
        = 0;

    while offset < central_directory.len() {
        let (central_directory_record, _) = CentralDirectoryRecord::ref_from_prefix(&central_directory[offset..])
            .map_err(|_| Error::InvalidZipFile("Failed to parse central directory record".to_string()))?;

        let name_offset
            = offset + std::mem::size_of::<CentralDirectoryRecord>();
        let name_length
            = central_directory_record.header.file_name_length.get() as usize;

        let name_bytes
            = central_directory.get(name_offset..name_offset + name_length)
                .ok_or_else(|| Error::InvalidZipFile("File name out of bounds".to_string()))?;

        let name
            = parse_entry_name(name_bytes)?;

        if first_entry.is_none() {
            first_entry = Some(read_file_entry(&mut file, file_len, central_directory_record, name.clone())?);
        }

        names.push(name);

        offset = name_offset
            + name_length
            + central_directory_record.header.extra_field_length.get() as usize
            + central_directory_record.file_comment_length.get() as usize;
    }

    Ok(ZipIndex {
        names,
        first_entry,
    })
}

fn read_file_range(file: &mut std::fs::File, offset: usize, size: usize) -> Result<Vec<u8>, Error> {
    let mut buffer
        = vec![0; size];

    file.seek(SeekFrom::Start(offset as u64))?;
    file.read_exact(&mut buffer)?;

    Ok(buffer)
}

pub(crate) fn parse_entry_name(name_bytes: &[u8]) -> Result<Path, Error> {
    let name_str
        = std::str::from_utf8(name_bytes)?;
    let name
        = Path::try_from(name_str)?;

    // Absolute paths and `..` segments would let the archive write
    // outside of the folder it's extracted into
    if !name.is_forward() {
        return Err(Error::InvalidZipFile(format!("Entry path escapes the archive: {}", name_str)));
    }

    Ok(name)
}

fn read_file_entry(file: &mut std::fs::File, file_len: usize, central_directory_record: &CentralDirectoryRecord, name: Path) -> Result<Entry<'static>, Error> {
    let local_file_header_offset
        = central_directory_record.relative_offset_of_local_header.get() as usize;

    if local_file_header_offset + std::mem::size_of::<GeneralRecord>() > file_len {
        return Err(Error::InvalidZipFile("Local file header out of bounds".to_string()));
    }

    let general_record_bytes
        = read_file_range(file, local_file_header_offset, std::mem::size_of::<GeneralRecord>())?;

    let general_record = GeneralRecord::read_from_bytes(&general_record_bytes)
        .map_err(|_| Error::InvalidZipFile("Failed to parse general record".to_string()))?;

    let data_offset
        = local_file_header_offset
            + std::mem::size_of::<GeneralRecord>()
            + general_record.header.file_name_length.get() as usize
            + general_record.header.extra_field_length.get() as usize;

    let data_size
        = central_directory_record.header.compressed_size.get() as usize;

    if data_offset + data_size > file_len {
        return Err(Error::InvalidZipFile("File data out of bounds".to_string()));
    }

    let data
        = read_file_range(file, data_offset, data_size)?;

    let mut entry = Entry {
        name,
        mode: (central_directory_record.external_file_attributes.get() >> 16) as u32,
        crc: general_record.header.crc_32.get(),
        data: Cow::Owned(data),
        compression: None,
    };

    if central_directory_record.header.compression_method.get() == 8 {
        let expected_size
            = Some(central_directory_record.header.uncompressed_size.get() as usize);

        let compressed_data
            = std::mem::take(entry.data.to_mut());

        entry.data = Cow::Owned(unpack_deflate(&compressed_data, expected_size)?);
        entry.compression = Some(Compression {
            data: Cow::Owned(compressed_data),
            algorithm: CompressionAlgorithm::Deflate(0),
        });
    }

    Ok(entry)
}

pub trait ToZip {
    fn to_zip(&self) -> Vec<u8>;
}

impl<'a> ToZip for Vec<Entry<'a>> {
    fn to_zip(&self) -> Vec<u8> {
        let mut local_headers_capacity = 0;
        let mut central_directory_capacity = std::mem::size_of::<EndOfCentralDirectoryRecord>();

        for entry in self {
            let compressed_data = entry.compression
                .as_ref()
                .map_or(&entry.data, |compressed_data| &compressed_data.data);

            let name_bytes
                = entry.name.as_str().as_bytes();

            local_headers_capacity
                += std::mem::size_of::<GeneralRecord>() + name_bytes.len() + compressed_data.len();

            central_directory_capacity
                += std::mem::size_of::<CentralDirectoryRecord>() + name_bytes.len();
        }

        let total_capacity
            = local_headers_capacity + central_directory_capacity;

        let mut buffer
            = Vec::with_capacity(total_capacity);

        // First pass: write local file headers + data, computing CRC32 inline
        let mut entry_metadata: Vec<(usize, u32)>
            = Vec::with_capacity(self.len());

        for entry in self {
            let compressed_data = entry.compression
                .as_ref()
                .map_or(&entry.data, |compressed_data| &compressed_data.data);

            let compression = entry.compression
                .as_ref()
                .map(|compressed_data| compressed_data.algorithm);

            let crc
                = crc32fast::hash(&entry.data);

            let offset = buffer.len();
            entry_metadata.push((offset, crc));

            inject_general_record(&mut buffer, entry, compressed_data, compression, crc);
        }

        assert_eq!(buffer.len(), local_headers_capacity);

        let central_directory_offset
            = buffer.len();

        // Second pass: write central directory records
        for (i, entry) in self.iter().enumerate() {
            let compressed_data = entry.compression
                .as_ref()
                .map_or(&entry.data, |compressed_data| &compressed_data.data);

            let compression = entry.compression
                .as_ref()
                .map(|compressed_data| compressed_data.algorithm);

            let (offset, crc) = entry_metadata[i];

            inject_central_directory_record(&mut buffer, entry, compressed_data, offset, compression, crc);
        }

        let central_directory_size
            = buffer.len() - central_directory_offset;

        buffer.extend_from_slice(
            EndOfCentralDirectoryRecord {
                signature: [0x50, 0x4b, 0x05, 0x06],
                disk_number: U16::new(0x00),
                disk_with_central_directory: U16::new(0x00),
                number_of_files_on_this_disk: U16::new(self.len() as u16),
                number_of_files: U16::new(self.len() as u16),
                size_of_central_directory: U32::new(central_directory_size as u32),
                offset_of_central_directory: U32::new(central_directory_offset as u32),
                comment_length: U16::new(0x00),
            }.as_bytes(),
        );

        assert_eq!(buffer.len(), total_capacity);

        buffer
    }
}

fn inject_general_record(target: &mut Vec<u8>, entry: &Entry, compressed_data: &[u8], compression: Option<CompressionAlgorithm>, crc: u32) {
    let compression_method: u16 = match compression {
        Some(CompressionAlgorithm::Deflate(_)) => 0x08, // Deflate compression
        None => 0x00, // No compression
    };

    let name_bytes
        = entry.name.as_str().as_bytes();

    target.extend_from_slice(
        GeneralRecord {
            signature: [0x50, 0x4b, 0x03, 0x04],
            header: FileHeader {
                version_needed_to_extract: U16::new(if compression_method == 0x08 { 0x14 } else { 0x0A }),
                general_purpose_bit_flag: U16::new(0x00),
                compression_method: U16::new(compression_method),
                last_mod_file_time: U16::new(0xae40),
                last_mod_file_date: U16::new(0x08d6),
                crc_32: U32::new(crc),
                compressed_size: U32::new(compressed_data.len() as u32),
                uncompressed_size: U32::new(entry.data.len() as u32),
                file_name_length: U16::new(name_bytes.len() as u16),
                extra_field_length: U16::new(0x00),
            },
        }.as_bytes(),
    );

    // File name
    target.extend_from_slice(name_bytes);

    // File data (compressed or uncompressed)
    target.extend_from_slice(compressed_data);
}

fn inject_central_directory_record(target: &mut Vec<u8>, entry: &Entry, compressed_data: &[u8], offset: usize, compression: Option<CompressionAlgorithm>, crc: u32) {
    let compression_method: u16 = match compression {
        Some(CompressionAlgorithm::Deflate(_)) => 0x08, // Deflate compression
        None => 0x00, // No compression
    };

    let name_bytes
        = entry.name.as_str().as_bytes();

    target.extend_from_slice(
        CentralDirectoryRecord {
            signature: [0x50, 0x4b, 0x01, 0x02],
            version_made_by: U16::new(0x0314), // UNIX
            header: FileHeader {
                version_needed_to_extract: U16::new(if compression_method == 0x08 { 0x14 } else { 0x14 }),
                general_purpose_bit_flag: U16::new(0x00),
                compression_method: U16::new(compression_method),
                last_mod_file_time: U16::new(0xae40),
                last_mod_file_date: U16::new(0x08d6),
                crc_32: U32::new(crc),
                compressed_size: U32::new(compressed_data.len() as u32),
                uncompressed_size: U32::new(entry.data.len() as u32),
                file_name_length: U16::new(name_bytes.len() as u16),
                extra_field_length: U16::new(0x00),
            },
            file_comment_length: U16::new(0x00),
            disk_number_start: U16::new(0x00),
            internal_file_attributes: U16::new(0x00),
            external_file_attributes: U32::new((entry.mode as u32) << 16),
            relative_offset_of_local_header: U32::new(offset as u32),
        }.as_bytes(),
    );

    // File name
    target.extend_from_slice(name_bytes);
}

pub trait ZipSupport {
    fn fs_read_text_from_zip_buffer(&self, buf: &[u8]) -> Result<String, Error>;
    fn fs_read_text_with_zip(&self) -> Result<String, Error>;
}

impl ZipSupport for Path {
    fn fs_read_text_from_zip_buffer(&self, zip_data: &[u8]) -> Result<String, Error> {
        let entries
            = entries_from_zip(zip_data)?;

        let entry = entries.iter()
            .find(|entry| &entry.name == self)
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

#[cfg(test)]
mod tests {
    use std::borrow::Cow;

    use zpm_utils::Path;

    use crate::{zip::{entries_from_zip, read_zip_index, ToZip}, CompressionAlgorithm, Entry};

    fn sample_entries() -> Vec<Entry<'static>> {
        vec![
            Entry::new_file(Path::try_from("node_modules/pkg/package.json").unwrap(), Cow::Borrowed(br#"{"name": "pkg"}"#)),
            Entry::new_file(Path::try_from("node_modules/pkg/index.js").unwrap(), Cow::Borrowed(b"module.exports = 42;\n")),
            Entry::new_file(Path::try_from("node_modules/pkg/binding.gyp").unwrap(), Cow::Borrowed(b"{}")),
        ]
    }

    fn read_index_of(zip_data: &[u8]) -> Result<crate::zip::ZipIndex, crate::Error> {
        let temp_dir
            = Path::temp_dir().unwrap();

        let archive_path = temp_dir
            .with_join_str("archive.zip");

        archive_path.fs_write(zip_data).unwrap();

        let index
            = read_zip_index(&archive_path);

        temp_dir.fs_rm().unwrap();

        index
    }

    fn assert_index_matches_full_parse(entries: Vec<Entry<'static>>) {
        let zip_data
            = entries.to_zip();

        let index
            = read_index_of(&zip_data).unwrap();
        let full_entries
            = entries_from_zip(&zip_data).unwrap();

        assert_eq!(index.names, full_entries.iter().map(|entry| entry.name.clone()).collect::<Vec<_>>());

        let first_entry
            = index.first_entry.unwrap();

        assert_eq!(first_entry.name, full_entries[0].name);
        assert_eq!(first_entry.mode, full_entries[0].mode);
        assert_eq!(first_entry.data, full_entries[0].data);
    }

    #[test]
    fn test_read_zip_index_matches_full_parse() {
        assert_index_matches_full_parse(sample_entries());
    }

    #[test]
    fn test_read_zip_index_matches_full_parse_when_compressed() {
        let mut entries
            = sample_entries();

        for entry in &mut entries {
            entry.compress_in_place(CompressionAlgorithm::Deflate(6));
        }

        assert_index_matches_full_parse(entries);
    }

    #[test]
    fn test_read_zip_index_of_empty_archive_has_no_first_entry() {
        let index
            = read_index_of(&Vec::<Entry>::new().to_zip()).unwrap();

        assert!(index.names.is_empty());
        assert!(index.first_entry.is_none());
    }

    #[test]
    fn test_read_zip_index_rejects_non_zip_input() {
        let mut tarball = vec![0x1f, 0x8b, 0x08, 0x00];
        tarball.extend(std::iter::repeat(0xff).take(4096));

        assert!(read_index_of(&tarball).is_err());
    }

    #[test]
    fn test_read_zip_index_rejects_escaping_entry() {
        let entries
            = vec![Entry::new_file(Path::try_from("../outside").unwrap(), Cow::Borrowed(b"data"))];

        assert!(read_index_of(&entries.to_zip()).is_err());
    }
}
