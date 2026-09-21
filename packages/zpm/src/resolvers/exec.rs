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
        = compute_exec_hash(params, &dependencies)?;

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

/**
 * Domain-separated hash of an `exec:` generator script; shared with the
 * up-to-date fast path, which re-derives it to detect changes.
 *
 * The script is normalized before being hashed: it generates the same
 * package whichever line endings it was checked out with, so it must not
 * produce two different locators.
 */
pub fn compute_exec_script_hash(script: &[u8]) -> zpm_utils::Hash64 {
    let mut writer
        = Hash64Writer::new();

    writer.update(b"exec-v2");
    writer.update(zpm_utils::normalize_line_endings(script));

    writer.finalize()
}

fn compute_exec_hash(params: &ExecRange, dependencies: &[InstallOpResult]) -> Result<zpm_utils::Hash64, Error> {
    let script_relative_path
        = Path::from_file_string(&params.path)?;

    let parent_context_directory = dependencies.first()
        .ok_or(Error::Unsupported)?
        .as_fetched()
        .package_data
        .context_directory()
        .clone();

    let script_path = if script_relative_path.is_absolute() {
        script_relative_path
    } else {
        parent_context_directory.with_join_str(&params.path)
    };

    Ok(compute_exec_script_hash(&script_path.fs_read()?))
}

pub async fn resolve_locator(context: &InstallContext<'_>, locator: &Locator, _params: &ExecReference, dependencies: Vec<InstallOpResult>) -> Result<ResolutionResult, Error> {
    let fetch_result
        = fetchers::fetch_locator(context.clone(), locator, false, dependencies).await?;

    fetch_result.into_resolution_result(context)
}
