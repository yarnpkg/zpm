use clipanion::cli;
use indexmap::IndexMap;
use zpm_primitives::Range;
use zpm_semver::PrereleasePattern;
use zpm_utils::{AbstractValue, FromFileString, ToFileString, ToHumanString, tree};

use crate::{
    error::Error,
    project::{self, Workspace},
    versioning::{self, ExactReleaseStrategy, ReleaseStrategy, Versioning},
};

/// Change versions of workspaces
///
/// This command will bump the version of the current workspace, according to the given strategy:
///
/// - If it is a valid semver, it will be used as the new version (build metadata is stripped).
/// - The `major`, `minor`, and `patch` strategies will bump to the next semver-major, semver-minor, or semver-patch version, respectively. A
///   prerelease of the corresponding level is bumped to its stable version (`1.2.0-3` becomes `1.2.0` with `minor`).
/// - The `prepatch` strategy will only increment the prerelease identifier if the current version is already a prerelease of a semver-patch or
///   higher level bump. Otherwise, the new version is calculated by adding prerelease identifier(s) to the next semver-patch version.
/// - The `preminor` and `premajor` strategies work similarly to `prepatch`, but for semver-minor and semver-major versions, respectively.
/// - The `prerelease` strategy will only increment the prerelease identifier if the current version is already a prerelease. Otherwise, it is
///   equivalent to `prepatch`.
/// - The `decline` strategy will not bump versions at all. It is mostly for use with deferred versioning.
///
/// By default, the version bump will only apply to the current workspace. If `--all` is set, the version bump will be applied to all workspaces in
/// the project. Note that the ranges that other workspaces use to depend on the bumped workspaces aren't updated; use the `workspace:^` /
/// `workspace:~` ranges if you need them to follow the version bumps.
///
/// If `--dry-run` is set, the command will print the new versions the workspace(s) would be bumped to (or the strategies that would be recorded)
/// without actually changing anything.
///
/// Attempting to bump a workspace to a lower version than its current one will throw an error unless `--force` is set. Note that when using deferred
/// versioning, `--force` is needed twice: once when recording the deferred version bump, and again when applying it.
///
/// ### Prerelease identifiers
///
/// By default, incrementing an existing prerelease identifier will increment the last numeric component, and `-0` will be used when adding a
/// prerelease identifier to a non-prerelease version.
///
/// The `--prerelease` option can be used to specify a prerelease pattern to use. In the pattern, `%n` can be used to specify an incrementing numeric
/// component. When incrementing an existing prerelease identifier, it will be matched against the pattern. If the pattern matches, all components
/// matched by `%n` will be incremented. If the pattern doesn't match, or if a prerelease identifier is being added to a non-prerelease version, the
/// `%n` components will be set to 1. If the `--prerelease` option is set without a value, the default pattern of `rc.%n` will be used.
///
/// The prerelease pattern is also applied to the `major`, `minor`, and `patch` strategies (so `yarn version patch --prerelease=alpha.0` turns
/// `1.0.32` into `1.0.33-alpha.0`), but not to explicit versions. When `--prerelease` is set, the deferred records of the bumped workspaces are kept
/// so that a later stable bump can still consume them.
///
/// ### Immediate vs deferred versioning
///
/// By default, the version bump will be applied immediately. If the `--deferred` flag or the `preferDeferredVersions` configuration option is set,
/// deferred versioning will be used instead: the version bump will only be recorded, and can be applied in the future by running `yarn version
/// apply`. The `--immediate` flag can be used to force immediate mode even if `preferDeferredVersions` is set.
///
/// An immediate version bump can be applied to workspaces for which deferred records exist. However, attempting to bump to a version that is lower
/// than the recorded deferred bump will throw an error unless `--force` is set. After the immediate bump, deferred records for the bumped
/// workspace(s) will be consumed.
///
#[cli::command]
#[cli::path("version")]
#[cli::category("Project management")]
pub struct Version {
    /// Record the release strategy only
    #[cli::option("-d,--deferred", default = false)]
    deferred: bool,

