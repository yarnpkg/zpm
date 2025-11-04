use std::collections::{HashMap, HashSet};

use clipanion::cli;
use zpm_parsers::{document::Document, JsonDocument, Value};
use zpm_primitives::{AnonymousSemverRange, Descriptor};
use zpm_semver::RangeKind;
use zpm_utils::{FromFileString, ToFileString, ToHumanString};

use crate::{
    algolia::query_algolia,
    descriptor_loose::{self, LooseDescriptor},
    error::Error,
    install::InstallContext,
    project::{self, InstallMode}
};

#[derive(Clone, Debug)]
struct AddRequest {
    prod: bool,
    peer: bool,
    dev: bool,
    optional: bool,
}

async fn expand_with_types<'a>(install_context: &InstallContext<'a>, _resolve_options: &descriptor_loose::ResolveOptions, requests: Vec<(Descriptor, AddRequest)>) -> Result<Vec<(Descriptor, AddRequest)>, Error> {
    let project = install_context.project
        .expect("Project not found");

    if !project.config.settings.enable_auto_types.value {
        return Ok(requests);
    }

    let mut type_requests = requests.clone();

    let ident_set = requests.iter()
        .map(|(descriptor, _)| descriptor.ident.clone())
        .collect::<HashSet<_>>();

    let mut search_space
        = Vec::new();
    let mut candidate_requests
        = HashMap::new();

    'request_loop: for (descriptor, request) in &requests {
        let type_ident
            = descriptor.ident.type_ident();

        // We don't want to check for types if the dependency is being explicitly added in the same command
        if ident_set.contains(&type_ident) {
            continue;
        }

        let type_request = AddRequest {
            prod: false,
            peer: request.peer,
            dev: true,
            optional: false,
        };

        for workspace in &project.workspaces {
            if !workspace.manifest.iter_hard_dependencies().any(|dependency| dependency.descriptor.ident == descriptor.ident) {
                continue;
            }

            let matching_type_dependency
                = workspace.manifest.iter_hard_dependencies()
                    .find(|dependency| dependency.descriptor.ident == type_ident);

            let Some(matching_type_dependency) = matching_type_dependency else {
                continue;
            };

            type_requests.push((matching_type_dependency.descriptor.clone(), type_request));
            continue 'request_loop;
        }

        // We only want to check for types if the dependency is a semver range or a tag, since other things may not map to DefinitelyTyped
        let Some(semver_range) = descriptor.range.to_semver_range() else {
            continue;
        };

        let Some(range_min) = semver_range.range_min() else {
            continue;
        };

        let type_descriptor = Descriptor::new(
            descriptor.ident.type_ident(),
            AnonymousSemverRange {
                // TODO: We don't use `caret` here to match the Yarn Berry testsuite; we
                // should clean that up once we can afford to update the tests.
                range: zpm_semver::Range::from_file_string(&format!("^{}", range_min.major)).unwrap(),
            }.into(),
        );

        search_space.push(descriptor.ident.clone());
        candidate_requests.insert(descriptor.ident.clone(), (type_descriptor, type_request));
    }

    let type_idents
        = query_algolia(&search_space, &project.http_client).await?;

    for (ident, _) in type_idents {
        let Some((descriptor, request)) = candidate_requests.remove(&ident) else {
            continue;
        };

        type_requests.push((descriptor, request));
    }

    Ok(type_requests)
}

