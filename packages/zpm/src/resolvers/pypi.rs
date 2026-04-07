use std::collections::BTreeMap;

use serde::{Deserialize, de::DeserializeOwned};
use zpm_parsers::JsonDocument;
use zpm_primitives::{Descriptor, Ident, Locator, PypiRegistryReference, PypiSpecifierRange, PypiTagRange, Reference, Range};
use zpm_utils::{FromFileString, ToFileString, UrlEncoded};

use crate::{
    error::Error,
    install::{InstallContext, InstallOpResult, IntoResolutionResult, ResolutionResult},
    pypi::{PypiDistribution, pypi_registry_base, encode_path_segment, select_best_wheel},
    resolvers::Resolution,
};

#[derive(Clone, Debug, Deserialize)]
struct PypiProjectMetadata {
    #[serde(default)]
    releases: BTreeMap<String, Vec<PypiDistribution>>,
}

#[derive(Clone, Debug, Deserialize)]
struct PypiVersionMetadata {
    #[serde(default)]
    info: PypiVersionInfo,
}

#[derive(Clone, Debug, Default, Deserialize)]
struct PypiVersionInfo {
    #[serde(default)]
    requires_dist: Option<Vec<String>>,
}

fn parse_requires_dist_entry(requirement: &str) -> Option<(Ident, Descriptor)> {
    if requirement.contains(';') {
        return None;
    }

    let requirement = requirement.trim();

    if requirement.is_empty() || requirement.contains(" @ ") {
        return None;
    }

    let mut name_end = 0usize;

    for (index, ch) in requirement.char_indices() {
        if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' || ch == '.' {
            name_end = index + ch.len_utf8();
        } else {
            break;
        }
    }

    if name_end == 0 {
        return None;
    }

    let name = &requirement[..name_end];
    let mut tail = requirement[name_end..].trim();

    if tail.starts_with('[') {
        let extras_end = tail.find(']')?;
        tail = tail[extras_end + 1..].trim();
    }

    let specifier = if tail.starts_with('(') {
        let close = tail.find(')')?;
        let inner = tail[1..close].trim();
        let rest = tail[close + 1..].trim();

        if !rest.is_empty() {
            return None;
        }

        inner
    } else {
        tail
    };

    let ident = Ident::from_file_string(name).ok()?;

    let specifier = match zpm_primitives::PypiSpecifierSet::from_file_string(specifier) {
        Ok(specifier) => specifier,
        Err(_) => return None,
    };

    let descriptor = Descriptor::new(ident.clone(), Range::PypiSpecifier(PypiSpecifierRange {
        ident: None,
        specifier,
    }));

    Some((ident, descriptor))
}

fn parse_requires_dist(requirements: &[String]) -> BTreeMap<Ident, Descriptor> {
    requirements.iter()
        .filter_map(|requirement| parse_requires_dist_entry(requirement))
        .collect()
}

fn project_pep440_to_semver(version: &zpm_primitives::PypiVersion) -> Result<zpm_semver::Version, Error> {
    // TODO: Replace this lossy projection once `Resolution.version` can represent
    // non-semver registry versions without information loss.
    version.to_lossy_semver()
        .map_err(|err| Error::InvalidResolution(err.to_string()))
}

fn build_resolution_result(context: &InstallContext<'_>, locator: Locator, version: &zpm_primitives::PypiVersion, requires_dist: &[String]) -> Result<ResolutionResult, Error> {
    let mut resolution = Resolution::new_empty(locator, project_pep440_to_semver(version)?);
    resolution.dependencies = parse_requires_dist(requires_dist);
    resolution.into_resolution_result(context)
}

fn select_version_for_specifier(releases: &BTreeMap<String, Vec<PypiDistribution>>, specifier: &zpm_primitives::PypiSpecifierSet) -> Result<Option<(zpm_primitives::PypiVersion, Vec<PypiDistribution>)>, Error> {
    let mut best: Option<(zpm_primitives::PypiVersion, Vec<PypiDistribution>)> = None;

    for (raw_version, distributions) in releases {
        let Ok(version) = zpm_primitives::PypiVersion::from_file_string(raw_version) else {
            continue;
        };

        if !version.satisfies(specifier)
            .map_err(|err| Error::InvalidRange(err.to_string()))?
        {
            continue;
        }

        let should_replace = best.as_ref()
            .map(|(best_version, _)| {
                version.cmp_pep440(best_version)
                    .map(|ordering| ordering.is_gt())
                    .unwrap_or(false)
            })
            .unwrap_or(true);

        if should_replace {
            best = Some((version, distributions.clone()));
        }
    }

    Ok(best)
}

fn select_latest_version(releases: &BTreeMap<String, Vec<PypiDistribution>>) -> Result<Option<(zpm_primitives::PypiVersion, Vec<PypiDistribution>)>, Error> {
    let mut latest_any: Option<(zpm_primitives::PypiVersion, Vec<PypiDistribution>)> = None;
    let mut latest_stable: Option<(zpm_primitives::PypiVersion, Vec<PypiDistribution>)> = None;

    for (raw_version, distributions) in releases {
        let Ok(version) = zpm_primitives::PypiVersion::from_file_string(raw_version) else {
            continue;
        };

        let should_replace_any = latest_any.as_ref()
            .map(|(best_version, _)| {
                version.cmp_pep440(best_version)
                    .map(|ordering| ordering.is_gt())
                    .unwrap_or(false)
            })
            .unwrap_or(true);

        if should_replace_any {
            latest_any = Some((version.clone(), distributions.clone()));
        }

        let is_stable = version.is_stable()
            .map_err(|err| Error::InvalidRange(err.to_string()))?;

        if is_stable {
            let should_replace_stable = latest_stable.as_ref()
                .map(|(best_version, _)| {
                    version.cmp_pep440(best_version)
                        .map(|ordering| ordering.is_gt())
                        .unwrap_or(false)
                })
                .unwrap_or(true);

            if should_replace_stable {
                latest_stable = Some((version, distributions.clone()));
            }
        }
    }

    Ok(latest_stable.or(latest_any))
}

