pub mod audit;
pub mod info;
pub mod login;
pub mod logout_all;
pub mod logout;
pub mod publish;
pub mod stage;
pub mod whoami;

use zpm_parsers::DataDocument;

use crate::{
    error::Error,
    http_npm,
};

/// Returns the `npmRegistries` keys of the given rc document that apply to
/// the registry, best match first (see `http_npm::matching_registry_keys`).
/// Used to edit an existing entry in place, whatever its spelling, rather
/// than adding a second one for the same registry.
fn matching_rc_registry_keys(document: &str, registry: &str) -> Result<Vec<String>, Error> {
    let document
        = DataDocument::new(document.as_bytes().to_vec())?;

    let mut keys
        = document.paths().keys()
            .filter(|path| path.len() >= 2 && path[0] == "npmRegistries")
            .map(|path| path[1].as_str())
            .collect::<Vec<_>>();

    keys.dedup();

    Ok(http_npm::matching_registry_keys(keys, registry).into_iter().map(|key| key.to_string()).collect())
}
