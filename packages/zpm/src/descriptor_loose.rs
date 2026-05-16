use std::{collections::{BTreeMap, BTreeSet}, io::ErrorKind, time::{Duration, SystemTime}};

use chrono::{DateTime, Utc};
use futures::{future::BoxFuture, FutureExt};
use serde::Deserialize;
use serde_with::{MapSkipError, serde_as};
use wax::{Glob, Program};
use zpm_formats::{iter_ext::IterExt, tar, tar_iter};
use zpm_macro_enum::zpm_enum;
use zpm_parsers::{JsonDocument, RawJsonValue};
use zpm_primitives::{AnonymousSemverRange, AnonymousTagRange, Descriptor, FolderRange, Ident, Locator, Range, RegistrySemverRange, RegistryTagRange, TarballRange, WorkspaceMagicRange};
use zpm_semver::RangeKind;
use zpm_utils::{Hash64, Path};

use crate::{error::Error, http_npm, install::{InstallContext, ResolutionResult}, manifest::helpers::{parse_manifest_from_bytes, read_manifest}, npm, project::Project, report::{with_report_result, StreamReport, StreamReportConfig}, resolvers};

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ResolveOptions {
    pub active_workspace_ident: Ident,
    pub range_kind: RangeKind,
    pub resolve_tags: bool,
    pub allow_reuse: bool,
}

#[derive(Clone, Debug)]
pub struct LooseResolution {
    pub descriptor: Descriptor,
    pub locator: Option<Locator>,
    pub resolution: Option<ResolutionResult>,
}

const ADD_PACKUMENT_CACHE_TTL: Duration = Duration::from_secs(30);

#[serde_as]
#[derive(Deserialize)]
struct RegistryMetadata<'a> {
    #[serde(default)]
    #[serde(rename(deserialize = "dist-tags"))]
    dist_tags: BTreeMap<String, zpm_semver::Version>,

    #[serde_as(as = "Option<MapSkipError<_, _>>")]
    time: Option<BTreeMap<zpm_semver::Version, DateTime<Utc>>>,

    #[serde(borrow)]
    versions: BTreeMap<zpm_semver::Version, RawJsonValue<'a>>,
}

fn add_packument_cache_path(project: &Project, registry_base: &str, registry_path: &str) -> Path {
    let cache_key
        = Hash64::from_data(format!("{registry_base}{registry_path}"));

    project.ignore_path()
        .with_join_str("npm-metadata")
        .with_join_str(format!("{}.json", cache_key.short()))
}

fn read_cached_packument(cache_path: &Path, allow_stale: bool) -> Result<Option<Vec<u8>>, Error> {
    let metadata = match cache_path.fs_metadata() {
        Ok(metadata) => metadata,

        Err(error) if error.io_kind() == Some(ErrorKind::NotFound) => {
            return Ok(None);
        },

        Err(error) => {
            return Err(error.into());
        },
    };

    if !allow_stale {
        let age = SystemTime::now()
            .duration_since(metadata.modified()?)
            .unwrap_or_default();

        if age > ADD_PACKUMENT_CACHE_TTL {
            return Ok(None);
        }
    }

    Ok(Some(cache_path.fs_read_prealloc()?))
}

