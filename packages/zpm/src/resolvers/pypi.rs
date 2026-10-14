//! PyPI resolver.
//!
//! A `pypi:` descriptor resolves to a `pypi:<version>` locator whose
//! dependencies come from the release's core metadata (`Requires-Dist`).
//! Markers are evaluated across the install's target environments (see
//! `python_env`): requirements true on every target become regular
//! dependencies, those false everywhere are dropped, and the others keep
//! their marker in the `marker` range parameter so the linker can skip
//! them where they don't apply.
//!
//! When no single artifact suits every target (platform-specific wheels),
//! the resolution lists one variant per target platform
//! (`pypi:==<version>#platform=<platform>`), each carrying its os/cpu/libc
//! requirements. The tree resolver picks the variant matching the current
//! system, like it does for `@yarnpkg/node`, and only the artifacts covered
//! by `supportedArchitectures` get downloaded.

use std::{collections::{BTreeMap, BTreeSet}, str::FromStr, sync::Arc};

use zpm_primitives::{
    Descriptor,
    Ident,
    Locator,
    PypiExtras,
    PypiRangeParameters,
    PypiRegistryReference,
    PypiSpecifierRange,
    PypiSpecifierSet,
    PypiTagRange,
    PypiVersion,
    Range,
    Reference,
    canonicalize_pypi_name,
};
use zpm_utils::{FromFileString, ToFileString, UrlEncoded};

use crate::{
    error::Error,
    install::{InstallContext, InstallOpResult, IntoResolutionResult, ResolutionResult},
    pypi::{self, PypiFile, PypiProject},
    python_env::{MarkerOutcome, Platform, PythonEnv, PythonTargets, evaluate_across, known_platforms, marker_mentions_extra},
    resolvers::Resolution,
};

pub fn context_targets(context: &InstallContext<'_>) -> PythonTargets {
    let project
        = context.project
            .expect("The project is required for resolving PyPI packages");

    PythonTargets::from_config(&project.config, context.python_version.as_deref())
}

pub fn platform_key(platform: &Platform) -> String {
    let cpu
        = platform.cpu.to_file_string();

    match &platform.libc {
        Some(libc) => format!("{}-{}-{}", platform.sys_platform(), cpu, libc.to_file_string()),
        None => format!("{}-{}", platform.sys_platform(), cpu),
    }
}

/// The key of a platform variant: the platform and the Python version, as
/// the same release resolves to different wheels in islands targeting
/// different Python versions (`darwin-arm64-cp312`).
pub fn variant_key(env: &PythonEnv) -> String {
    format!("{}-cp{}{}", platform_key(&env.platform), env.python.major, env.python.minor)
}

pub fn env_from_variant_key(key: &str) -> Option<PythonEnv> {
    let (platform, python) = key.rsplit_once("-cp")?;

    let python = crate::python_env::PythonVersion {
        major: python.get(..1)?.parse().ok()?,
        minor: python.get(1..)?.parse().ok()?,
        patch: None,
    };

    known_platforms().into_iter()
        .find(|candidate| platform_key(candidate) == platform)
        .map(|platform| PythonEnv {python, platform})
}

/// Pure wheels and sdists are the same for every target.
fn is_universal_file(file: &PypiFile) -> bool {
    file.is_sdist() || file.filename.ends_with("-none-any.whl")
}

fn comparator_to_str(comparator: pep_508::Comparator) -> &'static str {
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

pub fn specifier_from_pep508(spec: Option<&pep_508::Spec<'_>>) -> Option<PypiSpecifierSet> {
    let Some(spec) = spec else {
        return Some(PypiSpecifierSet::any());
    };

    let pep_508::Spec::Version(specifiers) = spec else {
        return None;
    };

    let specifier
        = specifiers.iter()
            .map(|specifier| format!("{}{}", comparator_to_str(specifier.comparator), specifier.version))
            .collect::<Vec<_>>()
            .join(",");

    PypiSpecifierSet::from_file_string(&specifier).ok()
}

