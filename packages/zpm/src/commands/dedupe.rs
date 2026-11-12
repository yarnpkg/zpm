use std::{collections::BTreeMap, process::ExitCode, str::FromStr};

use clipanion::cli;
use indexmap::IndexMap;
use zpm_primitives::{Descriptor, IdentGlob, Locator};
use zpm_utils::{tree, AbstractValue, ToFileString};

use crate::{
    dedupe::prepare_highest_dedupe,
    install::InstallState,
    error::Error,
    project::{InstallMode, Project, RunInstallOptions},
    report::{StreamReport, StreamReportConfig, with_report_result},
};

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

/// Deduplicate dependencies with overlapping ranges
///
/// Duplicates are defined as descriptors with overlapping ranges being resolved and locked to different locators. They are a natural consequence of
/// Yarn's deterministic installs, but they can sometimes pile up and unnecessarily increase the size of your project.
///
/// This command deduplicates dependencies in the current project using different strategies. Only one strategy is currently implemented:
///
/// - `highest`: Reuses (where possible) the locators with the highest versions. This means that dependencies can only be upgraded, never downgraded.
/// It's also guaranteed that it never takes more than a single pass to dedupe the entire dependency tree.
///
/// Note: Even though it never produces a wrong dependency tree, this command should be used with caution, as it modifies the dependency tree, which
/// can sometimes cause problems when packages don't strictly follow semver recommendations. Because of this, it is recommended to also review the
/// changes manually.
///
/// If `--check` is set, Yarn only reports duplicates and leaves the lockfile unchanged. If duplicates are found, the command exits with a non-zero
/// exit code, making it suitable for CI.
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
    /// Report duplicates without modifying the lockfile
    #[cli::option("--check", default = false)]
    check: bool,

    /// Select which install artifacts Yarn should generate after deduping
    #[cli::option("--mode")]
    mode: Option<InstallMode>,

    /// Format the output as an NDJSON stream
    #[cli::option("--json", default = false)]
    json: bool,

    /// The deduplication strategy to use
    #[cli::option("--strategy", default = DedupeStrategy::Highest)]
    strategy: DedupeStrategy,

    /// Package name patterns to deduplicate
    patterns: Vec<IdentGlob>,
}

impl Dedupe {
    pub async fn execute(&self) -> Result<ExitCode, Error> {
        let mut project
            = Project::new(None).await?;

        let mut options = RunInstallOptions {
            mode: self.mode,
            silent_or_error: self.check,
            reject_lockfile_conflicts: self.check,
            ..Default::default()
        };
        let report = StreamReport::new(StreamReportConfig {
            include_version: true,
            silent_or_error: self.check,
            ..StreamReportConfig::from_config(&project.config)
        });

        with_report_result(report, async {
            let package_cache
                = project.package_cache()?;
            let mut install
                = project.prepare_install(&options, &package_cache).await?;
            let enforced_resolutions
                = prepare_highest_dedupe(&install.install_state, &self.patterns);

            if self.check {
                if enforced_resolutions.is_empty() {
                    return Ok(ExitCode::SUCCESS);
                }

                self.report_dedupe_needed(&install.install_state, &enforced_resolutions)?;
                return Ok(ExitCode::FAILURE);
            }

            if !enforced_resolutions.is_empty() {
                options.enforced_resolutions = enforced_resolutions.into_iter()
                    .map(|(descriptor, locator)| (descriptor, Some(locator)))
                    .collect();

                install = project.prepare_install_from_lockfile(&options, &package_cache, install.lockfile).await?;
            }

            install.link_and_build(&mut project, &package_cache).await?;

            Ok(ExitCode::SUCCESS)
        }).await
    }

    fn report_dedupe_needed(&self, install_state: &InstallState, enforced_resolutions: &BTreeMap<Descriptor, Locator>) -> Result<(), Error> {
        if self.json {
            for (descriptor, locator) in enforced_resolutions {
                let entry = serde_json::json!({
                    "descriptor": descriptor.to_file_string(),
                    "currentResolution": install_state.descriptor_to_locator[descriptor].to_file_string(),
                    "updatedResolution": locator.to_file_string(),
                });

                println!("{}", serde_json::to_string(&entry).unwrap());
            }

            return Ok(());
        }

        let mut children
            = vec![];

        for (descriptor, locator) in enforced_resolutions {
            let mut child_children
                = IndexMap::new();

            child_children.insert("oldResolution".to_string(), tree::Node {
                label: Some("Old resolution".to_string()),
                value: Some(AbstractValue::new(install_state.descriptor_to_locator[descriptor].clone())),
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
                .render(&root_node, false);

        print!("{}", render);

        println!();
        println!("{} {} can be deduped using the highest strategy", enforced_resolutions.len(), if enforced_resolutions.len() == 1 {"package"} else {"packages"});

        Ok(())
    }
}