async fn fetch_registry_metadata(context: &InstallContext<'_>, package_ident: &Ident) -> Result<Vec<u8>, Error> {
    let project = context.project.as_ref()
        .expect("Project is required for resolving registry metadata");

    let registry_base
        = http_npm::get_registry(&project.config, package_ident.scope(), false)?;
    let registry_path
        = npm::registry_url_for_all_versions(package_ident);

    let authorization
        = http_npm::get_authorization(&http_npm::GetAuthorizationOptions {
            configuration: &project.config,
            http_client: &project.http_client,
            registry: &registry_base,
            ident: Some(package_ident),
            auth_mode: http_npm::AuthorizationMode::RespectConfiguration,
            allow_oidc: false,
        }).await?;

    let cache_path = authorization.is_none()
        .then(|| add_packument_cache_path(project, &registry_base, &registry_path));

    if let Some(cache_path) = cache_path.as_ref() {
        if let Some(bytes) = read_cached_packument(cache_path, false)? {
            return Ok(bytes);
        }
    }

    let bytes = match http_npm::get(&http_npm::NpmHttpParams {
        http_client: &project.http_client,
        registry: &registry_base,
        path: &registry_path,
        authorization: authorization.as_deref(),
        otp: None,
    }).await {
        Ok(bytes) => bytes.to_vec(),

        Err(error @ Error::NetworkDisabledError(_)) => {
            if let Some(cache_path) = cache_path.as_ref() {
                if let Some(bytes) = read_cached_packument(cache_path, true)? {
                    return Ok(bytes);
                }
            }

            return Err(error);
        },

        Err(error) => {
            return Err(error);
        },
    };

    if let Some(cache_path) = cache_path.as_ref() {
        let _ = cache_path.fs_create_parent()
            .and_then(|_| cache_path.fs_write(&bytes));
    }

    Ok(bytes)
}

fn find_semver_candidate<'a>(context: &InstallContext<'_>, package_ident: &Ident, range: &zpm_semver::Range, registry_data: &'a RegistryMetadata<'a>) -> Option<(&'a zpm_semver::Version, &'a RawJsonValue<'a>)> {
    let project = context.project.as_ref()
        .expect("Project is required for resolving semver candidates");

    registry_data.versions.iter().rev()
        .filter(|(version, _)| range.check(version))
        .find(|(version, _)| {
            let release_time = project.config.settings.npm_minimal_age_gate.value
                .and_then(|_| registry_data.time.as_ref())
                .and_then(|times| times.get(*version));

            resolvers::npm::is_package_approved(context, package_ident, version, release_time)
        })
}

fn build_registry_resolution_result(context: &InstallContext<'_>, descriptor: &Descriptor, package_ident: &Ident, version: &zpm_semver::Version, manifest: resolvers::npm::RemoteManifestWithScripts) -> Result<ResolutionResult, Error> {
    resolvers::npm::build_resolution_result(context, descriptor, package_ident, version.clone(), manifest)
}

#[zpm_enum(or_else = |s| Err(Error::InvalidRange(s.to_string())))]
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[derive_variants(Clone, Debug, PartialEq, Eq, Hash)]
pub enum LooseDescriptor {
    #[pattern(r"(?<descriptor>.*)")]
    #[to_file_string(|params| params.descriptor.to_file_string())]
    #[to_print_string(|params| params.descriptor.to_print_string())]
    Descriptor {
        descriptor: Descriptor,
    },

    #[pattern(r"(?<ident>.*)")]
    #[to_file_string(|params| params.ident.to_file_string())]
    #[to_print_string(|params| params.ident.to_print_string())]
    Ident {
        ident: Ident,
    },

    #[pattern(r"(?<range>.*)")]
    #[to_file_string(|params| params.range.to_file_string())]
    #[to_print_string(|params| params.range.to_print_string())]
    Range {
        range: Range,
    },
}

impl LooseDescriptor {
    pub fn expand(&self, all_idents: &BTreeSet<Ident>) -> Vec<LooseDescriptor> {
        match self {
            LooseDescriptor::Descriptor(descriptor_loose_descriptor) =>
                self.expand_ident(&descriptor_loose_descriptor.descriptor.ident, all_idents)
                    .into_iter()
                    .map(|ident| LooseDescriptor::Descriptor(DescriptorLooseDescriptor {descriptor: Descriptor::new(ident, descriptor_loose_descriptor.descriptor.range.clone())}))
                    .collect(),

            LooseDescriptor::Ident(ident_loose_descriptor) =>
                self.expand_ident(&ident_loose_descriptor.ident, all_idents)
                    .into_iter()
                    .map(|ident| LooseDescriptor::Ident(IdentLooseDescriptor {ident}))
                    .collect(),

            LooseDescriptor::Range(_) =>
                vec![self.clone()],
        }
    }

