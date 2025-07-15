// Yarn supports passing a cwd as first argument. In the case of yarn switch we want to support this,
// but we don't want to *actually* change the cwd - merely obtain the (future) cwd so we can't pull
// the `packageManager` field from the proper location.
//
// To that end we store the "fake cwd" in this global variable.

use std::sync::Mutex;

use zpm_utils::{Path, PathError, ToFileString};

static FAKE_CWD: Mutex<Option<Path>> = Mutex::new(None);

pub fn set_fake_cwd(cwd: Path) {
    *FAKE_CWD.lock().unwrap() = Some(cwd);
}

pub fn get_fake_cwd() -> Option<Path> {
    FAKE_CWD.lock().unwrap().clone()
}

pub fn get_final_cwd() -> Result<Path, PathError> {
    if let Some(cwd) = get_fake_cwd() {
        Ok(cwd)
    } else {
        Path::current_dir()
    }
}

pub fn restore_args(args: &mut Vec<String>) {
    if let Some(cwd) = get_fake_cwd() {
        // We add an explicit `--cwd` so that both implicit and explicit cwd arguments are correctly forwarded.
        args.insert(0, format!("--cwd={}", cwd.to_file_string()));
    }
}