async fn fetch_json<T>(context: &InstallContext<'_>, url: &str) -> Result<T, Error>
where
    T: DeserializeOwned,
{
    let project = context.project
        .expect("The project is required for resolving PyPI packages");

    let bytes = project.http_client.cached_get(url).await?;

    let value: T = JsonDocument::hydrate_from_slice(&bytes[..])?;
    Ok(value)
}

async fn fetch_project_metadata(context: &InstallContext<'_>, package_ident: &Ident) -> Result<PypiProjectMetadata, Error> {
    let base = pypi_registry_base();
    let url = format!("{}/pypi/{}/json", base, encode_path_segment(package_ident.as_str()));
    fetch_json(context, &url).await
}

async fn fetch_version_metadata(context: &InstallContext<'_>, package_ident: &Ident, version: &zpm_primitives::PypiVersion) -> Result<PypiVersionMetadata, Error> {
    let base = pypi_registry_base();
    let url = format!(
        "{}/pypi/{}/{}/json",
        base,
        encode_path_segment(package_ident.as_str()),
        encode_path_segment(&version.to_file_string()),
    );

    fetch_json(context, &url).await
}

pub fn resolve_aliased(descriptor: &Descriptor, dependencies: Vec<InstallOpResult>) -> Result<ResolutionResult, Error> {
    let mut inner_resolution = dependencies.iter()
        .find_map(|dependency| match dependency {
            InstallOpResult::Resolved(result) => Some(result.clone()),
            _ => None,
        })
        .unwrap_or_else(|| panic!("Expected at least one Resolved result in dependencies for aliased PyPI package; got {:?}", dependencies));

    let inner_reference = inner_resolution.resolution.locator.reference.clone();

    let new_reference = match inner_reference {
        Reference::PypiShorthand(inner_params) => PypiRegistryReference {
            ident: inner_resolution.resolution.locator.ident.clone(),
            version: inner_params.version.clone(),
            url: inner_params.url.clone(),
        }.into(),

        Reference::PypiRegistry(inner_params) => PypiRegistryReference {
            ident: inner_params.ident.clone(),
            version: inner_params.version.clone(),
            url: inner_params.url.clone(),
        }.into(),

        _ => unreachable!("Unexpected reference type in PyPI alias resolution: {:?}", inner_reference),
    };

    inner_resolution.resolution.locator = Locator::new(descriptor.ident.clone(), new_reference);
    Ok(inner_resolution)
}

pub async fn resolve_specifier_descriptor(context: &InstallContext<'_>, descriptor: &Descriptor, params: &PypiSpecifierRange) -> Result<ResolutionResult, Error> {
    let package_ident = params.ident.as_ref()
        .unwrap_or(&descriptor.ident);

    let project_metadata = fetch_project_metadata(context, package_ident).await?;
    let (resolved_version, release_distributions) = select_version_for_specifier(&project_metadata.releases, &params.specifier)?
        .ok_or_else(|| Error::NoCandidatesFound(descriptor.range.clone()))?;

    let wheel = select_best_wheel(&release_distributions)
        .ok_or_else(|| Error::InvalidResolution(format!("No wheel artifact found for {}@{}", package_ident.to_file_string(), resolved_version.to_file_string())))?;

    let version_metadata = fetch_version_metadata(context, package_ident, &resolved_version).await?;

    let locator = descriptor.resolve_with(PypiRegistryReference {
        ident: package_ident.clone(),
        version: resolved_version.clone(),
        url: Some(UrlEncoded::new(wheel.url.clone())),
    }.into());

    let requires_dist = version_metadata.info.requires_dist.unwrap_or_default();

    build_resolution_result(context, locator, &resolved_version, &requires_dist)
}

pub async fn resolve_tag_descriptor(context: &InstallContext<'_>, descriptor: &Descriptor, params: &PypiTagRange) -> Result<ResolutionResult, Error> {
    if params.tag.as_str() != "latest" {
        return Err(Error::TagNotFound(params.tag.to_string()));
    }

    let package_ident = params.ident.as_ref()
        .unwrap_or(&descriptor.ident);

    let project_metadata = fetch_project_metadata(context, package_ident).await?;
    let (resolved_version, release_distributions) = select_latest_version(&project_metadata.releases)?
        .ok_or_else(|| Error::NoCandidatesFound(descriptor.range.clone()))?;

    let wheel = select_best_wheel(&release_distributions)
        .ok_or_else(|| Error::InvalidResolution(format!("No wheel artifact found for {}@{}", package_ident.to_file_string(), resolved_version.to_file_string())))?;

    let version_metadata = fetch_version_metadata(context, package_ident, &resolved_version).await?;

    let locator = descriptor.resolve_with(PypiRegistryReference {
        ident: package_ident.clone(),
        version: resolved_version.clone(),
        url: Some(UrlEncoded::new(wheel.url.clone())),
    }.into());

    let requires_dist = version_metadata.info.requires_dist.unwrap_or_default();

    build_resolution_result(context, locator, &resolved_version, &requires_dist)
}

pub async fn resolve_locator(context: &InstallContext<'_>, locator: &Locator, params: &PypiRegistryReference) -> Result<ResolutionResult, Error> {
    let version_metadata = fetch_version_metadata(context, &params.ident, &params.version).await?;
    let requires_dist = version_metadata.info.requires_dist.unwrap_or_default();

    build_resolution_result(context, locator.clone(), &params.version, &requires_dist)
}
