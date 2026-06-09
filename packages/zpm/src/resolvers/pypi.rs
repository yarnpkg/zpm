use std::collections::{BTreeMap, BTreeSet};

use serde::{de::DeserializeOwned, Deserialize};
use zpm_parsers::JsonDocument;
use zpm_primitives::{Descriptor, Ident, Locator, MarkerExpr, MarkerValue, MarkerVariable, PypiRegistryReference, PypiSpecifierRange, PypiSpecifierSet, PypiTagRange, PythonFork, Reference, Range, canonicalize_pypi_name};
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PypiRequirement {
    pub ident: Ident,
    pub descriptor: Descriptor,
    pub marker: MarkerExpr,
}

impl PypiRequirement {
    fn specifier(&self) -> &PypiSpecifierSet {
        let Range::PypiSpecifier(params) = &self.descriptor.range else {
            unreachable!("PyPI requirements should always use PyPI specifier descriptors");
        };

        &params.specifier
    }
}

pub fn canonicalize_pypi_ident(ident: &Ident) -> Result<Ident, Error> {
    Ident::from_file_string(&canonicalize_pypi_name(ident.as_str()))
        .map_err(|err| Error::InvalidIdent(err.to_string()))
}

pub fn canonicalize_pypi_descriptor(descriptor: &Descriptor) -> Result<(Ident, Descriptor), Error> {
    let canonicalize_range_ident = |ident: &Ident| -> Result<Ident, Error> {
        canonicalize_pypi_ident(ident)
    };

    match &descriptor.range {
        Range::PypiSpecifier(params) => {
            let package_ident
                = canonicalize_range_ident(params.ident.as_ref().unwrap_or(&descriptor.ident))?;
            let descriptor_ident
                = if params.ident.is_some() {
                    descriptor.ident.clone()
                } else {
                    package_ident.clone()
                };
            let range = Range::PypiSpecifier(PypiSpecifierRange {
                ident: params.ident.as_ref().map(|_| package_ident.clone()),
                specifier: params.specifier.clone(),
            });

            Ok((package_ident, Descriptor::new_bound(descriptor_ident, range, descriptor.parent.clone())))
        },

        Range::PypiTag(params) => {
            let package_ident
                = canonicalize_range_ident(params.ident.as_ref().unwrap_or(&descriptor.ident))?;
            let descriptor_ident
                = if params.ident.is_some() {
                    descriptor.ident.clone()
                } else {
                    package_ident.clone()
                };
            let range = Range::PypiTag(PypiTagRange {
                ident: params.ident.as_ref().map(|_| package_ident.clone()),
                tag: params.tag.clone(),
            });

            Ok((package_ident, Descriptor::new_bound(descriptor_ident, range, descriptor.parent.clone())))
        },

        _ => Ok((descriptor.ident.clone(), descriptor.clone())),
    }
}

fn parse_requires_dist_entry(requirement: &str) -> Result<Option<PypiRequirement>, Error> {
    let dependency
        = pep_508::parse(requirement)
            .map_err(|errors| Error::InvalidResolution(format!("Invalid PyPI Requires-Dist entry `{requirement}`: {errors:?}")))?;

    if !dependency.extras.is_empty() {
        return Err(Error::InvalidResolution(format!(
            "Unsupported PyPI Requires-Dist entry `{requirement}`: requested dependency extras are not supported yet",
        )));
    }

    let marker
        = dependency.marker.as_ref()
            .map(MarkerExpr::from_pep508_marker)
            .transpose()
            .map_err(|err| Error::InvalidResolution(format!("Unsupported PyPI Requires-Dist marker in `{requirement}`: {err}")))?
            .unwrap_or(MarkerExpr::Any);

    let marker_variables
        = marker_variables(&marker);

    if marker_variables.contains(&MarkerVariable::Extra) {
        if marker_variables.len() == 1 {
            return Ok(None);
        }

        return Err(Error::InvalidResolution(format!(
            "Unsupported PyPI Requires-Dist marker in `{requirement}`: mixed `extra` markers are not supported yet",
        )));
    }

    let specifier
        = specifier_from_pep508_spec(dependency.spec.as_ref(), requirement)?;

    let ident
        = canonicalize_pypi_ident(&Ident::from_file_string(dependency.name)
            .map_err(|err| Error::InvalidIdent(err.to_string()))?)?;

    let descriptor
        = descriptor_from_pypi_requirement(ident.clone(), specifier);

    Ok(Some(PypiRequirement {
        ident,
        descriptor,
        marker,
    }))
}

