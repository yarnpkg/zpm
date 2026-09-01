use serde::Deserialize;
use zpm_parsers::JsonDocument;
use zpm_primitives::{Ident, Locator, PypiRegistryReference, PythonTargetEnv, Reference};
use zpm_utils::{FromFileString, Hash64, Path, ToFileString};

use crate::{
    error::Error,
    install::{FetchResult, InstallContext},
    prepare,
    pypi::{LocalWheelSource, PypiDistribution, encode_path_segment, get_artifact_authorization, get_authorization, get_registry, parse_local_wheel_url, parse_python_git_url, parse_simple_project, python_git_project_path, resolve_local_wheel_path, select_best_artifact},
};

use super::PackageData;

#[derive(Clone, Debug, Deserialize)]
struct PypiVersionMetadata {
    #[serde(default)]
    urls: Vec<PypiDistribution>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ArtifactKind {
    Wheel,
    Sdist,
}

#[derive(Clone, Debug)]
struct ResolvedArtifact {
    filename: String,
    kind: ArtifactKind,
    url: String,
    local_source: Option<LocalWheelSource>,
}

const PREPARED_SDIST_CACHE_EXT: &str = "-sdist-v1.zip";
const SDIST_SOURCE_CACHE_EXT: &str = ".sdist";
const PREPARED_GIT_CACHE_EXT: &str = "-python-git-v1.zip";

fn python_git_cache_locator(ident: &Ident, reference: &zpm_git::GitReference, environment: Option<&Hash64>) -> Locator {
    let locator = Locator::new(ident.clone(), zpm_primitives::GitReference {
        git: reference.clone(),
    }.into());

    match environment {
        Some(environment) => locator.env_qualified_with_hash(environment.clone()),
        None => locator,
    }
}

pub async fn prepare_git_wheel(
    context: &InstallContext<'_>,
    ident: &Ident,
    reference: &zpm_git::GitReference,
    target: Option<&PythonTargetEnv>,
    environment: Option<&Hash64>,
) -> Result<crate::cache::DataCacheEntry, Error> {
    let package_cache
        = context.package_cache
            .expect("The package cache is required for preparing Python Git packages");
    let cache_locator
        = python_git_cache_locator(ident, reference, environment);
    let managed_python
        = environment.and_then(|fork_id| context.python_build_runtimes.lock().unwrap().get(fork_id).cloned());
    let reference
        = reference.clone();
    let target
        = target.cloned();

    package_cache.upsert_blob(cache_locator, PREPARED_GIT_CACHE_EXT, || async move {
        let repository_path
            = Box::pin(crate::git::clone_repository(context, &reference.repo, &reference.commit)).await?;
        let project_path
            = match python_git_project_path(&repository_path, &reference) {
                Ok(path) => path,
                Err(error) => {
                    let _ = repository_path.fs_rm();
                    return Err(error);
                },
            };
        let result
            = Box::pin(prepare::python::prepare_source_tree(&project_path, target.as_ref(), managed_python.as_ref())).await;
        let _ = repository_path.fs_rm();
        result
    }).await
}

fn artifact_kind(filename: &str) -> Option<ArtifactKind> {
    if filename.ends_with(".whl") {
        Some(ArtifactKind::Wheel)
    } else if filename.ends_with(".tar.gz") || filename.ends_with(".tgz") || filename.ends_with(".zip") {
        Some(ArtifactKind::Sdist)
    } else {
        None
    }
}

fn artifact_from_url(url: &str) -> Result<ResolvedArtifact, Error> {
    if let Some(source) = parse_local_wheel_url(url)? {
        let path
            = Path::from_file_string(&source.path)?;
        let filename
            = path.basename()
                .filter(|filename| !filename.is_empty())
                .ok_or_else(|| Error::InvalidResolution(format!("Local PyPI wheel path has no filename: {}", source.path)))?
                .to_string();
        let kind
            = artifact_kind(&filename)
                .ok_or_else(|| Error::InvalidResolution(format!("Unsupported local PyPI artifact format: {filename}")))?;

        return Ok(ResolvedArtifact {
            filename,
            kind,
            url: url.to_string(),
            local_source: Some(source),
        });
    }

    let parsed = url::Url::parse(url)?;
    let filename = parsed.path_segments()
        .and_then(|mut segments| segments.next_back())
        .filter(|filename| !filename.is_empty())
        .ok_or_else(|| Error::InvalidResolution(format!("PyPI artifact URL has no filename: {url}")))?
        .to_string();
    let kind = artifact_kind(&filename)
        .ok_or_else(|| Error::InvalidResolution(format!("Unsupported PyPI artifact format: {filename}")))?;

    Ok(ResolvedArtifact {
        filename,
        kind,
        url: url.to_string(),
        local_source: None,
    })
}

fn artifact_from_distribution(distribution: &PypiDistribution) -> Result<ResolvedArtifact, Error> {
    let kind = artifact_kind(&distribution.filename)
        .ok_or_else(|| Error::InvalidResolution(format!("Unsupported PyPI artifact format: {}", distribution.filename)))?;

    Ok(ResolvedArtifact {
        filename: distribution.filename.clone(),
        kind,
        url: distribution.url.clone(),
        local_source: None,
    })
}

async fn resolve_artifact(context: &InstallContext<'_>, params: &PypiRegistryReference) -> Result<ResolvedArtifact, Error> {
    if let Some(url) = &params.url {
        return artifact_from_url(&url.0);
    }

    let project
        = context.project
        .expect("The project is required for fetching PyPI packages");

    let registry
        = get_registry(&project.config, &params.ident);
    let authorization
        = get_authorization(&project.config, &registry, &params.ident);
    let metadata_url
        = format!(
            "{}/pypi/{}/{}/json",
            registry,
            encode_path_segment(params.ident.as_str()),
            encode_path_segment(&params.version.to_file_string()),
        );

    let metadata = match project.http_client.cached_get_with_authorization(&metadata_url, authorization.as_deref()).await {
        Ok(bytes) => JsonDocument::hydrate_from_slice::<PypiVersionMetadata>(&bytes[..])?,
        Err(Error::HttpError {inner, ..}) if inner.status() == Some(reqwest::StatusCode::NOT_FOUND) => {
            let simple_url
                = format!("{}/{}/", registry, encode_path_segment(params.ident.as_str()));
            let bytes
                = project.http_client.cached_get_with_authorization(&simple_url, authorization.as_deref()).await?;
            let html
                = std::str::from_utf8(&bytes)
                    .map_err(|error| Error::InvalidResolution(format!("PyPI Simple API response is not UTF-8: {error}")))?;
            let releases
                = parse_simple_project(html, &simple_url, &params.ident)?;
            PypiVersionMetadata {
                urls: releases.get(&params.version.to_file_string()).cloned().unwrap_or_default(),
            }
        },
        Err(error) => return Err(error),
    };

    let artifact
        = select_best_artifact(&metadata.urls, None)
            .ok_or_else(|| Error::InvalidResolution(format!(
                "No supported artifact found for {}@{}",
                params.ident.to_file_string(),
                params.version.to_file_string(),
            )))?;

    artifact_from_distribution(artifact)
}

fn cache_ext_from_params(params: &PypiRegistryReference) -> Result<&'static str, Error> {
    match &params.url {
        Some(url) => match artifact_from_url(&url.0)?.kind {
            ArtifactKind::Wheel => Ok(".zip"),
            ArtifactKind::Sdist => Ok(PREPARED_SDIST_CACHE_EXT),
        },
        None => Ok(".zip"),
    }
}