    /// Bump the version immediately
    #[cli::option("-i,--immediate", default = false)]
    immediate: bool,

    /// Bump the version of all workspaces
    #[cli::option("--all", default = false)]
    all: bool,

    /// Specify a prerelease pattern to use when computing the new versions
    #[cli::option("--prerelease")]
    prerelease: Option<Option<String>>,

    /// Print version(s)/record(s) without actually bumping versions or recording deferred releases
    #[cli::option("--dry-run", default = false)]
    dry_run: bool,

    /// Bypass the check for bumping to a lower version than the current/deferred one
    #[cli::option("--force", default = false)]
    force: bool,

    /// Format the output as an NDJSON stream
    #[cli::option("--json", default = false)]
    json: bool,

    /// Version bump strategy, exact version, or `decline`
    strategy: String,
}

impl Version {
    pub async fn execute(&self) -> Result<(), Error> {
        let project
            = project::Project::new(None).await?;

        let deferred
            = (self.deferred || project.config.settings.prefer_deferred_versions.value) && !self.immediate;

        bump_versions(&project, BumpOptions {
            strategy: parse_strategy(&self.strategy)?,
            all: self.all,
            deferred,
            prerelease: parse_prerelease_pattern(&self.prerelease)?,
            dry_run: self.dry_run,
            force: self.force,
            json: self.json,
        }).await
    }
}

/// Parses the `--prerelease` option, which defaults to `rc.%n` when set
/// without a value.
pub fn parse_prerelease_pattern(prerelease: &Option<Option<String>>) -> Result<Option<PrereleasePattern>, Error> {
    match prerelease {
        Some(Some(pattern))
            => Ok(Some(PrereleasePattern::from_file_string(pattern)?)),

        Some(None)
            => Ok(Some(PrereleasePattern::default())),

        None
            => Ok(None),
    }
}

/// Parses a strategy, returning `None` for `decline`.
fn parse_strategy(strategy: &str) -> Result<Option<ReleaseStrategy>, Error> {
    match strategy {
        "major" => return Ok(Some(ReleaseStrategy::Major)),
        "minor" => return Ok(Some(ReleaseStrategy::Minor)),
        "patch" => return Ok(Some(ReleaseStrategy::Patch)),
        "premajor" => return Ok(Some(ReleaseStrategy::Premajor)),
        "preminor" => return Ok(Some(ReleaseStrategy::Preminor)),
        "prepatch" => return Ok(Some(ReleaseStrategy::Prepatch)),
        "prerelease" | "pre" | "rc" => return Ok(Some(ReleaseStrategy::Prerelease)),
        "decline" => return Ok(None),
        _ => {},
    }

    if let Ok(version) = zpm_semver::Version::from_strict_str(strategy) {
        return Ok(Some(ExactReleaseStrategy {version}.into()));
    }

    // Suggest the normalized version for strings that look like a version
    // but aren't strictly valid (for example `1.2.3-01`)
    let main_part = strategy.strip_prefix('v').unwrap_or(strategy)
        .split(['-', '+'])
        .next()
        .unwrap_or_default();

    let looks_like_version
        = main_part.split('.').count() == 3 && main_part.split('.').all(|part| !part.is_empty() && part.chars().all(|c| c.is_ascii_digit()));

    let suggestion = looks_like_version
        .then(|| zpm_semver::Version::from_file_string(strategy).ok())
        .flatten()
        .map(|version| version.to_file_string());

    Err(Error::InvalidVersionStrategy(strategy.to_string(), suggestion))
}

pub struct BumpOptions {
    /// The strategy to apply; `None` means `decline`, which only applies
    /// the deferred records
    pub strategy: Option<ReleaseStrategy>,
    pub all: bool,
    pub deferred: bool,
    pub prerelease: Option<PrereleasePattern>,
    pub dry_run: bool,
    pub force: bool,
    pub json: bool,
}

