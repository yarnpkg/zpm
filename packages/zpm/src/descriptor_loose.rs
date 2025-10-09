use std::collections::{BTreeMap, BTreeSet};

use bincode::{Decode, Encode};
use futures::{future::BoxFuture, FutureExt};
use wax::{Glob, Program};
use zpm_formats::{iter_ext::IterExt, tar, tar_iter};
use zpm_macro_enum::zpm_enum;
use zpm_primitives::{AnonymousSemverRange, AnonymousTagRange, Descriptor, FolderRange, Ident, Range, RegistrySemverRange, RegistryTagRange, TarballRange, WorkspaceMagicRange};
use zpm_semver::RangeKind;
use zpm_utils::{impl_file_string_from_str, impl_file_string_serialization, Path, ToFileString, ToHumanString};

use crate::{error::Error, install::InstallContext, manifest::helpers::{parse_manifest_from_bytes, read_manifest}, project::Project, resolvers};

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ResolveOptions {
    pub active_workspace_ident: Ident,
    pub range_kind: RangeKind,
    pub resolve_tags: bool,
}

#[zpm_enum(or_else = |s| Err(Error::InvalidRange(s.to_string())))]
#[derive(Clone, Debug, Decode, Encode, PartialEq, Eq, Hash)]
#[derive_variants(Clone, Debug, Decode, Encode, PartialEq, Eq, Hash)]
pub enum LooseDescriptor {
    #[pattern(spec = r"(?<descriptor>.*)")]
    Descriptor {
        descriptor: Descriptor,
    },

    #[pattern(spec = r"(?<ident>.*)")]
    Ident {
        ident: Ident,
    },

    #[pattern(spec = r"(?<range>.*)")]
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

    pub async fn resolve_all<'a>(context: &'a InstallContext<'a>, options: &'a ResolveOptions, loose_descriptors: &[LooseDescriptor]) -> Result<Vec<Descriptor>, Error> {
        let mut futures: Vec<BoxFuture<'a, Result<Descriptor, Error>>> = vec![];

        for loose_descriptor in loose_descriptors {
            let loose_descriptor
                = loose_descriptor.clone();

            let future
                = async move { loose_descriptor.resolve(context, &options).await };

            futures.push(future.boxed());
        }

        let descriptors
            = futures::future::join_all(futures).await
                .into_iter()
                .collect::<Result<Vec<_>, Error>>()?;

        Ok(descriptors)
    }

    pub async fn resolve(&self, context: &InstallContext<'_>, options: &ResolveOptions) -> Result<Descriptor, Error> {
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

                let descriptor = Descriptor::new(
                    ident,
                    TarballRange {
                        path: params.path.clone(),
                    }.into(),
                );

                Ok(descriptor)
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

                let descriptor = Descriptor::new(
                    ident,
                    FolderRange {
                        path: params.path.clone(),
                    }.into(),
                );

                Ok(descriptor)
            },

            LooseDescriptor::Range(RangeLooseDescriptor {range}) => {
                Err(Error::UnsufficientLooseDescriptor(range.clone()))
            },

            LooseDescriptor::Descriptor(DescriptorLooseDescriptor {descriptor: Descriptor {ident, range: Range::AnonymousSemver(AnonymousSemverRange {range}), ..}}) => {
                let descriptor = Descriptor::new(
                    ident.clone(),
                    RegistrySemverRange {
                        ident: None,
                        range: range.clone(),
                    }.into(),
                );

                let Range::RegistrySemver(range_params) = &descriptor.range else {
                    panic!("Invalid range");
                };

                let Some(range_kind) = range_params.range.kind() else {
                    return Ok(Descriptor::new(
                        ident.clone(),
                        AnonymousSemverRange {
                            range: range.clone(),
                        }.into(),
                    ));
                };

                let resolution_result
                    = resolvers::npm::resolve_semver_descriptor(context, &descriptor, &range_params).await?;

                let range = resolution_result.resolution.version
                    .to_range(range_kind);

                Ok(Descriptor::new(
                    ident.clone(),
                    AnonymousSemverRange {
                        range,
                    }.into(),
                ))
            }

            LooseDescriptor::Descriptor(DescriptorLooseDescriptor {descriptor: Descriptor {ident, range: Range::AnonymousTag(AnonymousTagRange {tag}), ..}}) => {
                if !options.resolve_tags {
                    return Ok(Descriptor::new(
                        ident.clone(),
                        AnonymousTagRange {
                            tag: tag.clone(),
                        }.into(),
                    ));
                }

                let descriptor = Descriptor::new(
                    ident.clone(),
                    RegistryTagRange {
                        ident: None,
                        tag: tag.clone(),
                    }.into(),
                );

                let Range::RegistryTag(range_params) = &descriptor.range else {
                    panic!("Invalid range");
                };

                let resolution_result
                    = resolvers::npm::resolve_tag_descriptor(context, &descriptor, &range_params).await?;

                let range = resolution_result.resolution.version
                    .to_range(options.range_kind);

                let descriptor = Descriptor::new(
                    ident.clone(),
                    RegistrySemverRange {
                        ident: None,
                        range: range.clone(),
                    }.into(),
                );

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
                    Ok(Descriptor::new(
                        ident.clone(),
                        Range::AnonymousSemver(AnonymousSemverRange {
                            range,
                        }),
                    ))
                } else {
                    let fixed_range = resolution_result.resolution.version
                        .to_range(RangeKind::Exact);

                    Ok(Descriptor::new(
                        ident.clone(),
                        Range::AnonymousSemver(AnonymousSemverRange {
                            range: fixed_range,
                        }),
                    ))
                }
            },

            LooseDescriptor::Descriptor(DescriptorLooseDescriptor {descriptor}) => {
                Ok(descriptor.clone())
            },

            LooseDescriptor::Ident(IdentLooseDescriptor {ident}) => {
                let project = context.project.as_ref()
                    .expect("Project is required for resolving loose identifiers");

                if ident != &options.active_workspace_ident && project.workspace_by_ident(&ident).is_ok() {
                    return Ok(Descriptor::new(
                        ident.clone(),
                        WorkspaceMagicRange {
                            magic: options.range_kind,
                        }.into(),
                    ));
                }

                if project.config.settings.prefer_reuse.value {
                    if let Some(descriptor) = find_project_descriptor(project, ident.clone())? {
                        return Ok(descriptor);
                    }
                }

                let descriptor = Descriptor::new(
                    ident.clone(),
                    RegistryTagRange {
                        ident: None,
                        tag: "latest".to_string(),
                    }.into(),
                );

                let Range::RegistryTag(range_params) = &descriptor.range else {
                    unreachable!("Invalid range");
                };

                let resolution_result
                    = resolvers::npm::resolve_tag_descriptor(context, &descriptor, &range_params).await?;

                let range = resolution_result.resolution.version
                    .to_range(options.range_kind);

                Ok(Descriptor::new(
                    ident.clone(),
                    Range::AnonymousSemver(AnonymousSemverRange {
                        range,
                    }),
                ))
            },
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
