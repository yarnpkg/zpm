use std::collections::HashMap;
use std::hash::Hash;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

pub fn convert_to_hashmap<U, T, F>(items: Vec<T>, mut key_fn: F) -> HashMap<U, Vec<T>> where U: Eq + Hash, F: FnMut(&T) -> U {
    let mut map: HashMap<U, Vec<T>> = HashMap::new();

    for item in items {
        let key = key_fn(&item);
        map.entry(key).or_insert_with(Vec::new).push(item);
    }

    map
}

pub fn change_file<P: AsRef<Path>, C: AsRef<[u8]>>(path: P, contents: C, mode: u32) -> Result<(), std::io::Error> {
    let update_content = std::fs::read(&path)
        .map(|current| {
            current.ne(contents.as_ref())
        })
        .or_else(|err| match err.kind() {
            std::io::ErrorKind::NotFound => Ok(true),
            _ => Err(err),
        })?;

    if update_content {
        std::fs::write(&path, contents)?;
    }

    let update_permissions = update_content ||
        (std::fs::metadata(&path)?.permissions().mode() & 0o777) != mode;

    if update_permissions {
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode))?;
    }

    Ok(())
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
