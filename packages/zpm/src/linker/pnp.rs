use std::{collections::{BTreeMap, BTreeSet}, str::FromStr};

use zpm_config::PnpFallbackMode;
use zpm_parsers::JsonDocument;
use zpm_primitives::{Ident, Locator, Reference};
use zpm_utils::{IoResultExt, Path, SyncEntryKind, ToHumanString};
use sha2::{Sha512, Digest};
use hex;
use itertools::Itertools;
use serde::{Serialize, Serializer};
use serde_with::serde_as;
use zpm_utils::ToFileString;

use crate::{
    build,
    error::Error,
    fetchers::{PackageData, PackageLinking},
    install::Install,
    linker::{self, LinkResult},
    misc,
    project::Project,
};

const PNP_CJS_TEMPLATE: &[u8] = std::include_bytes!("pnp-cjs.brotli.dat");
const PNP_MJS_TEMPLATE: &[u8] = std::include_bytes!("pnp-mjs.brotli.dat");

fn make_virtual_path(base: &Path, component: &str, to: &Path) -> Path {
    if base.basename() != Some("__virtual__") {
        panic!("Assertion failed: Virtual folders must be named '__virtual__'");
    }

    let rel_to = to
       .relative_to(base);

    let components = rel_to
        .components()
        .collect::<Vec<_>>();

    let mut components_iter
        = components.iter();

    let depth = components_iter
        .peeking_take_while(|&c| *c == "..")
        .count();

    let final_components = &components[depth..];
    let full_virtual_path = base
        .with_join_str(format!("hash-{component}"))
        .with_join_str(&(depth - 1).to_string())
        .with_join_str(final_components.join("/"));

    full_virtual_path
}

// Helper function to compute SHA512 hash and return as hex string
fn compute_sha512_hex(input: &str) -> String {
    let mut hasher = Sha512::new();
    hasher.update(input.as_bytes());
    hex::encode(hasher.finalize())
}

// Generates a Yarn Berry-compatible hash. Used for Sharp packages
fn yarn_berry_hash(locator: &Locator) -> Result<String, Error> {
    let package_version = locator.reference.to_file_string();

    // Extract scope without '@' prefix, or empty string if no scope
    let package_scope = locator.ident.scope()
        .and_then(|scope| scope.strip_prefix('@'))
        .unwrap_or("");

    // Step 1: Hash the package identifier (scope + name)
    let package_identifier = format!("{}{}", package_scope, locator.ident.name());
    let identifier_hash = compute_sha512_hex(&package_identifier);

    // Step 2: Hash the combination of identifier hash and version
    let combined_input = format!("{}{}", identifier_hash, package_version);
    let final_hash = compute_sha512_hex(&combined_input);

    // Return first 10 characters to match Yarn Berry's hash length
    Ok(final_hash[..10].to_string())
}

#[serde_as]
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct PnpReference(Locator);

impl ToFileString for PnpReference {
    fn to_file_string(&self) -> String {
        let serialized_locator = self.0.reference.to_file_string();

        let mut final_str = String::new();
        final_str.push_str(&serialized_locator);

        if let Some(parent) = &self.0.parent {
            final_str.push_str("::parent=");
            final_str.push_str(&parent.to_file_string());
        }

        final_str
    }
}

impl Serialize for PnpReference {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error> where S: Serializer {
        serializer.serialize_str(&self.to_file_string())
    }
}

#[serde_as]
#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
enum PnpDependencyTarget {
    Simple(PnpReference),
    Alias((Ident, PnpReference)),
    Missing(Option<()>),
}

#[serde_as]
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct PnpPackageInformation {
    package_location: String,
    #[serde_as(as = "Vec<(_, _)>")]
    package_dependencies: BTreeMap<Ident, PnpDependencyTarget>,
    package_peers: Vec<Ident>,
    link_type: PackageLinking,
    discard_from_lookup: bool,
}

#[derive(Debug, Clone, Serialize)]
struct PnpDependencyTreeRoot {
    name: Ident,
    reference: Reference,
}

#[serde_as]
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct PnpState {
    enable_top_level_fallback: bool,
    fallback_pool: Vec<()>,

    #[serde_as(as = "Vec<(_, _)>")]
    fallback_exclusion_list: BTreeMap<Ident, BTreeSet<PnpReference>>,

    ignore_pattern_data: Option<Vec<String>>,

    #[serde_as(as = "Vec<(_, Vec<(_, _)>)>")]
    package_registry_data: BTreeMap<Option<Ident>, BTreeMap<Option<PnpReference>, PnpPackageInformation>>,
    dependency_tree_roots: Vec<PnpDependencyTreeRoot>,
}

