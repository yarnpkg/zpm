use std::collections::BTreeMap;

use zpm_primitives::{CatalogRange, Ident, Range};

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
