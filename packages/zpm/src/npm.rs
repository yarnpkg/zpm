use std::{collections::{BTreeMap, BTreeSet}, fmt, marker::PhantomData, sync::LazyLock};

use itertools::Itertools;
use regex::Regex;
use serde::{de::{Error as _, IgnoredAny, MapAccess, Visitor}, Deserialize, Deserializer};
use serde_with::DeserializeAs;
use zpm_formats::{iter_ext::IterExt, Entry};
use zpm_parsers::JsonDocument;
use zpm_primitives::Ident;
use zpm_semver::Version;
use zpm_utils::{Path, ToFileString};

use crate::{error::Error, manifest::BinManifest};

pub trait NpmEntryExt<'a> {
    fn prepare_npm_entries(self, subdir: &Path) -> Result<Vec<Entry<'a>>, Error>;
}

impl<'a, T> NpmEntryExt<'a> for T where T: Iterator<Item = Entry<'a>> {
    fn prepare_npm_entries(self, subdir: &Path) -> Result<Vec<Entry<'a>>, Error> {
        let mut entries
            = self.collect::<Vec<_>>();

        mark_bin_entries_executable(&mut entries)?;

        Ok(entries
            .into_iter()

            // We first sort by file name; we do this first because we
            // can't return references from `sorted_by_cached_key`
            .sorted_by(|a, b| {
                a.name.cmp(&b.name)
            })

            // Now that we've sorted by name, we perform a second sort to
            // list values that are near the root first, and package.json
            // files as well. Since `sorted_by_cached_key` is a stable sort
            // we don't lose the by-name order for other entries.
            .sorted_by_cached_key(|entry| {
                let segment_count
                    = entry.name.as_str().chars()
                        .filter(|&c| c == '/')
                        .count();

                let is_package_json
                    = entry.name.basename() == Some("package.json");

                (segment_count, !is_package_json)
            })

            .prefix_path(subdir)
            .collect::<Vec<_>>())
    }
}

fn mark_bin_entries_executable(entries: &mut [Entry<'_>]) -> Result<(), Error> {
    let Some(manifest_entry) = entries.iter().find(|entry| entry.name.as_str() == "package.json") else {
        return Ok(());
    };

    let manifest
        = JsonDocument::hydrate_from_slice::<BinManifest>(&manifest_entry.data)?;

    let Some(bin) = manifest.bin else {
        return Ok(());
    };

    let bin_paths
        = bin.paths().cloned().collect::<BTreeSet<_>>();

    for entry in entries {
        if bin_paths.contains(&entry.name) {
            entry.mode = 0o755;
        }
    }

    Ok(())
}

static NPM_REGISTRY_URL_REGEX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^https?:(\/\/(?:[^/]+\.)?npmjs.org(?:$|\/))").unwrap()
});

pub fn is_conventional_tarball_url(registry: &str, ident: &Ident, version: &zpm_semver::Version, mut url: String) -> bool {
    // From time to time the npm registry returns http urls instead of https 🤡
    url = NPM_REGISTRY_URL_REGEX.replace(&url, "https:$1").to_string();

    let path
        = registry_url_for_package_data(ident, version);

    if url == format!("{}{}", registry, path) {
       return true;
    }

    let path_with_slash
        = path.replace("%2f", "/");

    if url == format!("{}{}", registry, path_with_slash) {
        return true;
    }

    false
}

pub fn registry_url_for_all_versions(ident: &Ident) -> String {
    let mut url = String::new();

    url.push('/');

    let (scope, name)
        = ident.split();

    if let Some(scope) = scope {
        url.push_str(scope);
        url.push_str("%2f");
    }

    url.push_str(name);

    url
}

pub fn registry_url_for_one_version(ident: &Ident, version: &Version) -> String {
    let mut url
        = registry_url_for_all_versions(ident);

    url.push('/');
    url.push_str(&version.to_file_string());

    url
}

pub fn registry_url_for_package_data(ident: &Ident, version: &Version) -> String {
    let mut url
        = registry_url_for_all_versions(ident);

    url.push_str("/-/");
    url.push_str(&ident.name());
    url.push('-');
    url.push_str(&version.to_file_string());
    url.push_str(".tgz");

    url
}

/**
 * Registries still serve versions published before npm enforced strict semver
 * (`1.0`, `1.2.3.4`, `01.02.03`, ...). Like Yarn 4 (and npm), we ignore them
 * rather than let a single one of them prevent the whole package from being
 * resolved. This visitor parses a version key of a packument map and yields
 * `None` for those we must ignore, without allocating.
 */
struct PackumentVersionVisitor;

