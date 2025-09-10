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

#[test]
fn simple_case() {
    let entries = vec![Entry {
        name: "index.ts".to_string(),
        mode: 0o644,
        crc: 0,
        data: Cow::Owned("this\nis\na\nfile\n".as_bytes().to_vec()),
        compression: None,
    }];

    let res
        = apply_patch(entries, PATCH, &zpm_semver::Version::new()).unwrap();

    assert_eq!(res, vec![Entry {
        name: "index.ts".to_string(),
        mode: 0o644,
        crc: 0,
        data: Cow::Owned("this\nis\nmy\nfile\n".as_bytes().to_vec()),
        compression: None,
    }]);
}
