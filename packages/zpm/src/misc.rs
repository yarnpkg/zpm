use std::collections::HashMap;
use std::hash::Hash;

pub fn convert_to_hashmap<U, T, F>(items: Vec<T>, mut key_fn: F) -> HashMap<U, Vec<T>> where U: Eq + Hash, F: FnMut(&T) -> U {
    let mut map: HashMap<U, Vec<T>> = HashMap::new();

    for item in items {
        let key = key_fn(&item);
        map.entry(key).or_default().push(item);
    }

    map
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
