use std::collections::BTreeSet;

use itertools::Itertools;
use zpm_primitives::{Descriptor, Ident, Locator, Reference, WorkspaceIdentRange, WorkspaceIdentReference, WorkspacePathRange, WorkspacePathReference};

use crate::{
    error::Error, install::{InstallContext, IntoResolutionResult, ResolutionResult}, manifest::Manifest, resolvers::Resolution
};

fn resolve_extends<'a>(settings: &'a zpm_config::Settings, mut extends_queue: Vec<&'a str>) -> Result<Vec<&'a str>, Error> {
    let mut extends_list
        = vec![];

    let mut extends_seen
        = BTreeSet::new();

    if settings.workspace_profiles.contains_key("default") {
        extends_queue.push("default");
    }

    while let Some(extend) = extends_queue.pop() {
        if extends_seen.insert(extend) {
            extends_list.push(extend);

            let profile
                = settings.workspace_profiles.get(extend)
                    .ok_or_else(|| Error::WorkspaceProfileNotFound(extend.to_string()))?;

            let followup_extends
                = profile.extends.iter()
                    .map(|s| s.value.as_str())
                    .chain(extends_queue.into_iter())
                    .collect_vec();

            extends_queue
                = followup_extends;
        }
    }

    Ok(extends_list)
}

fn resolve_workspace(context: &InstallContext<'_>, locator: Locator, mut manifest: Manifest) -> Result<ResolutionResult, Error> {
    let project = context.project
        .expect("The project is required for resolving a workspace package");

    let base_extends
        = manifest.extends.iter()
            .map(|s| s.as_str())
            .collect_vec();

    let extended_profiles
        = resolve_extends(&project.config.settings, base_extends)?;

    for profile_name in extended_profiles {
        if let Some(profile) = project.config.settings.workspace_profiles.get(profile_name) {
            for (ident, range) in &profile.dev_dependencies {
                if !manifest.dev_dependencies.contains_key(ident) {
                    manifest.dev_dependencies.insert(ident.clone(), Descriptor::new_bound(ident.clone(), range.value.clone(), None));
                }
            }
        }
    }

    let mut resolution
        = Resolution::from_remote_manifest(locator, manifest.remote);

    if !context.prune_dev_dependencies {
        resolution.dependencies.extend(manifest.dev_dependencies);
    }

    resolution.into_resolution_result(context)
}

pub fn resolve_name_descriptor(context: &InstallContext<'_>, descriptor: &Descriptor, params: &WorkspaceIdentRange) -> Result<ResolutionResult, Error> {
    let project = context.project
        .expect("The project is required for resolving a workspace package");

    let manifest
        = project.workspace_by_ident(&params.ident)?
            .manifest
            .clone();

    let reference = WorkspaceIdentReference {
        ident: params.ident.clone(),
    };

    let locator
        = descriptor.resolve_with(reference.into());

    resolve_workspace(context, locator, manifest)
}

pub fn resolve_path_descriptor(context: &InstallContext<'_>, descriptor: &Descriptor, params: &WorkspacePathRange) -> Result<ResolutionResult, Error> {
    let project = context.project
        .expect("The project is required for resolving a workspace package");

    let workspace
        = project.workspace_by_rel_path(&params.path)?;

    resolve_name_descriptor(context, descriptor, &WorkspaceIdentRange {ident: workspace.name.clone()})
}

pub fn resolve_ident(context: &InstallContext<'_>, ident: &Ident) -> Option<ResolutionResult> {
    let project = context.project
        .expect("The project is required for resolving a workspace package");

    let Ok(workspace) = project.workspace_by_ident(&ident) else {
        return None;
    };

    let locator = Locator::new(ident.clone(), WorkspaceIdentReference {
        ident: workspace.name.clone(),
    }.into());

    let Reference::WorkspaceIdent(reference) = &locator.reference else {
        panic!("Expected the locator to be a workspace ident");
    };

    let resolved
        = resolve_locator_ident(context, &locator, reference)
            .expect("Expected the locator to be resolved");

    Some(resolved)
}

pub fn resolve_locator_ident(context: &InstallContext<'_>, locator: &Locator, params: &WorkspaceIdentReference) -> Result<ResolutionResult, Error> {
    let project = context.project
        .expect("The project is required for resolving a workspace package");

    let manifest = project
        .workspace_by_ident(&params.ident)?
        .manifest
        .clone();

    resolve_workspace(context, locator.clone(), manifest)
}

pub fn resolve_locator_path(context: &InstallContext<'_>, locator: &Locator, params: &WorkspacePathReference) -> Result<ResolutionResult, Error> {
    let project = context.project
        .expect("The project is required for resolving a workspace package");

    let manifest = project
        .workspace_by_rel_path(&params.path)?
        .manifest
        .clone();

    resolve_workspace(context, locator.clone(), manifest)
}
