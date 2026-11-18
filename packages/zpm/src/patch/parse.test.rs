use crate::error::Error;

use super::*;

const PATCH: &str = "diff --git a/index.ts b/index.ts
index 2de83dd..842652c 100644
--- a/index.ts
+++ b/index.ts
@@ -1,4 +1,4 @@
 this
 is
-a
+my
 file
";

const INVALID_HEADERS_1: &str = "diff --git a/index.ts b/index.ts
index 2de83dd..842652c 100644
--- a/index.ts
+++ b/index.ts
@@ -1,4 +1,3 @@
 this
 is
-a
+my
 file
";

const INVALID_HEADERS_2: &str = "diff --git a/index.ts b/index.ts
index 2de83dd..842652c 100644
--- a/index.ts
+++ b/index.ts
@@ -1,3 +1,4 @@
 this
 is
-a
+my
 file
";

const INVALID_HEADERS_3: &str = "diff --git a/index.ts b/index.ts
index 2de83dd..842652c 100644
--- a/index.ts
+++ b/index.ts
@@ -1,0 +1,4 @@
 this
 is
-a
+my
 file
";

const INVALID_HEADERS_4: &str = "diff --git a/index.ts b/index.ts
index 2de83dd..842652c 100644
--- a/index.ts
+++ b/index.ts
@@ -1,4 +1,0 @@
 this
 is
-a
+my
 file
";

const INVALID_HEADERS_5: &str = "diff --git a/index.ts b/index.ts
index 2de83dd..842652c 100644
--- a/index.ts
+++ b/index.ts
@@ -1,4 +1,4@@
 this
 is
-a
+my
 file
";

const ACCIDENTAL_BLANK_LINE: &str = "diff --git a/index.ts b/index.ts
index 2de83dd..842652c 100644
--- a/index.ts
+++ b/index.ts
@@ -1,4 +1,4 @@
 this
 is
-a
+my
 file
";

#[test]
fn simple_case() {
    assert_eq!(PatchParser::parse(PATCH).unwrap(), vec![
        PatchFilePart::FilePatch {
            semver_exclusivity: None,
            path: Path::try_from("index.ts").unwrap(),
            hunks: vec![
                Hunk {
                    header: HunkHeader {
                        original: Range {
                            start: 1,
                            length: 4,
                        },
                        modified: Range {
                            start: 1,
                            length: 4,
                        },
                    },
                    parts: vec![
                        PatchMutationPart {
                            kind: PatchMutationPartKind::Context,
                            lines: vec![
                                "this".to_string(),
                                "is".to_string(),
                            ],
                            no_newline_at_eof: false,
                        },
                        PatchMutationPart {
                            kind: PatchMutationPartKind::Deletion,
                            lines: vec![
                                "a".to_string(),
                            ],
                            no_newline_at_eof: false,
                        },
                        PatchMutationPart {
                            kind: PatchMutationPartKind::Insertion,
                            lines: vec![
                                "my".to_string(),
                            ],
                            no_newline_at_eof: false,
                        },
                        PatchMutationPart {
                            kind: PatchMutationPartKind::Context,
                            lines: vec![
                                "file".to_string(),
                            ],
                            no_newline_at_eof: false,
                        },
                    ],
                },
            ],
            before_hash: Some(
                "2de83dd".to_string(),
            ),
            after_hash: Some(
                "842652c".to_string(),
            ),
        },
    ]);

    assert!(matches!(PatchParser::parse(INVALID_HEADERS_1), Err(Error::HunkIntegrityCheckFailed)));
    assert!(matches!(PatchParser::parse(INVALID_HEADERS_2), Err(Error::HunkIntegrityCheckFailed)));
    assert!(matches!(PatchParser::parse(INVALID_HEADERS_3), Err(Error::HunkIntegrityCheckFailed)));
    assert!(matches!(PatchParser::parse(INVALID_HEADERS_4), Err(Error::HunkIntegrityCheckFailed)));
    assert!(matches!(PatchParser::parse(INVALID_HEADERS_5), Err(Error::InvalidHunkHeader(x)) if x == *"@@ -1,4 +1,4@@"));

    assert_eq!(PatchParser::parse(ACCIDENTAL_BLANK_LINE).unwrap(), PatchParser::parse(PATCH).unwrap());
}

