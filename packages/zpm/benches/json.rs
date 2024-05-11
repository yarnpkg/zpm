use std::{collections::HashMap, fs, path::PathBuf, str::FromStr};

use criterion::{criterion_group, criterion_main, Criterion};
use serde::Deserialize;
use zpm::{manifest::Manifest, semver::{self, Range}};

// In those tests, I attempted to find the most efficient way to resolve a semver range from a JSON metadata. I
// selected a fairly large package (React) with a bunch of releases.
//
// The "custom" implementation is the slowest by far - 15ms to run, vs 5ms for the fastest (sonic). The "sonic_manual"
// implementation is similar in idea to the "custom" one (we try to avoid parsing the metadata fields we don't need), but
// is still slower than both general approaches (from both serde and sonic).

#[derive(Deserialize)]
struct Metadata {
    versions: HashMap<semver::Version, Manifest>,
}

fn resolve_json_sonic(registry_text: &str, range: semver::Range) {
    let registry_data: Metadata = sonic_rs::from_str(registry_text).unwrap();
    let mut candidates: Vec<semver::Version> = registry_data.versions.keys().cloned().collect();

    candidates.sort();
    candidates.reverse();

    let version = candidates.iter()
        .find(|v| range.check(*v))
        .unwrap();

    registry_data.versions.get(version).unwrap();
}

fn resolve_json_sonic_manual(registry_text: &str, range: semver::Range) {
    let versions = sonic_rs::get(registry_text, vec!["versions"])
        .unwrap();

    let mut candidates = sonic_rs::to_object_iter(versions.as_raw_str())
        .map(|entry| entry.unwrap())
        .map(|(k, v)| (semver::Version::from_str(k.as_str()).unwrap(), v))
        .filter(|(k, _)| range.check(k))
        .collect::<Vec<_>>();

    candidates.sort_by(|(a, _), (b, _)| b.cmp(a));

    if let Some((_, manifest_text)) = candidates.first() {
        sonic_rs::from_str::<Manifest>(manifest_text.as_raw_str()).unwrap();
    }
}

fn resolve_json_serde(registry_text: &str, range: semver::Range) {
    let registry_data: Metadata = serde_json::from_str(registry_text).unwrap();
    let mut candidates: Vec<semver::Version> = registry_data.versions.keys().cloned().collect();

    candidates.sort();
    candidates.reverse();

    let version = candidates.iter()
        .find(|v| range.check(*v))
        .unwrap();

    registry_data.versions.get(version).unwrap();
}

fn json_react_benchmark(c: &mut Criterion) {
    let mut react_json_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    react_json_path.push("data/react.json");

    let react_json_text = fs::read_to_string(react_json_path).unwrap();
    let range = Range::from_str("^15.0.0").unwrap();

    c.bench_function("extract_json/serde", |b| {
        b.iter(|| resolve_json_serde(&react_json_text, range.clone()));
    });

    c.bench_function("extract_json/sonic", |b| {
        b.iter(|| resolve_json_sonic(&react_json_text, range.clone()));
    });

    c.bench_function("extract_json/sonic_manual", |b| {
        b.iter(|| resolve_json_sonic_manual(&react_json_text, range.clone()));
    });
}

criterion_group!(benches, json_react_benchmark);
criterion_main!(benches);
