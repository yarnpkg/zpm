use std::{borrow::Cow, io::Read};

use zerocopy::FromBytes;
use zpm_utils::Path;

use crate::{zip_structs::{CentralDirectoryRecord, EndOfCentralDirectoryRecord, GeneralRecord}, Compression, CompressionAlgorithm, Entry, Error};

fn unpack_deflate(data: &[u8]) -> Result<Vec<u8>, Error> {
    let mut decoder
        = flate2::bufread::DeflateDecoder::new(data);

    let mut buffer
        = Vec::new();

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

        let name_str
            = std::str::from_utf8(&self.buffer[name_offset..name_offset + general_record.header.file_name_length.get() as usize])?;
        let name
            = Path::try_from(name_str)?;

        let data_size
            = central_directory_record.header.compressed_size.get() as usize;
        let data
            = &self.buffer[data_offset..data_offset + data_size];

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

                entry.data = Cow::Owned(unpack_deflate(data)?);
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

        let central_directory_record = match CentralDirectoryRecord::ref_from_prefix(&self.buffer[offset..]) {
            Ok((record, _)) => record,
            Err(_) => return Some(Err(Error::InvalidZipFile("Failed to parse central directory record".to_string()))),
        };

        let local_file_header_offset
            = central_directory_record.relative_offset_of_local_header.get() as usize;

        let general_record = match GeneralRecord::ref_from_prefix(&self.buffer[local_file_header_offset..]) {
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
