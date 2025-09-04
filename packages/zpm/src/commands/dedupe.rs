use std::{collections::BTreeMap, process::ExitCode};

use clipanion::cli;
use indexmap::IndexMap;
use itertools::Itertools;
use zpm_primitives::{Descriptor, Ident, IdentGlob, Locator, Range, Reference, RegistryReference, RegistrySemverRange, ShorthandReference};
use zpm_utils::AbstractValue;

use crate::{error::Error, project::{InstallMode, Project, RunInstallOptions}, ui};

#[cli::command]
#[cli::path("dedupe")]
#[cli::category("Scripting commands")]
#[cli::description("Run a shell command in the package environment")]
pub struct Dedupe {
    #[cli::option("--check", default = false)]
    check: bool,

    #[cli::option("--mode")]
    mode: Option<InstallMode>,

    #[cli::option("--json", default = false)]
    json: bool,

    patterns: Vec<IdentGlob>,
}

impl Dedupe {
    #[tokio::main()]
    pub async fn execute(&self) -> Result<ExitCode, Error> {
        let mut project
            = Project::new(None).await?;

        project
            .lazy_install().await?;

        let enforced_resolutions
            = prepare_highest_dedupe(&project, &self.patterns)?;

        if self.check {
            if enforced_resolutions.is_empty() {
                Ok(ExitCode::SUCCESS)
            } else {
                self.report_dedupe_needed(&project, &enforced_resolutions)?;
                Ok(ExitCode::FAILURE)
            }
        } else {
            project.run_install(RunInstallOptions {
                enforced_resolutions,
                mode: self.mode,
                ..Default::default()
            }).await?;

            Ok(ExitCode::SUCCESS)
        }
    }

    fn report_dedupe_needed(&self, project: &Project, enforced_resolutions: &BTreeMap<Descriptor, Locator>) -> Result<(), Error> {
        let install_state
            = project.install_state.as_ref()
                .ok_or(Error::InstallStateNotFound)?;

        let mut children
            = vec![];

        for (descriptor, locator) in enforced_resolutions {
            let mut child_children
                = IndexMap::new();

            let old_resolution
                = &install_state.descriptor_to_locator[descriptor];

            child_children.insert("oldResolution".to_string(), ui::tree::Node {
                label: Some("Old resolution".to_string()),
                value: Some(AbstractValue::new(old_resolution.clone())),
                children: None,
            });

            child_children.insert("newResolution".to_string(), ui::tree::Node {
                label: Some("New resolution".to_string()),
                value: Some(AbstractValue::new(locator.clone())),
                children: None,
            });

            children.push(ui::tree::Node {
                label: None,
                value: Some(AbstractValue::new(descriptor.clone())),
                children: Some(ui::tree::TreeNodeChildren::Map(child_children)),
            });
        }

        let root_node = ui::tree::Node {
            label: None,
            value: None,
            children: Some(ui::tree::TreeNodeChildren::Vec(children)),
        };

        let render
            = ui::tree::TreeRenderer::new()
                .render(&root_node, self.json);

        print!("{}", render);

        if !self.json {
            println!();
            println!("{} {} can be deduped using the highest strategy", enforced_resolutions.len(), if enforced_resolutions.len() == 1 {"package"} else {"packages"});
        }

        Ok(())
    }
}

fn extract_semver_version(locator: &Locator) -> Option<(&Ident, &zpm_semver::Version)> {
    match &locator.reference {
        Reference::Shorthand(params)
            => Some((&locator.ident, &params.version)),

        Reference::Registry(params)
            => Some((&params.ident, &params.version)),

        _ => None,
    }
}

fn prepare_highest_dedupe(project: &Project, patterns: &Vec<IdentGlob>) -> Result<BTreeMap<Descriptor, Locator>, Error> {
    let install_state
        = project.install_state.as_ref()
            .ok_or(Error::InstallStateNotFound)?;

    let locators_by_ident
        = install_state.normalized_resolutions.keys()
            .filter_map(extract_semver_version)
            .into_group_map_by(|(ident, _)| *ident);

    let best_version_for = |ident: &Ident, range: &zpm_semver::Range| {
        locators_by_ident.get(&ident).unwrap().iter()
            .map(|(_, version)| *version)
            .filter(|version| range.check(version))
            .max()
    };

    let attach_highest_version = |descriptor: &Descriptor| {
        let suggested_locator = match &descriptor.range {
            Range::AnonymousSemver(params) => {
                let best_version
                    = best_version_for(&descriptor.ident, &params.range);

                best_version.map(|version| {
                    Locator::new(descriptor.ident.clone(), ShorthandReference {
                        version: version.clone(),
                    }.into())
                })
            },

            Range::RegistrySemver(RegistrySemverRange {ident: None, range}) => {
                let best_version
                    = best_version_for(&descriptor.ident, range);

                best_version.map(|version| {
                    Locator::new(descriptor.ident.clone(), ShorthandReference {
                        version: version.clone(),
                    }.into())
                })
            },

            Range::RegistrySemver(RegistrySemverRange {ident: Some(ident), range}) => {
                let best_version
                    = best_version_for(ident, range);

                best_version.map(|version| {
                    Locator::new(descriptor.ident.clone(), RegistryReference {
                        ident: ident.clone(),
                        version: version.clone(),
                    }.into())
                })
            },

            _ => return None,
        };

        suggested_locator.and_then(|locator| {
            let current_resolution
                = install_state.descriptor_to_locator
                    .get(descriptor);

            if current_resolution != Some(&locator) {
                Some((descriptor.clone(), locator))
            } else {
                None
            }
        })
    };

    let upgradable_candidates
        = install_state.descriptor_to_locator.keys()
            .filter(|descriptor| patterns.is_empty() || patterns.iter().any(|matcher| matcher.check(&descriptor.ident)))
            .filter_map(attach_highest_version)
            .collect::<BTreeMap<_, _>>();

    Ok(upgradable_candidates)
}
