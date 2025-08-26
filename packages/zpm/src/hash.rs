use std::hash::Hash;

use bincode::{Decode, Encode};
use blake2::{Blake2b, Digest, digest::consts::U64};
use colored::Colorize;
use zpm_utils::{impl_serialization_traits, FromFileString, ToFileString, ToHumanString};

use crate::error::Error;

pub type Blake2b80 = Blake2b<U64>;

#[derive(Clone, Debug, Decode, Encode, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Sha256 {
    state: Vec<u8>,
}

impl Sha256 {
    pub fn from_data<T: AsRef<[u8]>>(data: T) -> Self {
        let mut hasher = Blake2b80::new();
        hasher.update(data.as_ref());

        Sha256 {state: hasher.finalize().to_vec()}
    }

    pub fn from_string<T: AsRef<str>>(str: &T) -> Self {
        let mut hasher = Blake2b80::new();
        hasher.update(str.as_ref());

        Sha256 {state: hasher.finalize().to_vec()}
    }

    pub fn short(&self) -> String {
        hex::encode(&self.state[0..16])
    }
}

pub trait CollectHash {
    fn collect_hash(self) -> Sha256;
}

impl<'a, I: Iterator<Item = &'a Sha256>> CollectHash for I {
    fn collect_hash(self) -> Sha256 {
        let mut hasher
            = Blake2b80::new();

        for hash in self {
            hasher.update(hash.state.as_slice());
        }

        Sha256 {state: hasher.finalize().to_vec()}
    }
}

impl FromFileString for Sha256 {
    type Error = Error;

    fn from_file_string(src: &str) -> Result<Self, Error> {
        let state = hex::decode(src)
            .map_err(|_| Error::InvalidSha256(src.to_string()))?;

        Ok(Sha256 {state})
    }
}

impl ToFileString for Sha256 {
    fn to_file_string(&self) -> String {
        hex::encode(self.state.clone())
    }
}

impl ToHumanString for Sha256 {
    fn to_print_string(&self) -> String {
        self.to_file_string().truecolor(135, 175, 255).to_string()
    }
}

impl_serialization_traits!(Sha256);
