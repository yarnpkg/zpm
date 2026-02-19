use std::hash::Hash;

use sha1::Digest as _;
use rkyv::Archive;

use crate::{impl_file_string_from_str, impl_file_string_serialization, DataType, FromFileString, ToFileString, ToHumanString};

pub struct Hash64Writer {
    hasher: blake2b_simd::State,
}

impl Hash64Writer {
    fn create_blake2b_state() -> blake2b_simd::State {
        blake2b_simd::Params::new()
            .hash_length(64)
            .to_state()
    }

    pub fn new() -> Self {
        Self {
            hasher: Self::create_blake2b_state(),
        }
    }

    pub fn update<T: AsRef<[u8]>>(&mut self, data: T) {
        self.hasher.update(data.as_ref());
    }

    pub fn finalize(self) -> Hash64 {
        Hash64 {state: self.hasher.finalize().as_bytes().to_vec()}
    }
}

impl Default for Hash64Writer {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Archive, rkyv::Serialize, rkyv::Deserialize)]
#[rkyv(derive(PartialEq, Eq, Hash, PartialOrd, Ord))]
pub struct Hash64 {
    state: Vec<u8>,
}

impl Hash64 {
    fn create_blake2b_state() -> blake2b_simd::State {
        blake2b_simd::Params::new()
            .hash_length(64)
            .to_state()
    }

    pub fn from_data<T: AsRef<[u8]>>(data: T) -> Self {
        let mut hasher = Self::create_blake2b_state();
        hasher.update(data.as_ref());

        Hash64 {state: hasher.finalize().as_bytes().to_vec()}
    }

    pub fn from_string<T: ToFileString>(str: &T) -> Self {
        let mut hasher = Self::create_blake2b_state();
        hasher.update(str.to_file_string().as_bytes());

        Hash64 {state: hasher.finalize().as_bytes().to_vec()}
    }

    pub fn mini(&self) -> String {
        hex::encode(&self.state[0..3])
    }

    pub fn short(&self) -> String {
        hex::encode(&self.state[0..16])
    }
}

pub trait CollectHash {
    fn collect_hash(self) -> Hash64;
}

impl<'a, I: Iterator<Item = &'a Hash64>> CollectHash for I {
    fn collect_hash(self) -> Hash64 {
        let mut hasher
            = Hash64::create_blake2b_state();

        for hash in self {
            hasher.update(hash.state.as_slice());
        }

        Hash64 {state: hasher.finalize().as_bytes().to_vec()}
    }
}

impl FromFileString for Hash64 {
    type Error = hex::FromHexError;

    fn from_file_string(src: &str) -> Result<Self, Self::Error> {
        Ok(Hash64 {state: hex::decode(src)?})
    }
}

impl ToFileString for Hash64 {
    fn to_file_string(&self) -> String {
        hex::encode(self.state.clone())
    }
}

impl ToHumanString for Hash64 {
    fn to_print_string(&self) -> String {
        DataType::Custom(135, 175, 255).colorize(&self.to_file_string())
    }
}

impl_file_string_from_str!(Hash64);
impl_file_string_serialization!(Hash64);

pub struct Sha1 {
    data: Vec<u8>,
}

impl Sha1 {
    pub fn new(data: &[u8]) -> Self {
        let mut hasher
            = sha1::Sha1::new();

        hasher.update(data);

        let data
            = hasher.finalize().to_vec();

        Self {
            data,
        }
    }

    pub fn to_hex(&self) -> String {
        hex::encode(&self.data)
    }

    pub fn to_base64(&self) -> String {
        base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &self.data)
    }
}

pub struct Sha256 {
    data: Vec<u8>,
}

impl Sha256 {
    pub fn new(data: &[u8]) -> Self {
        let mut hasher
            = sha2::Sha256::new();

        hasher.update(data);

        let data
            = hasher.finalize().to_vec();

        Self {
            data,
        }
    }

    pub fn to_hex(&self) -> String {
        hex::encode(&self.data)
    }

    pub fn to_base64(&self) -> String {
        base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &self.data)
    }
}

pub struct Sha512 {
    data: Vec<u8>,
}

impl Sha512 {
    pub fn new(data: &[u8]) -> Self {
        let mut hasher
            = sha2::Sha512::new();

        hasher.update(data);

        let data
            = hasher.finalize().to_vec();

        Self {
            data,
        }
    }

    pub fn to_hex(&self) -> String {
        hex::encode(&self.data)
    }

    pub fn to_base64(&self) -> String {
        base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &self.data)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash64_from_data_known_vector() {
        let hash
            = Hash64::from_data(b"abc");

        assert_eq!(
            hash.to_file_string(),
            "ba80a53f981c4d0d6a2797b69f12f6e94c212f14685ac4b74b12bb6fdbffa2d17d87c5392aab792dc252d5de4533cc9518d38aa8dbf1925ab92386edd4009923",
        );
    }

    #[test]
    fn hash64_output_length_invariants() {
        let hash
            = Hash64::from_data("x");

        assert_eq!(hash.to_file_string().len(), 128);
        assert_eq!(hash.mini().len(), 6);
        assert_eq!(hash.short().len(), 32);
    }

    #[test]
    fn collect_hash_determinism_and_order() {
        let hashes = vec![
            Hash64::from_data(b"alpha"),
            Hash64::from_data(b"beta"),
            Hash64::from_data(b"gamma"),
        ];

        let first
            = hashes.iter().collect_hash();
        let second
            = hashes.iter().collect_hash();
        let reversed
            = hashes.iter().rev().collect_hash();

        assert_eq!(first, second);
        assert_ne!(first, reversed);
    }
}
