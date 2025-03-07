use bincode::{Decode, Encode};
use colored::Colorize;
use futures::future::BoxFuture;
use futures::FutureExt;
use zpm_macros::parse_enum;
use zpm_semver::RangeKind;
use zpm_utils::{impl_serialization_traits, ToFileString, ToHumanString};

use crate::error::Error;
use crate::install::InstallContext;
use crate::resolvers;

use super::range::{RegistrySemverRange, RegistryTagRange};
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
    #[pattern(spec = r"git:(?<repository>.*)")]
    Repository {
        repository: String,
    },

    #[pattern(spec = "file:(?:(?<ident>@?[^@]+)@)?(?<path>.*)")]
    #[pattern(spec = "(?:(?<ident>@?[^@]+)@)?(?<path>/.*)")]
    File {
        ident: Option<Ident>,
        path: String,
    },


    #[pattern(spec = r"(?<ident>@?[^@]+)@(?<range>.*)")]
    Semver {
        ident: Ident,
        range: zpm_semver::Range,
    },

    #[pattern(spec = r"(?<ident>@?[^@]+)@(?<tag>.*)")]
    Tag {
        ident: Ident,
        tag: String,
    },

    #[pattern(spec = r"(?<ident>@?[^@]+)")]
    Ident {
        ident: Ident,
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
            LooseDescriptor::File(params) => {
                let descriptor = Descriptor::new(
                    params.ident.clone(),
                    Range::File(params.path.clone()),
                );

                Ok(descriptor)
            },

            LooseDescriptor::Semver(params) => {
                let descriptor = Descriptor::new(
                    params.ident.clone(),
                    Range::RegistrySemver(RegistrySemverRange {
                        ident: None,
                        range: params.range.clone(),
                    }),
                );

                let Range::RegistrySemver(range_params) = &descriptor.range else {
                    panic!("Invalid range");
                };

                let Some(range_kind) = range_params.range.kind() else {
                    return Ok(Descriptor::new(
                        params.ident.clone(),
                        Range::RegistrySemver(RegistrySemverRange {
                            ident: None,
                            range: params.range.clone(),
                        }),
                    ));
                };

                let resolution_result
                    = resolvers::npm::resolve_semver_descriptor(context, &descriptor, &range_params).await?;

                let range = resolution_result.resolution.version
                    .to_range(range_kind);

                Ok(Descriptor::new(
                    params.ident.clone(),
                    Range::RegistrySemver(RegistrySemverRange {
                        ident: None,
                        range,
                    }),
                ))
            }

            LooseDescriptor::Tag(params) => {
                let descriptor = Descriptor::new(
                    params.ident.clone(),
                    Range::RegistryTag(RegistryTagRange {
                        ident: None,
                        tag: params.tag.clone(),
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
                    params.ident.clone(),
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
                        params.ident.clone(),
                        Range::RegistrySemver(RegistrySemverRange {
                            ident: None,
                            range: fixed_range,
                        }),
                    )
                };

                Ok(descriptor)
            },

            LooseDescriptor::Ident(params) => {
                let descriptor = Descriptor::new(
                    params.ident.clone(),
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
                    params.ident.clone(),
                    Range::RegistrySemver(RegistrySemverRange {
                        ident: None,
                        range,
                    }),
                ))
            },

            _ => {
                Err(Error::Unsupported)
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
            LooseDescriptor::Repository(params) => format!("git:{}", params.repository),
            LooseDescriptor::File(params) => format!("file:{}", params.path),
            LooseDescriptor::Semver(params) => format!("{}@{}", params.ident.to_file_string(), params.range.to_file_string()),
            LooseDescriptor::Tag(params) => format!("{}@{}", params.ident.to_file_string(), params.tag),
            LooseDescriptor::Ident(params) => params.ident.to_file_string(),
        }
    }
}

impl ToHumanString for LooseDescriptor {
    fn to_print_string(&self) -> String {
        self.to_file_string().truecolor(0, 175, 175).to_string()
    }
}

impl_serialization_traits!(LooseDescriptor);
