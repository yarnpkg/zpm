use std::{collections::BTreeMap, io::Read, sync::LazyLock, time::Instant};

use crate::error::Error;

pub fn convert_to_hashmap<U, T, F>(items: Vec<T>, mut key_fn: F) -> BTreeMap<U, Vec<T>> where U: Eq + Ord, F: FnMut(&T) -> U {
    let mut map: BTreeMap<U, Vec<T>> = BTreeMap::new();

    for item in items {
        let key = key_fn(&item);
        map.entry(key).or_default().push(item);
    }

    map
}

pub fn unpack_brotli_data(data: &[u8]) -> Result<String, Error> {
    let mut decompressor
        = brotli::Decompressor::new(data, 1024 * 1024);

    let mut decompressed_bytes = Vec::new();
    decompressor.read_to_end(&mut decompressed_bytes).unwrap();

    let decompressed_string
        = String::from_utf8(decompressed_bytes)?;

    Ok(decompressed_string)
}

pub static FIRST_TIME: LazyLock<Instant> = LazyLock::new(Instant::now);

#[macro_export]
macro_rules! print_time {
    ($msg:expr) => {
        let now = std::time::Instant::now();
        let elapsed = now.duration_since(*$crate::misc::FIRST_TIME);

        println!("{:?} - {}", elapsed, $msg);
    };
}

#[macro_export]
macro_rules! yarn_track_time {
    ($label:expr, $code:block) => { {
        let start = std::time::Instant::now();

        let res = $code;

        let duration = start.elapsed();
        println!("{} {:?}", $label, duration);

        res
    } }
}
