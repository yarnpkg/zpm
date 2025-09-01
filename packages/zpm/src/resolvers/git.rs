use zpm_primitives::{Descriptor, GitRange, GitReference, Locator};

use crate::{
    error::Error,
    fetchers,
    git,
    install::{InstallContext, IntoResolutionResult, ResolutionResult},
};

pub async fn resolve_descriptor(context: &InstallContext<'_>, descriptor: &Descriptor, params: &GitRange) -> Result<ResolutionResult, Error> {
    let project = context.project
        .expect("The project is required for resolving a git package");

    let commit
        = git::resolve_git_treeish(&params.git, &project.http_client.config).await?;

    let git_reference = zpm_git::GitReference {
        repo: params.git.repo.clone(),
        commit: commit.clone(),
        prepare_params: params.git.prepare_params.clone(),
    };

    let locator = Locator::new(descriptor.ident.clone(), GitReference {
        git: git_reference,
    }.into());

    let fetch_result
        = fetchers::fetch_locator(context.clone(), &locator, false, vec![]).await?;

    Ok(fetch_result.into_resolution_result(context))
}

pub async fn resolve_locator(context: &InstallContext<'_>, locator: &Locator, _params: &GitReference) -> Result<ResolutionResult, Error> {
    let fetch_result
        = fetchers::fetch_locator(context.clone(), locator, false, vec![]).await?;

    Ok(fetch_result.into_resolution_result(context))
}
