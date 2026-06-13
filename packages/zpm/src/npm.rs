use std::{collections::BTreeSet, sync::LazyLock};

use itertools::Itertools;
use regex::Regex;
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
}
