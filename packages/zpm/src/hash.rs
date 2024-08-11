use std::hash::Hash;

use bincode::{Decode, Encode};
use blake2::{Blake2b, Digest, digest::consts::U64};

use crate::{error::Error, yarn_serialization_protocol};

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

yarn_serialization_protocol!(Sha256, "", {
    deserialize(src) {
        let state = hex::decode(src)
            .map_err(|_| Error::InvalidSha256(src.to_string()))?;

        Ok(Sha256 {state})
    }

    serialize(&self) {
        hex::encode(self.state.clone())
    }
});
