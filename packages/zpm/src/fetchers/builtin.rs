use zpm_primitives::{BuiltinReference, Locator};

use crate::{builtins, error::Error, fetchers::PackageData, install::{FetchResult, InstallContext}};

pub async fn fetch_builtin_locator(context: &InstallContext<'_>, locator: &Locator, params: &BuiltinReference, is_mock_request: bool) -> Result<FetchResult, Error> {
    if locator.ident.as_str().starts_with("@yarnpkg/node-") {
        return builtins::node::fetch_nodejs_locator(context, locator, &params.version, is_mock_request).await;
    }

    if builtins::python::is_python_variant_ident(&locator.ident) {
        return builtins::python::fetch_python_locator(context, locator, &params.version, is_mock_request).await;
    }

    match locator.ident.as_str() {
        "@yarnpkg/node"
            => Ok(FetchResult::new(PackageData::Abstract)),

        builtins::python::PYTHON_IDENT
            => Ok(FetchResult::new(PackageData::Abstract)),

        _ => Err(Error::Unsupported)?,
    }
}
