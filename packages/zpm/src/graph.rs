use std::{hash::Hash, sync::Arc};

use dashmap::DashMap;
use tokio::sync::OnceCell;

use crate::error::Error;

#[cfg(test)]
#[path = "./graph.test.rs"]
mod graph_tests;

pub struct WaitMap<K, V> {
    map: DashMap<K, Arc<OnceCell<Result<V, Arc<Error>>>>>,
}

impl<K: Eq + Hash + Clone, V> WaitMap<K, V> {
    pub fn new() -> Self {
        Self {
            map: DashMap::new(),
        }
    }

    /// Returns the OnceCell for the key, inserting a new empty one if absent.
    pub fn entry(&self, key: K) -> Arc<OnceCell<Result<V, Arc<Error>>>> {
        self.map
            .entry(key)
            .or_insert_with(|| Arc::new(OnceCell::new()))
            .clone()
    }

    /// Collect all errors from the map.
    pub fn collect_errors(&self) -> Vec<Arc<Error>> {
        let mut errors = Vec::new();
        for entry in self.map.iter() {
            if let Some(Err(err)) = entry.value().get() {
                errors.push(err.clone());
            }
        }
        errors
    }

    /// Iterate over all keys in the map (with a placeholder value).
    /// Useful for waiting on all entries after they've been spawned.
    pub fn iter(&self) -> Vec<(K, ())> {
        self.map.iter().map(|entry| (entry.key().clone(), ())).collect()
    }

    /// Consume the map and return all entries as (key, result) pairs.
    /// Only returns entries that have been initialized.
    pub fn into_results(self) -> Vec<(K, Result<V, Arc<Error>>)>
    where
        V: Clone,
    {
        self.map
            .into_iter()
            .filter_map(|(key, cell)| {
                // Try to unwrap the Arc first (zero-copy); fall back to cloning
                // if other references still exist.
                let result = match Arc::try_unwrap(cell) {
                    Ok(cell) => cell.into_inner(),
                    Err(arc) => arc.get().cloned(),
                };
                result.map(|r| (key, r))
            })
            .collect()
    }
}
