use std::collections::{BTreeMap, BTreeSet};

use zpm_primitives::{CatalogPeerRange, CatalogRange, Ident, Locator, PeerRange, Range};
use zpm_utils::ToFileString;

use crate::{
    error::Error,
    project::Project,
};

pub fn catalog_name(params: &CatalogRange) -> &str {
    params.catalog.as_deref()
        .unwrap_or("default")
}

fn lookup_catalog_value<'a, T>(catalogs: &'a BTreeMap<String, BTreeMap<Ident, T>>, params: &CatalogRange, ident: &Ident) -> Result<&'a T, Error> {
    let catalog_name
        = catalog_name(params);

    let catalog
        = catalogs
            .get(catalog_name)
            .ok_or_else(|| Error::CatalogNotFound(catalog_name.to_string()))?;

    catalog
        .get(ident)
        .ok_or_else(|| Error::CatalogEntryNotFound { catalog: catalog_name.to_string(), ident: ident.clone() })
}

pub fn lookup_catalog_entry(project: &Project, params: &CatalogRange, ident: &Ident) -> Result<Range, Error> {
    lookup_catalog_value(&project.config.settings.catalogs, params, ident)
        .map(|setting| setting.value.clone())
}

pub fn lookup_catalog_entry_in(catalogs: &BTreeMap<String, BTreeMap<Ident, Range>>, params: &CatalogRange, ident: &Ident) -> Result<Range, Error> {
    lookup_catalog_value(catalogs, params, ident)
        .cloned()
}

/**
 * Peer dependencies can reference catalogs too, in which case the peer
 * range is the range stored in the catalog for that package. Only semver
 * ranges (without aliases) can be used that way, as they're the only ones
 * peer dependencies are checked against. Each catalog we go through is
 * reported to `on_catalog_used`, as they need to be stored in the lockfile.
 */
pub fn lookup_catalog_peer_range_in(catalogs: &BTreeMap<String, BTreeMap<Ident, Range>>, params: &CatalogPeerRange, locator: &Locator, ident: &Ident, mut on_catalog_used: impl FnMut(&str)) -> Result<PeerRange, Error> {
    let mut catalog_range
        = CatalogRange {catalog: params.catalog.clone()};

    let mut expanded_catalogs
        = BTreeSet::new();

    loop {
        let catalog_name
            = catalog_name(&catalog_range).to_string();

        if !expanded_catalogs.insert(catalog_name.clone()) {
            return Err(Error::CatalogCycle {catalog: catalog_name, ident: ident.clone()});
        }

        let catalog = catalogs.get(&catalog_name)
            .ok_or_else(|| Error::PeerCatalogNotFound {locator: locator.clone(), ident: ident.clone(), catalog: catalog_name.clone()})?;

        let range = catalog.get(ident)
            .ok_or_else(|| Error::PeerCatalogEntryNotFound {locator: locator.clone(), ident: ident.clone(), catalog: catalog_name.clone()})?;

        on_catalog_used(&catalog_name);

        match range {
            Range::Catalog(params) => {
                catalog_range = params.clone();
            },

            // Aliases can't be expressed as peer ranges
            Range::RegistrySemver(params) if params.ident.is_some() => {
                return Err(Error::InvalidPeerCatalogEntry {locator: locator.clone(), ident: ident.clone(), catalog: catalog_name, range: range.to_file_string()});
            },

            _ => {
                return range.to_peer_range()
                    .map_err(|_| Error::InvalidPeerCatalogEntry {locator: locator.clone(), ident: ident.clone(), catalog: catalog_name, range: range.to_file_string()});
            },
        }
    }
}
