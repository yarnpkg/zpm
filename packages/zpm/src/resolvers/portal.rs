use zpm_formats::zip::ZipSupport;
use zpm_primitives::{Descriptor, Locator, PortalRange, PortalReference, Reference};
use zpm_utils::{Hash64, Path};

use crate::{
    error::Error,
    install::{InstallContext, InstallOpResult, IntoResolutionResult, ResolutionResult},
    manifest::helpers::parse_manifest,
    resolvers::Resolution,
};

/**
 * Recursively sorts the keys of every object, so that two manifests that
 * only differ by the order in which their fields were written produce the
 * same bytes. `serde_json` is built with `preserve_order`, so the map we
 * collect into keeps the order we insert in.
 */
fn canonicalize_json(value: &serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Object(fields) => {
            let mut fields: Vec<_> = fields.iter()
                .map(|(key, value)| (key.clone(), canonicalize_json(value)))
                .collect();

            fields.sort_by(|left, right| left.0.cmp(&right.0));

            serde_json::Value::Object(fields.into_iter().collect())
        },

        serde_json::Value::Array(items) => {
            serde_json::Value::Array(items.iter().map(canonicalize_json).collect())
        },

        value => value.clone(),
    }
}

/**
 * Domain-separated hash of the portal target's manifest; shared with
 * the up-to-date fast path, which re-derives it to detect changes.
 *
 * The hash goes into the locator, hence into the lockfile, so it must only
 * depend on what the manifest *says*: we hash a canonical form of the
 * parsed document rather than its bytes, which keeps reindentation, field
 * reordering and line-ending conventions from rewriting `yarn.lock`. Any
 * actual field change still changes the hash (the install caches the
 * package's `ContentFlags` per locator, so `bin` and `scripts` edits have
 * to produce a new one).
 */
pub fn compute_portal_manifest_hash(manifest_text: &str) -> Hash64 {
    let canonical = serde_json::from_str::<serde_json::Value>(manifest_text).ok()
        .and_then(|value| serde_json::to_vec(&canonicalize_json(&value)).ok());

    let mut writer = zpm_utils::Hash64Writer::new();
    writer.update(b"portal-manifest-v2");

    match canonical {
        Some(canonical) => writer.update(canonical),
        // A manifest we can't parse still has to be watched for changes
        None => writer.update(zpm_utils::normalize_line_endings(manifest_text.as_bytes())),
    }

    writer.finalize()
}

fn portal_manifest_path(context_directory: &Path, portal_path: &str) -> Path {
    context_directory
        .with_join_str(portal_path)
        .with_join_str("package.json")
}

pub fn resolve_descriptor(ctx: &InstallContext, descriptor: &Descriptor, params: &PortalRange, dependencies: Vec<InstallOpResult>) -> Result<ResolutionResult, Error> {
    let parent_data
        = dependencies[0].as_fetched();

    let manifest_text = portal_manifest_path(parent_data.package_data.context_directory(), &params.path)
        .fs_read_text_with_zip()?;

    let reference = PortalReference {
        path: params.path.clone(),
        hash: Some(compute_portal_manifest_hash(&manifest_text)),
    };

    let locator
        = descriptor.resolve_with(reference.into());

    let Reference::Portal(params) = &locator.reference else {
        unreachable!()
    };

    resolve_locator(ctx, &locator, params, dependencies)
}

pub fn resolve_locator(context: &InstallContext, locator: &Locator, params: &PortalReference, dependencies: Vec<InstallOpResult>) -> Result<ResolutionResult, Error> {
    let parent_data
        = dependencies[0].as_fetched();

    let manifest_text = portal_manifest_path(parent_data.package_data.context_directory(), &params.path)
        .fs_read_text_with_zip()?;

    let manifest
        = parse_manifest(&manifest_text)?;

    let resolution
        = Resolution::from_remote_manifest(locator.clone(), manifest.remote);

    resolution.into_resolution_result(context)
}

#[cfg(test)]
mod tests {
    use super::*;

    const MANIFEST: &str = r#"{"name": "portal", "version": "1.0.0", "dependencies": {"a": "1.0.0", "b": "2.0.0"}}"#;

    #[test]
    fn portal_manifest_hash_ignores_formatting() {
        let reference
            = compute_portal_manifest_hash(MANIFEST);

        // Reindented, reordered, and checked out with CRLF line endings
        let reformatted = "{\r\n  \"version\": \"1.0.0\",\r\n  \"dependencies\": {\r\n    \"b\": \"2.0.0\",\r\n    \"a\": \"1.0.0\"\r\n  },\r\n  \"name\": \"portal\"\r\n}\r\n";

        assert_eq!(compute_portal_manifest_hash(reformatted), reference);
    }

    #[test]
    fn portal_manifest_hash_follows_the_content() {
        let reference
            = compute_portal_manifest_hash(MANIFEST);

        for changed in [
            r#"{"name": "portal", "version": "1.0.1", "dependencies": {"a": "1.0.0", "b": "2.0.0"}}"#,
            r#"{"name": "portal", "version": "1.0.0", "dependencies": {"a": "1.0.1", "b": "2.0.0"}}"#,
            r#"{"name": "portal", "version": "1.0.0", "dependencies": {"a": "1.0.0"}}"#,
            r#"{"name": "portal", "version": "1.0.0", "scripts": {"postinstall": "true"}, "dependencies": {"a": "1.0.0", "b": "2.0.0"}}"#,
        ] {
            assert_ne!(compute_portal_manifest_hash(changed), reference, "{changed}");
        }
    }

    #[test]
    fn portal_manifest_hash_falls_back_to_the_raw_text() {
        // Not valid JSON; the resolver still has to notice changes
        let reference
            = compute_portal_manifest_hash("not json\r\n");

        assert_eq!(compute_portal_manifest_hash("not json\n"), reference);
        assert_ne!(compute_portal_manifest_hash("not json either\n"), reference);
    }
}