fn render_variable(variable: &pep_508::Variable<'_>) -> String {
    match variable {
        pep_508::Variable::PythonVersion => "python_version".to_string(),
        pep_508::Variable::PythonFullVersion => "python_full_version".to_string(),
        pep_508::Variable::OsName => "os_name".to_string(),
        pep_508::Variable::SysPlatform => "sys_platform".to_string(),
        pep_508::Variable::PlatformRelease => "platform_release".to_string(),
        pep_508::Variable::PlatformSystem => "platform_system".to_string(),
        pep_508::Variable::PlatformVersion => "platform_version".to_string(),
        pep_508::Variable::PlatformMachine => "platform_machine".to_string(),
        pep_508::Variable::PlatformPythonImplementation => "platform_python_implementation".to_string(),
        pep_508::Variable::ImplementationName => "implementation_name".to_string(),
        pep_508::Variable::ImplementationVersion => "implementation_version".to_string(),
        pep_508::Variable::Extra => "extra".to_string(),
        pep_508::Variable::String(value) => format!("'{}'", value),
    }
}

/// Renders a marker without its `extra == "..."` clauses; used once the
/// extra has been applied, to only keep the environment conditions.
fn render_environment_marker(marker: &pep_508::Marker<'_>) -> Option<String> {
    match marker {
        pep_508::Marker::And(lhs, rhs) => match (render_environment_marker(lhs), render_environment_marker(rhs)) {
            (Some(lhs), Some(rhs)) => Some(format!("({}) and ({})", lhs, rhs)),
            (Some(value), None) | (None, Some(value)) => Some(value),
            (None, None) => None,
        },

        pep_508::Marker::Or(lhs, rhs) => match (render_environment_marker(lhs), render_environment_marker(rhs)) {
            (Some(lhs), Some(rhs)) => Some(format!("({}) or ({})", lhs, rhs)),
            (Some(value), None) | (None, Some(value)) => Some(value),
            (None, None) => None,
        },

        pep_508::Marker::Operator(lhs, operator, rhs) => {
            if matches!(lhs, pep_508::Variable::Extra) || matches!(rhs, pep_508::Variable::Extra) {
                return None;
            }

            let operator = match operator {
                pep_508::Operator::Comparator(comparator) => comparator_to_str(*comparator).to_string(),
                pep_508::Operator::In => "in".to_string(),
                pep_508::Operator::NotIn => "not in".to_string(),
            };

            Some(format!("{} {} {}", render_variable(lhs), operator, render_variable(rhs)))
        },
    }
}

/// The environment marker of a requirement (without `extra` clauses), as
/// written back in `pypi:` ranges.
pub fn marker_of(requirement: &str) -> Option<String> {
    let parsed
        = pep_508::parse(requirement).ok()?;

    parsed.marker.as_ref().and_then(render_environment_marker)
}

pub struct ConvertedRequirement {
    pub ident: Ident,
    pub descriptor: Descriptor,
}

/// Converts a PEP 508 requirement into a `pypi:` descriptor. Returns `None`
/// when the requirement doesn't apply to any target (or to the requested
/// extra), or when it can't be represented (direct URL requirements).
pub fn convert_requirement(requirement: &str, targets: &PythonTargets, extra: Option<&str>) -> Option<ConvertedRequirement> {
    let parsed
        = pep_508::parse(requirement).ok()?;

    let mentions_extra
        = parsed.marker.as_ref().map_or(false, marker_mentions_extra);

    // Base dependencies never include requirements guarded by an extra, and
    // the dependencies of an extra only include those mentioning it.
    if extra.is_some() != mentions_extra {
        return None;
    }

    let outcome
        = evaluate_across(parsed.marker.as_ref(), &targets.envs, extra);

    let marker = match outcome {
        MarkerOutcome::Never => return None,
        MarkerOutcome::Always => None,
        MarkerOutcome::Sometimes => parsed.marker.as_ref().and_then(render_environment_marker),
    };

    let ident
        = Ident::from_file_string(&canonicalize_pypi_name(parsed.name)).ok()?;

    let specifier
        = specifier_from_pep508(parsed.spec.as_ref())?;

    let extras
        = PypiExtras::from_iter(parsed.extras).ok()?;

    let descriptor
        = Descriptor::new(ident.clone(), Range::PypiSpecifier(PypiSpecifierRange {
            ident: None,
            specifier,
            parameters: PypiRangeParameters::new(extras, marker),
        }.into()));

    Some(ConvertedRequirement {ident, descriptor})
}

