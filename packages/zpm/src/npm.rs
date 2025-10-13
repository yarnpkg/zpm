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
            .sorted_by(|a, b| {
                let a_is_pkg = a.name.basename() == Some("package.json");
                let b_is_pkg = b.name.basename() == Some("package.json");

                match (a_is_pkg, b_is_pkg) {
                    // Both are package.json: sort by path length (shortest first)
                    (true, true) => a.name.as_str().len().cmp(&b.name.as_str().len()),
                    // Only a is package.json: a comes first
                    (true, false) => std::cmp::Ordering::Less,
                    // Only b is package.json: b comes first
                    (false, true) => std::cmp::Ordering::Greater,
                    // Neither is package.json: sort alphabetically
                    (false, false) => a.name.cmp(&b.name),
                }
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
