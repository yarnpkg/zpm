use std::collections::{BTreeMap, BTreeSet};

use bincode::{Decode, Encode};
use futures::{future::BoxFuture, FutureExt};
use wax::{Glob, Program};
use zpm_formats::{iter_ext::IterExt, tar, tar_iter};
use zpm_macro_enum::zpm_enum;
use zpm_primitives::{AnonymousSemverRange, AnonymousTagRange, Descriptor, FolderRange, Ident, Locator, Range, RegistrySemverRange, RegistryTagRange, TarballRange, WorkspaceMagicRange};
use zpm_semver::RangeKind;
use zpm_utils::{impl_file_string_from_str, impl_file_string_serialization, Path, ToFileString, ToHumanString};

use crate::{error::Error, install::InstallContext, manifest::helpers::{parse_manifest_from_bytes, read_manifest}, project::Project, report::{with_report_result, StreamReport, StreamReportConfig}, resolvers};

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ResolveOptions {
    pub active_workspace_ident: Ident,
    pub range_kind: RangeKind,
    pub resolve_tags: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct LooseResolution {
    pub descriptor: Descriptor,
    pub locator: Option<Locator>,
}

#[zpm_enum(or_else = |s| Err(Error::InvalidRange(s.to_string())))]
#[derive(Clone, Debug, Decode, Encode, PartialEq, Eq, Hash)]
#[derive_variants(Clone, Debug, Decode, Encode, PartialEq, Eq, Hash)]
pub enum LooseDescriptor {
    #[pattern(r"(?<descriptor>.*)")]
    Descriptor {
        descriptor: Descriptor,
    },

    #[pattern(r"(?<ident>.*)")]
    Ident {
        ident: Ident,
    },

    #[pattern(r"(?<range>.*)")]
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
                let path
                    = Path::try_from(&params.path)?;

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

                let descriptor
                    = Descriptor::new(ident, TarballRange {path: params.path.clone()}.into());

                Ok(LooseResolution {
                    descriptor,
                    locator: None,
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
                LooseDescriptor::resolve_registry_tag(context, options, ident, None, tag).await
            },

            LooseDescriptor::Descriptor(DescriptorLooseDescriptor {descriptor: Descriptor {ident, range: Range::RegistryTag(RegistryTagRange {ident: ident_range, tag}), ..}}) => {
                LooseDescriptor::resolve_registry_tag(context, options, ident, ident_range.as_ref(), tag).await
            },

            LooseDescriptor::Descriptor(DescriptorLooseDescriptor {descriptor}) => {
                Ok(LooseResolution {
                    descriptor: descriptor.clone(),
                    locator: None,
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
                    });
                }

                if project.config.settings.prefer_reuse.value {
                    if let Some(descriptor) = find_project_descriptor(project, ident.clone())? {
                        return Ok(LooseResolution {
                            descriptor: descriptor.clone(),
                            locator: None,
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
            });
        };

        // Otherwise we resolve them
        let resolution_result
            = resolvers::npm::resolve_semver_descriptor(context, &descriptor, &range_params).await?;

        let range = resolution_result.resolution.version
            .to_range(range_kind);

        let descriptor
            = Descriptor::new(ident.clone(), RegistrySemverRange {ident: range_ident.cloned(), range: range.clone()}.into());

        Ok(LooseResolution {
            descriptor,
            locator: Some(resolution_result.resolution.locator),
        })
    }

    async fn resolve_registry_tag(context: &InstallContext<'_>, options: &ResolveOptions, ident: &Ident, range_ident: Option<&Ident>, tag: &str) -> Result<LooseResolution, Error> {
        if !options.resolve_tags {
            let descriptor
                = Descriptor::new(ident.clone(), RegistryTagRange {ident: range_ident.cloned(), tag: tag.to_string()}.into());

            return Ok(LooseResolution {
                descriptor,
                locator: None,
            });
        }

        let descriptor
            = Descriptor::new(ident.clone(), RegistryTagRange {ident: range_ident.cloned(), tag: tag.to_string()}.into());

        let Range::RegistryTag(range_params) = &descriptor.range else {
            panic!("Invalid range");
        };

        let resolution_result
            = resolvers::npm::resolve_tag_descriptor(context, &descriptor, &range_params).await?;

        let range = resolution_result.resolution.version
            .to_range(options.range_kind);

        let descriptor
            = Descriptor::new(ident.clone(), RegistrySemverRange {ident: range_ident.cloned(), range: range.clone()}.into());

        let Range::RegistrySemver(range_params) = &descriptor.range else {
            panic!("Invalid range");
        };

        // We must check whether resolving the range would yield a
        // different version than the one we just resolved (this can
        // happen if, say, we have `rc: 1.0.0-rc.1`, and there's a
        // release version `1.1.0`).
        //
        // In that case we force the descriptor to use a fixed version
        // rather than the requested range_kind.

        let resolution_check_result
            = resolvers::npm::resolve_semver_descriptor(context, &descriptor, &range_params).await?;

        if resolution_check_result.resolution.version == resolution_result.resolution.version {
            let descriptor
                = Descriptor::new(ident.clone(), RegistrySemverRange {ident: range_ident.cloned(), range: range.clone()}.into());

            Ok(LooseResolution {
                descriptor,
                locator: Some(resolution_check_result.resolution.locator),
            })
        } else {
            let fixed_range = resolution_result.resolution.version
                .to_range(RangeKind::Exact);

            let descriptor
                = Descriptor::new(ident.clone(), RegistrySemverRange {ident: range_ident.cloned(), range: fixed_range.clone()}.into());

            Ok(LooseResolution {
                descriptor,
                locator: Some(resolution_result.resolution.locator),
            })
        }
    }
}

impl Default for LooseDescriptor {
    fn default() -> Self {
        LooseDescriptor::Ident(IdentLooseDescriptor {
            ident: Ident::new("")
        })
    }
}

impl ToFileString for LooseDescriptor {
    fn to_file_string(&self) -> String {
        match self {
            LooseDescriptor::Descriptor(DescriptorLooseDescriptor {descriptor}) => {
                descriptor.to_file_string()
            },

            LooseDescriptor::Ident(IdentLooseDescriptor {ident}) => {
                ident.to_file_string()
            },

            LooseDescriptor::Range(RangeLooseDescriptor {range}) => {
                range.to_file_string()
            },
        }
    }
}

impl ToHumanString for LooseDescriptor {
    fn to_print_string(&self) -> String {
        match self {
            LooseDescriptor::Descriptor(DescriptorLooseDescriptor {descriptor}) => {
                descriptor.to_print_string()
            },

            LooseDescriptor::Ident(IdentLooseDescriptor {ident}) => {
                ident.to_print_string()
            },

            LooseDescriptor::Range(RangeLooseDescriptor {range}) => {
                range.to_print_string()
            },
        }
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

impl_file_string_from_str!(LooseDescriptor);
impl_file_string_serialization!(LooseDescriptor);
