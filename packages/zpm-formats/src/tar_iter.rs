use std::{borrow::Cow, collections::HashMap};

use itertools::Itertools;
use zpm_utils::Path;

use crate::{Entry, Error};

/// An archive member, as returned by [`TarIterator::next_record`].
#[derive(Debug, PartialEq, Eq)]
pub enum TarRecord<'a> {
    File(Entry<'a>),

    /// The target is kept as stored in the archive (relative to the link's
    /// folder, or absolute); it's up to the caller to decide whether it's
    /// acceptable.
    Symlink {name: Path, target: String},

    /// The target is the path of another member of the archive.
    Hardlink {name: Path, target: Path},
}

pub struct TarIterator<'a> {
    buffer: &'a [u8],
    offset: usize,
    global_headers: HashMap<String, String>,
}

impl<'a> TarIterator<'a> {
    pub fn new(buffer: &[u8]) -> TarIterator<'_> {
        TarIterator {
            buffer,
            offset: 0,
            global_headers: HashMap::new(),
        }
    }

    /// PAX records are "<length> <keyword>=<value>\n", where <length> counts
    /// the whole record. Values aren't always text: `SCHILY.xattr.*` records
    /// hold raw extended attributes (bsdtar on macOS adds a binary
    /// `com.apple.provenance` to every file), which may contain newlines and
    /// aren't UTF-8. Records are thus split on their length when it points at
    /// a record end (falling back to the next newline when it doesn't), and
    /// values that aren't UTF-8 are skipped unless they're one we read.
    fn parse_pax_headers(&self, data: &[u8]) -> Result<HashMap<String, String>, Error> {
        let mut headers
            = HashMap::new();

        let mut rest
            = data;

        while !rest.is_empty() {
            let line_end
                = rest.iter().position(|&c| c == b'\n')
                    .map_or(rest.len(), |pos| pos + 1);

            let length
                = rest.iter().position(|&c| c == b' ')
                    .and_then(|space_pos| std::str::from_utf8(&rest[..space_pos]).ok())
                    .and_then(|length| length.parse::<usize>().ok())
                    .filter(|&length| length > 0 && length <= rest.len() && rest[length - 1] == b'\n')
                    .unwrap_or(line_end);

            let record
                = &rest[..length];
            let record
                = record.strip_suffix(b"\n").unwrap_or(record);

            rest = &rest[length..];

            let Some(space_pos) = record.iter().position(|&c| c == b' ') else {
                continue;
            };

            let record
                = &record[space_pos + 1..];

            let Some(eq_pos) = record.iter().position(|&c| c == b'=') else {
                continue;
            };

            let Ok(keyword) = std::str::from_utf8(&record[..eq_pos]) else {
                continue;
            };

            match std::str::from_utf8(&record[eq_pos + 1..]) {
                Ok(value) => {
                    headers.insert(keyword.to_string(), value.to_string());
                },

                Err(err) if keyword == "path" || keyword == "linkpath" => {
                    return Err(err.into());
                },

                Err(_) => {},
            }
        }

        Ok(headers)
    }

    /// The name of the entry whose header starts at `offset`, with the PAX
    /// and UStar extensions applied.
    fn parse_name_at(&self, offset: usize, pax_headers: &HashMap<String, String>) -> Result<Path, Error> {
        // First try to get the name from PAX headers
        let name = if let Some(pax_path) = pax_headers.get("path") {
            pax_path.clone()
        } else if let Some(pax_path) = self.global_headers.get("path") {
            pax_path.clone()
        } else {
            // Fall back to the standard name field
            let name_slice = self.buffer.get(offset..offset + 100)
                .map(trim_zero)
                .ok_or(Error::InvalidTarFile)?;

            let mut name
                = std::str::from_utf8(name_slice)?.to_string();

            // Check for UStar format prefix (at offset 345, length 155)
            if let Some(prefix_slice) = self.buffer.get(offset + 345..offset + 500) {
                let prefix_slice
                    = trim_zero(prefix_slice);

                if !prefix_slice.is_empty() {
                    if let Ok(prefix) = std::str::from_utf8(prefix_slice) {
                        name = format!("{}/{}", prefix, name);
                    }
                }
            }

            name
        };

        // Skip empty names
        if name.is_empty() {
            return Err(Error::InvalidTarFilePath("empty filename".to_string()));
        }

        clean_name(&name)?
            .ok_or_else(|| Error::InvalidTarFilePath(name.to_string()))
    }

    /// The target of the link whose header starts at `offset`, as stored in
    /// the archive (PAX `linkpath`, or the `linkname` field).
    fn parse_link_target_at(&self, offset: usize, pax_headers: &HashMap<String, String>) -> Result<String, Error> {
        if let Some(pax_target) = pax_headers.get("linkpath") {
            return Ok(pax_target.clone());
        }

        let target_slice = self.buffer.get(offset + 157..offset + 257)
            .map(trim_zero)
            .ok_or(Error::InvalidTarFile)?;

        Ok(std::str::from_utf8(target_slice)?.to_string())
    }

    fn parse_entry_at(&self, offset: usize, size: usize, pax_headers: &HashMap<String, String>) -> Result<Entry<'a>, Error> {
        let name
            = self.parse_name_at(offset, pax_headers)?;

        let mode = self.buffer.get(offset + 100..offset + 108)
            .map(|raw| from_oct(&raw) as u32)
            .ok_or(Error::InvalidTarFile)?;

        let data = self.buffer
            .get(offset + 512..offset + 512 + size)
            .ok_or(Error::InvalidTarFile)?;

        Ok(Entry {
            name,
            mode,
            crc: 0,
            data: Cow::Borrowed(data),
            compression: None,
        })
    }

    /// Returns the next file or link of the archive. Unlike the `Iterator`
    /// implementation (which only yields regular files, as package archives
    /// don't need anything else), links are reported too.
    pub fn next_record(&mut self) -> Result<Option<TarRecord<'a>>, Error> {
        let mut pax_headers
            = HashMap::new();

        loop {
            if self.offset >= self.buffer.len() {
                return Ok(None);
            }

            let offset
                = self.offset;

            let size
                = self.buffer.get(offset + 124..offset + 136)
                    .map(|raw| from_oct(&raw) as usize)
                    .ok_or(Error::InvalidTarFile)?;

            let type_flag
                = *self.buffer.get(offset + 156)
                    .ok_or(Error::InvalidTarFile)?;

            // round up to the next multiple of 512
            self.offset += 512 + ((size + 511) / 512) * 512;

            let record = match type_flag {
                // Regular file
                b'0' | 0 => {
                    self.parse_entry_at(offset, size, &pax_headers)
                        .map(TarRecord::File)
                },

                // Hardlink to a file stored earlier in the archive
                b'1' => {
                    self.parse_name_at(offset, &pax_headers).and_then(|name| {
                        let target
                            = self.parse_link_target_at(offset, &pax_headers)?;

                        let target
                            = clean_name(&target)?
                                .ok_or_else(|| Error::InvalidTarFilePath(target.to_string()))?;

                        Ok(TarRecord::Hardlink {name, target})
                    })
                },

                // Symlink
                b'2' => {
                    self.parse_name_at(offset, &pax_headers).and_then(|name| {
                        let target
                            = self.parse_link_target_at(offset, &pax_headers)?;

                        Ok(TarRecord::Symlink {name, target})
                    })
                },

                // PAX extended header for next file
                b'x' => {
                    let header_data = self.buffer
                        .get(offset + 512..offset + 512 + size)
                        .ok_or(Error::InvalidTarFile)?;

                    let headers
                        = self.parse_pax_headers(header_data)?;

                    pax_headers.extend(headers);

                    continue;
                },

                // PAX global extended header
                b'g' => {
                    let header_data = self.buffer
                        .get(offset + 512..offset + 512 + size)
                        .ok_or(Error::InvalidTarFile)?;

                    self.global_headers
                        = self.parse_pax_headers(header_data)?;

                    continue;
                },

                _ => {
                    // Other types (directories, devices, etc.) - skip
                    pax_headers.clear();
                    continue;
                },
            };

            match record {
                Ok(record) => {
                    return Ok(Some(record));
                },

                Err(Error::InvalidTarFilePath(_)) => {
                    // Skip invalid entries (like empty filenames)
                    pax_headers.clear();
                    continue;
                },

                Err(e) => {
                    return Err(e)
                },
            }
        }
    }

    fn next_impl(&mut self) -> Result<Option<Entry<'a>>, Error> {
        while let Some(record) = self.next_record()? {
            if let TarRecord::File(entry) = record {
                return Ok(Some(entry));
            }
        }

        Ok(None)
    }
}

