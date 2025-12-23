use itertools::Itertools;
use serde::Deserialize;
use zpm_parsers::JsonDocument;
use zpm_primitives::{BuiltinRange, BuiltinReference, Descriptor, Ident, Locator};
use zpm_utils::{ToFileString};

use crate::{
    error::Error, fetchers::PackageData, install::{InstallContext, IntoResolutionResult, ResolutionResult}, resolvers::{Resolution, Variant}
};

async fn resolve_nodejs_descriptor(context: &InstallContext<'_>, descriptor: &Descriptor, params: &BuiltinRange) -> Result<ResolutionResult, Error> {
    let project = context.project
        .expect("The project is required for resolving a workspace package");

    let release_url
        = format!("https://nodejs.org/dist/index.json");

    let text
        = project.http_client.get(&release_url)?.send().await?.text().await?;

    #[derive(Deserialize)]
    struct NodejsManifest {
        version: zpm_semver::Version,
    }

    let releases: Vec<NodejsManifest>
        = JsonDocument::hydrate_from_str(&text)?;

    let highest_match_release
        = releases.iter()
            .filter(|release| params.range.check(&release.version))
            .max_by_key(|release| release.version.clone());

    let Some(highest_match_release) = highest_match_release else {
        return Err(Error::NoCandidatesFound(descriptor.range.clone()));
    };

    let systems = context.systems
        .expect("The systems are required for resolving a nodejs package");

    let variants
        = systems.iter()
            .map(|system| {
                let system
                    = system.without_libc();

                let package_name
                    = format!("@builtin/node-{}", system.to_file_string());

                let ident
                    = Ident::new(&package_name);
                let reference
                    = BuiltinReference {version: highest_match_release.version.clone()};

                Variant {
                    requirements: system.to_requirements(),
                    locator: Locator::new(ident, reference.into()),
                }
            })
            .collect_vec();

    let locator = descriptor.resolve_with(BuiltinReference {
        version: highest_match_release.version.clone(),
    }.into());

    let mut resolution
        = Resolution::new_empty(locator, highest_match_release.version.clone());

    resolution.variants = variants;

    let mut resolution_result
        = resolution.into_resolution_result(context);

    resolution_result.package_data = Some(PackageData::Abstract);

    Ok(resolution_result)
}

async fn resolve_nodejs_locator(context: &InstallContext<'_>, locator: &Locator, version: &zpm_semver::Version) -> Result<ResolutionResult, Error> {
    let resolution
        = Resolution::new_empty(locator.clone(), version.clone());

    Ok(resolution.into_resolution_result(context))
}

pub async fn resolve_builtin_descriptor(context: &InstallContext<'_>, descriptor: &Descriptor, params: &BuiltinRange) -> Result<ResolutionResult, Error> {
    match descriptor.ident.as_str() {
        "@builtin/node" => resolve_nodejs_descriptor(context, descriptor, params).await,
        _ => Err(Error::Unsupported)?,
    }
}

pub async fn resolve_builtin_locator(context: &InstallContext<'_>, locator: &Locator, version: &zpm_semver::Version) -> Result<ResolutionResult, Error> {
    match locator.ident.as_str() {
        "@builtin/node-linux-x64" |
        "@builtin/node-linux-arm64" |
        "@builtin/node-darwin-x64" |
        "@builtin/node-darwin-arm64"
            => resolve_nodejs_locator(context, locator, version).await,

        _
            => Err(Error::Unsupported)?,
    }
}
