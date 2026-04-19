use std::{collections::BTreeMap, str::FromStr, sync::LazyLock};

use chrono::{DateTime, Utc};
use regex::Regex;
use serde::Deserialize;
use serde_with::{serde_as, MapSkipError};
use zpm_parsers::{JsonDocument, RawJsonValue};
use zpm_primitives::{AnonymousSemverRange, Descriptor, Ident, Locator, Reference, RegistryReference, RegistrySemverRange, RegistryTagRange};
use zpm_utils::UrlEncoded;

use crate::{
    error::Error,
    http_npm,
    install::{InstallContext, InstallOpResult, IntoResolutionResult, ResolutionResult},
    manifest::RemoteManifest,
    npm,
    resolvers::{Resolution, workspace},
};

static NODE_GYP_IDENT: LazyLock<Ident> = LazyLock::new(|| Ident::from_str("node-gyp").unwrap());
static NODE_GYP_MATCH: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\b(node-gyp|prebuild-install)\b").unwrap());

/**
 * We need to read the scripts to figure out whether the package has an implicit node-gyp dependency.
 */
#[derive(Clone, Deserialize, Debug)]
struct RemoteManifestWithScripts {
    #[serde(flatten)]
    remote: RemoteManifest,

    #[serde(default)]
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    scripts: BTreeMap<String, String>,
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

fn build_resolution_result(context: &InstallContext, descriptor: &Descriptor, package_ident: &Ident, version: zpm_semver::Version, mut manifest: RemoteManifestWithScripts) -> Result<ResolutionResult, Error> {
    let project = context.project
        .expect("The project is required for resolving a workspace package");

    fix_manifest(&mut manifest);

    let dist_manifest = manifest.remote.dist
        .as_ref()
        .expect("Expected the registry to return a 'dist' field amongst the manifest data");

    let registry_base
        = http_npm::get_registry(&project.config, package_ident.scope(), false)?;

    // Store the tarball URL only if it's non-conventional (can't be computed from registry + path)
    let url = if npm::is_conventional_tarball_url(&registry_base, &package_ident, &version, dist_manifest.tarball.clone()) {
        None
    } else {
        Some(UrlEncoded::new(dist_manifest.tarball.clone()))
    };

    let registry_reference = RegistryReference {
        ident: package_ident.clone(),
        version,
        url,
    };

    let locator
        = descriptor.resolve_with(registry_reference.into());

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

fn is_package_approved(context: &InstallContext<'_>, ident: &Ident, version: &zpm_semver::Version, release_time: Option<&DateTime<Utc>>) -> bool {
    let project = context.project
        .expect("The project is required for resolving a workspace package");

    let check_config
        = || project.config.settings.npm_preapproved_packages.iter().any(|setting| setting.value.check(ident, version));

    if let Some(minimal_age_gate) = project.config.settings.npm_minimal_age_gate.value {
        if release_time.map_or(false, |time| context.install_time < *time + minimal_age_gate) {
            return check_config();
        }
    }

    true
}

pub fn resolve_aliased(descriptor: &Descriptor, dependencies: Vec<InstallOpResult>) -> Result<ResolutionResult, Error> {
    // When the inner resolution returns Pinned (e.g., during lockfile migration),
    // the graph will add a Refresh dependency that produces Resolved.
    // We need to find the first Resolved result in the dependencies.
    let mut inner_resolution = dependencies.iter()
        .find_map(|dep| match dep {
            InstallOpResult::Resolved(res) => Some(res.clone()),
            _ => None,
        })
        .unwrap_or_else(|| panic!("Expected at least one Resolved result in dependencies for aliased package; got {:?}", dependencies));

    let inner_reference
        = inner_resolution.resolution.locator.reference.clone();

    let new_reference = match inner_reference {
        Reference::Shorthand(inner_params) => RegistryReference {
            ident: inner_resolution.resolution.locator.ident.clone(),
            version: inner_params.version.clone(),
            url: None,
        }.into(),

        Reference::Registry(inner_params) => RegistryReference {
            ident: inner_params.ident.clone(),
            version: inner_params.version.clone(),
            url: inner_params.url.clone(),
        }.into(),

        // For non-conventional tarball URLs, preserve the URL reference as-is
        // (kept for backwards compatibility with old lockfiles)
        Reference::Url(_) => inner_reference,

        _ => unreachable!("Unexpected reference type in resolve_aliased: {:?}", inner_reference),
    };

    inner_resolution.resolution.locator
        = Locator::new(descriptor.ident.clone(), new_reference);

    Ok(inner_resolution)
}

pub async fn resolve_semver_descriptor(context: &InstallContext<'_>, descriptor: &Descriptor, params: &RegistrySemverRange) -> Result<ResolutionResult, Error> {
    let project = context.project
        .expect("The project is required for resolving a workspace package");

    let package_ident = params.ident.as_ref()
        .unwrap_or(&descriptor.ident);

    let registry_base
        = http_npm::get_registry(&project.config, package_ident.scope(), false)?;
    let registry_path
        = npm::registry_url_for_all_versions(&package_ident);

    let authorization
        = http_npm::get_authorization(&http_npm::GetAuthorizationOptions {
            configuration: &project.config,
            http_client: &project.http_client,
            registry: &registry_base,
            ident: Some(package_ident),
            auth_mode: http_npm::AuthorizationMode::RespectConfiguration,
            allow_oidc: false,
        }).await?;

    let bytes
        = http_npm::get(&http_npm::NpmHttpParams {
            http_client: &project.http_client,
            registry: &registry_base,
            path: &registry_path,
            authorization: authorization.as_deref(),
            otp: None,
        }).await?;

    #[serde_as]
    #[derive(Deserialize)]
    struct RegistryMetadata<'a> {
        #[serde_as(as = "Option<MapSkipError<_, _>>")]
        time: Option<BTreeMap<zpm_semver::Version, DateTime<Utc>>>,
        #[serde(borrow)]
        versions: BTreeMap<zpm_semver::Version, RawJsonValue<'a>>,
    }

    let registry_data: RegistryMetadata
        = JsonDocument::hydrate_from_slice(&bytes[..])?;

    // Iterate in reverse order as we assume that users will most likely use newer versions.
    for (version, manifest) in registry_data.versions.iter().rev() {
        // Skip if the version is not in the range
        if !params.range.check(version) {
            continue;
        }

        // Skip if the version is more recent than the minimum age gate
        let time
            = project.config.settings.npm_minimal_age_gate.value
                .and_then(|_| registry_data.time.as_ref())
                .and_then(|map| map.get(version));

        if !is_package_approved(context, package_ident, version, time) {
            continue;
        }

        let manifest
            = JsonDocument::hydrate_from_value(manifest)?;

        return build_resolution_result(context, descriptor, package_ident, version.clone(), manifest);
    }

    Err(Error::NoCandidatesFound(descriptor.range.clone()))
}

pub async fn resolve_tag_descriptor(context: &InstallContext<'_>, descriptor: &Descriptor, params: &RegistryTagRange) -> Result<ResolutionResult, Error> {
    let project = context.project
        .expect("The project is required for resolving a workspace package");

    let package_ident = params.ident.as_ref()
        .unwrap_or(&descriptor.ident);

    let registry_base
        = http_npm::get_registry(&project.config, package_ident.scope(), false)?;
    let registry_path
        = npm::registry_url_for_all_versions(&package_ident);

    let authorization
        = http_npm::get_authorization(&http_npm::GetAuthorizationOptions {
            configuration: &project.config,
            http_client: &project.http_client,
            registry: &registry_base,
            ident: Some(package_ident),
            auth_mode: http_npm::AuthorizationMode::RespectConfiguration,
            allow_oidc: false,
        }).await?;

    let bytes
        = http_npm::get(&http_npm::NpmHttpParams {
            http_client: &project.http_client,
            registry: &registry_base,
            path: &registry_path,
            authorization: authorization.as_deref(),
            otp: None,
        }).await?;

    #[serde_as]
    #[derive(Deserialize)]
    struct RegistryMetadata<'a> {
        #[serde(rename(deserialize = "dist-tags"))]
        dist_tags: BTreeMap<String, zpm_semver::Version>,
        #[serde_as(as = "Option<MapSkipError<_, _>>")]
        time: Option<BTreeMap<zpm_semver::Version, DateTime<Utc>>>,
        #[serde(borrow)]
        versions: BTreeMap<zpm_semver::Version, RawJsonValue<'a>>,
    }

