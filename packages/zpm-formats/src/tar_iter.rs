use std::{borrow::Cow, sync::LazyLock};

use itertools::Itertools;
use regex::Regex;

use crate::{Entry, Error};

pub struct TarIterator<'a> {
    buffer: &'a [u8],
    offset: usize,
}

impl<'a> TarIterator<'a> {
    pub fn new(buffer: &[u8]) -> TarIterator {
        TarIterator {
            buffer,
            offset: 0,
        }
    }

    fn parse_entry_at(&self, offset: usize, size: usize) -> Result<Entry<'a>, Error> {
        let name_slice = self.buffer.get(offset..offset + 100)
            .map(trim_zero)
            .ok_or(Error::InvalidTarFile)?;

        let name
            = std::str::from_utf8(name_slice)?;

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
        loop {
            if self.offset >= self.buffer.len() {
                return Ok(None);
            }

            let offset = self.offset;

            let size
                = self.buffer.get(offset + 124..offset + 136)
                    .map(|raw| from_oct(&raw) as usize)
                    .ok_or(Error::InvalidTarFile)?;

            // round up to the next multiple of 512
            self.offset += 512 + ((size + 511) / 512) * 512;

            if self.buffer[offset + 156] != 0 {
                if self.buffer[offset + 156] == b'0' {
                    return Ok(Some(self.parse_entry_at(offset, size)?))
                }
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
