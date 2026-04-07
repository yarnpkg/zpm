use std::sync::Arc;

use super::WaitMap;
use crate::error::Error;

#[tokio::test]
async fn entry_returns_same_cell_for_same_key() {
    let map: WaitMap<String, i32> = WaitMap::new();
    let cell1 = map.entry("key".to_string());
    let cell2 = map.entry("key".to_string());

    cell1.get_or_init(|| async { Ok(42) }).await;

    assert!(cell2.initialized());
    assert_eq!(cell2.get().unwrap().as_ref().unwrap(), &42);
}

#[tokio::test]
async fn entry_returns_different_cells_for_different_keys() {
    let map: WaitMap<String, i32> = WaitMap::new();
    let cell_a = map.entry("a".to_string());
    let cell_b = map.entry("b".to_string());

    cell_a.get_or_init(|| async { Ok(1) }).await;
    cell_b.get_or_init(|| async { Ok(2) }).await;

    assert_eq!(cell_a.get().unwrap().as_ref().unwrap(), &1);
    assert_eq!(cell_b.get().unwrap().as_ref().unwrap(), &2);
}

#[tokio::test]
async fn collect_errors_returns_only_errors() {
    let map: WaitMap<String, i32> = WaitMap::new();

    let cell_ok = map.entry("ok".to_string());
    cell_ok.get_or_init(|| async { Ok(42) }).await;

    let cell_err = map.entry("err".to_string());
    cell_err.get_or_init(|| async { Err(Arc::new(Error::TaskTimeout)) }).await;

    let errors = map.collect_errors();
    assert_eq!(errors.len(), 1);
}

#[tokio::test]
async fn into_results_returns_all_initialized_entries() {
    let map: WaitMap<String, i32> = WaitMap::new();

    let cell1 = map.entry("a".to_string());
    cell1.get_or_init(|| async { Ok(1) }).await;

    let cell2 = map.entry("b".to_string());
    cell2.get_or_init(|| async { Ok(2) }).await;

    let cell3 = map.entry("c".to_string());
    cell3.get_or_init(|| async { Err(Arc::new(Error::TaskTimeout)) }).await;

    let results = map.into_results();
    assert_eq!(results.len(), 3);

    let ok_count = results.iter().filter(|(_, r)| r.is_ok()).count();
    let err_count = results.iter().filter(|(_, r)| r.is_err()).count();
    assert_eq!(ok_count, 2);
    assert_eq!(err_count, 1);
}

#[tokio::test]
async fn get_or_init_runs_only_once() {
    let map: WaitMap<String, i32> = WaitMap::new();
    let counter = Arc::new(std::sync::atomic::AtomicU32::new(0));

    let cell = map.entry("key".to_string());
    let counter_clone = counter.clone();
    cell.get_or_init(|| async move {
        counter_clone.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        Ok(42)
    }).await;

    // Second call should not run the init
    let cell = map.entry("key".to_string());
    let counter_clone = counter.clone();
    cell.get_or_init(|| async move {
        counter_clone.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        Ok(99)
    }).await;

    assert_eq!(counter.load(std::sync::atomic::Ordering::Relaxed), 1);
    assert_eq!(cell.get().unwrap().as_ref().unwrap(), &42);
}

#[tokio::test]
async fn concurrent_init_deduplicates() {
    let map = Arc::new(WaitMap::<String, i32>::new());
    let counter = Arc::new(std::sync::atomic::AtomicU32::new(0));

    let mut handles = Vec::new();
    for _ in 0..10 {
        let map = map.clone();
        let counter = counter.clone();
        handles.push(tokio::spawn(async move {
            let cell = map.entry("key".to_string());
            cell.get_or_init(|| {
                let counter = counter.clone();
                async move {
                    counter.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                    Ok(42)
                }
            }).await;
        }));
    }

    for handle in handles {
        handle.await.unwrap();
    }

    // Only one init should have run
    assert_eq!(counter.load(std::sync::atomic::Ordering::Relaxed), 1);
}
