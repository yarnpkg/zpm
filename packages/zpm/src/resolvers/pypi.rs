use std::{collections::{BTreeMap, BTreeSet}, str::FromStr, sync::Arc};

use serde::{de::DeserializeOwned, Deserialize};
use zpm_parsers::JsonDocument;
use zpm_primitives::{Descriptor, Ident, Locator, MarkerVariable, PypiExtras, PypiFileRange, PypiGitRange, PypiRangeParameters, PypiRegistryReference, PypiSpecifierRange, PypiSpecifierSet, PypiTagRange, PypiVersion, PythonFork, PythonTargetEnv, Reference, Range, canonicalize_pypi_name};
use zpm_utils::{FromFileString, Hash64, ToFileString, UrlEncoded};

use crate::{
    error::Error,
    install::{InstallContext, InstallOpResult, IntoResolutionResult, ResolutionResult},
    prepare,
    pypi::{PypiDistribution, encode_path_segment, format_local_wheel_url, format_python_git_url, get_artifact_authorization, get_authorization, get_build_registry, get_registry, metadata_from_source_tree, metadata_from_wheel, parse_local_wheel_url, parse_python_git_url, parse_simple_project, python_git_project_path, requires_dist_from_wheel, resolve_local_wheel_path, select_best_artifact, select_best_wheel},
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
    pub marker: pep508_rs::MarkerTree,
}

pub fn canonicalize_pypi_ident(ident: &Ident) -> Result<Ident, Error> {
    Ident::from_file_string(&canonicalize_pypi_name(ident.as_str()))
        .map_err(|err| Error::InvalidIdent(err.to_string()))
}

pub fn canonicalize_pypi_descriptor(descriptor: &Descriptor) -> Result<(Ident, Descriptor), Error> {
    let canonicalize_range_ident = |ident: &Ident| -> Result<Ident, Error> {
        canonicalize_pypi_ident(ident)
    };

    match descriptor.range.physical_range() {
        Range::PypiSpecifier(params) => {
            let package_ident
                = canonicalize_range_ident(params.ident.as_ref().unwrap_or(&descriptor.ident))?;
            let descriptor_ident
                = if params.ident.is_some() {
                    descriptor.ident.clone()
                } else {
                    package_ident.clone()
                };
            let physical_range = Range::PypiSpecifier(PypiSpecifierRange {
                ident: params.ident.as_ref().map(|_| package_ident.clone()),
                specifier: params.specifier.clone(),
                parameters: params.parameters.clone(),
            });
            let range
                = descriptor.range.clone().map_physical(|_| physical_range);

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
            let physical_range = Range::PypiTag(PypiTagRange {
                ident: params.ident.as_ref().map(|_| package_ident.clone()),
                tag: params.tag.clone(),
                parameters: params.parameters.clone(),
            });
            let range
                = descriptor.range.clone().map_physical(|_| physical_range);

            Ok((package_ident, Descriptor::new_bound(descriptor_ident, range, descriptor.parent.clone())))
        },

        Range::PypiFile(_) => {
            let package_ident
                = canonicalize_range_ident(&descriptor.ident)?;
            Ok((
                package_ident.clone(),
                Descriptor::new_bound(package_ident, descriptor.range.clone(), descriptor.parent.clone()),
            ))
        },

        Range::PypiGit(_) => {
            let package_ident
                = canonicalize_range_ident(&descriptor.ident)?;
            Ok((
                package_ident.clone(),
                Descriptor::new_bound(package_ident, descriptor.range.clone(), descriptor.parent.clone()),
            ))
        },

        _ => Ok((descriptor.ident.clone(), descriptor.clone())),
    }
}