fn parse_requires_dist(requirements: &[String]) -> Result<Vec<PypiRequirement>, Error> {
    let mut parsed
        = Vec::new();

    for requirement in requirements {
        if let Some(requirement) = parse_requires_dist_entry(requirement)? {
            parsed.push(requirement);
        }
    }

    Ok(parsed)
}

fn build_unconditional_dependency_map(requirements: &[PypiRequirement]) -> Result<BTreeMap<Ident, Descriptor>, Error> {
    build_dependency_map(requirements.iter().filter(|requirement| requirement.marker == MarkerExpr::Any))
}

fn build_targetless_island_dependency_map(requirements: &[PypiRequirement]) -> Result<BTreeMap<Ident, Descriptor>, Error> {
    for requirement in requirements {
        if requirement.marker != MarkerExpr::Any && requirement.marker != MarkerExpr::Never {
            return Err(Error::InvalidResolution(format!(
                "Cannot evaluate PyPI marker for {} without a Python target environment; configure supportedTargets with python.version",
                requirement.ident.to_file_string(),
            )));
        }
    }

    build_unconditional_dependency_map(requirements)
}

fn build_fork_dependency_map(requirements: &[PypiRequirement], fork: &PythonFork) -> Result<BTreeMap<Ident, Descriptor>, Error> {
    let mut active_requirements
        = Vec::new();

    for requirement in requirements {
        if is_requirement_active_for_fork(requirement, fork)? {
            active_requirements.push(requirement);
        }
    }

    let dependencies
        = build_dependency_map(active_requirements)?;

    Ok(dependencies.into_iter()
        .map(|(ident, descriptor)| {
            (ident, descriptor.env_qualified_with_hash(fork.id.clone()))
        })
        .collect())
}

fn is_requirement_active_for_fork(requirement: &PypiRequirement, fork: &PythonFork) -> Result<bool, Error> {
    match &requirement.marker {
        MarkerExpr::Any => Ok(true),
        MarkerExpr::Never => Ok(false),
        marker => {
            let target
                = fork.target.as_ref()
                    .ok_or_else(|| Error::InvalidResolution(format!("Cannot evaluate PyPI marker for {} without a Python target environment", requirement.ident.to_file_string())))?;

            marker.evaluate(target)
                .map_err(|err| Error::InvalidResolution(format!("Cannot evaluate PyPI marker for {}: {err}", requirement.ident.to_file_string())))
        },
    }
}

pub fn build_dependency_map<'a>(requirements: impl IntoIterator<Item = &'a PypiRequirement>) -> Result<BTreeMap<Ident, Descriptor>, Error> {
    let mut grouped
        = BTreeMap::<Ident, PypiSpecifierSet>::new();

    for requirement in requirements {
        if let Some(specifier) = grouped.get_mut(&requirement.ident) {
            *specifier = specifier.intersection(requirement.specifier())
                .map_err(|err| Error::InvalidRange(err.to_string()))?;
        } else {
            grouped.insert(requirement.ident.clone(), requirement.specifier().clone());
        }
    }

    Ok(grouped.into_iter()
        .map(|(ident, specifier)| {
            let descriptor
                = descriptor_from_pypi_requirement(ident.clone(), specifier);

            (ident, descriptor)
        })
        .collect())
}

fn descriptor_from_pypi_requirement(ident: Ident, specifier: PypiSpecifierSet) -> Descriptor {
    Descriptor::new(ident, Range::PypiSpecifier(PypiSpecifierRange {
        ident: None,
        specifier,
    }))
}

fn specifier_from_pep508_spec(spec: Option<&pep_508::Spec<'_>>, requirement: &str) -> Result<PypiSpecifierSet, Error> {
    let Some(spec) = spec else {
        return Ok(PypiSpecifierSet::any());
    };

    match spec {
        pep_508::Spec::Url(_)
            => Err(Error::InvalidResolution(format!("Unsupported PyPI Requires-Dist entry `{requirement}`: direct URL requirements are not supported yet"))),

        pep_508::Spec::Version(specifiers) => {
            let specifier
                = specifiers.iter()
                    .map(|specifier| format!("{}{}", format_pep508_comparator(specifier.comparator), specifier.version))
                    .collect::<Vec<_>>()
                    .join(",");

            PypiSpecifierSet::from_file_string(&specifier)
                .map_err(|err| Error::InvalidRange(err.to_string()))
        },
    }
}