pub(crate) fn environment_hash(reference: &Reference) -> Option<&Hash64> {
    match reference {
        Reference::Env(params) => Some(&params.hash),
        Reference::Virtual(params) => environment_hash(&params.inner),
        _ => None,
    }
}

pub(crate) fn preparation_target(context: &InstallContext<'_>, locator: &Locator) -> Result<Option<PythonTargetEnv>, Error> {
    let Some(hash) = environment_hash(&locator.reference) else {
        return Ok(None);
    };
    let project = context.project
        .expect("The project is required for fetching PyPI packages");
    let targets = project.config.settings.python_target_envs()
        .map_err(|error| Error::PythonPreparation(format!("invalid Python target environment: {error}")))?;

    targets.into_iter()
        .find(|target| &target.fork_id() == hash)
        .map(Some)
        .ok_or_else(|| Error::PythonPreparation(format!(
            "cannot find the target environment for fork {}",
            hash.to_file_string(),
        )))
}

fn cache_locator(locator: &Locator, kind: ArtifactKind) -> Locator {
    match kind {
        ArtifactKind::Wheel => locator.physical_locator(),
        ArtifactKind::Sdist => locator.clone(),
    }
}

pub fn try_fetch_locator_sync(context: &InstallContext<'_>, locator: &Locator, params: &PypiRegistryReference, is_mock_request: bool) -> Result<Option<FetchResult>, Error> {
    let package_cache
        = context.package_cache
            .expect("The package cache is required for fetching PyPI packages");

    let git_reference
        = params.url.as_ref().map(|url| parse_python_git_url(&url.0)).transpose()?.flatten();
    let (cache_locator, cache_ext) = match (&params.url, git_reference) {
        (_, Some(reference)) => (
            python_git_cache_locator(&params.ident, &reference, environment_hash(&locator.reference)),
            PREPARED_GIT_CACHE_EXT,
        ),
        (Some(url), None) => {
            let kind = artifact_from_url(&url.0)?.kind;
            (cache_locator(locator, kind), cache_ext_from_params(params)?)
        },
        (None, None) => (locator.physical_locator(), ".zip"),
    };

    if is_mock_request {
        let archive_path
            = package_cache
            .key_path(&cache_locator, cache_ext);

        return Ok(Some(FetchResult::new_mock(archive_path.clone(), archive_path)));
    }

    let cache_entry
        = package_cache
        .check_cache_entry(cache_locator, cache_ext)?;

    Ok(cache_entry.map(|cache_entry| FetchResult::new(PackageData::Zip {
        archive_path: cache_entry.path.clone(),
        checksum: cache_entry.checksum,
        context_directory: cache_entry.path.clone(),
        package_directory: cache_entry.path,
    })))
}