/**
 * We use this function rather than JsonDocument::to_string_pretty because we want a single quote string, to
 * avoid having to escape the very common double quote found in the JSON payload.
 */
fn single_quote_stringify(s: &str) -> String {
    let mut escaped
        = String::with_capacity(s.len() * 110 / 100);

    escaped.push('\'');

    for c in s.chars() {
        if matches!(c, '\'' | '\n' | '\\') {
            escaped.push('\\');
        }

        escaped.push(c);
    }

    escaped.push('\'');

    escaped
}

fn generate_inline_files(project: &Project, state: &PnpState) -> Result<(), Error> {
    let script = vec![
        project.config.settings.pnp_shebang.value.as_str(), "\n",
        "/* eslint-disable */\n",
        "// @ts-nocheck\n",
        "\"use strict\";\n",
        "\n",
        "const RAW_RUNTIME_STATE =\n",
        &single_quote_stringify(&JsonDocument::to_string_pretty(&state)?), ";\n",
        "\n",
        "function $$SETUP_STATE(hydrateRuntimeState, basePath) {\n",
        "  return hydrateRuntimeState(JSON.parse(RAW_RUNTIME_STATE), {basePath: basePath || __dirname});\n",
        "}\n",
        &misc::unpack_brotli_data(PNP_CJS_TEMPLATE)?,
    ].join("");

    project.pnp_path()
        .fs_create_parent()?
        .fs_change(script, false)?;

    Ok(())
}

fn generate_split_setup(project: &Project, state: &PnpState) -> Result<(), Error> {
    let script = vec![
        project.config.settings.pnp_shebang.value.as_str(), "\n",
        "/* eslint-disable */\n",
        "// @ts-nocheck\n",
        "\"use strict\";\n",
        "\n",
        "function $$SETUP_STATE(hydrateRuntimeState, basePath) {\n",
        "  const fs = require('fs');\n",
        "  const path = require('path');\n",
        "  const pnpDataFilepath = path.resolve(__dirname, '.pnp.data.json');\n",
        "  return hydrateRuntimeState(JSON.parse(fs.readFileSync(pnpDataFilepath, 'utf8')), {basePath: basePath || __dirname});\n",
        "}\n",
        &misc::unpack_brotli_data(PNP_CJS_TEMPLATE)?,
    ].join("");

    project.pnp_path()
        .fs_create_parent()?
        .fs_change(script, false)?;

    project.pnp_data_path()
        .fs_create_parent()?
        .fs_change(JsonDocument::to_string(&state)?, false)?;

    Ok(())
}