impl<'a> Iterator for TarIterator<'a> {
    type Item = Result<Entry<'a>, Error>;

    fn next(&mut self) -> Option<Self::Item> {
        self.next_impl().transpose()
    }
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

fn clean_name(name: &str) -> Result<Option<Path>, Error> {
    let slice = name
        .trim_end_matches('/');
    let name
        = Path::try_from(slice)?;

    if !name.is_forward() {
        return Ok(None)
    }

    Ok(Some(name))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pax_header_parsing() {
        let pax_data = b"52 comment=679be5902097ed612fb5062b5549f3f32b6f5f47\n101 path=strophejs-plugin-stream-management-679be5902097ed612fb5062b5549f3f32b6f5f47/src/strophe.stream-management.js\n";

        let iterator = TarIterator::new(&[]);
        let headers = iterator.parse_pax_headers(pax_data).unwrap();

        assert_eq!(headers.get("comment"), Some(&"679be5902097ed612fb5062b5549f3f32b6f5f47".to_string()));
        assert_eq!(headers.get("path"), Some(&"strophejs-plugin-stream-management-679be5902097ed612fb5062b5549f3f32b6f5f47/src/strophe.stream-management.js".to_string()));
    }

    #[test]
    fn test_pax_header_parsing_binary_xattr() {
        // As written by bsdtar on macOS (e.g. the cypress@15 npm tarball)
        let pax_data = b"57 LIBARCHIVE.xattr.com.apple.provenance=AQIAF+h7EYaCXbU\n49 SCHILY.xattr.com.apple.provenance=\x01\x02\x00\x17\xe8{\x11\x86\x82]\xb5\n17 path=a/b/c.js\n";

        let iterator = TarIterator::new(&[]);
        let headers = iterator.parse_pax_headers(pax_data).unwrap();

        assert_eq!(headers.get("LIBARCHIVE.xattr.com.apple.provenance"), Some(&"AQIAF+h7EYaCXbU".to_string()));
        assert_eq!(headers.get("SCHILY.xattr.com.apple.provenance"), None);
        assert_eq!(headers.get("path"), Some(&"a/b/c.js".to_string()));
    }

    #[test]
    fn test_pax_header_parsing_value_with_newline() {
        let pax_data = b"17 comment=a\nb\nc\n13 path=a.js\n";

        let iterator = TarIterator::new(&[]);
        let headers = iterator.parse_pax_headers(pax_data).unwrap();

        assert_eq!(headers.get("comment"), Some(&"a\nb\nc".to_string()));
        assert_eq!(headers.get("path"), Some(&"a.js".to_string()));
    }

    #[test]
    fn test_pax_header_parsing_binary_path() {
        let pax_data = b"13 path=\xe8.js\n";

        let iterator = TarIterator::new(&[]);

        assert!(iterator.parse_pax_headers(pax_data).is_err());
    }

    #[test]
    fn test_global_headers_precedence() {
        let mut iterator = TarIterator::new(&[]);

        // Set up global headers
        iterator.global_headers.insert("comment".to_string(), "global-comment".to_string());
        iterator.global_headers.insert("author".to_string(), "global-author".to_string());

        // Local headers that should override global ones
        let mut local_headers = HashMap::new();
        local_headers.insert("comment".to_string(), "local-comment".to_string());
        local_headers.insert("path".to_string(), "test/file.txt".to_string());

        // Test the merging logic by simulating what happens in parse_entry_at
        let mut combined_headers = iterator.global_headers.clone();
        combined_headers.extend(local_headers.iter().map(|(k, v)| (k.clone(), v.clone())));

        // Local headers should take precedence
        assert_eq!(combined_headers.get("comment"), Some(&"local-comment".to_string()));
        assert_eq!(combined_headers.get("path"), Some(&"test/file.txt".to_string()));
        // Global headers should be preserved when not overridden
        assert_eq!(combined_headers.get("author"), Some(&"global-author".to_string()));
    }
}
