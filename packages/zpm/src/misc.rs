use std::{collections::HashMap, hash::Hash, sync::LazyLock, time::Instant};

pub fn convert_to_hashmap<U, T, F>(items: Vec<T>, mut key_fn: F) -> HashMap<U, Vec<T>> where U: Eq + Hash, F: FnMut(&T) -> U {
    let mut map: HashMap<U, Vec<T>> = HashMap::new();

    for item in items {
        let key = key_fn(&item);
        map.entry(key).or_default().push(item);
    }

    map
}

pub static FIRST_TIME: LazyLock<Instant> = LazyLock::new(Instant::now);

#[macro_export]
macro_rules! print_time {
    ($msg:expr) => {
        let now = std::time::Instant::now();
        let elapsed = now.duration_since(*crate::misc::FIRST_TIME);

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