fn format_pep508_comparator(comparator: pep_508::Comparator) -> &'static str {
    match comparator {
        pep_508::Comparator::Lt => "<",
        pep_508::Comparator::Le => "<=",
        pep_508::Comparator::Ne => "!=",
        pep_508::Comparator::Eq => "==",
        pep_508::Comparator::Ge => ">=",
        pep_508::Comparator::Gt => ">",
        pep_508::Comparator::Cp => "~=",
        pep_508::Comparator::Ae => "===",
    }
}

fn marker_variables(marker: &MarkerExpr) -> BTreeSet<MarkerVariable> {
    let mut variables
        = BTreeSet::new();

    collect_marker_variables(marker, &mut variables);

    variables
}

fn collect_marker_variables(marker: &MarkerExpr, variables: &mut BTreeSet<MarkerVariable>) {
    match marker {
        MarkerExpr::Any | MarkerExpr::Never => {},

        MarkerExpr::And {lhs, rhs} | MarkerExpr::Or {lhs, rhs} => {
            collect_marker_variables(lhs, variables);
            collect_marker_variables(rhs, variables);
        },

        MarkerExpr::Not {expr} => {
            collect_marker_variables(expr, variables);
        },

        MarkerExpr::Compare {lhs, rhs, ..} => {
            collect_marker_value_variables(lhs, variables);
            collect_marker_value_variables(rhs, variables);
        },
    }
}

fn collect_marker_value_variables(value: &MarkerValue, variables: &mut BTreeSet<MarkerVariable>) {
    if let MarkerValue::Variable(variable) = value {
        variables.insert(*variable);
    }
}

fn project_pep440_to_semver(version: &zpm_primitives::PypiVersion) -> Result<zpm_semver::Version, Error> {
    // TODO: Replace this lossy projection once `Resolution.version` can represent
    // non-semver registry versions without information loss.
    version.to_lossy_semver()
        .map_err(|err| Error::InvalidResolution(err.to_string()))
}

fn build_resolution_result(context: &InstallContext<'_>, locator: Locator, version: &zpm_primitives::PypiVersion, requires_dist: &[String]) -> Result<ResolutionResult, Error> {
    let mut resolution
        = Resolution::new_empty(locator, project_pep440_to_semver(version)?);
    let requirements
        = parse_requires_dist(requires_dist)?;
    resolution.dependencies = build_unconditional_dependency_map(&requirements)?;
    resolution.into_resolution_result(context)
}

fn build_targetless_island_resolution_result(context: &InstallContext<'_>, locator: Locator, version: &zpm_primitives::PypiVersion, requires_dist: &[String]) -> Result<ResolutionResult, Error> {
    let mut resolution
        = Resolution::new_empty(locator, project_pep440_to_semver(version)?);
    let requirements
        = parse_requires_dist(requires_dist)?;
    resolution.dependencies = build_targetless_island_dependency_map(&requirements)?;
    resolution.into_resolution_result(context)
}

fn build_fork_resolution_result(context: &InstallContext<'_>, locator: Locator, version: &zpm_primitives::PypiVersion, requires_dist: &[String], fork: &PythonFork) -> Result<ResolutionResult, Error> {
    let mut resolution
        = Resolution::new_empty(locator.env_qualified_with_hash(fork.id.clone()), project_pep440_to_semver(version)?);
    let requirements
        = parse_requires_dist(requires_dist)?;
    resolution.dependencies = build_fork_dependency_map(&requirements, fork)?;
    resolution.into_resolution_result(context)
}