pub async fn fetch_locator<'a>(context: &InstallContext<'a>, locator: &Locator, params: &PypiRegistryReference, is_mock_request: bool) -> Result<FetchResult, Error> {
    let package_cache
        = context.package_cache
        .expect("The package cache is required for fetching PyPI packages");

    if is_mock_request {
        let git_reference
            = params.url.as_ref().map(|url| parse_python_git_url(&url.0)).transpose()?.flatten();
        let (cache_locator, cache_ext) = match git_reference {
            Some(reference) => (
                python_git_cache_locator(&params.ident, &reference, environment_hash(&locator.reference)),
                PREPARED_GIT_CACHE_EXT,
            ),
            None => (locator.clone(), ".zip"),
        };
        let archive_path
            = package_cache
            .key_path(&cache_locator, cache_ext);

        return Ok(FetchResult::new_mock(archive_path.clone(), archive_path));
    }

    let project
        = context.project
        .expect("The project is required for fetching PyPI packages");

    if let Some(reference) = params.url.as_ref().map(|url| parse_python_git_url(&url.0)).transpose()?.flatten() {
        let target
            = preparation_target(context, locator)?;
        let cached_blob
            = Box::pin(prepare_git_wheel(
                context,
                &params.ident,
                &reference,
                target.as_ref(),
                environment_hash(&locator.reference),
            )).await?;
        let metadata
            = crate::pypi::metadata_from_wheel(&cached_blob.data)?;
        if metadata.ident != params.ident || !metadata.version.cmp_pep440(&params.version)
            .map_err(|error| Error::InvalidResolution(error.to_string()))?
            .is_eq()
        {
            return Err(Error::InvalidResolution(format!(
                "Python Git dependency metadata doesn't match {}@{}",
                params.ident.to_file_string(),
                params.version.to_file_string(),
            )));
        }

        return Ok(FetchResult::new(PackageData::Zip {
            archive_path: cached_blob.info.path.clone(),
            checksum: cached_blob.info.checksum,
            context_directory: cached_blob.info.path.clone(),
            package_directory: cached_blob.info.path,
        }));
    }

    let artifact
        = resolve_artifact(context, params).await?;
    let registry
        = get_registry(&project.config, &params.ident);
    let authorization
        = get_artifact_authorization(&project.config, &registry, &params.ident, &artifact.url);
    let artifact_cache_locator
        = cache_locator(locator, artifact.kind);
    let local_wheel_path
        = artifact.local_source.as_ref()
            .map(|source| resolve_local_wheel_path(project, locator.parent.as_deref(), &source.path))
            .transpose()?;
    let local_wheel_checksum
        = artifact.local_source.as_ref().map(|source| source.checksum.clone());

    let cached_blob = match artifact.kind {
        ArtifactKind::Wheel => {
            package_cache.ensure_blob(artifact_cache_locator, ".zip", || async {
                if let Some(path) = local_wheel_path {
                    let bytes
                        = path.fs_read()
                            .map_err(|error| Error::InvalidResolution(format!("Cannot read local PyPI wheel `{}`: {error}", path.to_file_string())))?;
                    if local_wheel_checksum.as_ref().is_some_and(|checksum| checksum != &Hash64::from_data(&bytes)) {
                        return Err(Error::InvalidResolution(format!(
                            "Local PyPI wheel `{}` changed since the lockfile was generated; run install to refresh the lockfile",
                            path.to_file_string(),
                        )));
                    }
                    return Ok(bytes);
                }

                let (_, bytes)
                    = project.http_client.get(&artifact.url)?
                        .header("authorization", authorization.as_deref())
                        .send_bytes()
                        .await?;

                Ok(bytes.to_vec())
            }).await?.into_info()
        },

        ArtifactKind::Sdist => {
            let source_locator = artifact_cache_locator.clone();
            let source_url = artifact.url.clone();
            let filename = artifact.filename.clone();
            let expected_ident = params.ident.clone();
            let expected_version = params.version.clone();
            let target = preparation_target(context, locator)?;
            let managed_python = environment_hash(&locator.reference)
                .and_then(|fork_id| context.python_build_runtimes.lock().unwrap().get(fork_id).cloned());

            package_cache.ensure_blob(artifact_cache_locator, PREPARED_SDIST_CACHE_EXT, || async {
                let source = package_cache.upsert_blob(source_locator, SDIST_SOURCE_CACHE_EXT, || async {
                    let (_, bytes)
                        = project.http_client.get(&source_url)?
                            .header("authorization", authorization.as_deref())
                            .send_bytes()
                            .await?;

                    Ok(bytes.to_vec())
                }).await?;

                prepare::python::prepare_sdist(
                    &source.data,
                    &filename,
                    &expected_ident,
                    &expected_version,
                    target.as_ref(),
                    managed_python.as_ref(),
                ).await
            }).await?.into_info()
        },
    };

    Ok(FetchResult::new(PackageData::Zip {
        archive_path: cached_blob.path.clone(),
        checksum: cached_blob.checksum,
        context_directory: cached_blob.path.clone(),
        package_directory: cached_blob.path,
    }))
}

#[cfg(test)]
mod tests {
    use zpm_utils::FromFileString;

    use super::*;

    #[test]
    fn test_sdist_cache_identity_preserves_python_fork() {
        let fork = Hash64::from_data("python-target");
        let locator = Locator::from_file_string(&format!(
            "demo@env:{}#pypi:demo@1.0.0#https%3A%2F%2Fexample.com%2Fdemo-1.0.0.tar.gz",
            fork.to_file_string(),
        )).unwrap();

        assert_eq!(locator, cache_locator(&locator, ArtifactKind::Sdist));
        assert_eq!(locator.physical_locator(), cache_locator(&locator, ArtifactKind::Wheel));
    }
}
