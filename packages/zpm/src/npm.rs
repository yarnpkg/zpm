use zpm_semver::Version;
use zpm_utils::ToFileString;

use crate::primitives::Ident;

pub fn registry_url_for_all_versions(registry_base: &str, ident: &Ident) -> String {
    let mut url = String::new();

    url.push_str(&registry_base);
    url.push('/');
    url.push_str(&ident.to_file_string());

    url
}

pub fn registry_url_for_one_version(registry_base: &str, ident: &Ident, version: &Version) -> String {
    let mut url = String::new();

    url.push_str(&registry_base);
    url.push('/');
    url.push_str(&ident.to_file_string());
    url.push('/');
    url.push_str(&version.to_file_string());

    url
}

pub fn registry_url_for_package_data(registry_base: &str, ident: &Ident, version: &Version) -> String {
    let mut url = String::new();

    url.push_str(&registry_base);
    url.push('/');
    url.push_str(&ident.to_file_string());
    url.push_str("/-/");
    url.push_str(&ident.name());
    url.push('-');
    url.push_str(&version.to_file_string());
    url.push_str(".tgz");

    url
}