impl<'de> Visitor<'de> for PackumentVersionVisitor {
    type Value = Option<Version>;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("a version")
    }

    fn visit_str<E: serde::de::Error>(self, value: &str) -> Result<Self::Value, E> {
        Ok(Version::from_strict_str(value))
    }
}

struct PackumentVersionKey(Option<Version>);

impl<'de> Deserialize<'de> for PackumentVersionKey {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_str(PackumentVersionVisitor).map(PackumentVersionKey)
    }
}

/**
 * `serde_with` adapter that only accepts the version keys we don't ignore
 * (see `PackumentVersionVisitor`); meant to be combined with `MapSkipError`
 * for maps (such as `time`) that also contain non-version keys.
 */
pub struct PackumentVersion;

impl<'de> DeserializeAs<'de, Version> for PackumentVersion {
    fn deserialize_as<D: Deserializer<'de>>(deserializer: D) -> Result<Version, D::Error> {
        deserializer.deserialize_str(PackumentVersionVisitor)?
            .ok_or_else(|| D::Error::custom("invalid semver version"))
    }
}

/**
 * Deserializes the `versions` field of a packument, skipping the entries whose
 * key isn't a valid version (see `PackumentVersionVisitor`). Their values are
 * skipped without being parsed, and the other values are deserialized in
 * place, so it works with borrowed values such as `RawJsonValue`.
 */
pub fn deserialize_packument_versions<'de, D, V>(deserializer: D) -> Result<BTreeMap<Version, V>, D::Error>
where
    D: Deserializer<'de>,
    V: Deserialize<'de>,
{
    struct VersionsVisitor<V>(PhantomData<V>);

    impl<'de, V: Deserialize<'de>> Visitor<'de> for VersionsVisitor<V> {
        type Value = BTreeMap<Version, V>;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("a map of versions")
        }

        fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
            let mut versions
                = BTreeMap::new();

            while let Some(PackumentVersionKey(version)) = map.next_key()? {
                match version {
                    Some(version) => {
                        versions.insert(version, map.next_value()?);
                    },

                    None => {
                        map.next_value::<IgnoredAny>()?;
                    },
                }
            }

            Ok(versions)
        }
    }

    deserializer.deserialize_map(VersionsVisitor(PhantomData))
}

/**
 * Returns the version a dist-tag points to. Dist-tags are kept as raw strings
 * so that a tag pointing to an ignored version only fails when requested.
 */
pub fn get_dist_tag_version(ident: &Ident, dist_tags: &BTreeMap<String, String>, tag: &str) -> Result<Version, Error> {
    let version
        = dist_tags.get(tag)
            .ok_or_else(|| Error::TagNotFound(tag.to_string()))?;

    Version::from_strict_str(version)
        .ok_or_else(|| Error::InvalidDistTagVersion(ident.clone(), tag.to_string(), version.clone()))
}

/**
 * Returns an error explaining that the packument lists `version` but that we
 * ignore it, if that's the case. It parses the packument again, so it's only
 * meant to improve the error reported when a resolution fails.
 */
pub fn find_ignored_version_error(ident: &Ident, packument: &[u8], version: &str) -> Option<Error> {
    #[derive(Deserialize)]
    struct RegistryMetadata {
        versions: BTreeMap<String, IgnoredAny>,
    }

    let registry_data: RegistryMetadata
        = JsonDocument::hydrate_from_slice(packument).ok()?;

    let is_ignored
        = registry_data.versions.contains_key(version)
            && Version::from_strict_str(version).is_none();

    is_ignored.then(|| Error::InvalidRegistryVersion(ident.clone(), version.to_string()))
}

#[cfg(test)]
mod tests {
    use std::borrow::Cow;

    use zpm_formats::Entry;
    use zpm_utils::Path;

    use crate::npm::NpmEntryExt;

