use crate::{error::Error, fetchers, git, install::{InstallContext, IntoResolutionResult, ResolutionResult}, primitives::{range::GitRange, reference, Descriptor, Locator}};

pub async fn resolve_descriptor(context: &InstallContext<'_>, descriptor: &Descriptor, params: &GitRange) -> Result<ResolutionResult, Error> {
    let commit = git::resolve_git_treeish(&params.git).await?;

    let git_reference = git::GitReference {
        repo: params.git.repo.clone(),
        commit: commit.clone(),
        prepare_params: params.git.prepare_params.clone(),
    };

    let locator = Locator::new(descriptor.ident.clone(), reference::GitReference {
        git: git_reference,
    }.into());

    let fetch_result
        = fetchers::fetch(context.clone(), &locator, false, vec![]).await?;

    Ok(fetch_result.into_resolution_result(context))
}