    // Added lifetime bound to fix 'lifetime may not live long enough'
    let registry_data: RegistryMetadata<'_>
        = JsonDocument::hydrate_from_slice(&bytes[..])?;

    let latest_version
        = registry_data.dist_tags
        .get(params.tag.as_str())
        .ok_or_else(|| Error::TagNotFound(params.tag.to_string()))?;

    let time
        = registry_data.time;

    let (version, manifest)
        = registry_data.versions.into_iter()
            .rev()
            .filter(|(version, _)| version <= &latest_version)
            .filter(|(version, _)| !version.rc.is_some() || latest_version.rc.is_some())
            .find(|(version, _)| is_package_approved(context, package_ident, version, time.as_ref().and_then(|map| map.get(version))))
            .ok_or_else(|| Error::NoCandidatesFound(AnonymousSemverRange {range: zpm_semver::Range::lte(latest_version.clone())}.into()))?;

    let manifest
        = JsonDocument::hydrate_from_value(&manifest)?;

    build_resolution_result(context, descriptor, package_ident, version, manifest)
}

pub async fn resolve_locator(context: &InstallContext<'_>, locator: &Locator, params: &RegistryReference) -> Result<ResolutionResult, Error> {
    let project = context.project
        .expect("The project is required for resolving a workspace package");

    let registry_base
        = http_npm::get_registry(&project.config, params.ident.scope(), false)?;
    let registry_path
        = npm::registry_url_for_one_version(&params.ident, &params.version);

    let authorization
        = http_npm::get_authorization(&http_npm::GetAuthorizationOptions {
            configuration: &project.config,
            http_client: &project.http_client,
            registry: &registry_base,
            ident: Some(&params.ident),
            auth_mode: http_npm::AuthorizationMode::RespectConfiguration,
            allow_oidc: false,
        }).await?;

    let bytes
        = http_npm::get(&http_npm::NpmHttpParams {
            http_client: &project.http_client,
            registry: &registry_base,
            path: &registry_path,
            authorization: authorization.as_deref(),
            otp: None,
        }).await?;

    let mut manifest: RemoteManifestWithScripts
        = JsonDocument::hydrate_from_slice(&bytes[..])?;

    fix_manifest(&mut manifest);

    let resolution
        = Resolution::from_remote_manifest(locator.clone(), manifest.remote.clone());

    resolution.into_resolution_result(context)
}