pub fn merge_dependency_descriptor(existing: &mut Descriptor, incoming: Descriptor) -> Result<(), Error> {
    if existing == &incoming {
        return Ok(());
    }

    let existing_file_string
        = existing.to_file_string();
    let incoming_file_string
        = incoming.to_file_string();

    match (&mut existing.range, incoming.range) {
        (Range::PypiSpecifier(existing), Range::PypiSpecifier(incoming)) => {
            existing.specifier = existing.specifier.intersection(&incoming.specifier)
                .map_err(|err| Error::InvalidRange(err.to_string()))?;

            let lhs_conditional
                = existing.parameters.as_ref().map_or(false, |parameters| parameters.marker.is_some());
            let rhs_conditional
                = incoming.parameters.as_ref().map_or(false, |parameters| parameters.marker.is_some());

            let mut merged = match (&existing.parameters, &incoming.parameters) {
                (Some(lhs), Some(rhs)) => lhs.merge(rhs).map_err(|err| Error::InvalidRange(err.to_string()))?,
                (Some(parameters), None) | (None, Some(parameters)) => parameters.clone(),
                (None, None) => PypiRangeParameters::empty(),
            };

            // An unconditional edge absorbs a conditional one
            if !lhs_conditional || !rhs_conditional {
                merged.marker = None;
            }

            existing.parameters = (!merged.is_empty()).then_some(merged);

            Ok(())
        },

        _ => Err(Error::InvalidResolution(format!(
            "Cannot merge PyPI dependency descriptors {} and {}",
            existing_file_string,
            incoming_file_string,
        ))),
    }
}

pub fn build_dependencies(requirements: &[String], targets: &PythonTargets, extra: Option<&str>) -> Result<BTreeMap<Ident, Descriptor>, Error> {
    let mut dependencies
        = BTreeMap::<Ident, Descriptor>::new();

    for converted in requirements.iter().filter_map(|requirement| convert_requirement(requirement, targets, extra)) {
        match dependencies.get_mut(&converted.ident) {
            Some(existing) => {
                merge_dependency_descriptor(existing, converted.descriptor)?;
            },

            None => {
                dependencies.insert(converted.ident, converted.descriptor);
            },
        }
    }

    Ok(dependencies)
}

fn project_pep440_to_semver(version: &PypiVersion) -> Result<zpm_semver::Version, Error> {
    version.to_lossy_semver()
        .map_err(|err| Error::InvalidResolution(err.to_string()))
}

pub async fn fetch_project(context: &InstallContext<'_>, ident: &Ident, known_version: Option<&PypiVersion>) -> Result<Arc<PypiProject>, Error> {
    let project
        = context.project
            .expect("The project is required for resolving PyPI packages");

    let known_version
        = known_version.filter(|_| !context.refresh_lockfile);

    pypi::fetch_project(&project.config, &project.http_client, ident, known_version).await
}

pub fn version_files<'a>(project: &'a PypiProject, ident: &Ident, version: &PypiVersion) -> Result<&'a Vec<PypiFile>, Error> {
    project.releases.get(version)
        .ok_or_else(|| Error::InvalidResolution(format!("Version {} of {} isn't available on the index", version.to_file_string(), ident.as_str())))
}

pub async fn fetch_requires_dist(context: &InstallContext<'_>, ident: &Ident, version: &PypiVersion) -> Result<Vec<String>, Error> {
    let project
        = context.project
            .expect("The project is required for resolving PyPI packages");

    let index_project
        = fetch_project(context, ident, Some(version)).await?;

    let files
        = version_files(&index_project, ident, version)?;

    let targets
        = context_targets(context);

    let file
        = pypi::select_metadata_file(files, &targets, None)
            .or_else(|| files.iter().find(|file| file.is_wheel()))
            .or_else(|| files.first())
            .ok_or_else(|| Error::InvalidResolution(format!("No artifact found for {}@{}", ident.as_str(), version.to_file_string())))?;

    let metadata
        = pypi::fetch_core_metadata(&project.config, &project.http_client, ident, file).await?;

    Ok(metadata.requires_dist.clone())
}

