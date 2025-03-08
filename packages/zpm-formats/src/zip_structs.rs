#[allow(dead_code)]
#[repr(packed)]
pub struct FileHeader {
    pub version_needed_to_extract: u16,
    pub general_purpose_bit_flag: u16,
    pub compression_method: u16,
    pub last_mod_file_time: u16,
    pub last_mod_file_date: u16,
    pub crc_32: u32,
    pub compressed_size: u32,
    pub uncompressed_size: u32,
    pub file_name_length: u16,
    pub extra_field_length: u16,
}

#[allow(dead_code)]
#[repr(packed)]
pub struct GeneralRecord {
    pub signature: [u8; 4],
    pub header: FileHeader,
}

#[allow(dead_code)]
#[repr(packed)]
pub struct CentralDirectoryRecord {
    pub signature: [u8; 4],
    pub version_made_by: u16,
    pub header: FileHeader,
    pub file_comment_length: u16,
    pub disk_number_start: u16,
    pub internal_file_attributes: u16,
    pub external_file_attributes: u32,
    pub relative_offset_of_local_header: u32,
}

#[allow(dead_code)]
#[repr(packed)]
#[derive(Clone, Copy)]
pub struct EndOfCentralDirectoryRecord {
    pub signature: [u8; 4],
    pub disk_number: u16,
    pub disk_with_central_directory: u16,
    pub number_of_files_on_this_disk: u16,
    pub number_of_files: u16,
    pub size_of_central_directory: u32,
    pub offset_of_central_directory: u32,
    pub comment_length: u16,
}
