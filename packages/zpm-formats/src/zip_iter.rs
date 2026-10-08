use std::{borrow::Cow, io::Read};

use zerocopy::FromBytes;
use zpm_utils::Path;

use crate::{zip_structs::{CentralDirectoryRecord, EndOfCentralDirectoryRecord, GeneralRecord}, Compression, CompressionAlgorithm, Entry, Error};

fn unpack_deflate(data: &[u8], expected_size: Option<usize>) -> Result<Vec<u8>, Error> {
    let mut decoder
        = flate2::bufread::DeflateDecoder::new(data);

    let mut buffer
        = Vec::with_capacity(expected_size.unwrap_or(0));

    decoder.read_to_end(&mut buffer)?;

    Ok(buffer)
}

pub struct ZipIterator<'a> {
    buffer: &'a [u8],

    central_directory_record_offset: usize,
    end_of_central_directory_record_offset: usize,
}

impl<'a> ZipIterator<'a> {
    pub fn new(buffer: &'a [u8]) -> Result<ZipIterator<'a>, Error> {
        let end_of_central_directory_record_size
            = std::mem::size_of::<EndOfCentralDirectoryRecord>();

        if end_of_central_directory_record_size > buffer.len() {
            return Err(Error::InvalidZipFile("Too small to contain the end of central directory record".to_string()))
        }

        let end_of_central_directory_record_offset
            = buffer.len() - end_of_central_directory_record_size;

        let end_of_central_directory_record = EndOfCentralDirectoryRecord::read_from_bytes(&buffer[end_of_central_directory_record_offset..])
            .map_err(|_| Error::InvalidZipFile("Failed to parse end of central directory record".to_string()))?;

        let central_directory_record_offset
            = end_of_central_directory_record.offset_of_central_directory.get() as usize;

        // Anything else than a zip (a gzipped sdist, say) lands here with a
        // garbage offset
        if central_directory_record_offset > end_of_central_directory_record_offset {
            return Err(Error::InvalidZipFile("Central directory offset out of bounds".to_string()));
        }

        Ok(ZipIterator {
            buffer,

            central_directory_record_offset,
            end_of_central_directory_record_offset,
        })
    }

    fn parse_entry_at(&self, local_file_header_offset: usize, central_directory_record: &CentralDirectoryRecord, general_record: &GeneralRecord) -> Result<Entry<'a>, Error> {
        let name_offset
            = local_file_header_offset + std::mem::size_of::<GeneralRecord>();
        let data_offset
            = name_offset + general_record.header.file_name_length.get() as usize + general_record.header.extra_field_length.get() as usize;

        let name_bytes
            = self.buffer.get(name_offset..name_offset + general_record.header.file_name_length.get() as usize)
                .ok_or_else(|| Error::InvalidZipFile("File name out of bounds".to_string()))?;

        let name_str
            = std::str::from_utf8(name_bytes)?;
        let name
            = Path::try_from(name_str)?;

        let data_size
            = central_directory_record.header.compressed_size.get() as usize;
        let data
            = self.buffer.get(data_offset..data_offset + data_size)
                .ok_or_else(|| Error::InvalidZipFile("File data out of bounds".to_string()))?;

        let mut entry = Entry {
            name,
            mode: (central_directory_record.external_file_attributes.get() >> 16) as u32,
            crc: general_record.header.crc_32.get(),
            data: Cow::Borrowed(data),
            compression: None,
        };

        match central_directory_record.header.compression_method.get() {
            8 => {
                entry.compression = Some(Compression {
                    data: Cow::Borrowed(data),
                    algorithm: CompressionAlgorithm::Deflate(0),
                });

                let expected_size
                    = Some(central_directory_record.header.uncompressed_size.get() as usize);
                entry.data = Cow::Owned(unpack_deflate(data, expected_size)?);
            },

            _ => {

            },
        }

        Ok(entry)
    }
}

impl<'a> Iterator for ZipIterator<'a> {
    type Item = Result<Entry<'a>, Error>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.central_directory_record_offset >= self.end_of_central_directory_record_offset {
            return None;
        }

        let offset = self.central_directory_record_offset;

        let Some(record_bytes) = self.buffer.get(offset..) else {
            return Some(Err(Error::InvalidZipFile("Central directory record out of bounds".to_string())));
        };

        let central_directory_record = match CentralDirectoryRecord::ref_from_prefix(record_bytes) {
            Ok((record, _)) => record,
            Err(_) => return Some(Err(Error::InvalidZipFile("Failed to parse central directory record".to_string()))),
        };

        let local_file_header_offset
            = central_directory_record.relative_offset_of_local_header.get() as usize;

        let Some(header_bytes) = self.buffer.get(local_file_header_offset..) else {
            return Some(Err(Error::InvalidZipFile("Local file header out of bounds".to_string())));
        };

        let general_record = match GeneralRecord::ref_from_prefix(header_bytes) {
            Ok((record, _)) => record,
            Err(_) => return Some(Err(Error::InvalidZipFile("Failed to parse general record".to_string()))),
        };

        self.central_directory_record_offset += std::mem::size_of::<CentralDirectoryRecord>()
            + central_directory_record.header.file_name_length.get() as usize
            + central_directory_record.header.extra_field_length.get() as usize
            + central_directory_record.file_comment_length.get() as usize;

        Some(self.parse_entry_at(local_file_header_offset, central_directory_record, general_record))
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_non_zip_input_is_an_error_not_a_panic() {
        // A gzipped tarball (a PyPI sdist) handed to the zip reader
        let mut tarball = vec![0x1f, 0x8b, 0x08, 0x00];
        tarball.extend(std::iter::repeat(0xff).take(4096));

        assert!(crate::zip::entries_from_zip(&tarball).is_err());
    }
}
