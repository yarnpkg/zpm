use zpm_primitives::{CatalogRange, Ident, Range};

use crate::{
    error::Error,
    project::Project,
};

pub fn lookup_catalog_entry(project: &Project, params: &CatalogRange, ident: &Ident) -> Result<Range, Error> {
    let catalog_name
        = params.catalog.as_deref()
            .unwrap_or("default");

    let catalog
        = project.config.settings.catalogs
            .get(catalog_name)
            .ok_or_else(|| Error::CatalogNotFound(catalog_name.to_string()))?;

    catalog
        .get(ident)
        .map(|setting| setting.value.clone())
        .ok_or_else(|| Error::CatalogEntryNotFound(catalog_name.to_string(), ident.clone()))
}