    // Glob expansion doesn't work amazingly well with scoped packages since
    // they stop at slashes. To avoid that we just replace all slashes with
    // an arbitrary symbol that doesn't appear in valid identifiers.
    fn expand_ident(&self, ident: &Ident, all_idents: &BTreeSet<Ident>) -> Vec<Ident> {
        let noslash_glob = ident.as_str()
            .replace("/", "&");

        let glob
            = Glob::new(&noslash_glob).unwrap();

        let mut idents = Vec::new();

        for ident in all_idents.iter() {
            let noslash_ident = ident.as_str()
                .replace("/", "&");

            if glob.is_match(noslash_ident.as_str()) {
                idents.push(ident.clone());
            }
        }

        idents
    }

    pub async fn resolve_all<'a>(context: &'a InstallContext<'a>, options: &'a ResolveOptions, loose_descriptors: &[LooseDescriptor]) -> Result<Vec<LooseResolution>, Error> {
        let mut futures: Vec<BoxFuture<'a, Result<LooseResolution, Error>>> = vec![];

        for loose_descriptor in loose_descriptors {
            let loose_descriptor
                = loose_descriptor.clone();

            let future
                = async move { loose_descriptor.resolve(context, &options).await };

            futures.push(future.boxed());
        }

        let report = StreamReport::new(StreamReportConfig {
            ..StreamReportConfig::default()
        });

        let descriptors = with_report_result(report, async {
            futures::future::join_all(futures).await
                .into_iter()
                .collect::<Result<Vec<_>, Error>>()
        }).await?;

        Ok(descriptors)
    }

    pub async fn resolve(&self, context: &InstallContext<'_>, options: &ResolveOptions) -> Result<LooseResolution, Error> {
        match self {
            LooseDescriptor::Range(RangeLooseDescriptor {range: Range::Tarball(params)}) => {
                let params_path
                    = params.path.clone();

                let ident = tokio::task::spawn_blocking(move || -> Result<Ident, Error> {
                    let path
                        = Path::try_from(&params_path)?;

                    let tgz_content = path
                        .fs_read_prealloc()?;

                    let tar_content
                        = tar::unpack_tgz(&tgz_content)?;

                    let package_json_entry
                        = tar_iter::TarIterator::new(&tar_content)
                            .filter_map(|entry| entry.ok())
                            .strip_first_segment()
                            .find(|entry| entry.name.basename() == Some("package.json"));

                    let Some(package_json_entry) = package_json_entry else {
                        return Err(Error::ManifestNotFound(path.with_join_str("package.json")));
                    };

                    let manifest
                        = parse_manifest_from_bytes(&package_json_entry.data)?;

                    let ident = manifest.name
                        .ok_or_else(|| Error::MissingPackageName)?;

                    Ok(ident)
                }).await??;

                let descriptor
                    = Descriptor::new(ident, TarballRange {path: params.path.clone()}.into());

                Ok(LooseResolution {
                    descriptor,
                    locator: None,
                    resolution: None,
                })
            }

            LooseDescriptor::Range(RangeLooseDescriptor {range: Range::Folder(params)}) => {
                let path
                    = Path::try_from(&params.path)?;

                let manifest_path = path
                    .with_join_str("package.json");
                let manifest
                    = read_manifest(&manifest_path)?;

                let ident = manifest.name
                    .ok_or_else(|| Error::MissingPackageName)?;

                let descriptor
                    = Descriptor::new(ident, FolderRange {path: params.path.clone()}.into());

                Ok(LooseResolution {
                    descriptor,
                    locator: None,
                    resolution: None,
                })
            },

            LooseDescriptor::Range(RangeLooseDescriptor {range}) => {
                Err(Error::UnsufficientLooseDescriptor(range.clone()))
            },

            LooseDescriptor::Descriptor(DescriptorLooseDescriptor {descriptor: Descriptor {ident, range: Range::AnonymousSemver(AnonymousSemverRange {range}), ..}}) => {
                LooseDescriptor::resolve_registry_semver(context, ident, None, range).await
            },

            LooseDescriptor::Descriptor(DescriptorLooseDescriptor {descriptor: Descriptor {ident, range: Range::RegistrySemver(RegistrySemverRange {ident: ident_range, range}), ..}}) => {
                LooseDescriptor::resolve_registry_semver(context, ident, ident_range.as_ref(), range).await
            }

            LooseDescriptor::Descriptor(DescriptorLooseDescriptor {descriptor: Descriptor {ident, range: Range::AnonymousTag(AnonymousTagRange {tag}), ..}}) => {
                LooseDescriptor::resolve_registry_tag(context, options, ident, None, tag.as_str()).await
            },

            LooseDescriptor::Descriptor(DescriptorLooseDescriptor {descriptor: Descriptor {ident, range: Range::RegistryTag(RegistryTagRange {ident: ident_range, tag}), ..}}) => {
                LooseDescriptor::resolve_registry_tag(context, options, ident, ident_range.as_ref(), tag.as_str()).await
            },

            LooseDescriptor::Descriptor(DescriptorLooseDescriptor {descriptor}) => {
                Ok(LooseResolution {
                    descriptor: descriptor.clone(),
                    locator: None,
                    resolution: None,
                })
            },

            LooseDescriptor::Ident(IdentLooseDescriptor {ident}) => {
                let project = context.project.as_ref()
                    .expect("Project is required for resolving loose identifiers");

                if ident != &options.active_workspace_ident && project.workspace_by_ident(&ident).is_ok() {
                    let descriptor
                        = Descriptor::new(ident.clone(), WorkspaceMagicRange {magic: options.range_kind}.into());

                    return Ok(LooseResolution {
                        descriptor,
                        locator: None,
                        resolution: None,
                    });
                }

                if options.allow_reuse && project.config.settings.prefer_reuse.value {
                    if let Some(descriptor) = find_project_descriptor(project, ident.clone())? {
                        return Ok(LooseResolution {
                            descriptor: descriptor.clone(),
                            locator: None,
                            resolution: None,
                        });
                    }
                }

                LooseDescriptor::resolve_registry_tag(context, options, ident, None, "latest").await
            },
        }
    }

    async fn resolve_registry_semver(context: &InstallContext<'_>, ident: &Ident, range_ident: Option<&Ident>, range: &zpm_semver::Range) -> Result<LooseResolution, Error> {
        let descriptor
            = Descriptor::new(ident.clone(), RegistrySemverRange {ident: range_ident.cloned(), range: range.clone()}.into());

        let Range::RegistrySemver(range_params) = &descriptor.range else {
            panic!("Invalid range");
        };

        // We use as-is ranges declared using a prefix (ie `^x.y.w`, `~x.y.z`, etc)
        let Some(range_kind) = range_params.range.kind() else {
            let descriptor
                = Descriptor::new(ident.clone(), RegistrySemverRange {ident: range_ident.cloned(), range: range.clone()}.into());

            return Ok(LooseResolution {
                descriptor,
                locator: None,
                resolution: None,
            });
        };

        let package_ident = range_ident
            .unwrap_or(ident);

        let bytes
            = fetch_registry_metadata(context, package_ident).await?;
        let registry_data: RegistryMetadata<'_>
            = JsonDocument::hydrate_from_slice(&bytes[..])?;

        let (resolved_version, manifest_value) = find_semver_candidate(context, package_ident, &range_params.range, &registry_data)
            .ok_or_else(|| Error::NoCandidatesFound(descriptor.range.clone()))?;

        let manifest: resolvers::npm::RemoteManifestWithScripts
            = JsonDocument::hydrate_from_value(manifest_value)?;

        let range = resolved_version
            .to_range(range_kind);

        let descriptor
            = Descriptor::new(ident.clone(), RegistrySemverRange {ident: range_ident.cloned(), range: range.clone()}.into());
        let resolution
            = build_registry_resolution_result(context, &descriptor, package_ident, resolved_version, manifest)?;
        let locator = resolution.resolution.locator.clone();

        Ok(LooseResolution {
            descriptor,
            locator: Some(locator),
            resolution: Some(resolution),
        })
    }

    async fn resolve_registry_tag(context: &InstallContext<'_>, options: &ResolveOptions, ident: &Ident, range_ident: Option<&Ident>, tag: &str) -> Result<LooseResolution, Error> {
        if !options.resolve_tags {
            let descriptor
                = Descriptor::new(ident.clone(), RegistryTagRange {ident: range_ident.cloned(), tag: tag.into()}.into());

            return Ok(LooseResolution {
                descriptor,
                locator: None,
                resolution: None,
            });
        }

        let package_ident = range_ident
            .unwrap_or(ident);

        let bytes
            = fetch_registry_metadata(context, package_ident).await?;
        let registry_data: RegistryMetadata<'_>
            = JsonDocument::hydrate_from_slice(&bytes[..])?;

        let latest_version = registry_data.dist_tags
            .get(tag)
            .ok_or_else(|| Error::TagNotFound(tag.to_string()))?;

        let (resolved_version, manifest_value) = registry_data.versions.iter().rev()
            .filter(|(version, _)| *version <= latest_version)
            .filter(|(version, _)| !version.rc.is_some() || latest_version.rc.is_some())
            .find(|(version, _)| {
                let release_time = registry_data.time.as_ref()
                    .and_then(|times| times.get(*version));

                resolvers::npm::is_package_approved(context, package_ident, version, release_time)
            })
            .ok_or_else(|| Error::NoCandidatesFound(AnonymousSemverRange {range: zpm_semver::Range::lte(latest_version.clone())}.into()))?;

        let manifest: resolvers::npm::RemoteManifestWithScripts
            = JsonDocument::hydrate_from_value(manifest_value)?;

        let derived_range = resolved_version
            .to_range(options.range_kind);
        let range_matches = find_semver_candidate(context, package_ident, &derived_range, &registry_data)
            .map(|(version, _)| version == resolved_version)
            .unwrap_or(false);

        let final_range = if range_matches {
            derived_range
        } else {
            resolved_version.to_range(RangeKind::Exact)
        };

        let descriptor
            = Descriptor::new(ident.clone(), RegistrySemverRange {ident: range_ident.cloned(), range: final_range}.into());
        let resolution
            = build_registry_resolution_result(context, &descriptor, package_ident, resolved_version, manifest)?;
        let locator = resolution.resolution.locator.clone();

        Ok(LooseResolution {
            descriptor,
            locator: Some(locator),
            resolution: Some(resolution),
        })
    }
}

impl Default for LooseDescriptor {
    fn default() -> Self {
        LooseDescriptor::Ident(IdentLooseDescriptor {
            ident: Ident::new("")
        })
    }
}


fn find_project_descriptor(project: &Project, ident: Ident) -> Result<Option<Descriptor>, Error> {
    let mut occurrences
        = BTreeMap::new();

    fn try_match<'a>(descriptor: &'a Descriptor, occurrences: &mut BTreeMap<&'a Descriptor, usize>) {
        occurrences.entry(descriptor)
            .and_modify(|count| *count += 1)
            .or_insert(1);
    }

    for workspace in project.workspaces.iter() {
        if let Some(regular_descriptor) = workspace.manifest.remote.dependencies.get(&ident) {
            try_match(regular_descriptor, &mut occurrences);
        }

        if let Some(dev_descriptor) = workspace.manifest.dev_dependencies.get(&ident) {
            try_match(dev_descriptor, &mut occurrences);
        }
    }

    let best_match
        = occurrences.into_iter()
            .max_by_key(|(_, count)| *count)
            .map(|(descriptor, _)| descriptor.clone());

    Ok(best_match)
}
