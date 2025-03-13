use arca::Path;
use bincode::{Decode, Encode};
use colored::Colorize;
use futures::future::BoxFuture;
use futures::FutureExt;
use zpm_formats::{tar, tar_iter, iter_ext::IterExt};
use zpm_macros::parse_enum;
use zpm_semver::RangeKind;
use zpm_utils::{impl_serialization_traits, ToFileString, ToHumanString};

use crate::error::Error;
use crate::install::InstallContext;
use crate::manifest::{parse_manifest_from_bytes, read_manifest};
use crate::resolvers;

use super::range::{AnonymousSemverRange, AnonymousTagRange, FolderRange, RegistrySemverRange, RegistryTagRange, TarballRange, WorkspaceMagicRange};
use super::{Ident, Range, Descriptor};

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ResolveOptions {
    pub range_kind: RangeKind,
    pub resolve_tags: bool,
}

#[parse_enum(or_else = |s| Err(Error::InvalidRange(s.to_string())))]
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
                    = Path::from(&params.path);

                let tgz_content = path
                    .fs_read_prealloc()?;

                let tar_content
                    = tar::unpack_tgz(&tgz_content)?;

                let package_json_entry
                    = tar_iter::TarIterator::new(&tar_content)
                        .strip_first_segment()
                        .filter_map(|entry| entry.ok())
                        .next();

                let Some(package_json_entry) = package_json_entry else {
                    return Err(Error::ManifestNotFound);
                };

                let manifest
                    = parse_manifest_from_bytes(&package_json_entry.data)?;

                let ident = manifest.name
                    .ok_or_else(|| Error::MissingPackageName)?;

                let descriptor = Descriptor::new(
                    ident,
                    Range::Tarball(TarballRange {
                        path: params.path.clone(),
                    }),
                );

                Ok(descriptor)
            }

            LooseDescriptor::Range(RangeLooseDescriptor {range: Range::Folder(params)}) => {
                let path
                    = Path::from(&params.path);

                let manifest_path = path
                    .with_join_str("package.json");

                let manifest
                    = read_manifest(&manifest_path)?;

                let ident = manifest.name
                    .ok_or_else(|| Error::MissingPackageName)?;

                let descriptor = Descriptor::new(
                    ident,
                    Range::Folder(FolderRange {
                        path: params.path.clone(),
                    }),
                );

                Ok(descriptor)
            },

            LooseDescriptor::Range(RangeLooseDescriptor {range}) => {
                Err(Error::UnsufficientLooseDescriptor(range.clone()))
            },

            LooseDescriptor::Descriptor(DescriptorLooseDescriptor {descriptor: Descriptor {ident, range: Range::AnonymousSemver(AnonymousSemverRange {range}), ..}}) => {
                let descriptor = Descriptor::new(
                    ident.clone(),
                    Range::RegistrySemver(RegistrySemverRange {
                        ident: None,
                        range: range.clone(),
                    }),
                );

                let Range::RegistrySemver(range_params) = &descriptor.range else {
                    panic!("Invalid range");
                };

                let Some(range_kind) = range_params.range.kind() else {
                    return Ok(Descriptor::new(
                        ident.clone(),
                        Range::RegistrySemver(RegistrySemverRange {
                            ident: None,
                            range: range.clone(),
                        }),
                    ));
                };

                let resolution_result
                    = resolvers::npm::resolve_semver_descriptor(context, &descriptor, &range_params).await?;

                let range = resolution_result.resolution.version
                    .to_range(range_kind);

                Ok(Descriptor::new(
                    ident.clone(),
                    Range::RegistrySemver(RegistrySemverRange {
                        ident: None,
                        range,
                    }),
                ))
            }

            LooseDescriptor::Descriptor(DescriptorLooseDescriptor {descriptor: Descriptor {ident, range: Range::AnonymousTag(AnonymousTagRange {tag}), ..}}) => {
                let descriptor = Descriptor::new(
                    ident.clone(),
                    Range::RegistryTag(RegistryTagRange {
                        ident: None,
                        tag: tag.clone(),
                    }),
                );

                if !options.resolve_tags {
                    return Ok(descriptor);
                }

                let Range::RegistryTag(range_params) = &descriptor.range else {
                    panic!("Invalid range");
                };

                let resolution_result
                    = resolvers::npm::resolve_tag_descriptor(context, &descriptor, &range_params).await?;

                let range = resolution_result.resolution.version
                    .to_range(options.range_kind);

                let descriptor = Descriptor::new(
                    ident.clone(),
                    Range::RegistrySemver(RegistrySemverRange {
                        ident: None,
                        range,
                    }),
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

                let descriptor = if resolution_check_result.resolution.version == resolution_result.resolution.version {
                    descriptor
                } else {
                    let fixed_range = resolution_result.resolution.version
                        .to_range(RangeKind::Exact);

                    Descriptor::new(
                        ident.clone(),
                        Range::RegistrySemver(RegistrySemverRange {
                            ident: None,
                            range: fixed_range,
                        }),
                    )
                };

                Ok(descriptor)
            },

            LooseDescriptor::Descriptor(DescriptorLooseDescriptor {descriptor}) => {
                Ok(descriptor.clone())
            },

            LooseDescriptor::Ident(IdentLooseDescriptor {ident}) => {
                let project = context.project.as_ref()
                    .expect("Project is required for resolving loose identifiers");

                if project.workspace_by_ident(&ident).is_ok() {
                    let range = Range::WorkspaceMagic(match options.range_kind {
                        RangeKind::Exact => WorkspaceMagicRange {magic: "*".to_string()},
                        RangeKind::Tilde => WorkspaceMagicRange {magic: "~".to_string()},
                        RangeKind::Caret => WorkspaceMagicRange {magic: "^".to_string()},
                    });

                    return Ok(Descriptor::new(
                        ident.clone(),
                        range,
                    ));
                }

                let descriptor = Descriptor::new(
                    ident.clone(),
                    Range::RegistryTag(RegistryTagRange {
                        ident: None,
                        tag: "latest".to_string(),
                    }),
                );

                let Range::RegistryTag(range_params) = &descriptor.range else {
                    panic!("Invalid range");
                };

                let resolution_result
                    = resolvers::npm::resolve_tag_descriptor(context, &descriptor, &range_params).await?;

                let range = resolution_result.resolution.version
                    .to_range(options.range_kind);

                Ok(Descriptor::new(
                    ident.clone(),
                    Range::RegistrySemver(RegistrySemverRange {
                        ident: None,
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
            LooseDescriptor::Descriptor(DescriptorLooseDescriptor {descriptor}) => descriptor.to_file_string(),
            LooseDescriptor::Ident(IdentLooseDescriptor {ident}) => ident.to_file_string(),
            LooseDescriptor::Range(RangeLooseDescriptor {range}) => range.to_file_string(),
        }
    }
}

impl ToHumanString for LooseDescriptor {
    fn to_print_string(&self) -> String {
        self.to_file_string().truecolor(0, 175, 175).to_string()
    }
}

impl_serialization_traits!(LooseDescriptor);
