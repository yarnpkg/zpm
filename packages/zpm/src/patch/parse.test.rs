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
