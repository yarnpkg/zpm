use std::hash::Hash;

use bincode::{Decode, Encode};
use blake2::{Blake2b, Digest, digest::consts::U64};

use crate::{impl_file_string_from_str, impl_file_string_serialization, DataType, FromFileString, ToFileString, ToHumanString};

pub type Blake2b80 = Blake2b<U64>;

#[derive(Clone, Debug, Decode, Encode, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Hash64 {
    state: Vec<u8>,
}

impl Hash64 {
    pub fn from_data<T: AsRef<[u8]>>(data: T) -> Self {
        let mut hasher = Blake2b80::new();
        hasher.update(data.as_ref());

        Hash64 {state: hasher.finalize().to_vec()}
    }

    pub fn from_string<T: ToFileString>(str: &T) -> Self {
        let mut hasher = Blake2b80::new();
        hasher.update(str.to_file_string().as_bytes());

        Hash64 {state: hasher.finalize().to_vec()}
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
            = Blake2b80::new();

        for hash in self {
            hasher.update(hash.state.as_slice());
        }

        Hash64 {state: hasher.finalize().to_vec()}
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