pub async fn bump_versions(project: &project::Project, options: BumpOptions) -> Result<(), Error> {
    let workspaces: Vec<&Workspace> = match options.all {
        true => project.workspaces.iter().collect(),
        false => vec![project.active_workspace()?],
    };

    let versioning
        = Versioning::new(project);

    let mut warnings
        = Vec::new();

    match &options.strategy {
        Some(ReleaseStrategy::Exact(params)) => {
            for workspace in &workspaces {
                let Some(current_version) = &workspace.manifest.remote.version else {
                    continue;
                };

                if params.version < *current_version {
                    if !options.force {
                        return Err(Error::VersionBumpLowerThanCurrent(workspace.name.clone(), current_version.clone(), params.version.clone()));
                    }

                    warnings.push(format!("Bumping {} to a lower version ({}) than the current one ({})", workspace.name.to_print_string(), params.version.to_print_string(), current_version.to_print_string()));
                }
            }
        },

        Some(_) => {
            if workspaces.iter().any(|workspace| workspace.manifest.remote.version.is_none()) {
                return Err(Error::NoVersionFoundForActiveWorkspace);
            }
        },

        None => {
        },
    }

    if options.deferred {
        if !options.dry_run {
            for workspace in &workspaces {
                versioning.set_workspace_release_strategy(&workspace.name, options.strategy.clone()).await?;
            }
        }

        let strategy_label = match &options.strategy {
            Some(strategy) => strategy.to_file_string(),
            None => "decline".to_string(),
        };

        let records = workspaces.iter().map(|workspace| {
            let mut children
                = IndexMap::new();

            children.insert("strategy".to_string(), tree::Node {
                label: Some("Deferred strategy".to_string()),
                value: Some(AbstractValue::new(strategy_label.clone())),
                children: None,
            });

            (workspace.name.clone(), children)
        }).collect();

        render_workspaces(records, options.json);
        print_warnings(&warnings, options.json);

        return Ok(());
    }

    let stored_versions
        = versioning.resolve_releases(versioning::ResolveOptions {
            prerelease: options.prerelease.as_ref(),
        })?;

    let mut releases
        = Vec::new();

    for workspace in &workspaces {
        let stored_version
            = stored_versions.get(&workspace.name);

        let Some(strategy) = &options.strategy else {
            match stored_version {
                Some(stored_version) => {
                    releases.push((*workspace, stored_version.clone()));
                },

                None if !options.all => {
                    warnings.push(format!("Workspace {} doesn't seem to require a version bump.", workspace.name.to_print_string()));
                },

                None => {
                },
            }

            continue;
        };

        let new_version = match (strategy, &workspace.manifest.remote.version) {
            (ReleaseStrategy::Exact(params), _)
                => params.version.clone(),

            (_, Some(current_version))
                => strategy.apply(current_version, options.prerelease.as_ref()),

            (_, None)
                => return Err(Error::NoVersionFoundForActiveWorkspace),
        };

        if let Some(stored_version) = stored_version {
            if new_version < *stored_version {
                if !options.force {
                    return Err(Error::VersionBumpLowerThanDeferred(stored_version.clone()));
                }

                warnings.push(format!("Bumping {} to a lower version ({}) than the current deferred one ({})", workspace.name.to_print_string(), new_version.to_print_string(), stored_version.to_print_string()));
            }
        }

        releases.push((*workspace, new_version));
    }

    if options.all && releases.is_empty() {
        warnings.push("The project doesn't seem to require a version bump.".to_string());
    }

    for (workspace, new_version) in &releases {
        let Some(current_version) = &workspace.manifest.remote.version else {
            continue;
        };

        // Lower versions for explicit strategies have already been reported
        // above, and bumps lower than the deferred one are reported in the
        // loop that computes them
        let is_already_reported
            = matches!(options.strategy, Some(ReleaseStrategy::Exact(_)))
                || stored_versions.get(&workspace.name).is_some_and(|stored_version| new_version < stored_version);

        if new_version < current_version && !is_already_reported {
            if !options.force {
                return Err(Error::VersionBumpLowerThanCurrent(workspace.name.clone(), current_version.clone(), new_version.clone()));
            }

            warnings.push(format!("Bumping {} to a lower version ({}) than the current one ({})", workspace.name.to_print_string(), new_version.to_print_string(), current_version.to_print_string()));
        }
    }

    let records = releases.iter().map(|(workspace, new_version)| {
        let mut children
            = IndexMap::new();

        if let Some(current_version) = &workspace.manifest.remote.version {
            children.insert("current".to_string(), tree::Node {
                label: Some("Current version".to_string()),
                value: Some(AbstractValue::new(current_version.clone())),
                children: None,
            });
        }

        children.insert("next".to_string(), tree::Node {
            label: Some("Next version".to_string()),
            value: Some(AbstractValue::new(new_version.clone())),
            children: None,
        });

        (workspace.name.clone(), children)
    }).collect();

    render_workspaces(records, options.json);
    print_warnings(&warnings, options.json);

    if options.dry_run {
        return Ok(());
    }

    for (workspace, new_version) in &releases {
        versioning.set_manifest_version(&workspace.name, new_version)?;

        // When a prerelease pattern is set the deferred records are kept,
        // so that a later stable bump can still consume them
        if options.prerelease.is_none() && stored_versions.contains_key(&workspace.name) {
            versioning.discard_workspace_from_release(&workspace.name)?;
        }
    }

    if !options.json {
        for (workspace, _) in &releases {
            report_non_upgradeable_dependents(project, &workspace.name);
        }
    }

    Ok(())
}

