use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout, Unaligned};
use zerocopy::little_endian::{U16, U32};

#[derive(Debug, Clone, Copy, FromBytes, IntoBytes, Immutable, KnownLayout, Unaligned)]
#[repr(C)]
pub struct FileHeader {
    pub version_needed_to_extract: U16,
    pub general_purpose_bit_flag: U16,
    pub compression_method: U16,
    pub last_mod_file_time: U16,
    pub last_mod_file_date: U16,
    pub crc_32: U32,
    pub compressed_size: U32,
    pub uncompressed_size: U32,
    pub file_name_length: U16,
    pub extra_field_length: U16,
}

#[derive(Debug, Clone, Copy, FromBytes, IntoBytes, Immutable, KnownLayout, Unaligned)]
#[repr(C)]
pub struct GeneralRecord {
    pub signature: [u8; 4],
    pub header: FileHeader,
}

#[derive(Debug, Clone, Copy, FromBytes, IntoBytes, Immutable, KnownLayout, Unaligned)]
#[repr(C)]
pub struct CentralDirectoryRecord {
    pub signature: [u8; 4],
    pub version_made_by: U16,
    pub header: FileHeader,
    pub file_comment_length: U16,
    pub disk_number_start: U16,
    pub internal_file_attributes: U16,
    pub external_file_attributes: U32,
    pub relative_offset_of_local_header: U32,
}

#[derive(Debug, Clone, Copy, FromBytes, IntoBytes, Immutable, KnownLayout, Unaligned)]
#[repr(C)]
pub struct EndOfCentralDirectoryRecord {
    pub signature: [u8; 4],
    pub disk_number: U16,
    pub disk_with_central_directory: U16,
    pub number_of_files_on_this_disk: U16,
    pub number_of_files: U16,
    pub size_of_central_directory: U32,
    pub offset_of_central_directory: U32,
    pub comment_length: U16,
}
