use itertools::Itertools;
use zpm_formats::{iter_ext::IterExt, Entry};
use zpm_primitives::Ident;
use zpm_semver::Version;
use zpm_utils::ToFileString;

pub trait NpmEntryExt<'a> {
    fn prepare_npm_entries(self, ident: &Ident) -> impl Iterator<Item = Entry<'a>>;
}

impl<'a, T> NpmEntryExt<'a> for T where T: Iterator<Item = Entry<'a>> {
    fn prepare_npm_entries(self, ident: &Ident) -> impl Iterator<Item = Entry<'a>> {
        self
            .into_iter()
            .sorted_by(|a, b| a.name.cmp(&b.name))
            .move_to_front(|entry| entry.name == "package.json")
            .prefix_path(&ident.nm_subdir())
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
