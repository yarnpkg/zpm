use crate::{error::Error, fetchers, install::{InstallContext, InstallOpResult, IntoResolutionResult, ResolutionResult}, primitives::{range, reference, Descriptor}};

pub async fn resolve_descriptor(context: &InstallContext<'_>, descriptor: &Descriptor, range: &range::FolderRange, dependencies: Vec<InstallOpResult>) -> Result<ResolutionResult, Error> {
    let locator = descriptor.resolve_with(reference::FolderReference {
        path: range.path.to_string(),
    }.into());

    let fetch_result
        = fetchers::fetch(context.clone(), &locator, false, dependencies).await?;

    Ok(fetch_result.into_resolution_result(context))
}
