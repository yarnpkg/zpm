use assert_cmd::prelude::*; // Add methods on commands
use predicates::prelude::*; // Add predicates for use in assertions
use zpm_utils::Path; // Used for writing assertions
use std::process::Command; // Run programs

#[test]
fn list_cache() -> Result<(), Box<dyn std::error::Error>> {
    let mut cache_path = Path::current_dir()?;
    cache_path.join_str("./tests/fixtures/cache");

    let mut cmd
        = Command::cargo_bin("yarn")
            .expect("Failed to get yarn command");
    cmd
            .env("YARNSW_CACHE_PATH", cache_path.as_str())
            .args(vec!["switch", "cache"]);

    cmd.assert()
        .success()
        .stdout(predicate::str::contains("6.0.0-test"))
        .stdout(predicate::str::contains("b1069c168e09d6e327b9cb88fb86bf52"));

    Ok(())
}

#[test]
fn fails_with_relative_cache_path() -> Result<(), Box<dyn std::error::Error>> {
    let mut cmd
        = Command::cargo_bin("yarn")
            .expect("Failed to get yarn command");
    cmd
            .env("YARNSW_CACHE_PATH", "tests/fixtures/cache")
            .args(vec!["switch", "cache"]);

    cmd.assert()
        .failure()
        .stdout(predicate::str::contains("Error: Cache path must be absolute but got tests/fixtures/cache"));

    Ok(())
}