    #[test]
    pub fn should_sort_npm_entries() {
        let entries = vec![
            Entry::new(Path::try_from("b").unwrap()),
            Entry::new(Path::try_from("a/b/c").unwrap()),
            Entry::new(Path::try_from("a/package.json").unwrap()),
            Entry::new_file(Path::try_from("package.json").unwrap(), Cow::Borrowed(br#"{}"#)),
            Entry::new(Path::try_from("a/b/package.json").unwrap()),
        ];

        let prepared_entries
            = entries.into_iter()
                .prepare_npm_entries(&Path::try_from("foo").unwrap())
                .unwrap();

        let prepared_names = prepared_entries.iter()
            .map(|entry| entry.name.as_str())
            .collect::<Vec<_>>();

        assert_eq!(prepared_names, vec![
            "foo/package.json",
            "foo/b",
            "foo/a/package.json",
            "foo/a/b/package.json",
            "foo/a/b/c",
        ]);
    }

    #[test]
    pub fn should_mark_manifest_bins_executable() {
        let entries = vec![
            Entry::new_file(
                Path::try_from("package.json").unwrap(),
                Cow::Borrowed(br#"{"bin":"./bin.js"}"#),
            ),
            Entry::new_file(Path::try_from("bin.js").unwrap(), Cow::Borrowed(b"")),
            Entry::new_file(Path::try_from("index.js").unwrap(), Cow::Borrowed(b"")),
        ];

        let prepared_entries
            = entries.into_iter()
                .prepare_npm_entries(&Path::try_from("foo").unwrap())
                .unwrap();

        let bin_entry = prepared_entries.iter()
            .find(|entry| entry.name.as_str() == "foo/bin.js")
            .unwrap();
        let regular_entry = prepared_entries.iter()
            .find(|entry| entry.name.as_str() == "foo/index.js")
            .unwrap();

        assert_eq!(bin_entry.mode, 0o755);
        assert_eq!(regular_entry.mode, 0o644);
    }

    #[derive(serde::Deserialize)]
    struct Packument<'a> {
        #[serde(borrow, deserialize_with = "crate::npm::deserialize_packument_versions")]
        versions: std::collections::BTreeMap<zpm_semver::Version, zpm_parsers::RawJsonValue<'a>>,
    }

    fn version(version: &str) -> zpm_semver::Version {
        zpm_semver::Version::from_strict_str(version).unwrap()
    }

    #[test]
    pub fn should_keep_valid_packument_versions() {
        let packument
            = br#"{"versions": {"2.0.0": {"name": "foo", "version": "2.0.0"}, "1.0.0": {"name": "foo", "version": "1.0.0", "dist": {"tarball": "a\"b"}}, "1.0.0-rc.1": [1, {"a": null}]}}"#;

        let data: Packument<'_>
            = zpm_parsers::JsonDocument::hydrate_from_slice(packument).unwrap();

        let entries = data.versions.iter()
            .map(|(version, value)| (version.clone(), zpm_parsers::JsonDocument::hydrate_from_value::<serde_json::Value>(value).unwrap()))
            .collect::<Vec<_>>();

        assert_eq!(entries, vec![
            (version("1.0.0-rc.1"), serde_json::json!([1, {"a": null}])),
            (version("1.0.0"), serde_json::json!({"name": "foo", "version": "1.0.0", "dist": {"tarball": "a\"b"}})),
            (version("2.0.0"), serde_json::json!({"name": "foo", "version": "2.0.0"})),
        ]);
    }

    #[test]
    pub fn should_skip_invalid_packument_versions() {
        let packument
            = br#"{"versions": {"1.0": {"version": "1.0"}, "1.0.0": {"version": "1.0.0"}, "1.2.3.4": {"deep": {"a": [1, 2, {"b": "}"}]}}, "01.0.0": 42, "9007199254740992.0.0": "x", "2.0.0-01": null, "\u0032.0.0": {"version": "2.0.0"}, "3.0.0": {"version": "3.0.0"}}}"#;

        let data: Packument<'_>
            = zpm_parsers::JsonDocument::hydrate_from_slice(packument).unwrap();

        let entries = data.versions.iter()
            .map(|(version, value)| (version.clone(), zpm_parsers::JsonDocument::hydrate_from_value::<serde_json::Value>(value).unwrap()))
            .collect::<Vec<_>>();

        assert_eq!(entries, vec![
            (version("1.0.0"), serde_json::json!({"version": "1.0.0"})),
            (version("2.0.0"), serde_json::json!({"version": "2.0.0"})),
            (version("3.0.0"), serde_json::json!({"version": "3.0.0"})),
        ]);
    }

    #[test]
    pub fn should_skip_invalid_packument_times() {
        #[serde_with::serde_as]
        #[derive(serde::Deserialize)]
        struct Times {
            #[serde_as(as = "serde_with::MapSkipError<crate::npm::PackumentVersion, _>")]
            time: std::collections::BTreeMap<zpm_semver::Version, String>,
        }

        let data: Times
            = zpm_parsers::JsonDocument::hydrate_from_slice(br#"{"time": {"created": "a", "1.0": "b", "1.0.0": "c", "2.0.0": 42}}"#).unwrap();

        assert_eq!(data.time.into_iter().collect::<Vec<_>>(), vec![
            (version("1.0.0"), "c".to_string()),
        ]);
    }
}
