use std::collections::{HashMap, HashSet};

use clipanion::cli;
use zpm_parsers::{JsonFormatter, JsonValue};
use zpm_semver::RangeKind;
use zpm_utils::{FromFileString, ToFileString, ToHumanString};

use crate::{algolia::query_algolia, error::Error, install::InstallContext, primitives::{loose_descriptor, range::AnonymousSemverRange, Descriptor, LooseDescriptor, Range}, project};

#[derive(Clone, Debug)]
struct AddRequest {
    prod: bool,
    peer: bool,
    dev: bool,
    optional: bool,
}

async fn expand_with_types<'a>(install_context: &InstallContext<'a>, _resolve_options: &loose_descriptor::ResolveOptions, requests: Vec<(Descriptor, AddRequest)>) -> Result<Vec<(Descriptor, AddRequest)>, Error> {
    let project = install_context.project
        .expect("Project not found");

    if !project.config.project.enable_auto_types.value {
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
            if !workspace.manifest.iter_hard_dependencies().any(|(ident, _)| ident == &descriptor.ident) {
                continue;
            }

            let matching_type_dependency
                = workspace.manifest.iter_hard_dependencies()
                    .find(|(ident, _)| ident == &&type_ident);

            if let Some((_, matching_type_dependency)) = matching_type_dependency {
                type_requests.push((matching_type_dependency.clone(), type_request));
                continue 'request_loop;
            }
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
            Range::AnonymousSemver(AnonymousSemverRange {
                // TODO: We don't use `caret` here to match the Yarn Berry testsuite; we
                // should clean that up once we can afford to update the tests.
                range: zpm_semver::Range::from_file_string(&format!("^{}", range_min.major)).unwrap(),
            }),
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

#[cli::command]
#[cli::path("add")]
#[cli::category("Dependency management")]
#[cli::description("Add new dependencies to the project")]
pub struct Add {
    #[cli::option("-F,--fixed", default = false)]
    fixed: bool,

    #[cli::option("-E,--exact", default = false)]
    exact: bool,

    #[cli::option("-T,--tilde", default = false)]
    tilde: bool,

    #[cli::option("-C,--caret", default = false)]
    caret: bool,

    // ---

    #[cli::option("-P,--peer", default = false)]
    peer: bool,

    #[cli::option("-D,--dev", default = false)]
    dev: bool,

    #[cli::option("-O,--optional", default = false)]
    optional: bool,

    #[cli::option("--prefer-dev", default = false)]
    prefer_dev: bool,

    // ---

    descriptors: Vec<LooseDescriptor>,
}

impl Add {
    #[tokio::main()]
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
            project.config.project.default_semver_range_prefix.value
        };

        let resolve_options = loose_descriptor::ResolveOptions {
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
            .fs_read_text_prealloc()?;

        let mut formatter
            = JsonFormatter::from(&manifest_content)?;

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
                formatter.set(
                    vec!["devDependencies".to_string(), descriptor.ident.to_file_string()], 
                    JsonValue::String(descriptor.range.to_file_string()),
                )?;
            }

            if request.optional {
                formatter.set(
                    vec!["optionalDependencies".to_string(), descriptor.ident.to_file_string()], 
                    JsonValue::String(descriptor.range.to_file_string()),
                )?;
            }

            if request.peer {
                formatter.set(
                    vec!["peerDependencies".to_string(), descriptor.ident.to_file_string()], 
                    JsonValue::String("*".to_string()),
                )?;
            }

            if request.prod {
                formatter.set(
                    vec!["dependencies".to_string(), descriptor.ident.to_file_string()], 
                    JsonValue::String(descriptor.range.to_file_string()),
                )?;
            }
        }    

        let updated_content
            = formatter.to_string();

        manifest_path
            .fs_change(&updated_content, false)?;

        let mut project
            = project::Project::new(None).await?;

        project.run_install(project::RunInstallOptions {
            ..Default::default()
        }).await?;

        Ok(())
    }
}
