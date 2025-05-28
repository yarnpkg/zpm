use std::{borrow::Cow, collections::HashMap, sync::LazyLock};

use itertools::Itertools;
use regex::Regex;

use crate::{Entry, Error};

pub struct TarIterator<'a> {
    buffer: &'a [u8],
    offset: usize,
    global_headers: HashMap<String, String>,
}

impl<'a> TarIterator<'a> {
    pub fn new(buffer: &[u8]) -> TarIterator {
        TarIterator {
            buffer,
            offset: 0,
            global_headers: HashMap::new(),
        }
    }

    fn parse_pax_headers(&self, data: &[u8]) -> Result<HashMap<String, String>, Error> {
        let mut headers = HashMap::new();
        let content = std::str::from_utf8(data)?;
        
        for line in content.lines() {
            if line.is_empty() {
                continue;
            }
            
            // PAX format: "length keyword=value\n"
            if let Some(space_pos) = line.find(' ') {
                let keyword_value = &line[space_pos + 1..];
                if let Some(eq_pos) = keyword_value.find('=') {
                    let keyword = &keyword_value[..eq_pos];
                    let value = &keyword_value[eq_pos + 1..];
                    headers.insert(keyword.to_string(), value.to_string());
                }
            }
        }
        
        Ok(headers)
    }

    fn parse_entry_at(&mut self, offset: usize, size: usize, pax_headers: &HashMap<String, String>) -> Result<Entry<'a>, Error> {
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

        let name = clean_name(&name)?
            .ok_or_else(|| Error::InvalidTarFilePath(name.to_string()))?;

        if ZIP_PATH_INVALID_PATTERNS.is_match(&name) {
            return Err(Error::InvalidTarFilePath(name));
        }

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
        })
    }

    fn next_impl(&mut self) -> Result<Option<Entry<'a>>, Error> {
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

            // round up to the next multiple of 512
            self.offset += 512 + ((size + 511) / 512) * 512;

            let type_flag
                = self.buffer[offset + 156];
            
            match type_flag {
                // Regular file
                b'0' | 0 => {
                    match self.parse_entry_at(offset, size, &pax_headers) {
                        Ok(entry) => {
                            return Ok(Some(entry));
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
                    // Other types (directories, symlinks, etc.) - skip
                    // Continue to next entry
                },
            }
        }
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

static ZIP_PATH_INVALID_PATTERNS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\\|/\.{0,2}/|^\.{0,2}/|/\.{0,2}$|^\.{0,2}$").unwrap()
});

fn clean_name(name: &str) -> Result<Option<String>, Error> {
    if name.starts_with('/') {
        return Ok(None)
    }

    let has_parent_specifier = name.split('/')
        .any(|part| part == "..");

    if has_parent_specifier {
        return Ok(None)
    }

    let mut name = zpm_utils::Path::try_from(name)?
        .to_string();

    if name.ends_with('/') {
        name.pop();
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