/// Add new dependencies to the project
///
/// This command adds a package to the package.json for the nearest workspace.
///
/// - If it didn't exist before, the package will by default be added to the regular `dependencies` field, but this behavior can be overriden thanks to the `-D,--dev` flag (which will cause the dependency to be added to the `devDependencies` field instead) and the `-P,--peer` flag (which will do the same but for `peerDependencies`).
///
/// - If the package was already listed in your dependencies, it will by default be upgraded whether it's part of your `dependencies` or `devDependencies` (it won't ever update `peerDependencies`, though).
///
/// - If set, the `--prefer-dev` flag will operate as a more flexible `-D,--dev` in that it will add the package to your `devDependencies` if it isn't already listed in either `dependencies` or `devDependencies`, but it will also happily upgrade your `dependencies` if that's what you already use (whereas `-D,--dev` would throw an exception).
///
/// - If set, the `-O,--optional` flag will add the package to the `optionalDependencies` field and, in combination with the `-P,--peer` flag, it will add the package as an optional peer dependency. If the package was already listed in your `dependencies`, it will be upgraded to `optionalDependencies`. If the package was already listed in your `peerDependencies`, in combination with the `-P,--peer` flag, it will be upgraded to an optional peer dependency: `"peerDependenciesMeta": { "<package>": { "optional": true } }`
///
/// - If the added package doesn't specify a range at all its `latest` tag will be resolved and the returned version will be used to generate a new semver range (using the `^` modifier by default unless otherwise configured via the `defaultSemverRangePrefix` configuration, or the `~` modifier if `-T,--tilde` is specified, or no modifier at all if `-E,--exact` is specified). Two exceptions to this rule: the first one is that if the package is a workspace then its local version will be used, and the second one is that if you use `-P,--peer` the default range will be `*` and won't be resolved at all.
///
/// - If the added package specifies a range (such as `^1.0.0`, `latest`, or `rc`), Yarn will add this range as-is in the resulting package.json entry (in particular, tags such as `rc` will be encoded as-is rather than being converted into a semver range).
///
/// If the `--cached` option is used, Yarn will preferably reuse the highest version already used somewhere within the project, even if through a transitive dependency.
///
/// If the `-i,--interactive` option is used (or if the `preferInteractive` settings is toggled on) the command will first try to check whether other workspaces in the project use the specified package and, if so, will offer to reuse them.
///
/// If the `--mode=<mode>` option is set, Yarn will change which artifacts are generated. The modes currently supported are:
///
/// - `skip-build` will not run the build scripts at all. Note that this is different from setting `enableScripts` to false because the latter will disable build scripts, and thus affect the content of the artifacts generated on disk, whereas the former will just disable the build step - but not the scripts themselves, which just won't run.
///
/// - `update-lockfile` will skip the link step altogether, and only fetch packages that are missing from the lockfile (or that have no associated checksums). This mode is typically used by tools like Renovate or Dependabot to keep a lockfile up-to-date without incurring the full install cost.
///
/// For a compilation of all the supported protocols, please consult the dedicated page from our website: https://yarnpkg.com/protocols.
#[cli::command]
#[cli::path("add")]
#[cli::category("Dependency management")]
pub struct Add {
    /// Store dependency tags as-is instead of resolving them
    #[cli::option("-F,--fixed", default = false)]
    fixed: bool,

    /// Don't use any semver modifier on the resolved range
    #[cli::option("-E,--exact", default = false)]
    exact: bool,

    /// Use the `~` semver modifier on the resolved range
    #[cli::option("-T,--tilde", default = false)]
    tilde: bool,

    /// Use the `^` semver modifier on the resolved range
    #[cli::option("-C,--caret", default = false)]
    caret: bool,

    // ---

    /// Add a package as a peer dependency
    #[cli::option("-P,--peer", default = false)]
    peer: bool,

    /// Add a package as a dev dependency
    #[cli::option("-D,--dev", default = false)]
    dev: bool,

    /// Add / upgrade a package to an optional regular / peer dependency
    #[cli::option("-O,--optional", default = false)]
    optional: bool,

    /// Add / upgrade a package to a dev dependency
    #[cli::option("--prefer-dev", default = false)]
    prefer_dev: bool,

    // ---

    /// Select the artifacts this install will generate
    #[cli::option("--mode")]
    mode: Option<InstallMode>,

    // ---

    /// Packages to add
    descriptors: Vec<LooseDescriptor>,
}

