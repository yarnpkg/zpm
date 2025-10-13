use itertools::Itertools;
use zpm_formats::{iter_ext::IterExt, Entry};
use zpm_primitives::Ident;
use zpm_semver::Version;
use zpm_utils::{Path, ToFileString};

pub trait NpmEntryExt<'a> {
    fn prepare_npm_entries(self, subdir: &Path) -> impl Iterator<Item = Entry<'a>>;
}

impl<'a, T> NpmEntryExt<'a> for T where T: Iterator<Item = Entry<'a>> {
    fn prepare_npm_entries(self, subdir: &Path) -> impl Iterator<Item = Entry<'a>> {
        self
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
    }
}

pub fn registry_url_for_all_versions(registry_base: &str, ident: &Ident) -> String {
    let mut url = String::new();

    url.push_str(&registry_base);
    url.push('/');
    url.push_str(&ident.to_file_string());

    url
}

pub fn registry_url_for_one_version(ident: &Ident, version: &Version) -> String {
    let mut url = String::new();

    url.push('/');
    url.push_str(&ident.to_file_string());
    url.push('/');
    url.push_str(&version.to_file_string());

    url
}

pub fn registry_url_for_package_data(ident: &Ident, version: &Version) -> String {
    let mut url = String::new();

    url.push('/');
    url.push_str(&ident.to_file_string());
    url.push_str("/-/");
    url.push_str(&ident.name());
    url.push('-');
    url.push_str(&version.to_file_string());
    url.push_str(".tgz");

    url
}

#[cfg(test)]
mod tests {
    use zpm_formats::Entry;
    use zpm_utils::Path;

    use crate::npm::NpmEntryExt;

    #[test]
    pub fn should_sort_npm_entries() {
        let entries = vec![
            Entry::new(Path::try_from("b").unwrap()),
            Entry::new(Path::try_from("a/b/c").unwrap()),
            Entry::new(Path::try_from("a/package.json").unwrap()),
            Entry::new(Path::try_from("package.json").unwrap()),
            Entry::new(Path::try_from("a/b/package.json").unwrap()),
        ];

        let prepared_entries
            = entries.into_iter()
                .prepare_npm_entries(&Path::try_from("foo").unwrap())
                .collect::<Vec<_>>();

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
}
