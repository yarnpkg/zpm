use zpm_primitives::{Descriptor, ExecRange, ExecReference, Locator};
use zpm_utils::{FromFileString, Hash64Writer, Path};

use crate::{
    error::Error,
    fetchers,
    install::{InstallContext, InstallOpResult, IntoResolutionResult, ResolutionResult},
};

pub async fn resolve_descriptor(context: &InstallContext<'_>, descriptor: &Descriptor, params: &ExecRange, dependencies: Vec<InstallOpResult>) -> Result<ResolutionResult, Error> {
    validate_workspace_parent(context, descriptor)?;

    let hash
        = compute_exec_hash(context, params, &dependencies)?;

    let locator = descriptor.resolve_with(ExecReference {
        path: params.path.clone(),
        hash: Some(hash),
    }.into());

    let fetch_result
        = fetchers::fetch_locator(context.clone(), &locator, false, dependencies).await?;

    fetch_result.into_resolution_result(context)
}

fn validate_workspace_parent(context: &InstallContext<'_>, descriptor: &Descriptor) -> Result<(), Error> {
    let project = context.project
        .expect("The project is required for resolving exec packages");

    let Some(parent) = &descriptor.parent else {
        return Err(Error::Unsupported);
    };

    if project.try_workspace_by_locator(&parent.physical_locator())?.is_none() {
        return Err(Error::ExecDependencyFromNonWorkspace {
            parent: parent.physical_locator(),
            descriptor: descriptor.clone(),
        });
    }

    Ok(())
}

fn compute_exec_hash(context: &InstallContext<'_>, params: &ExecRange, dependencies: &[InstallOpResult]) -> Result<zpm_utils::Hash64, Error> {
    let script_relative_path
        = Path::from_file_string(&params.path)?;

    let parent_context_directory = dependencies.first()
        .ok_or(Error::Unsupported)?
        .as_fetched()
        .package_data
        .context_directory()
        .clone();

    let script_path = if script_relative_path.is_absolute() {
        context.absolute_source_path(&script_relative_path)?
    } else {
        context.relative_source_path(&parent_context_directory, &params.path)?
    };

    let mut writer
        = Hash64Writer::new();
    writer.update(b"exec-v2");
    writer.update(script_path.fs_read()?);

    Ok(writer.finalize())
}

pub async fn resolve_locator(context: &InstallContext<'_>, locator: &Locator, _params: &ExecReference, dependencies: Vec<InstallOpResult>) -> Result<ResolutionResult, Error> {
    let fetch_result
        = fetchers::fetch_locator(context.clone(), locator, false, dependencies).await?;

    fetch_result.into_resolution_result(context)
}