impl Add {
    pub async fn execute(&self) -> Result<(), Error> {
        let project
            = project::Project::new(None).await?;

        let range_kind = if self.fixed {
            RangeKind::Exact
        } else if self.exact {
            RangeKind::Exact
        } else if self.tilde {
            RangeKind::Tilde
        } else if self.caret {
            RangeKind::Caret
        } else {
            project.config.settings.default_semver_range_prefix.value
        };

        let resolve_options = descriptor_loose::ResolveOptions {
            active_workspace_ident: project.active_workspace()?.name.clone(),
            range_kind,
            resolve_tags: !self.fixed,
        };

        let package_cache
            = project.package_cache()?;

        let install_context = InstallContext::default()
            .with_package_cache(Some(&package_cache))
            .with_project(Some(&project));

        let descriptors
            = LooseDescriptor::resolve_all(&install_context, &resolve_options, &self.descriptors).await?;

        let mut requests = vec![];

        let active_workspace
            = project.active_workspace()?;

        for descriptor in descriptors {
            let mut dev = self.dev;
            let mut optional = self.optional;
            let mut prod = false;

            // FUTURE: We probably should use a similar logic as for the dev/optional/prod flags, where
            // we check if the dependency is already listed in the manifest, and if so, we set the flag
            // to true. But since we always set peer dependencies to `*` at the moment, it's not very
            // useful. In the future I'd like to instead set it to the current major/rc.
            let peer = self.peer;

            if !dev && !optional && !peer {
                if active_workspace.manifest.dev_dependencies.contains_key(&descriptor.ident) {
                    dev = true;
                }
                if active_workspace.manifest.remote.optional_dependencies.contains_key(&descriptor.ident) {
                    optional = true;
                }
                if !dev && !optional && !peer {
                    prod = true;
                }
            }

            requests.push((descriptor, AddRequest {
                prod, peer, dev, optional
            }));
        }

        let requests
            = expand_with_types(&install_context, &resolve_options, requests).await?;

        let manifest_path = active_workspace.path
            .with_join_str(project::MANIFEST_NAME);

        let manifest_content = manifest_path
            .fs_read_prealloc()?;

        let mut document
            = JsonDocument::new(manifest_content)?;

        for (descriptor, request) in &requests {
            if request.dev && active_workspace.manifest.remote.dependencies.contains_key(&descriptor.ident) {
                return Err(Error::ConflictingOptions(format!("{} is already listed as a regular dependency of this workspace", descriptor.ident.to_print_string())));
            }

            if request.optional && active_workspace.manifest.remote.dependencies.contains_key(&descriptor.ident) {
                return Err(Error::ConflictingOptions(format!("{} is already listed as an regular dependency of this workspace", descriptor.ident.to_print_string())));
            }

            if request.peer && active_workspace.manifest.remote.dependencies.contains_key(&descriptor.ident) {
                return Err(Error::ConflictingOptions(format!("{} is already listed as a regular dependency of this workspace", descriptor.ident.to_print_string())));
            }

            if request.prod && active_workspace.manifest.remote.peer_dependencies.contains_key(&descriptor.ident) {
                return Err(Error::ConflictingOptions(format!("{} is already listed as a peer dependency of this workspace", descriptor.ident.to_print_string())));
            }

            if request.dev {
                document.set_path(
                    &zpm_parsers::Path::from_segments(vec!["devDependencies".to_string(), descriptor.ident.to_file_string()]),
                    Value::String(descriptor.range.to_file_string()),
                )?;
            }

            if request.optional {
                document.set_path(
                    &zpm_parsers::Path::from_segments(vec!["optionalDependencies".to_string(), descriptor.ident.to_file_string()]),
                    Value::String(descriptor.range.to_file_string()),
                )?;
            }

            if request.peer {
                document.set_path(
                    &zpm_parsers::Path::from_segments(vec!["peerDependencies".to_string(), descriptor.ident.to_file_string()]),
                    Value::String("*".to_string()),
                )?;
            }

            if request.prod {
                document.set_path(
                    &zpm_parsers::Path::from_segments(vec!["dependencies".to_string(), descriptor.ident.to_file_string()]),
                    Value::String(descriptor.range.to_file_string()),
                )?;
            }
        }

        manifest_path
            .fs_change(&document.input, false)?;

        let mut project
            = project::Project::new(None).await?;

        project.run_install(project::RunInstallOptions {
            mode: self.mode,
            ..Default::default()
        }).await?;

        Ok(())
    }
}