fn render_workspaces(records: Vec<(zpm_primitives::Ident, IndexMap<String, tree::Node<'_>>)>, json: bool) {
    let root_children = records.into_iter().map(|(ident, children)| tree::Node {
        label: None,
        value: Some(AbstractValue::new(ident)),
        children: Some(tree::TreeNodeChildren::Map(children)),
    }).collect();

    let root_node = tree::Node {
        label: None,
        value: None,
        children: Some(tree::TreeNodeChildren::Vec(root_children)),
    };

    let rendering
        = tree::TreeRenderer::new()
            .render(&root_node, json);

    print!("{}", rendering);
}

fn print_warnings(warnings: &[String], json: bool) {
    if json {
        return;
    }

    for warning in warnings {
        println!("{}", warning);
    }
}

fn report_non_upgradeable_dependents(project: &project::Project, bumped_ident: &zpm_primitives::Ident) {
    for workspace in project.workspaces.iter() {
        // Collect both hard and peer ranges that target the bumped
        // workspace. Peers were missed before; they share the same
        // upgrade story as hard deps.
        let hard_ranges = workspace.manifest.iter_hard_dependencies()
            .filter(|hard| hard.ident == bumped_ident)
            .map(|hard| hard.descriptor.range.clone());

        let peer_ranges = workspace.manifest.iter_peer_dependencies()
            .filter(|peer| peer.ident == bumped_ident)
            .map(|peer| peer.range.to_range());

        for range in hard_ranges.chain(peer_ranges) {
            // `workspace:^`, `workspace:~`, `workspace:*` (magics) and
            // explicit `workspace:<semver>` ranges all encode a pin
            // that won't follow a major bump. `WorkspaceIdent` and
            // `WorkspacePath` aren't versioned — nothing to flag.
            let report = matches!(
                range,
                Range::WorkspaceMagic(_) | Range::WorkspaceSemver(_),
            );

            if report {
                println!(
                    "Couldn't auto-upgrade range {} (in {})",
                    range.to_file_string(),
                    workspace.locator_path().to_print_string(),
                );
            }
        }
    }
}
