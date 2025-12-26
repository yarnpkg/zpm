use zpm_primitives::{BuiltinReference, Locator};

use crate::{builtins, error::Error, fetchers::PackageData, install::{FetchResult, InstallContext}};

pub async fn fetch_builtin_locator(context: &InstallContext<'_>, locator: &Locator, params: &BuiltinReference, is_mock_request: bool) -> Result<FetchResult, Error> {
    if locator.ident.as_str().starts_with("@builtin/node-") {
        return builtins::node::fetch_nodejs_locator(context, locator, &params.version, is_mock_request).await;
    }

    match locator.ident.as_str() {
        "@builtin/node"
            => Ok(FetchResult::new(PackageData::Abstract)),

        _ => Err(Error::Unsupported)?,
    }
}