pub fn variant_descriptor(locator: &Locator, ident: &Ident, version: &PypiVersion, env: &PythonEnv) -> Descriptor {
    let mut parameters
        = PypiRangeParameters::empty();

    parameters.platform = Some(variant_key(env));

    let specifier
        = PypiSpecifierSet::from_file_string(&format!("=={}", version.to_file_string()))
            .unwrap();

    let range_ident
        = (ident != &locator.ident).then(|| ident.clone());

    Descriptor::new(locator.ident.clone(), Range::PypiSpecifier(PypiSpecifierRange {
        ident: range_ident,
        specifier,
        parameters: Some(parameters),
    }.into()))
}

/// Builds the resolution of a PyPI release. With `extra` set, only the
/// dependencies of that extra are listed (used by the island solver).
pub async fn resolve_release(context: &InstallContext<'_>, locator: Locator, ident: &Ident, version: &PypiVersion, extra: Option<&str>) -> Result<ResolutionResult, Error> {
    let targets
        = context_targets(context);

    let requires_dist
        = fetch_requires_dist(context, ident, version).await?;

    let mut resolution
        = Resolution::new_empty(locator.clone(), project_pep440_to_semver(version)?);

    resolution.dependencies
        = build_dependencies(&requires_dist, &targets, extra)?;

    if extra.is_none() {
        let index_project
            = fetch_project(context, ident, Some(version)).await?;

        let files
            = version_files(&index_project, ident, version)?;

        let per_target
            = targets.envs.iter()
                .map(|env| (env, pypi::select_pinned_file(files, env, &targets)))
                .collect::<Vec<_>>();

        let distinct_files
            = per_target.iter()
                .filter_map(|(_, file)| file.map(|file| file.filename.as_str()))
                .collect::<BTreeSet<_>>();

        let needs_variants
            = distinct_files.len() > 1
                || per_target.iter().any(|(_, file)| file.map_or(true, |file| !is_universal_file(file)));

        if needs_variants {
            resolution.variants = per_target.iter()
                .filter(|(_, file)| file.is_some())
                .map(|(env, _)| variant_descriptor(&locator, ident, version, env))
                .collect();
        }
    }

    resolution.into_resolution_result(context)
}