fn select_version_for_specifier(releases: &BTreeMap<String, Vec<PypiDistribution>>, specifier: &zpm_primitives::PypiSpecifierSet) -> Result<Option<(zpm_primitives::PypiVersion, Vec<PypiDistribution>)>, Error> {
    let mut best: Option<(zpm_primitives::PypiVersion, Vec<PypiDistribution>)>
        = None;

    for (raw_version, distributions) in releases {
        let Ok(version) = zpm_primitives::PypiVersion::from_file_string(raw_version) else {
            continue;
        };

        if !version.satisfies(specifier)
            .map_err(|err| Error::InvalidRange(err.to_string()))?
        {
            continue;
        }

        let should_replace
            = best.as_ref()
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
    let mut latest_any: Option<(zpm_primitives::PypiVersion, Vec<PypiDistribution>)>
        = None;
    let mut latest_stable: Option<(zpm_primitives::PypiVersion, Vec<PypiDistribution>)>
        = None;

    for (raw_version, distributions) in releases {
        let Ok(version) = zpm_primitives::PypiVersion::from_file_string(raw_version) else {
            continue;
        };

        let should_replace_any
            = latest_any.as_ref()
                .map(|(best_version, _)| {
                    version.cmp_pep440(best_version)
                        .map(|ordering| ordering.is_gt())
                        .unwrap_or(false)
                })
                .unwrap_or(true);

        if should_replace_any {
            latest_any = Some((version.clone(), distributions.clone()));
        }

        let is_stable
            = version.is_stable()
                .map_err(|err| Error::InvalidRange(err.to_string()))?;

        if is_stable {
            let should_replace_stable
                = latest_stable.as_ref()
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
    let project
        = context.project
        .expect("The project is required for resolving PyPI packages");

    let bytes
        = project.http_client.cached_get(url).await?;

    let value: T
        = JsonDocument::hydrate_from_slice(&bytes[..])?;
    Ok(value)
}

async fn fetch_project_metadata(context: &InstallContext<'_>, package_ident: &Ident) -> Result<PypiProjectMetadata, Error> {
    let package_ident
        = canonicalize_pypi_ident(package_ident)?;
    let base
        = pypi_registry_base();
    let url
        = format!("{}/pypi/{}/json", base, encode_path_segment(package_ident.as_str()));
    fetch_json(context, &url).await
}

async fn fetch_version_metadata(context: &InstallContext<'_>, package_ident: &Ident, version: &zpm_primitives::PypiVersion) -> Result<PypiVersionMetadata, Error> {
    let package_ident
        = canonicalize_pypi_ident(package_ident)?;
    let base
        = pypi_registry_base();
    let url
        = format!(
            "{}/pypi/{}/{}/json",
            base,
            encode_path_segment(package_ident.as_str()),
            encode_path_segment(&version.to_file_string()),
        );

    fetch_json(context, &url).await
}

pub async fn resolve_versions(context: &InstallContext<'_>, package_ident: &Ident) -> Result<Vec<Locator>, Error> {
    let package_ident
        = canonicalize_pypi_ident(package_ident)?;
    let project_metadata
        = fetch_project_metadata(context, &package_ident).await?;

    let mut locators
        = Vec::new();

    for (raw_version, release_distributions) in project_metadata.releases {
        let Ok(version) = zpm_primitives::PypiVersion::from_file_string(&raw_version) else {
            continue;
        };

        let Some(wheel) = select_best_wheel(&release_distributions) else {
            continue;
        };

        locators.push(Locator::new(package_ident.clone(), PypiRegistryReference {
            ident: package_ident.clone(),
            version,
            url: Some(UrlEncoded::new(wheel.url.clone())),
        }.into()));
    }

    locators.sort_by(|a, b| {
        let a_version = match a.reference.physical_reference() {
            Reference::PypiRegistry(params) => &params.version,
            Reference::PypiShorthand(params) => &params.version,
            _ => return a.cmp(b),
        };
        let b_version = match b.reference.physical_reference() {
            Reference::PypiRegistry(params) => &params.version,
            Reference::PypiShorthand(params) => &params.version,
            _ => return a.cmp(b),
        };

        a_version.cmp_pep440(b_version)
            .unwrap_or_else(|_| a.cmp(b))
    });

    Ok(locators)
}

pub fn resolve_aliased(descriptor: &Descriptor, dependencies: Vec<InstallOpResult>) -> Result<ResolutionResult, Error> {
    let mut inner_resolution
        = dependencies.iter()
            .find_map(|dependency| match dependency {
                InstallOpResult::Resolved(result)
                    => Some(result.clone()),

                _
                    => None,
            })
            .unwrap_or_else(|| panic!("Expected at least one Resolved result in dependencies for aliased PyPI package; got {:?}", dependencies));

    let inner_reference
        = inner_resolution.resolution.locator.reference.clone();

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

    inner_resolution.resolution.locator
        = Locator::new(descriptor.ident.clone(), new_reference);
    Ok(inner_resolution)
}

pub async fn resolve_specifier_descriptor(context: &InstallContext<'_>, descriptor: &Descriptor, params: &PypiSpecifierRange) -> Result<ResolutionResult, Error> {
    let package_ident
        = params.ident.as_ref()
        .unwrap_or(&descriptor.ident);
    let package_ident
        = canonicalize_pypi_ident(package_ident)?;

    let project_metadata
        = fetch_project_metadata(context, &package_ident).await?;
    let (resolved_version, release_distributions)
        = select_version_for_specifier(&project_metadata.releases, &params.specifier)?
            .ok_or_else(|| Error::NoCandidatesFound(descriptor.range.clone()))?;

    let wheel
        = select_best_wheel(&release_distributions)
            .ok_or_else(|| Error::InvalidResolution(format!("No wheel artifact found for {}@{}", package_ident.to_file_string(), resolved_version.to_file_string())))?;

    let version_metadata
        = fetch_version_metadata(context, &package_ident, &resolved_version).await?;

    let reference: Reference
        = PypiRegistryReference {
            ident: package_ident.clone(),
            version: resolved_version.clone(),
            url: Some(UrlEncoded::new(wheel.url.clone())),
        }.into();
    let locator
        = if params.ident.is_some() {
            descriptor.resolve_with(reference)
        } else {
            Locator::new(package_ident.clone(), reference)
        };

    let requires_dist
        = version_metadata.info.requires_dist.unwrap_or_default();

    build_resolution_result(context, locator, &resolved_version, &requires_dist)
}

pub async fn resolve_tag_descriptor(context: &InstallContext<'_>, descriptor: &Descriptor, params: &PypiTagRange) -> Result<ResolutionResult, Error> {
    if params.tag.as_str() != "latest" {
        return Err(Error::TagNotFound(params.tag.to_string()));
    }

    let package_ident
        = params.ident.as_ref()
        .unwrap_or(&descriptor.ident);
    let package_ident
        = canonicalize_pypi_ident(package_ident)?;

    let project_metadata
        = fetch_project_metadata(context, &package_ident).await?;
    let (resolved_version, release_distributions)
        = select_latest_version(&project_metadata.releases)?
            .ok_or_else(|| Error::NoCandidatesFound(descriptor.range.clone()))?;

    let wheel
        = select_best_wheel(&release_distributions)
            .ok_or_else(|| Error::InvalidResolution(format!("No wheel artifact found for {}@{}", package_ident.to_file_string(), resolved_version.to_file_string())))?;

    let version_metadata
        = fetch_version_metadata(context, &package_ident, &resolved_version).await?;

    let reference: Reference
        = PypiRegistryReference {
            ident: package_ident.clone(),
            version: resolved_version.clone(),
            url: Some(UrlEncoded::new(wheel.url.clone())),
        }.into();
    let locator
        = if params.ident.is_some() {
            descriptor.resolve_with(reference)
        } else {
            Locator::new(package_ident.clone(), reference)
        };

    let requires_dist
        = version_metadata.info.requires_dist.unwrap_or_default();

    build_resolution_result(context, locator, &resolved_version, &requires_dist)
}

pub async fn resolve_locator(context: &InstallContext<'_>, locator: &Locator, params: &PypiRegistryReference) -> Result<ResolutionResult, Error> {
    let package_ident
        = canonicalize_pypi_ident(&params.ident)?;
    let version_metadata
        = fetch_version_metadata(context, &package_ident, &params.version).await?;
    let requires_dist
        = version_metadata.info.requires_dist.unwrap_or_default();

    build_resolution_result(context, locator.clone(), &params.version, &requires_dist)
}

pub async fn resolve_locator_requiring_python_target(context: &InstallContext<'_>, locator: &Locator, params: &PypiRegistryReference) -> Result<ResolutionResult, Error> {
    let package_ident
        = canonicalize_pypi_ident(&params.ident)?;
    let version_metadata
        = fetch_version_metadata(context, &package_ident, &params.version).await?;
    let requires_dist
        = version_metadata.info.requires_dist.unwrap_or_default();

    build_targetless_island_resolution_result(context, locator.clone(), &params.version, &requires_dist)
}

pub async fn resolve_locator_for_fork(context: &InstallContext<'_>, locator: &Locator, params: &PypiRegistryReference, fork: &PythonFork) -> Result<ResolutionResult, Error> {
    let package_ident
        = canonicalize_pypi_ident(&params.ident)?;
    let version_metadata
        = fetch_version_metadata(context, &package_ident, &params.version).await?;
    let requires_dist
        = version_metadata.info.requires_dist.unwrap_or_default();

    build_fork_resolution_result(context, locator.clone(), &params.version, &requires_dist, fork)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_one(requirement: &str) -> PypiRequirement {
        parse_requires_dist_entry(requirement).unwrap().unwrap()
    }

    fn invalid_resolution_message(err: Error) -> String {
        match err {
            Error::InvalidResolution(message) => message,
            other => panic!("Expected InvalidResolution error, got {other:?}"),
        }
    }

    #[test]
    fn test_parse_requires_dist_canonicalizes_names_and_keeps_markers() {
        let requirement
            = parse_one("Friendly_Bard.Name (>=1.0.0); python_version >= '3.11'");

        assert_eq!("friendly-bard-name", requirement.ident.to_file_string());
        assert_eq!("friendly-bard-name@pypi:>=1.0.0", requirement.descriptor.to_file_string());
        assert_ne!(MarkerExpr::Any, requirement.marker);
    }

    #[test]
    fn test_canonicalize_pypi_descriptor_normalizes_direct_names() {
        let descriptor
            = Descriptor::from_file_string("Friendly_Bard.Name@pypi:>=1.0.0").unwrap();
        let (package_ident, descriptor)
            = canonicalize_pypi_descriptor(&descriptor).unwrap();

        assert_eq!("friendly-bard-name", package_ident.to_file_string());
        assert_eq!("friendly-bard-name@pypi:>=1.0.0", descriptor.to_file_string());
    }

    #[test]
    fn test_canonicalize_pypi_descriptor_preserves_alias_names() {
        let descriptor
            = Descriptor::from_file_string("alias@pypi:Friendly_Bard.Name@>=1.0.0").unwrap();
        let (package_ident, descriptor)
            = canonicalize_pypi_descriptor(&descriptor).unwrap();

        assert_eq!("friendly-bard-name", package_ident.to_file_string());
        assert_eq!("alias@pypi:friendly-bard-name@>=1.0.0", descriptor.to_file_string());
    }

    #[test]
    fn test_parse_requires_dist_rejects_requested_extras() {
        let message
            = invalid_resolution_message(parse_requires_dist_entry("friendly-bard[http] >=1.0.0").unwrap_err());

        assert!(message.contains("requested dependency extras"));
    }

    #[test]
    fn test_parse_requires_dist_extra_only_markers_are_inactive() {
        assert!(parse_requires_dist_entry("friendly-bard >=1.0.0; extra == 'http'").unwrap().is_none());
    }

    #[test]
    fn test_parse_requires_dist_rejects_mixed_extra_markers() {
        let message
            = invalid_resolution_message(parse_requires_dist_entry("friendly-bard >=1.0.0; extra == 'http' and python_version >= '3.11'").unwrap_err());

        assert!(message.contains("mixed `extra` markers"));
    }

    #[test]
    fn test_parse_requires_dist_rejects_direct_urls() {
        let message
            = invalid_resolution_message(parse_requires_dist_entry("friendly-bard @ http://foo.com").unwrap_err());

        assert!(message.contains("direct URL requirements"));
    }

    #[test]
    fn test_build_dependency_map_intersects_same_ident_requirements() {
        let requirements
            = vec![
                parse_one("Friendly_Bard >=1.0.0"),
                parse_one("friendly.bard <2.0.0"),
            ];
        let dependencies
            = build_dependency_map(requirements.iter()).unwrap();

        assert_eq!(1, dependencies.len());
        assert_eq!(
            "friendly-bard@pypi:>=1.0.0, <2.0.0",
            dependencies.get(&Ident::from_file_string("friendly-bard").unwrap()).unwrap().to_file_string(),
        );
    }

    #[test]
    fn test_unconditional_dependency_map_ignores_marker_requirements_for_now() {
        let requirements
            = parse_requires_dist(&[
                "friendly-bard >=1.0.0; sys_platform == 'linux'".to_string(),
                "always-bard >=1.0.0".to_string(),
            ]).unwrap();
        let dependencies
            = build_unconditional_dependency_map(&requirements).unwrap();

        assert_eq!(1, dependencies.len());
        assert!(dependencies.contains_key(&Ident::from_file_string("always-bard").unwrap()));
    }

    #[test]
    fn test_targetless_island_dependency_map_rejects_marker_requirements() {
        let requirements
            = parse_requires_dist(&[
                "friendly-bard >=1.0.0; sys_platform == 'linux'".to_string(),
            ]).unwrap();
        let message
            = invalid_resolution_message(build_targetless_island_dependency_map(&requirements).unwrap_err());

        assert!(message.contains("without a Python target environment"), "{message}");
        assert!(message.contains("supportedTargets"), "{message}");
    }
}
