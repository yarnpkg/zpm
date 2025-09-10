use std::{borrow::Cow, io::Write};

use flate2::write::DeflateEncoder;

use crate::{Compression, CompressionAlgorithm, Entry};

pub trait VecExt<'a> {
    fn normalize(&mut self);
}

impl<'a> VecExt<'a> for Vec<Entry<'a>> {
    fn normalize(&mut self) {
        self.sort_by(|a, b| {
            a.name.cmp(&b.name)
        });

        if let Some(manifest_idx) = self.iter().position(|entry| entry.name == "package.json") {
            let manifest_entry = self.remove(manifest_idx);
            self.insert(0, manifest_entry);
        }
    }
}

pub trait IterExt<'a> {
    fn strip_first_segment(self) -> StripFirstSegment<Self> where Self: Sized;
    fn strip_path_prefix(self, prefix: String) -> StripPathPrefix<Self> where Self: Sized;
    fn prefix_path(self, prefix: &str) -> PrefixPath<Self> where Self: Sized;
    fn update_crc32(self) -> UpdateCrc32<Self> where Self: Sized;
    fn compress(self, algorithm: Option<CompressionAlgorithm>) -> Compress<Self> where Self: Sized;
}

impl<'a, T> IterExt<'a> for T where T: Iterator<Item = Entry<'a>> {
    fn strip_first_segment(self) -> StripFirstSegment<Self> {
        StripFirstSegment::new(self)
    }

    fn strip_path_prefix(self, prefix: String) -> StripPathPrefix<Self> {
        StripPathPrefix::new(self, prefix)
    }

    fn prefix_path(self, prefix: &str) -> PrefixPath<Self> {
        PrefixPath::new(self, prefix.to_string())
    }

    fn update_crc32(self) -> UpdateCrc32<Self> {
        UpdateCrc32::new(self)
    }

    fn compress(self, algorithm: Option<CompressionAlgorithm>) -> Compress<Self> {
        Compress::new(self, algorithm)
    }
}

pub struct StripFirstSegment<T> {
    pub(crate) iter: T,
}

impl<T> StripFirstSegment<T> {
    pub fn new(iter: T) -> Self {
        Self {iter}
    }
}

impl<'a, T> Iterator for StripFirstSegment<T> where T: Iterator<Item = Entry<'a>> {
    type Item = Entry<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let next
                = self.iter.next();

            let Some(mut next) = next else {
                return None;
            };

            if let Some(slash_index) = next.name.find('/') {
                next.name = next.name[slash_index + 1..].to_string();
                return Some(next);
            }
        }
    }
}

pub struct StripPathPrefix<T> {
    pub(crate) iter: T,
    pub(crate) prefix: String,
}

impl<T> StripPathPrefix<T> {
    pub fn new(iter: T, prefix: String) -> Self {
        Self {iter, prefix}
    }
}

impl<'a, T> Iterator for StripPathPrefix<T> where T: Iterator<Item = Entry<'a>> {
    type Item = Entry<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        let next
            = self.iter.next();

        let Some(mut next) = next else {
            return None;
        };

        if next.name.starts_with(&self.prefix) {
            next.name = next.name[self.prefix.len() + 1..].to_string();
        }

        Some(next)
    }
}

pub struct PrefixPath<T> {
    pub(crate) iter: T,
    pub(crate) prefix: String,
}

impl<T> PrefixPath<T> {
    pub fn new(iter: T, prefix: String) -> Self {
        Self {iter, prefix}
    }
}

impl<'a, T> Iterator for PrefixPath<T> where T: Iterator<Item = Entry<'a>> {
    type Item = T::Item;

    fn next(&mut self) -> Option<Self::Item> {
        let next
            = self.iter.next();

        let Some(mut next) = next else {
            return None;
        };

        next.name = format!("{}/{}", self.prefix, next.name);

        Some(next)
    }
}

pub struct UpdateCrc32<T> {
    pub(crate) iter: T,
}

impl<T> UpdateCrc32<T> {
    pub fn new(iter: T) -> Self {
        Self {iter}
    }
}

impl<'a, T> Iterator for UpdateCrc32<T> where T: Iterator<Item = Entry<'a>> {
    type Item = Entry<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        let next
            = self.iter.next();

        let Some(mut next) = next else {
            return None;
        };

        next.crc = crc32fast::hash(&next.data);

        Some(next)
    }
}

pub struct Compress<T> {
    pub(crate) iter: T,
    pub(crate) algorithm: Option<CompressionAlgorithm>,
}

impl<T> Compress<T> {
    pub fn new(iter: T, algorithm: Option<CompressionAlgorithm>) -> Self {
        Self {iter, algorithm}
    }
}

impl<'a, T> Iterator for Compress<T> where T: Iterator<Item = Entry<'a>> {
    type Item = Entry<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        let next
            = self.iter.next();

        let Some(mut next) = next else {
            return None;
        };

        let Some(algorithm) = self.algorithm else {
            return Some(next);
        };

        let compressed_data = match algorithm {
            CompressionAlgorithm::Deflate(level) => {
                let mut encoder
                    = DeflateEncoder::new(Vec::new(), flate2::Compression::new(level as u32));

                encoder.write_all(&next.data).unwrap();
                encoder.finish().unwrap()
            },
        };

        next.compression = Some(Compression {
            data: Cow::Owned(compressed_data),
            algorithm,
        });

        Some(next)
    }
}
