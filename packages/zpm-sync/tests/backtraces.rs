// Lives in its own test binary (and thus its own process) because it sets
// RUST_BACKTRACE and reads a process-wide counter, which the other tests
// running in parallel would disturb.

use std::{borrow::Cow, str::FromStr};

use zpm_sync::{SyncItem, SyncTree};
use zpm_utils::{captured_backtrace_count, Path, PathError};

fn p(path: &str) -> Path {
    Path::from_str(path).unwrap()
}

#[test]
fn syncing_into_an_empty_folder_captures_no_backtraces() {
    // The std caches the setting on the first capture, so this must happen
    // before anything else
    std::env::set_var("RUST_BACKTRACE", "1");
    std::env::remove_var("RUST_LIB_BACKTRACE");

    // Make sure we'd see the captures if there were any
    let before_sanity_check
        = captured_backtrace_count();

    let _ = PathError::from(std::io::Error::from(std::io::ErrorKind::NotFound));

    assert_eq!(captured_backtrace_count(), before_sanity_check + 1);

    let root
        = Path::temp_dir().unwrap();

    let before
        = captured_backtrace_count();

    // Every path of the tree is missing, as on a first install; probing
    // them is expected to fail and mustn't capture a backtrace
    let mut tree
        = SyncTree::new();

    tree.dry_run = false;

    for package in ["a", "b", "@scope/c"] {
        for file in ["package.json", "index.js", "lib/util.js", "lib/deep/nested.js"] {
            tree.register_entry(p(&format!("{package}/{file}")), SyncItem::File {
                data: Cow::Borrowed(b"module.exports = 42;\n"),
                is_exec: false,
            }).unwrap();
        }

        tree.register_entry(p(&format!("{package}/node_modules/self")), SyncItem::Symlink {
            target_path: p(".."),
        }).unwrap();
    }

    tree.run(root.clone()).unwrap();

    assert!(root.with_join_str("@scope/c/lib/deep/nested.js").fs_is_file());
    assert!(root.with_join_str("a/node_modules/self").fs_is_symlink_to(&p("..")).unwrap());

    // The filesystem probes of `Path` must be as cheap on missing paths
    let missing
        = root.with_join_str("missing/file");

    assert!(!missing.fs_exists());
    assert!(!missing.fs_is_file());
    assert!(!missing.fs_is_dir());
    assert!(!missing.fs_is_real_dir());
    assert!(!missing.fs_is_symlink());
    assert!(missing.if_exists().is_none());
    assert!(missing.fs_metadata_if_exists().unwrap().is_none());
    assert!(missing.fs_symlink_metadata_if_exists().unwrap().is_none());

    root.with_join_str("new-file")
        .fs_change(b"content", false)
        .unwrap();

    assert_eq!(captured_backtrace_count(), before);
}