#[test]
fn unquotes_c_style_paths() {
    let cases: &[(&str, &str, &str)] = &[
        (r#""a/index.js""#, "a/index.js", ""),
        (r#""a/caf\303\251.js""#, "a/caf\u{e9}.js", ""),
        (r#""a/\346\227\245\346\234\254.js""#, "a/\u{65e5}\u{672c}.js", ""),
        (r#""a/with \"quote\".js""#, "a/with \"quote\".js", ""),
        (r#""a/back\\slash.js""#, "a/back\\slash.js", ""),
        (r#""a/tab\there.js""#, "a/tab\there.js", ""),
        (r#""a/new\nline.js""#, "a/new\nline.js", ""),
        (r#""a/\a\b\v\f\r.js""#, "a/\u{7}\u{8}\u{b}\u{c}\r.js", ""),
        (r#""a/\001\177.js""#, "a/\u{1}\u{7f}.js", ""),
        // With `core.quotePath=false`, non-ASCII characters are left as-is
        (r#""a/café \"quote\".js""#, "a/caf\u{e9} \"quote\".js", ""),
        ("\"a/with \\\"quote\\\".js\"\t", "a/with \"quote\".js", "\t"),
        (r#""a/from.js" "b/to.js""#, "a/from.js", " \"b/to.js\""),
    ];

    for (quoted, path, rest) in cases {
        assert_eq!(parse_quoted_path(quoted), Some((path.to_string(), *rest)), "{quoted}");
    }

    let invalid_cases = [
        r#"a/index.js"#,
        r#""a/index.js"#,
        r#""a/index.js\""#,
        r#""a/\q.js""#,
        r#""a/\30""#,
        r#""a/\38.js""#,
        r#""a/\400.js""#,
        // Not valid UTF-8
        r#""a/\377.js""#,
    ];

    for quoted in invalid_cases {
        assert_eq!(parse_quoted_path(quoted), None, "{quoted}");
    }
}

#[test]
fn parses_diff_line_paths() {
    let cases: &[(&str, &str, &str)] = &[
        ("diff --git a/index.js b/index.js", "index.js", "index.js"),
        ("diff --git a/with space.js b/with space.js", "with space.js", "with space.js"),
        ("diff --git a/ leading.js b/ leading.js", " leading.js", " leading.js"),
        (r#"diff --git "a/caf\303\251.js" "b/caf\303\251.js""#, "caf\u{e9}.js", "caf\u{e9}.js"),
        (r#"diff --git "a/with \"quote\".js" "b/with \"quote\".js""#, "with \"quote\".js", "with \"quote\".js"),
        (r#"diff --git "a/back\\slash.js" "b/back\\slash.js""#, "back\\slash.js", "back\\slash.js"),
        (r#"diff --git "a/tab\there.js" "b/tab\there.js""#, "tab\there.js", "tab\there.js"),
        (r#"diff --git "a/a b/c.js" "b/a b/c.js""#, "a b/c.js", "a b/c.js"),
        // Renames may only quote one of the two paths
        (r#"diff --git "a/old\tname.js" b/new name.js"#, "old\tname.js", "new name.js"),
        (r#"diff --git a/old name.js "b/new\tname.js""#, "old name.js", "new\tname.js"),
        (r#"diff --git a/x "b/y.js" "b/caf\303\251.js""#, "x \"b/y.js\"", "caf\u{e9}.js"),
    ];

    for (line, from, to) in cases {
        let (parsed_from, parsed_to)
            = parse_diff_line_paths(line).unwrap_or_else(|| panic!("{line}"));

        assert_eq!((parsed_from.as_ref(), parsed_to.as_ref()), (*from, *to), "{line}");
    }

    let invalid_cases = [
        "diff --git index.js index.js",
        r#"diff --git "a/index.js "b/index.js""#,
        r#"diff --git "a/index.js""b/index.js""#,
        r#"diff --git "x/index.js" "b/index.js""#,
    ];

    for line in invalid_cases {
        assert!(parse_diff_line_paths(line).is_none(), "{line}");
    }
}

#[test]
fn parses_file_line_paths() {
    let cases: &[(&str, &str)] = &[
        ("a/index.js", "index.js"),
        ("b/with space.js\t", "with space.js"),
        ("a/index.js\t2024-01-01 00:00:00.000000000 +0000", "index.js"),
        (r#""b/caf\303\251.js""#, "caf\u{e9}.js"),
        ("\"b/with \\\"quote\\\".js\"\t", "with \"quote\".js"),
        (r#""b/back\\slash.js""#, "back\\slash.js"),
    ];

    for (src, path) in cases {
        assert_eq!(parse_file_line_path(src).as_deref(), Some(*path), "{src}");
    }

    assert_eq!(parse_file_line_path(r#""b/unterminated.js"#), None);
    assert_eq!(parse_file_line_path(r#""b/index.js" trailing"#), None);
}

#[test]
fn parses_header_paths() {
    assert_eq!(parse_header_path("with space.js").as_deref(), Some("with space.js"));
    assert_eq!(parse_header_path(r#""caf\303\251.js""#).as_deref(), Some("caf\u{e9}.js"));
    assert_eq!(parse_header_path(r#""old \"name\".js""#).as_deref(), Some("old \"name\".js"));
    assert_eq!(parse_header_path(r#""old.js" trailing"#), None);
}

// Generated by `git diff --no-index` and `git show -M`, with special characters
// in the file names; git follows the `---`/`+++` paths that contain spaces with
// a tab (written `{TAB}` here)
const QUOTED_PATHS_PATCH: &str = r#"diff --git "a/caf\303\251.js" "b/caf\303\251.js"
new file mode 100644
index 0000000000000000000000000000000000000000..3e757656cf36eca53338e520d134963a44f793f8
--- /dev/null
+++ "b/caf\303\251.js"
@@ -0,0 +1 @@
+new
diff --git "a/tab\there.js" "b/tab\there.js"
deleted file mode 100644
index 78981922613b2afb6025042ff6bd878ac1994e85..0000000000000000000000000000000000000000
--- "a/tab\there.js"
+++ /dev/null
@@ -1 +0,0 @@
-a
diff --git "a/with \"quote\".js" "b/with \"quote\".js"
index 78981922613b2afb6025042ff6bd878ac1994e85..61780798228d17af2d34fce4cfbdf35556832472 100644
--- "a/with \"quote\".js"{TAB}
+++ "b/with \"quote\".js"{TAB}
@@ -1 +1 @@
-a
+b
diff --git a/with space.js b/with space.js
index 78981922613b2afb6025042ff6bd878ac1994e85..61780798228d17af2d34fce4cfbdf35556832472 100644
--- a/with space.js{TAB}
+++ b/with space.js{TAB}
@@ -1 +1 @@
-a
+b
diff --git "a/old \"name\".js" "b/caf\303\251.js"
similarity index 100%
rename from "old \"name\".js"
rename to "caf\303\251.js"
"#;

#[test]
fn parses_patches_with_quoted_paths() {
    let parts
        = PatchParser::parse(&QUOTED_PATHS_PATCH.replace("{TAB}", "\t")).unwrap();

    let paths = parts.iter().map(|part| match part {
        PatchFilePart::FileCreation { path, .. } => format!("create {}", path.as_str()),
        PatchFilePart::FileDeletion { path, .. } => format!("delete {}", path.as_str()),
        PatchFilePart::FilePatch { path, .. } => format!("patch {}", path.as_str()),
        PatchFilePart::FileRename { from, to, .. } => format!("rename {} -> {}", from.as_str(), to.as_str()),
        PatchFilePart::FileModeChange { path, .. } => format!("chmod {}", path.as_str()),
    }).collect::<Vec<_>>();

    assert_eq!(paths, vec![
        "create caf\u{e9}.js",
        "delete tab\there.js",
        "patch with \"quote\".js",
        "patch with space.js",
        "rename old \"name\".js -> caf\u{e9}.js",
    ]);
}