fn parse_requires_dist_entry(requirement: &str) -> Result<Option<PypiRequirement>, Error> {
    let dependency
        = pep508_rs::Requirement::<pep508_rs::VerbatimUrl>::from_str(requirement)
            .map_err(|error| Error::InvalidResolution(format!("Invalid PyPI Requires-Dist entry `{requirement}`: {error}")))?;

    let specifier = match dependency.version_or_url.as_ref() {
        None => PypiSpecifierSet::any(),
        Some(pep508_rs::VersionOrUrl::VersionSpecifier(specifiers)) => {
            PypiSpecifierSet::from_file_string(&specifiers.to_string())
                .map_err(|err| Error::InvalidRange(err.to_string()))?
        },
        Some(pep508_rs::VersionOrUrl::Url(_)) => {
            return Err(Error::InvalidResolution(format!("Unsupported PyPI Requires-Dist entry `{requirement}`: direct URL requirements are not supported yet")));
        },
    };

    let ident
        = canonicalize_pypi_ident(&Ident::from_file_string(dependency.name.as_ref())
            .map_err(|err| Error::InvalidIdent(err.to_string()))?)?;

    let extras = PypiExtras::from_iter(dependency.extras.iter().map(|extra| extra.as_ref()))
        .map_err(|err| Error::InvalidRange(err.to_string()))?;
    let descriptor = descriptor_from_pypi_requirement(ident.clone(), specifier, extras);

    Ok(Some(PypiRequirement {
        ident,
        descriptor,
        marker: dependency.marker,
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

#[cfg(test)]
fn build_unconditional_dependency_map(requirements: &[PypiRequirement]) -> Result<BTreeMap<Ident, Descriptor>, Error> {
    build_context_dependency_map(requirements, true, &PypiExtras::empty(), None, false)
}

#[cfg(test)]
fn build_targetless_island_dependency_map(requirements: &[PypiRequirement]) -> Result<BTreeMap<Ident, Descriptor>, Error> {
    build_context_dependency_map(requirements, true, &PypiExtras::empty(), None, true)
}

fn build_fork_dependency_map_with_extras(requirements: &[PypiRequirement], fork: &PythonFork, include_base: bool, active_extras: &PypiExtras) -> Result<BTreeMap<Ident, Descriptor>, Error> {
    let dependencies = build_context_dependency_map(requirements, include_base, active_extras, fork.target.as_ref(), true)?;

    Ok(dependencies.into_iter()
        .map(|(ident, descriptor)| {
            (ident, descriptor.env_qualified_with_hash(fork.id.clone()))
        })
        .collect())
}

pub fn build_dependency_map<'a>(requirements: impl IntoIterator<Item = &'a PypiRequirement>) -> Result<BTreeMap<Ident, Descriptor>, Error> {
    let mut grouped = BTreeMap::<Ident, Descriptor>::new();

    for requirement in requirements {
        if let Some(existing) = grouped.get_mut(&requirement.ident) {
            merge_dependency_descriptor(existing, requirement.descriptor.clone())?;
        } else {
            grouped.insert(requirement.ident.clone(), requirement.descriptor.clone());
        }
    }

    Ok(grouped)
}

fn build_context_dependency_map(requirements: &[PypiRequirement], include_base: bool, active_extras: &PypiExtras, target: Option<&PythonTargetEnv>, require_target: bool) -> Result<BTreeMap<Ident, Descriptor>, Error> {
    let mut active = Vec::new();

    for requirement in requirements {
        let variables = marker_variables(&requirement.marker);
        let has_extra = variables.contains(&MarkerVariable::Extra);
        let needs_target = variables.iter().any(|variable| *variable != MarkerVariable::Extra);

        if needs_target && target.is_none() {
            if require_target {
                return Err(Error::InvalidResolution(format!(
                    "Cannot evaluate PyPI marker for {} without a Python target environment; configure supportedTargets with python.version",
                    requirement.ident.to_file_string(),
                )));
            }
            continue;
        }

        let applies = if has_extra {
            let mut applies = false;
            for extra in active_extras.iter() {
                if evaluate_marker(&requirement.marker, target, Some(extra))
                    .map_err(|err| Error::InvalidResolution(format!("Cannot evaluate PyPI marker for {}: {err}", requirement.ident.to_file_string())))? {
                    applies = true;
                    break;
                }
            }
            applies
        } else if include_base {
            evaluate_marker(&requirement.marker, target, None)
                .map_err(|err| Error::InvalidResolution(format!("Cannot evaluate PyPI marker for {}: {err}", requirement.ident.to_file_string())))?
        } else {
            false
        };

        if applies {
            active.push(requirement);
        }
    }

    build_dependency_map(active)
}

pub fn merge_dependency_descriptor(existing: &mut Descriptor, incoming: Descriptor) -> Result<(), Error> {
    if existing == &incoming {
        return Ok(());
    }
    if existing.ident != incoming.ident || existing.parent != incoming.parent {
        return Err(Error::InvalidResolution(format!("Cannot merge PyPI dependency descriptors {} and {}", existing.to_file_string(), incoming.to_file_string())));
    }

    let existing_file_string = existing.to_file_string();
    let incoming_file_string = incoming.to_file_string();
    merge_dependency_ranges(&mut existing.range, incoming.range).map_err(|_| {
        Error::InvalidResolution(format!("Cannot merge PyPI dependency descriptors {} and {}", existing_file_string, incoming_file_string))
    })
}

fn merge_dependency_ranges(existing: &mut Range, incoming: Range) -> Result<(), ()> {
    match (existing, incoming) {
        (Range::PypiSpecifier(existing), Range::PypiSpecifier(incoming)) => {
            existing.specifier = existing.specifier.intersection(&incoming.specifier)
                .map_err(|_| ())?;
            existing.parameters = match (&existing.parameters, &incoming.parameters) {
                (Some(left), Some(right)) => Some(left.merge(right).map_err(|_| ())?),
                (Some(left), None) => Some(left.clone()),
                (None, Some(right)) => Some(right.clone()),
                (None, None) => None,
            };
            Ok(())
        },
        (Range::Env(existing), Range::Env(incoming)) if existing.hash == incoming.hash => {
            merge_dependency_ranges(existing.inner.as_mut(), *incoming.inner)
        },
        _ => Err(()),
    }
}

fn descriptor_from_pypi_requirement(ident: Ident, specifier: PypiSpecifierSet, extras: PypiExtras) -> Descriptor {
    Descriptor::new(ident, Range::PypiSpecifier(PypiSpecifierRange {
        ident: None,
        specifier,
        parameters: (!extras.is_empty()).then(|| PypiRangeParameters::from_extras(extras)),
    }))
}

fn marker_variables(marker: &pep508_rs::MarkerTree) -> BTreeSet<MarkerVariable> {
    let mut variables
        = BTreeSet::new();

    collect_marker_variables(marker, &mut variables);

    variables
}

fn collect_marker_variables(marker: &pep508_rs::MarkerTree, variables: &mut BTreeSet<MarkerVariable>) {
    use pep508_rs::MarkerTreeKind;

    match marker.kind() {
        MarkerTreeKind::True | MarkerTreeKind::False => {},
        MarkerTreeKind::Version(node) => {
            variables.insert(match node.key() {
                pep508_rs::MarkerValueVersion::PythonVersion => MarkerVariable::PythonVersion,
                pep508_rs::MarkerValueVersion::PythonFullVersion => MarkerVariable::PythonFullVersion,
                pep508_rs::MarkerValueVersion::ImplementationVersion => MarkerVariable::ImplementationVersion,
            });
            for (_, child) in node.edges() {
                collect_marker_variables(&child, variables);
            }
        },
        MarkerTreeKind::String(node) => {
            variables.insert(marker_string_variable(node.key()));
            for (_, child) in node.children() {
                collect_marker_variables(&child, variables);
            }
        },
        MarkerTreeKind::In(node) => {
            variables.insert(marker_string_variable(node.key()));
            for (_, child) in node.children() {
                collect_marker_variables(&child, variables);
            }
        },
        MarkerTreeKind::Contains(node) => {
            variables.insert(marker_string_variable(node.key()));
            for (_, child) in node.children() {
                collect_marker_variables(&child, variables);
            }
        },
        MarkerTreeKind::Extra(node) => {
            variables.insert(MarkerVariable::Extra);
            for (_, child) in node.children() {
                collect_marker_variables(&child, variables);
            }
        },
    }
}

fn marker_string_variable(variable: &pep508_rs::MarkerValueString) -> MarkerVariable {
    use pep508_rs::MarkerValueString;

    match variable {
        MarkerValueString::ImplementationName => MarkerVariable::ImplementationName,
        MarkerValueString::OsName | MarkerValueString::OsNameDeprecated => MarkerVariable::OsName,
        MarkerValueString::PlatformMachine | MarkerValueString::PlatformMachineDeprecated => MarkerVariable::PlatformMachine,
        MarkerValueString::PlatformPythonImplementation
        | MarkerValueString::PlatformPythonImplementationDeprecated
        | MarkerValueString::PythonImplementationDeprecated => MarkerVariable::PlatformPythonImplementation,
        MarkerValueString::PlatformRelease => MarkerVariable::PlatformRelease,
        MarkerValueString::PlatformSystem => MarkerVariable::PlatformSystem,
        MarkerValueString::PlatformVersion | MarkerValueString::PlatformVersionDeprecated => MarkerVariable::PlatformVersion,
        MarkerValueString::SysPlatform | MarkerValueString::SysPlatformDeprecated => MarkerVariable::SysPlatform,
    }
}

fn target_marker_value<'a>(target: &'a PythonTargetEnv, variable: MarkerVariable) -> Option<&'a str> {
    match variable {
        MarkerVariable::PythonVersion => Some(&target.python_version),
        MarkerVariable::PythonFullVersion => target.python_full_version.as_deref(),
        MarkerVariable::OsName => target.os_name.as_deref(),
        MarkerVariable::SysPlatform => target.sys_platform.as_deref(),
        MarkerVariable::PlatformMachine => target.platform_machine.as_deref(),
        MarkerVariable::PlatformSystem => target.platform_system.as_deref(),
        MarkerVariable::PlatformRelease => target.platform_release.as_deref(),
        MarkerVariable::PlatformVersion => target.platform_version.as_deref(),
        MarkerVariable::PlatformPythonImplementation => target.platform_python_implementation.as_deref(),
        MarkerVariable::ImplementationName => target.implementation_name.as_deref(),
        MarkerVariable::ImplementationVersion => target.implementation_version.as_deref(),
        MarkerVariable::Extra => None,
    }
}

fn evaluate_marker(marker: &pep508_rs::MarkerTree, target: Option<&PythonTargetEnv>, extra: Option<&str>) -> Result<bool, String> {
    let extras = extra
        .map(pep508_rs::ExtraName::from_str)
        .transpose()
        .map_err(|error| error.to_string())?
        .into_iter()
        .collect::<Vec<_>>();

    let Some(target) = target else {
        return Ok(marker.evaluate_optional_environment(None, &extras));
    };

    for variable in marker_variables(marker) {
        if variable != MarkerVariable::Extra && target_marker_value(target, variable).is_none() {
            return Err(format!("marker target field {} is unavailable for this Python target", variable.as_str()));
        }
    }

    let python_full_version = target.python_full_version.as_deref().unwrap_or(&target.python_version);
    let environment = pep508_rs::MarkerEnvironment::try_from(pep508_rs::MarkerEnvironmentBuilder {
        implementation_name: target.implementation_name.as_deref().unwrap_or(""),
        implementation_version: target.implementation_version.as_deref().unwrap_or(python_full_version),
        os_name: target.os_name.as_deref().unwrap_or(""),
        platform_machine: target.platform_machine.as_deref().unwrap_or(""),
        platform_python_implementation: target.platform_python_implementation.as_deref().unwrap_or(""),
        platform_release: target.platform_release.as_deref().unwrap_or(""),
        platform_system: target.platform_system.as_deref().unwrap_or(""),
        platform_version: target.platform_version.as_deref().unwrap_or(""),
        python_full_version,
        python_version: &target.python_version,
        sys_platform: target.sys_platform.as_deref().unwrap_or(""),
    }).map_err(|error| error.to_string())?;

    Ok(marker.evaluate(&environment, &extras))
}

fn project_pep440_to_semver(version: &zpm_primitives::PypiVersion) -> Result<zpm_semver::Version, Error> {
    // TODO: Replace this lossy projection once `Resolution.version` can represent
    // non-semver registry versions without information loss.
    version.to_lossy_semver()
        .map_err(|err| Error::InvalidResolution(err.to_string()))
}

fn build_resolution_result(context: &InstallContext<'_>, locator: Locator, version: &zpm_primitives::PypiVersion, requires_dist: &[String], include_base: bool, active_extras: &PypiExtras) -> Result<ResolutionResult, Error> {
    let mut resolution
        = Resolution::new_empty(locator, project_pep440_to_semver(version)?);
    let requirements
        = parse_requires_dist(requires_dist)?;
    resolution.dependencies = build_context_dependency_map(&requirements, include_base, active_extras, None, false)?;
    resolution.into_resolution_result(context)
}

enum LocalWheelResolutionTarget<'a> {
    Unqualified,
    RequireTarget,
    Fork(&'a PythonFork),
}

fn resolve_file_descriptor_inner(
    context: &InstallContext<'_>,
    descriptor: &Descriptor,
    params: &PypiFileRange,
    target_mode: LocalWheelResolutionTarget<'_>,
) -> Result<ResolutionResult, Error> {
    let project
        = context.project
            .expect("The project is required for resolving local PyPI wheels");
    let wheel_path
        = resolve_local_wheel_path(project, descriptor.parent.as_ref(), &params.path)?;
    let wheel
        = wheel_path.fs_read()
            .map_err(|error| Error::InvalidResolution(format!("Cannot read local PyPI wheel `{}`: {error}", wheel_path.to_file_string())))?;
    let metadata
        = metadata_from_wheel(&wheel)?;
    let requested_ident
        = canonicalize_pypi_ident(&descriptor.ident)?;
    if metadata.ident != requested_ident {
        return Err(Error::InvalidResolution(format!(
            "Local PyPI wheel `{}` contains package `{}`, but is required as `{}`",
            params.path,
            metadata.ident.to_file_string(),
            requested_ident.to_file_string(),
        )));
    }

    if let LocalWheelResolutionTarget::Fork(fork) = &target_mode {
        if let Some(target) = &fork.target {
            let filename
                = wheel_path.basename()
                    .ok_or_else(|| Error::InvalidResolution(format!("Local PyPI wheel path has no filename: {}", params.path)))?;
            let distribution = PypiDistribution {
                filename: filename.to_string(),
                packagetype: "bdist_wheel".to_string(),
                url: String::new(),
                upload_time: None,
                upload_time_iso_8601: None,
                requires_python: None,
            };
            if select_best_wheel(&[distribution], Some(target)).is_none() {
                return Err(Error::InvalidResolution(format!(
                    "Local PyPI wheel `{}` is incompatible with the selected Python target",
                    params.path,
                )));
            }
        }
    }

    let checksum
        = Hash64::from_data(&wheel);
    let reference: Reference
        = PypiRegistryReference {
            ident: metadata.ident.clone(),
            version: metadata.version.clone(),
            url: Some(UrlEncoded::new(format_local_wheel_url(&params.path, &checksum))),
        }.into();
    let locator
        = Locator::new_bound(descriptor.ident.clone(), reference, descriptor.parent.clone().map(Arc::new));
    let active_extras
        = params.parameters.as_ref().and_then(|parameters| parameters.extras.clone()).unwrap_or_default();

    match target_mode {
        LocalWheelResolutionTarget::Unqualified => build_resolution_result(
            context,
            locator,
            &metadata.version,
            &metadata.requires_dist,
            true,
            &active_extras,
        ),
        LocalWheelResolutionTarget::RequireTarget => build_targetless_island_resolution_result(
            context,
            locator,
            &metadata.version,
            &metadata.requires_dist,
            true,
            &active_extras,
        ),
        LocalWheelResolutionTarget::Fork(fork) => build_fork_resolution_result(
            context,
            locator,
            &metadata.version,
            &metadata.requires_dist,
            fork,
            true,
            &active_extras,
        ),
    }
}

pub fn resolve_file_descriptor(context: &InstallContext<'_>, descriptor: &Descriptor, params: &PypiFileRange) -> Result<ResolutionResult, Error> {
    resolve_file_descriptor_inner(context, descriptor, params, LocalWheelResolutionTarget::Unqualified)
}

pub fn resolve_file_descriptor_requiring_python_target(context: &InstallContext<'_>, descriptor: &Descriptor, params: &PypiFileRange) -> Result<ResolutionResult, Error> {
    resolve_file_descriptor_inner(context, descriptor, params, LocalWheelResolutionTarget::RequireTarget)
}

pub fn resolve_file_descriptor_for_fork(context: &InstallContext<'_>, descriptor: &Descriptor, params: &PypiFileRange, fork: &PythonFork) -> Result<ResolutionResult, Error> {
    resolve_file_descriptor_inner(context, descriptor, params, LocalWheelResolutionTarget::Fork(fork))
}

async fn resolve_git_descriptor_inner(
    context: &InstallContext<'_>,
    descriptor: &Descriptor,
    params: &PypiGitRange,
    target_mode: LocalWheelResolutionTarget<'_>,
) -> Result<ResolutionResult, Error> {
    let project
        = context.project
            .expect("The project is required for resolving Python Git dependencies");
    let requested_ident
        = canonicalize_pypi_ident(&descriptor.ident)?;
    let build_registry
        = get_build_registry(&project.config, &requested_ident)?;
    let commit
        = Box::pin(crate::git::resolve_git_treeish(
            &params.git,
            &project.http_client.config,
            &project.config.settings.approved_git_repositories,
        )).await?;
    let reference = zpm_git::GitReference {
        repo: params.git.repo.clone(),
        commit,
        prepare_params: params.git.prepare_params.clone(),
    };
    let repository_path
        = Box::pin(crate::git::clone_repository(context, &reference.repo, &reference.commit)).await?;
    let metadata_result = match python_git_project_path(&repository_path, &reference) {
        Ok(project_path) => match metadata_from_source_tree(&project_path) {
            Ok(metadata) => Ok(metadata),
            Err(_) => {
                Box::pin(prepare::python::prepare_source_tree(
                    &project_path,
                    None,
                    None,
                    &build_registry,
                )).await
                    .and_then(|wheel| metadata_from_wheel(&wheel))
            },
        },
        Err(error) => Err(error),
    };
    let _ = repository_path.fs_rm();
    let metadata
        = metadata_result?;
    if metadata.ident != requested_ident {
        return Err(Error::InvalidResolution(format!(
            "Python Git dependency contains package `{}`, but is required as `{}`",
            metadata.ident.to_file_string(),
            requested_ident.to_file_string(),
        )));
    }

    let locator = Locator::new(
        requested_ident,
        PypiRegistryReference {
            ident: metadata.ident.clone(),
            version: metadata.version.clone(),
            url: Some(UrlEncoded::new(format_python_git_url(&reference))),
        }.into(),
    );

    match target_mode {
        LocalWheelResolutionTarget::Unqualified => build_resolution_result(
            context,
            locator,
            &metadata.version,
            &metadata.requires_dist,
            true,
            &PypiExtras::empty(),
        ),
        LocalWheelResolutionTarget::RequireTarget => build_targetless_island_resolution_result(
            context,
            locator,
            &metadata.version,
            &metadata.requires_dist,
            true,
            &PypiExtras::empty(),
        ),
        LocalWheelResolutionTarget::Fork(fork) => build_fork_resolution_result(
            context,
            locator,
            &metadata.version,
            &metadata.requires_dist,
            fork,
            true,
            &PypiExtras::empty(),
        ),
    }
}

pub async fn resolve_git_descriptor(context: &InstallContext<'_>, descriptor: &Descriptor, params: &PypiGitRange) -> Result<ResolutionResult, Error> {
    resolve_git_descriptor_inner(context, descriptor, params, LocalWheelResolutionTarget::Unqualified).await
}

pub async fn resolve_git_descriptor_requiring_python_target(context: &InstallContext<'_>, descriptor: &Descriptor, params: &PypiGitRange) -> Result<ResolutionResult, Error> {
    resolve_git_descriptor_inner(context, descriptor, params, LocalWheelResolutionTarget::RequireTarget).await
}

pub async fn resolve_git_descriptor_for_fork(context: &InstallContext<'_>, descriptor: &Descriptor, params: &PypiGitRange, fork: &PythonFork) -> Result<ResolutionResult, Error> {
    resolve_git_descriptor_inner(context, descriptor, params, LocalWheelResolutionTarget::Fork(fork)).await
}

fn build_targetless_island_resolution_result(context: &InstallContext<'_>, locator: Locator, version: &zpm_primitives::PypiVersion, requires_dist: &[String], include_base: bool, active_extras: &PypiExtras) -> Result<ResolutionResult, Error> {
    let mut resolution
        = Resolution::new_empty(locator, project_pep440_to_semver(version)?);
    let requirements
        = parse_requires_dist(requires_dist)?;
    resolution.dependencies = build_context_dependency_map(&requirements, include_base, active_extras, None, true)?;
    resolution.into_resolution_result(context)
}

fn build_fork_resolution_result(context: &InstallContext<'_>, locator: Locator, version: &zpm_primitives::PypiVersion, requires_dist: &[String], fork: &PythonFork, include_base: bool, active_extras: &PypiExtras) -> Result<ResolutionResult, Error> {
    let mut resolution
        = Resolution::new_empty(locator.env_qualified_with_hash(fork.id.clone()), project_pep440_to_semver(version)?);
    let requirements
        = parse_requires_dist(requires_dist)?;
    resolution.dependencies = build_fork_dependency_map_with_extras(&requirements, fork, include_base, active_extras)?;
    resolution.into_resolution_result(context)
}

fn select_version_for_specifier(releases: &BTreeMap<String, Vec<PypiDistribution>>, specifier: &zpm_primitives::PypiSpecifierSet) -> Result<Option<(zpm_primitives::PypiVersion, Vec<PypiDistribution>)>, Error> {
    let mut best_any: Option<(zpm_primitives::PypiVersion, Vec<PypiDistribution>)>
        = None;
    let mut best_stable: Option<(zpm_primitives::PypiVersion, Vec<PypiDistribution>)>
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

        let should_replace_any
            = best_any.as_ref()
                .map(|(best_version, _)| {
                    version.cmp_pep440(best_version)
                        .map(|ordering| ordering.is_gt())
                        .unwrap_or(false)
                })
                .unwrap_or(true);

        if should_replace_any {
            best_any = Some((version.clone(), distributions.clone()));
        }

        if version.is_stable()
            .map_err(|err| Error::InvalidRange(err.to_string()))?
        {
            let should_replace_stable
                = best_stable.as_ref()
                    .map(|(best_version, _)| {
                        version.cmp_pep440(best_version)
                            .map(|ordering| ordering.is_gt())
                            .unwrap_or(false)
                    })
                    .unwrap_or(true);

            if should_replace_stable {
                best_stable = Some((version, distributions.clone()));
            }
        }
    }

    let allows_prereleases
        = specifier.allows_prereleases()
            .map_err(|err| Error::InvalidRange(err.to_string()))?;

    Ok(if allows_prereleases {
        best_any
    } else {
        best_stable.or(best_any)
    })
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

async fn fetch_json<T>(context: &InstallContext<'_>, url: &str, authorization: Option<&str>) -> Result<T, Error>
where
    T: DeserializeOwned,
{
    let project
        = context.project
        .expect("The project is required for resolving PyPI packages");

    let bytes
        = project.http_client.cached_get_with_authorization(url, authorization).await?;

    let value: T
        = JsonDocument::hydrate_from_slice(&bytes[..])?;
    Ok(value)
}

fn is_not_found(error: &Error) -> bool {
    matches!(error, Error::HttpError {inner, ..} if inner.status() == Some(reqwest::StatusCode::NOT_FOUND))
}

async fn fetch_simple_project_metadata(context: &InstallContext<'_>, package_ident: &Ident, base: &str, authorization: Option<&str>) -> Result<PypiProjectMetadata, Error> {
    let project
        = context.project
            .expect("The project is required for resolving PyPI packages");
    let url
        = format!("{}/{}/", base, encode_path_segment(package_ident.as_str()));
    let bytes
        = project.http_client.cached_get_with_authorization(&url, authorization).await?;
    let html
        = std::str::from_utf8(&bytes)
            .map_err(|error| Error::InvalidResolution(format!("PyPI Simple API response is not UTF-8: {error}")))?;

    Ok(PypiProjectMetadata {
        releases: parse_simple_project(html, &url, package_ident)?,
    })
}

async fn fetch_project_metadata(context: &InstallContext<'_>, package_ident: &Ident) -> Result<PypiProjectMetadata, Error> {
    let package_ident
        = canonicalize_pypi_ident(package_ident)?;
    let project
        = context.project
            .expect("The project is required for resolving PyPI packages");
    let base
        = get_registry(&project.config, &package_ident);
    let authorization
        = get_authorization(&project.config, &base, &package_ident);
    let url
        = format!("{}/pypi/{}/json", base, encode_path_segment(package_ident.as_str()));
    match fetch_json(context, &url, authorization.as_deref()).await {
        Ok(metadata) => Ok(metadata),
        Err(error) if is_not_found(&error) => {
            fetch_simple_project_metadata(context, &package_ident, &base, authorization.as_deref()).await
        },
        Err(error) => Err(error),
    }
}

async fn fetch_version_metadata(context: &InstallContext<'_>, package_ident: &Ident, version: &zpm_primitives::PypiVersion) -> Result<PypiVersionMetadata, Error> {
    let package_ident
        = canonicalize_pypi_ident(package_ident)?;
    let project
        = context.project
            .expect("The project is required for resolving PyPI packages");
    let base
        = get_registry(&project.config, &package_ident);
    let authorization
        = get_authorization(&project.config, &base, &package_ident);
    let url
        = format!(
            "{}/pypi/{}/{}/json",
            base,
            encode_path_segment(package_ident.as_str()),
            encode_path_segment(&version.to_file_string()),
        );

    fetch_json(context, &url, authorization.as_deref()).await
}

async fn fetch_requires_dist(context: &InstallContext<'_>, locator: &Locator, package_ident: &Ident, version: &PypiVersion, artifact_url: Option<&str>) -> Result<Vec<String>, Error> {
    if let Some(source) = artifact_url.map(parse_local_wheel_url).transpose()?.flatten() {
        let project
            = context.project
                .expect("The project is required for resolving local PyPI wheels");
        let wheel_path
            = resolve_local_wheel_path(project, locator.parent.as_deref(), &source.path)?;
        let wheel
            = wheel_path.fs_read()
                .map_err(|error| Error::InvalidResolution(format!("Cannot read local PyPI wheel `{}`: {error}", wheel_path.to_file_string())))?;
        if Hash64::from_data(&wheel) != source.checksum {
            return Err(Error::InvalidResolution(format!(
                "Local PyPI wheel `{}` changed since the lockfile was generated; run install to refresh the lockfile",
                source.path,
            )));
        }
        let metadata
            = metadata_from_wheel(&wheel)?;
        if &metadata.ident != package_ident || !metadata.version.cmp_pep440(version)
            .map_err(|error| Error::InvalidResolution(error.to_string()))?
            .is_eq()
        {
            return Err(Error::InvalidResolution(format!(
                "Local PyPI wheel `{}` metadata doesn't match {}@{}",
                source.path,
                package_ident.to_file_string(),
                version.to_file_string(),
            )));
        }
        return Ok(metadata.requires_dist);
    }

    if let Some(reference) = artifact_url.map(parse_python_git_url).transpose()?.flatten() {
        let target
            = crate::fetchers::pypi::preparation_target(context, locator)?;
        let prepared
            = crate::fetchers::pypi::prepare_git_wheel(
                context,
                package_ident,
                &reference,
                target.as_ref(),
                crate::fetchers::pypi::environment_hash(&locator.reference),
            ).await?;
        let metadata
            = metadata_from_wheel(&prepared.data)?;
        if &metadata.ident != package_ident || !metadata.version.cmp_pep440(version)
            .map_err(|error| Error::InvalidResolution(error.to_string()))?
            .is_eq()
        {
            return Err(Error::InvalidResolution(format!(
                "Python Git dependency metadata doesn't match {}@{}",
                package_ident.to_file_string(),
                version.to_file_string(),
            )));
        }
        return Ok(metadata.requires_dist);
    }

    match fetch_version_metadata(context, package_ident, version).await {
        Ok(metadata) => return Ok(metadata.info.requires_dist.unwrap_or_default()),
        Err(error) if is_not_found(&error) => {},
        Err(error) => return Err(error),
    }

    let project
        = context.project
            .expect("The project is required for resolving PyPI packages");
    let base
        = get_registry(&project.config, package_ident);
    let authorization
        = get_authorization(&project.config, &base, package_ident);
    let simple_metadata
        = fetch_simple_project_metadata(context, package_ident, &base, authorization.as_deref()).await?;
    let release
        = simple_metadata.releases.get(&version.to_file_string())
            .ok_or_else(|| Error::InvalidResolution(format!("PyPI Simple API has no artifacts for {}@{}", package_ident.to_file_string(), version.to_file_string())))?;
    let metadata_artifact = artifact_url
        .filter(|url| url::Url::parse(url).ok().and_then(|url| url.path_segments()?.next_back().map(|name| name.ends_with(".whl"))).unwrap_or(false))
        .map(str::to_string)
        .or_else(|| release.iter().find(|distribution| distribution.filename.ends_with(".whl")).map(|distribution| distribution.url.clone()))
        .ok_or_else(|| Error::InvalidResolution(format!(
            "Cannot read dependency metadata for {}@{} from a Simple API registry because the release has no wheel",
            package_ident.to_file_string(),
            version.to_file_string(),
        )))?;
    let artifact_authorization
        = get_artifact_authorization(&project.config, &base, package_ident, &metadata_artifact);
    let wheel
        = project.http_client.cached_get_with_authorization(&metadata_artifact, artifact_authorization.as_deref()).await?;

    requires_dist_from_wheel(&wheel)
}

pub async fn resolve_versions(context: &InstallContext<'_>, package_ident: &Ident) -> Result<Vec<Locator>, Error> {
    resolve_versions_for_target(context, package_ident, None).await
}

pub async fn resolve_versions_for_target(context: &InstallContext<'_>, package_ident: &Ident, target: Option<&PythonTargetEnv>) -> Result<Vec<Locator>, Error> {
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

        let Some(artifact) = select_best_artifact(&release_distributions, target) else {
            continue;
        };

        locators.push(Locator::new(package_ident.clone(), PypiRegistryReference {
            ident: package_ident.clone(),
            version,
            url: Some(UrlEncoded::new(artifact.url.clone())),
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

    let artifact
        = select_best_artifact(&release_distributions, None)
            .ok_or_else(|| Error::InvalidResolution(format!("No compatible PyPI artifact found for {}@{}", package_ident.to_file_string(), resolved_version.to_file_string())))?;

    let reference: Reference
        = PypiRegistryReference {
            ident: package_ident.clone(),
            version: resolved_version.clone(),
            url: Some(UrlEncoded::new(artifact.url.clone())),
        }.into();
    let locator
        = if params.ident.is_some() {
            descriptor.resolve_with(reference)
        } else {
            Locator::new(package_ident.clone(), reference)
        };

    let requires_dist
        = fetch_requires_dist(context, &locator, &package_ident, &resolved_version, Some(&artifact.url)).await?;

    let active_extras = params.parameters.as_ref().and_then(|parameters| parameters.extras.clone()).unwrap_or_default();
    build_resolution_result(context, locator, &resolved_version, &requires_dist, true, &active_extras)
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

    let artifact
        = select_best_artifact(&release_distributions, None)
            .ok_or_else(|| Error::InvalidResolution(format!("No compatible PyPI artifact found for {}@{}", package_ident.to_file_string(), resolved_version.to_file_string())))?;

    let reference: Reference
        = PypiRegistryReference {
            ident: package_ident.clone(),
            version: resolved_version.clone(),
            url: Some(UrlEncoded::new(artifact.url.clone())),
        }.into();
    let locator
        = if params.ident.is_some() {
            descriptor.resolve_with(reference)
        } else {
            Locator::new(package_ident.clone(), reference)
        };

    let requires_dist
        = fetch_requires_dist(context, &locator, &package_ident, &resolved_version, Some(&artifact.url)).await?;

    let active_extras = params.parameters.as_ref().and_then(|parameters| parameters.extras.clone()).unwrap_or_default();
    build_resolution_result(context, locator, &resolved_version, &requires_dist, true, &active_extras)
}

pub async fn resolve_locator(context: &InstallContext<'_>, locator: &Locator, params: &PypiRegistryReference) -> Result<ResolutionResult, Error> {
    let package_ident
        = canonicalize_pypi_ident(&params.ident)?;
    let requires_dist
        = fetch_requires_dist(context, locator, &package_ident, &params.version, params.url.as_ref().map(|url| url.0.as_str())).await?;

    build_resolution_result(context, locator.clone(), &params.version, &requires_dist, true, &PypiExtras::empty())
}

pub async fn resolve_locator_requiring_python_target(context: &InstallContext<'_>, locator: &Locator, params: &PypiRegistryReference) -> Result<ResolutionResult, Error> {
    let package_ident
        = canonicalize_pypi_ident(&params.ident)?;
    let requires_dist
        = fetch_requires_dist(context, locator, &package_ident, &params.version, params.url.as_ref().map(|url| url.0.as_str())).await?;

    build_targetless_island_resolution_result(context, locator.clone(), &params.version, &requires_dist, true, &PypiExtras::empty())
}

pub async fn resolve_locator_for_fork(context: &InstallContext<'_>, locator: &Locator, params: &PypiRegistryReference, fork: &PythonFork) -> Result<ResolutionResult, Error> {
    let package_ident
        = canonicalize_pypi_ident(&params.ident)?;
    let requires_dist
        = fetch_requires_dist(context, locator, &package_ident, &params.version, params.url.as_ref().map(|url| url.0.as_str())).await?;

    build_fork_resolution_result(context, locator.clone(), &params.version, &requires_dist, fork, true, &PypiExtras::empty())
}

pub async fn resolve_locator_extra(context: &InstallContext<'_>, locator: &Locator, params: &PypiRegistryReference, extra: &str) -> Result<ResolutionResult, Error> {
    resolve_locator_extra_without_target(context, locator, params, extra, false).await
}

pub async fn resolve_locator_extra_requiring_python_target(context: &InstallContext<'_>, locator: &Locator, params: &PypiRegistryReference, extra: &str) -> Result<ResolutionResult, Error> {
    resolve_locator_extra_without_target(context, locator, params, extra, true).await
}

pub async fn resolve_locator_extra_for_fork(context: &InstallContext<'_>, locator: &Locator, params: &PypiRegistryReference, extra: &str, fork: &PythonFork) -> Result<ResolutionResult, Error> {
    let package_ident = canonicalize_pypi_ident(&params.ident)?;
    let requires_dist = fetch_requires_dist(context, locator, &package_ident, &params.version, params.url.as_ref().map(|url| url.0.as_str())).await?;
    let active_extras = PypiExtras::from_iter([extra]).map_err(|err| Error::InvalidRange(err.to_string()))?;

    build_fork_resolution_result(context, locator.clone(), &params.version, &requires_dist, fork, false, &active_extras)
}

async fn resolve_locator_extra_without_target(context: &InstallContext<'_>, locator: &Locator, params: &PypiRegistryReference, extra: &str, require_target: bool) -> Result<ResolutionResult, Error> {
    let package_ident = canonicalize_pypi_ident(&params.ident)?;
    let requires_dist = fetch_requires_dist(context, locator, &package_ident, &params.version, params.url.as_ref().map(|url| url.0.as_str())).await?;
    let active_extras = PypiExtras::from_iter([extra]).map_err(|err| Error::InvalidRange(err.to_string()))?;

    if require_target {
        build_targetless_island_resolution_result(context, locator.clone(), &params.version, &requires_dist, false, &active_extras)
    } else {
        build_resolution_result(context, locator.clone(), &params.version, &requires_dist, false, &active_extras)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn releases(versions: &[&str]) -> BTreeMap<String, Vec<PypiDistribution>> {
        versions.iter()
            .map(|version| ((*version).to_string(), Vec::new()))
            .collect()
    }

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
    fn test_select_version_prefers_stable_for_ordinary_specifier() {
        let selected = select_version_for_specifier(
            &releases(&["0.28.1", "1.0.dev6"]),
            &PypiSpecifierSet::from_file_string(">=0.28.0").unwrap(),
        ).unwrap().unwrap();

        assert_eq!("0.28.1", selected.0.to_file_string());
    }

    #[test]
    fn test_select_version_allows_explicit_prerelease() {
        let selected = select_version_for_specifier(
            &releases(&["1.0.dev5", "1.0.dev6"]),
            &PypiSpecifierSet::from_file_string(">=1.0.dev5").unwrap(),
        ).unwrap().unwrap();

        assert_eq!("1.0.dev6", selected.0.to_file_string());
    }

    #[test]
    fn test_select_version_falls_back_to_prerelease() {
        let selected = select_version_for_specifier(
            &releases(&["1.0.dev5", "1.0.dev6"]),
            &PypiSpecifierSet::from_file_string("*").unwrap(),
        ).unwrap().unwrap();

        assert_eq!("1.0.dev6", selected.0.to_file_string());
    }

    #[test]
    fn test_parse_requires_dist_canonicalizes_names_and_keeps_markers() {
        let requirement
            = parse_one("Friendly_Bard.Name (>=1.0.0); python_version >= '3.11'");

        assert_eq!("friendly-bard-name", requirement.ident.to_file_string());
        assert_eq!("friendly-bard-name@pypi:>=1.0.0", requirement.descriptor.to_file_string());
        assert!(!requirement.marker.is_true());
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
    fn test_canonicalize_pypi_descriptor_preserves_env_qualification() {
        let fork_id
            = zpm_utils::Hash64::from_data("python-fork");
        let descriptor
            = Descriptor::from_file_string("Friendly_Bard.Name@pypi:>=1.0.0").unwrap()
                .env_qualified_with_hash(fork_id.clone());
        let (package_ident, descriptor)
            = canonicalize_pypi_descriptor(&descriptor).unwrap();

        assert_eq!("friendly-bard-name", package_ident.to_file_string());
        assert_eq!(
            format!("friendly-bard-name@env:{}#pypi:>=1.0.0", fork_id.to_file_string()),
            descriptor.to_file_string(),
        );
    }

    #[test]
    fn test_parse_requires_dist_keeps_requested_extras() {
        let requirement = parse_one("friendly-bard[http] >=1.0.0");
        let Range::PypiSpecifier(range) = requirement.descriptor.range else {
            panic!("expected PyPI specifier range");
        };

        assert!(range.parameters.unwrap().extras.unwrap().contains("http"));
    }

    #[test]
    fn test_parse_requires_dist_keeps_extra_only_markers() {
        let requirement = parse_one("friendly-bard >=1.0.0; extra == 'http'");
        assert!(marker_variables(&requirement.marker).contains(&MarkerVariable::Extra));
    }

    #[test]
    fn test_parse_requires_dist_keeps_mixed_extra_markers() {
        let requirement = parse_one("friendly-bard >=1.0.0; extra == 'http' and python_version >= '3.11'");
        let variables = marker_variables(&requirement.marker);

        assert!(variables.contains(&MarkerVariable::Extra));
        assert!(variables.contains(&MarkerVariable::PythonFullVersion));
    }

    #[test]
    fn test_parse_requires_dist_supports_parenthesized_markers() {
        let requirement = parse_one(
            "Brotli>=1.2; (platform_python_implementation == 'CPython' and sys_platform != 'android' and sys_platform != 'ios') and extra == 'speedups'",
        );
        let variables = marker_variables(&requirement.marker);

        assert!(variables.contains(&MarkerVariable::PlatformPythonImplementation));
        assert!(variables.contains(&MarkerVariable::SysPlatform));
        assert!(variables.contains(&MarkerVariable::Extra));
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