pub async fn link_project_pnp<'a>(project: &'a Project, install: &'a Install) -> Result<LinkResult, Error> {
    let tree
        = &install.install_state.resolution_tree;

    let nm_path = project.project_cwd
        .with_join_str("node_modules");

    let virtual_folder = project.project_cwd
        .with_join(&project.config.settings.virtual_folder.value);

    if project.config.settings.enable_local_cache_cleanup.value {
        linker::helpers::fs_remove_nm(nm_path)?;
    }

    let dependencies_meta
        = linker::helpers::TopLevelConfiguration::from_project(project);

    let mut packages_by_location
        = BTreeMap::new();

    let mut package_registry_data: BTreeMap<_, BTreeMap<_, _>>
        = BTreeMap::new();
    let mut dependency_tree_roots
        = Vec::new();

    let mut all_build_entries
        = Vec::new();
    let mut package_build_entries
        = BTreeMap::new();

    let unplugged_path
        = project.unplugged_path();

    let mut extraneous_unplugged_packages
        = unplugged_path.fs_read_dir()
            .ok_missing()?
            .into_iter()
            .flatten()
            .filter_map(|entry| entry.ok())
            .filter_map(|entry| Path::try_from(entry.path()).ok())
            .collect::<BTreeSet<_>>();

    let mut concrete_unplugged_packages
        = BTreeMap::new();

    for (locator, resolution) in &tree.locator_resolutions {
        let physical_package_data = install.package_data.get(&locator.physical_locator())
            .unwrap_or_else(|| panic!("Failed to find physical package data for {}", locator.physical_locator().to_print_string()));

        let mut package_dependencies: BTreeMap<Ident, PnpDependencyTarget> = resolution.dependencies.iter().map(|(ident, descriptor)| {
            let dependency_resolution = tree.descriptor_to_locator.get(descriptor)
                .unwrap_or_else(|| panic!("Failed to find dependency resolution for {}", descriptor.to_print_string()));

            let dependency_target = if &dependency_resolution.ident == ident {
                PnpDependencyTarget::Simple(PnpReference(dependency_resolution.clone()))
            } else {
                PnpDependencyTarget::Alias((dependency_resolution.ident.clone(), PnpReference(dependency_resolution.clone())))
            };

            (ident.clone(), dependency_target)
        }).collect();

        for peer in &resolution.missing_peer_dependencies {
            package_dependencies.entry(peer.clone())
                .or_insert(PnpDependencyTarget::Missing(None));
        }

        package_dependencies.entry(locator.ident.clone())
            .or_insert(PnpDependencyTarget::Simple(PnpReference(locator.clone())));

        let mut package_peers = resolution.peer_dependencies.keys()
            .cloned()
            .collect::<Vec<_>>();

        package_peers.sort();

        let rel_path = physical_package_data.package_directory()
            .relative_to(physical_package_data.data_root());

        let mut package_location_abs = physical_package_data.data_root()
            .with_join(&rel_path);

        if let Reference::Virtual(params) = &locator.reference {
            package_location_abs = make_virtual_path(
                &virtual_folder,
                &params.hash.to_file_string()[..16],
                &package_location_abs,
            );
        }

        let discard_from_lookup = match physical_package_data {
            PackageData::Local {is_synthetic_package, ..} => *is_synthetic_package,
            _ => false,
        };

        let package_build_info = linker::helpers::get_package_internal_info(
            project,
            install,
            &dependencies_meta,
            &locator,
            &resolution,
            &physical_package_data,
        );

        let mut is_physically_on_disk = false;
        let mut is_freshly_unplugged = false;

        if locator.reference.is_disk_reference() {
            is_physically_on_disk = true;
        } else if package_build_info.must_extract {
            let package_unplugged_wrapper_path = unplugged_path
                .with_join_str(format!("{}-{}-{}", locator.ident.slug(), locator.reference.slug(), yarn_berry_hash(locator)?));

            package_location_abs = package_unplugged_wrapper_path
                .with_join(&physical_package_data.package_subpath());

            if !matches!(physical_package_data, PackageData::MissingZip {..}) {
                extraneous_unplugged_packages.remove(&package_unplugged_wrapper_path);

                concrete_unplugged_packages.insert(
                    locator.clone(),
                    package_location_abs.clone(),
                );

                is_freshly_unplugged = linker::helpers::fs_extract_archive(
                    &package_location_abs,
                    physical_package_data,
                )?;
            }

            is_physically_on_disk = true;
        }

        let package_location_rel = package_location_abs
            .relative_to(&project.project_cwd);

        if !matches!(physical_package_data, PackageData::MissingZip {..}) {
            packages_by_location.insert(
                package_location_rel.clone(),
                locator.clone(),
            );
        }

        let mut package_location = package_location_rel
            .to_file_string();

        if package_location.is_empty() {
            package_location = "./".to_string();
        }

        if !package_location.ends_with('/') {
            package_location.push('/');
        }

        if !package_location.starts_with("./") && !package_location.starts_with("../") {
            package_location.insert_str(0, "./");
        }

        package_registry_data.entry(Some(locator.ident.clone()))
            .or_default()
            .insert(Some(PnpReference(locator.clone())), PnpPackageInformation {
                package_location,
                package_dependencies,
                package_peers,
                link_type: physical_package_data.link_type(),
                discard_from_lookup,
            });

        if let Some(build_commands) = package_build_info.build_commands {
            let build_cwd = match is_physically_on_disk {
                true => {
                    package_location_rel.clone()
                },

                false => {
                    let build_dir_base
                        = Path::temp_dir_pattern("zpm-<>")?;

                    let build_dir
                        = build_dir_base
                            .with_join_str(format!("build/{}", locator.slug()));

                    build_dir
                        .fs_create_dir_all()?;

                    build_dir.relative_to(&project.project_cwd)
                },
            };

            package_build_entries.insert(
                locator.clone(),
                all_build_entries.len(),
            );

            all_build_entries.push(build::BuildRequest {
                cwd: build_cwd,
                locator: locator.clone(),
                commands: build_commands,
                allowed_to_fail: install.install_state.resolution_tree.optional_builds.contains(locator),
                force_rebuild: is_freshly_unplugged,
            });
        }
    }

    for path in extraneous_unplugged_packages {
        path.fs_rm()?;
    }

    // Native dynamic libraries sometimes have runtime dependencies on other dynamic libraries. They
    // address that by using rpath to encode the place where those dependencies can be found. In
    // typical node_modules structures that's either node_modules/<dependency_name>, or one of the
    // parent folders. In PnP installs it's trickier because the unplugged folder is flat, so all
    // entries inside it have arbitrary hashes.
    //
    // To solve this, detect cases where an unplugged package depends on another unplugged package
    // and create a symlink between them in node_modules/<dependency_name>. See `sharp` for an example.

    for (locator, package_location_abs) in &concrete_unplugged_packages {
        let resolution
            = tree.locator_resolutions.get(locator)
                .expect("Failed to find resolution for unplugged package");

        let mut symlinks_to_create
            = BTreeMap::new();

        for descriptor in resolution.dependencies.values() {
            let dependency
                = tree.descriptor_to_locator.get(descriptor)
                    .expect("Failed to find dependency resolution");

            let dependency_locator
                = dependency.physical_locator();

            let Some(dependency_location_abs) = concrete_unplugged_packages.get(&dependency_locator) else {
                continue;
            };

            symlinks_to_create.insert(
                Path::from_str(&dependency.ident.as_str())?,
                SyncEntryKind::Symlink(dependency_location_abs.clone()),
            );
        }

        package_location_abs
            .with_join_str("node_modules")
            .fs_create_dir_all()?
            .fs_sync_dir(symlinks_to_create)
            .map_err(Error::from)?;
    }

    for descriptor in &install.roots {
        let workspace = project
            .try_workspace_by_descriptor(&descriptor)?
            .expect("Install roots are expected to always be workspaces");

        let locator
            = workspace.locator();

        if workspace.path == project.project_cwd {
            let entry = package_registry_data
                .get(&Some(locator.ident.clone()))
                .expect("Failed to find workspace entry")
                .get(&Some(PnpReference(locator.clone())))
                .expect("Failed to find workspace entry")
                .clone();

            package_registry_data
                .entry(None)
                .or_default()
                .entry(None)
                .or_insert(entry);
        }

        dependency_tree_roots.push(PnpDependencyTreeRoot {
            name: locator.ident,
            reference: locator.reference,
        });
    }

    let enable_top_level_fallback
        = project.config.settings.pnp_fallback_mode.value != PnpFallbackMode::None;

    let mut fallback_exclusion_list: BTreeMap<Ident, BTreeSet<PnpReference>>
        = BTreeMap::new();

    let fallback_pool = vec![];

    if project.config.settings.pnp_fallback_mode.value == PnpFallbackMode::DependenciesOnly {
        for locator in tree.locator_resolutions.keys() {
            if let Reference::WorkspaceIdent(_) = locator.physical_locator().reference {
                fallback_exclusion_list.entry(locator.ident.clone())
                    .or_default()
                    .insert(PnpReference(locator.clone()));
            }
        }
    }

    let ignore_pattern_data = project.config.settings.pnp_ignore_patterns
        .iter()
        .map(|pattern| pattern.value.to_regex_string())
        .collect::<Vec<String>>();

    let state = PnpState {
        enable_top_level_fallback,
        fallback_exclusion_list,
        fallback_pool,

        ignore_pattern_data: match ignore_pattern_data.is_empty() {
            true => None,
            false => Some(ignore_pattern_data),
        },

        package_registry_data,
        dependency_tree_roots,
    };

    if project.config.settings.pnp_enable_inlining.value {
        generate_inline_files(project, &state)?;
    } else {
        generate_split_setup(project, &state)?;
    }

    project.pnp_loader_path()
        .fs_change(&misc::unpack_brotli_data(PNP_MJS_TEMPLATE)?, false)?;

    let package_build_dependencies = linker::helpers::populate_build_entry_dependencies(
        &package_build_entries,
        &tree.locator_resolutions,
        &tree.descriptor_to_locator,
    );

    let build_requests = build::BuildRequests {
        entries: all_build_entries,
        dependencies: package_build_dependencies?,
    };

    Ok(LinkResult {
        packages_by_location,
        build_requests,
    })
}
