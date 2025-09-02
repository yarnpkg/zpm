use assert_cmd::prelude::*; // Add methods on commands
use zpm_utils::{Path, ToFileString}; // Used for writing assertions
use std::process::Command; // Run programs

struct TestEnv {
    cmd: Command,
    tmp_dir: Path,
}

fn init_test_env() -> TestEnv {
    let cmd
        = Command::cargo_bin("yarn")
            .expect("Failed to get yarn command");

    let tmp_dir
        = Path::temp_dir()
            .expect("Failed to create temp dir");

    TestEnv {
        cmd,
        tmp_dir,
    }
}

#[test]
fn empty_profile_file() -> Result<(), Box<dyn std::error::Error>> {
    let TestEnv {
        mut cmd,
        tmp_dir,
    } = init_test_env();

    cmd.args(vec!["switch", "postinstall", "--home-dir", tmp_dir.as_str()]);
    cmd.env("SHELL", "/bin/bash");

    cmd.assert()
        .success();

    let profile_content = tmp_dir
        .with_join_str(".profile")
        .fs_read_text_prealloc()
        .expect("Failed to read .profile");

    assert_eq!(profile_content, format!(". \"{}/.yarn/switch/env\"\n", tmp_dir.to_file_string()));

    Ok(())
}

#[test]
fn profile_file_with_existing_path() -> Result<(), Box<dyn std::error::Error>> {
    let TestEnv {
        mut cmd,
        tmp_dir,
    } = init_test_env();

    tmp_dir
        .with_join_str(".profile")
        .fs_write_text("# Hello world!\n")
        .expect("Failed to write .profile");

    cmd.args(vec!["switch", "postinstall", "--home-dir", tmp_dir.as_str()]);
    cmd.env("SHELL", "/bin/bash");

    cmd.assert()
        .success();

    let profile_content = tmp_dir
        .with_join_str(".profile")
        .fs_read_text_prealloc()
        .expect("Failed to read .profile");

    assert_eq!(profile_content, format!("# Hello world!\n. \"{}/.yarn/switch/env\"\n", tmp_dir.to_file_string()));

    Ok(())
}

#[test]
fn profile_file_with_duplicate_path() -> Result<(), Box<dyn std::error::Error>> {
    let TestEnv {
        mut cmd,
        tmp_dir,
    } = init_test_env();

    let initial_profile_content
        = format!(". \"{}/.yarn/switch/env\"\n", tmp_dir.to_file_string());

    tmp_dir
        .with_join_str(".profile")
        .fs_write_text(&initial_profile_content)
        .expect("Failed to write .profile");

    cmd.args(vec!["switch", "postinstall", "--home-dir", tmp_dir.as_str()]);
    cmd.env("SHELL", "/bin/bash");

    cmd.assert()
        .success();

    let profile_content = tmp_dir
        .with_join_str(".profile")
        .fs_read_text_prealloc()
        .expect("Failed to read .profile");

    assert_eq!(profile_content, initial_profile_content);

    Ok(())
}
