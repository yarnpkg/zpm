use std::{collections::BTreeMap, process::ExitCode, str::FromStr};

use clipanion::cli;
use indexmap::IndexMap;
use itertools::Itertools;
use zpm_primitives::{Descriptor, Ident, IdentGlob, Locator, Range, Reference, RegistryReference, RegistrySemverRange, ShorthandReference};
use zpm_utils::{tree, AbstractValue};

use crate::{error::Error, project::{InstallMode, Project, RunInstallOptions}};

#[derive(Debug, Default)]
enum DedupeStrategy {
    #[default]
    Highest,
}

impl FromStr for DedupeStrategy {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "highest"
                => Ok(Self::Highest),

            _
                => Err(Error::InvalidDedupeStrategy(s.to_string())),
        }
    }
}

/// Run a shell command in the package environment
///
/// Duplicates are defined as descriptors with overlapping ranges being resolved and locked to different locators. They are a natural consequence of
/// Yarn's deterministic installs, but they can sometimes pile up and unnecessarily increase the size of your project.
///
/// This command dedupes dependencies in the current project using different strategies (only one is implemented at the moment):
///
/// - `highest`: Reuses (where possible) the locators with the highest versions. This means that dependencies can only be upgraded, never downgraded.
/// It's also guaranteed that it never takes more than a single pass to dedupe the entire dependency tree.
///
/// Note: Even though it never produces a wrong dependency tree, this command should be used with caution, as it modifies the dependency tree, which
/// can sometimes cause problems when packages don't strictly follow semver recommendations. Because of this, it is recommended to also review the
/// changes manually.
///
/// If set, the `-c,--check` flag will only report the found duplicates, without persisting the modified dependency tree. If changes are found, the
/// command will exit with a non-zero exit code, making it suitable for CI purposes.
///
/// If the `--mode=<mode>` option is set, Yarn will change which artifacts are generated. The modes currently supported are:
///
/// - `skip-build` will not run the build scripts at all. Note that this is different from setting `enableScripts` to false because the latter will
///   disable build scripts, and thus affect the content of the artifacts generated on disk, whereas the former will just disable the build step but
///   not the scripts themselves, which just won't run.
///
/// - `update-lockfile` will skip the link step altogether, and only fetch packages that are missing from the lockfile (or that have no associated
///   checksums). This mode is typically used by tools like Renovate or Dependabot to keep a lockfile up-to-date without incurring the full install
///   cost.
///
/// This command accepts glob patterns as arguments. Make sure to escape the patterns, to prevent your own shell from trying to expand them.
///
#[cli::command]
#[cli::path("dedupe")]
#[cli::category("Scripting commands")]
pub struct Dedupe {
    /// Return with a non-zero exit code if duplicates are found instead of fixing them
    #[cli::option("--check", default = false)]
    check: bool,

    /// Select the artifacts this install will generate
    #[cli::option("--mode")]
    mode: Option<InstallMode>,

    /// Format the output as a NDJSON stream
    #[cli::option("--json", default = false)]
    json: bool,

    /// The strategy to use when deduping dependencies
    #[cli::option("--strategy", default = DedupeStrategy::Highest)]
    strategy: DedupeStrategy,

    /// An optional list of patterns to dedupe
    patterns: Vec<IdentGlob>,
}

impl Dedupe {
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

            child_children.insert("oldResolution".to_string(), tree::Node {
                label: Some("Old resolution".to_string()),
                value: Some(AbstractValue::new(old_resolution.clone())),
                children: None,
            });

            child_children.insert("newResolution".to_string(), tree::Node {
                label: Some("New resolution".to_string()),
                value: Some(AbstractValue::new(locator.clone())),
                children: None,
            });

            children.push(tree::Node {
                label: None,
                value: Some(AbstractValue::new(descriptor.clone())),
                children: Some(tree::TreeNodeChildren::Map(child_children)),
            });
        }

        let root_node = tree::Node {
            label: None,
            value: None,
            children: Some(tree::TreeNodeChildren::Vec(children)),
        };

        let render
            = tree::TreeRenderer::new()
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