/// Resolution of a platform variant: the artifact for one platform, with
/// the platform requirements.
pub async fn resolve_variant(context: &InstallContext<'_>, descriptor: &Descriptor, ident: &Ident, version: &PypiVersion, platform_key: &str) -> Result<ResolutionResult, Error> {
    let targets
        = context_targets(context);

    let env
        = env_from_variant_key(platform_key)
            .ok_or_else(|| Error::InvalidResolution(format!("Unknown Python platform {}", platform_key)))?;

    // The variant key carries the Python version it was resolved for
    let mut targets
        = targets;

    targets.python = env.python.clone();

    let index_project
        = fetch_project(context, ident, Some(version)).await?;

    let files
        = version_files(&index_project, ident, version)?;

    let file
        = pypi::select_pinned_file(files, &env, &targets)
            .ok_or_else(|| Error::InvalidResolution(format!("No artifact of {}@{} supports {}", ident.as_str(), version.to_file_string(), platform_key)))?;

    let locator
        = descriptor.resolve_with(PypiRegistryReference {
            ident: ident.clone(),
            version: version.clone(),
            url: Some(UrlEncoded::new(file.url.clone())),
        }.into());

    let requires_dist
        = fetch_requires_dist(context, ident, version).await?;

    let mut resolution
        = Resolution::new_empty(locator, project_pep440_to_semver(version)?);

    // The tree resolver substitutes the variant to its parent, so it needs
    // to list the same dependencies
    resolution.dependencies
        = build_dependencies(&requires_dist, &targets, None)?;

    resolution.requirements
        = env.platform.to_requirements();

    resolution.into_resolution_result(context)
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

/// The candidate versions of a package, highest first, after filtering out
/// releases that are yanked, too recent for the age gate, incompatible
/// with the target Python, or without an artifact for this platform.
/// Whether a version satisfies the island's `pypiConstraints` (uv's
/// constraint-dependencies) for the given package.
pub fn satisfies_constraints(context: &InstallContext<'_>, ident: &Ident, version: &PypiVersion) -> bool {
    context.pypi_constraints.iter()
        .filter_map(|requirement| pep_508::parse(requirement).ok().map(|parsed| (canonicalize_pypi_name(parsed.name), specifier_from_pep508(parsed.spec.as_ref()))))
        .filter(|(name, _)| name == ident.as_str())
        .filter_map(|(_, specifier)| specifier)
        .all(|constraint| specifier_matches(&constraint, version))
}

pub async fn candidate_versions(context: &InstallContext<'_>, ident: &Ident) -> Result<Vec<PypiVersion>, Error> {
    let project
        = context.project
            .expect("The project is required for resolving PyPI packages");

    let targets
        = context_targets(context);

    let cutoff
        = pypi::get_upload_cutoff(&project.config, ident);

    let index_project
        = fetch_project(context, ident, None).await?;

    Ok(index_project.sorted_versions().into_iter()
        .filter(|(_, version)| satisfies_constraints(context, ident, version))
        // A version is a candidate if any of the target platforms can
        // install it: platform-only packages (pywin32) get locked for their
        // platform, and markers keep them away from the others
        .filter(|(_, version)| targets.envs.iter().any(|env| pypi::select_file(&index_project.releases[*version], env, &targets, cutoff).is_some()))
        .map(|(_, version)| version.clone())
        .collect())
}

pub fn specifier_matches(specifier: &PypiSpecifierSet, version: &PypiVersion) -> bool {
    if specifier.is_any() {
        return true;
    }

    version.satisfies(specifier).unwrap_or(false)
}

/// Pre-releases are only selected when the specifier mentions one, or when
/// nothing else matches (PEP 440 / pip behavior).
pub fn select_version(candidates: &[PypiVersion], specifier: &PypiSpecifierSet) -> Option<PypiVersion> {
    let allows_prereleases
        = specifier.as_str().chars().any(|c| c.is_ascii_alphabetic());

    candidates.iter()
        .filter(|version| allows_prereleases || version.is_stable().unwrap_or(true))
        .find(|version| specifier_matches(specifier, version))
        .or_else(|| candidates.iter().find(|version| specifier_matches(specifier, version)))
        .cloned()
}

pub async fn resolve_specifier_descriptor(context: &InstallContext<'_>, descriptor: &Descriptor, params: &PypiSpecifierRange) -> Result<ResolutionResult, Error> {
    let package_ident
        = params.ident.as_ref()
            .unwrap_or(&descriptor.ident);

    if let Some(platform) = params.parameters.as_ref().and_then(|parameters| parameters.platform.as_ref()) {
        let version
            = PypiVersion::from_file_string(params.specifier.as_str().trim_start_matches("=="))
                .map_err(|err| Error::InvalidRange(err.to_string()))?;

        return resolve_variant(context, descriptor, package_ident, &version, platform).await;
    }

    let candidates
        = candidate_versions(context, package_ident).await?;

    let version
        = select_version(&candidates, &params.specifier)
            .ok_or_else(|| Error::NoCandidatesFound(descriptor.range.clone()))?;

    let locator
        = descriptor.resolve_with(PypiRegistryReference {
            ident: package_ident.clone(),
            version: version.clone(),
            url: None,
        }.into());

    let mut result
        = resolve_release(context, locator, package_ident, &version, None).await?;

    if let Some(extras) = params.parameters.as_ref().and_then(|parameters| parameters.extras.clone()) {
        merge_extras(context, &mut result, package_ident, &version, &extras).await?;
    }

    Ok(result)
}

/// Folds the dependencies of the requested extras into the resolution.
/// Only used outside of islands; the island solver models extras itself.
async fn merge_extras(context: &InstallContext<'_>, result: &mut ResolutionResult, ident: &Ident, version: &PypiVersion, extras: &PypiExtras) -> Result<(), Error> {
    let targets
        = context_targets(context);

    let requires_dist
        = fetch_requires_dist(context, ident, version).await?;

    let mut resolution
        = result.original_resolution.clone();

    for extra in extras.iter() {
        for (dep_ident, descriptor) in build_dependencies(&requires_dist, &targets, Some(extra))? {
            match resolution.dependencies.get_mut(&dep_ident) {
                Some(existing) => {
                    merge_dependency_descriptor(existing, descriptor)?;
                },

                None => {
                    resolution.dependencies.insert(dep_ident, descriptor);
                },
            }
        }
    }

    let package_data
        = result.package_data.take();

    *result = resolution.into_resolution_result(context)?;
    result.package_data = package_data;

    Ok(())
}

pub async fn resolve_tag_descriptor(context: &InstallContext<'_>, descriptor: &Descriptor, params: &PypiTagRange) -> Result<ResolutionResult, Error> {
    if params.tag.as_str() != "latest" {
        return Err(Error::TagNotFound(params.tag.to_string()));
    }

    resolve_specifier_descriptor(context, descriptor, &PypiSpecifierRange {
        ident: params.ident.clone(),
        specifier: PypiSpecifierSet::any(),
        parameters: params.parameters.clone(),
    }).await
}

pub async fn resolve_locator(context: &InstallContext<'_>, locator: &Locator, params: &PypiRegistryReference) -> Result<ResolutionResult, Error> {
    resolve_release(context, locator.clone(), &params.ident, &params.version, None).await
}

pub async fn resolve_locator_extra(context: &InstallContext<'_>, locator: &Locator, params: &PypiRegistryReference, extra: &str) -> Result<ResolutionResult, Error> {
    resolve_release(context, locator.clone(), &params.ident, &params.version, Some(extra)).await
}

pub fn parse_pep440(version: &PypiVersion) -> Option<pep440_rs::Version> {
    pep440_rs::Version::from_str(version.as_str()).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::python_env::PythonVersion;
    use zpm_utils::{Cpu, Libc, Os};

    fn targets() -> PythonTargets {
        let python
            = PythonVersion {major: 3, minor: 12, patch: None};

        PythonTargets {
            python: python.clone(),
            envs: vec![
                PythonEnv {python: python.clone(), platform: Platform {os: Os::MacOS, cpu: Cpu::Aarch64, libc: None}},
                PythonEnv {python: python.clone(), platform: Platform {os: Os::Linux, cpu: Cpu::X86_64, libc: Some(Libc::Glibc)}},
            ],
            macos_target: (14, 0),
            manylinux_target: (2, 34),
        }
    }

    #[test]
    fn requirement_conversion() {
        let targets
            = targets();

        let always
            = convert_requirement("Foo_Bar>=1.0", &targets, None).unwrap();
        assert_eq!(always.descriptor.to_file_string(), "foo-bar@pypi:>=1.0");

        assert!(convert_requirement("foo; python_version < '3.10'", &targets, None).is_none());
        assert!(convert_requirement("foo; sys_platform == 'win32'", &targets, None).is_none());
        assert!(convert_requirement("foo; extra == 'x'", &targets, None).is_none());

        let sometimes
            = convert_requirement("foo>=1; sys_platform == 'darwin'", &targets, None).unwrap();
        assert!(sometimes.descriptor.to_file_string().contains("marker="));

        let extra
            = convert_requirement("foo[bar]>=1; extra == 'x' and python_version >= '3.8'", &targets, Some("x")).unwrap();
        assert_eq!(extra.descriptor.to_file_string(), "foo@pypi:>=1#extras=bar");

        assert!(convert_requirement("foo; extra == 'y'", &targets, Some("x")).is_none());
        assert!(convert_requirement("foo", &targets, Some("x")).is_none());
    }

    #[test]
    fn version_selection() {
        let candidates
            = ["2.0.0rc1", "1.1.0", "1.0.0"].iter().map(|v| PypiVersion::from_file_string(v).unwrap()).collect::<Vec<_>>();

        assert_eq!(select_version(&candidates, &PypiSpecifierSet::any()).unwrap().as_str(), "1.1.0");
        assert_eq!(select_version(&candidates, &PypiSpecifierSet::from_file_string(">=2.0.0rc1").unwrap()).unwrap().as_str(), "2.0.0rc1");
        assert_eq!(select_version(&candidates, &PypiSpecifierSet::from_file_string("<1.1").unwrap()).unwrap().as_str(), "1.0.0");
    }
}
