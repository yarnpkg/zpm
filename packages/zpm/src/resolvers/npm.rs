use std::{collections::BTreeMap, str::FromStr, sync::{Arc, LazyLock}};

use regex::Regex;
use serde::Deserialize;
use zpm_config::ConfigExt;
use zpm_primitives::{Descriptor, Ident, Locator, RegistryReference, RegistrySemverRange, RegistryTagRange, UrlReference};

use crate::{
    error::Error,
    install::{InstallContext, IntoResolutionResult, ResolutionResult},
    manifest::RemoteManifest,
    npm,
    resolvers::{workspace, Resolution},
};

static NODE_GYP_IDENT: LazyLock<Ident> = LazyLock::new(|| Ident::from_str("node-gyp").unwrap());
static NODE_GYP_MATCH: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\b(node-gyp|prebuild-install)\b").unwrap());

/**
 * We need to read the scripts to figure out whether the package has an implicit node-gyp dependency.
 */
#[derive(Clone, Deserialize, Debug)]
pub struct RemoteManifestWithScripts {
    #[serde(flatten)]
    remote: RemoteManifest,

    #[serde(default)]
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    scripts: BTreeMap<String, String>,
}

#[derive(Clone, Deserialize, Debug)]
pub struct NpmPayload {
    #[serde(rename = "dist-tags")]
    dist_tags: BTreeMap<String, zpm_semver::Version>,
    versions: BTreeMap<zpm_semver::Version, RemoteManifestWithScripts>,
}

fn fix_manifest(manifest: &mut RemoteManifestWithScripts) {
    // Manually add node-gyp dependency if there is a script using it and not already set
    // This is because the npm registry will automatically add a `node-gyp rebuild` install script
    // in the metadata if there is not already an install script and a binding.gyp file exists.
    // Also, node-gyp is not always set as a dependency in packages, so it will also be added if used in scripts.
    //
    if !manifest.remote.dependencies.contains_key(&NODE_GYP_IDENT) && !manifest.remote.peer_dependencies.contains_key(&NODE_GYP_IDENT) {
        for script in manifest.scripts.values() {
            if NODE_GYP_MATCH.is_match(script.as_str()) {
                manifest.remote.dependencies.insert(NODE_GYP_IDENT.clone(), Descriptor::new_semver(NODE_GYP_IDENT.clone(), "*").unwrap());
                break;
            }
        }
    }
}

fn build_resolution_result(context: &InstallContext, descriptor: &Descriptor, package_ident: &Ident, version: zpm_semver::Version, mut manifest: RemoteManifestWithScripts) -> ResolutionResult {
    let project = context.project
        .expect("The project is required for resolving a workspace package");

    fix_manifest(&mut manifest);

    let dist_manifest = manifest.remote.dist
        .as_ref()
        .expect("Expected the registry to return a 'dist' field amongst the manifest data");

    let registry_reference = RegistryReference {
        ident: package_ident.clone(),
        version,
    };

    let expected_registry_url
        = npm::registry_url_for_package_data(&project.config.registry_base_for(&registry_reference.ident), &registry_reference.ident, &registry_reference.version);

    let locator = descriptor.resolve_with(match expected_registry_url == dist_manifest.tarball {
        true => registry_reference.into(),
        false => UrlReference {url: dist_manifest.tarball.clone()}.into(),
    });

    Resolution::from_remote_manifest(locator, manifest.remote)
        .into_resolution_result(context)
}

pub async fn resolve_semver_or_workspace_descriptor(context: &InstallContext<'_>, descriptor: &Descriptor, params: &RegistrySemverRange) -> Result<ResolutionResult, Error> {
    let project = context.project
        .expect("The project is required for resolving a workspace package");

    if project.config.settings.enable_transparent_workspaces.value {
        if let Some(resolved) = workspace::resolve_ident(context, &descriptor.ident) {
            if params.range.check(&resolved.resolution.version) {
                return Ok(resolved);
            }
        }
    }

    resolve_semver_descriptor(context, descriptor, params).await
}

async fn process_registry_data<R>(context: &InstallContext<'_>, package_ident: &Ident, f: impl FnOnce(&NpmPayload) -> Result<R, Error>) -> Result<R, Error> {
    let npm_metadata_cache
        = context.npm_metadata_cache
            .expect("The npm metadata cache is required for resolving a semver descriptor");

    let cached_registry_data
        = npm_metadata_cache.get(package_ident);

    if let Some(cached_registry_data) = cached_registry_data {
        return Ok(f(&cached_registry_data.value())?);
    }

    let project = context.project
        .expect("The project is required for resolving a workspace package");

    let registry_url
        = npm::registry_url_for_all_versions(&project.config.registry_base_for(package_ident), package_ident);

    let response
        = project.http_client.get(&registry_url)?.send().await?;

    let registry_text = response.text().await
        .map_err(|err| Error::RemoteRegistryError(Arc::new(err)))?;
    let registry_data: NpmPayload
        = sonic_rs::from_str(registry_text.as_str())?;

    npm_metadata_cache.insert(package_ident.clone(), registry_data.clone());

    Ok(f(&registry_data)?)
}

pub async fn resolve_semver_descriptor(context: &InstallContext<'_>, descriptor: &Descriptor, params: &RegistrySemverRange) -> Result<ResolutionResult, Error> {
    let package_ident = params.ident.as_ref()
        .unwrap_or(&descriptor.ident);

    let (version, manifest) = process_registry_data(context, package_ident, |registry_data| {
        registry_data.versions.iter()
            .filter(|(version, _)| params.range.check(version))
            .max_by(|(version, _), (other_version, _)| version.cmp(other_version))
            .ok_or_else(|| Error::NoCandidatesFound(descriptor.range.clone()))
            .map(|(version, manifest)| (version.clone(), manifest.clone()))
    }).await?;


    Ok(build_resolution_result(context, descriptor, package_ident, version.clone(), manifest.clone()))
}

pub async fn resolve_tag_or_workspace_descriptor(context: &InstallContext<'_>, descriptor: &Descriptor, params: &RegistryTagRange) -> Result<ResolutionResult, Error> {
    let project = context.project
        .expect("The project is required for resolving a workspace package");

    if project.config.settings.enable_transparent_workspaces.value {
        if let Some(resolved) = workspace::resolve_ident(context, &descriptor.ident) {
            return Ok(resolved);
        }
    }

    resolve_tag_descriptor(context, descriptor, params).await
}

pub async fn resolve_tag_descriptor(context: &InstallContext<'_>, descriptor: &Descriptor, params: &RegistryTagRange) -> Result<ResolutionResult, Error> {
    let package_ident = params.ident.as_ref()
        .unwrap_or(&descriptor.ident);

    let (version, manifest) = process_registry_data(context, package_ident, |registry_data| {
        let version = registry_data.dist_tags
            .get(params.tag.as_str())
            .ok_or_else(|| Error::TagNotFound(params.tag.clone()))?;

        let manifest = registry_data.versions
            .get(version)
            .ok_or_else(|| Error::TagNotFound(params.tag.clone()))?;

        Ok((version.clone(), manifest.clone()))
    }).await?;

    Ok(build_resolution_result(context, descriptor, package_ident, version.clone(), manifest.clone()))
}

pub async fn resolve_locator(context: &InstallContext<'_>, locator: &Locator, params: &RegistryReference) -> Result<ResolutionResult, Error> {
    let project = context.project
        .expect("The project is required for resolving a workspace package");

    let registry_url
        = npm::registry_url_for_one_version(&project.config.registry_base_for(&params.ident), &params.ident, &params.version);

    let response
        = project.http_client.get(&registry_url)?.send().await?;

    let registry_text = response.text().await
        .map_err(|err| Error::RemoteRegistryError(Arc::new(err)))?;

    let mut manifest: RemoteManifestWithScripts
        = sonic_rs::from_str(registry_text.as_str())?;

    fix_manifest(&mut manifest);

    let resolution
        = Resolution::from_remote_manifest(locator.clone(), manifest.remote);

    Ok(resolution.into_resolution_result(context))
}
