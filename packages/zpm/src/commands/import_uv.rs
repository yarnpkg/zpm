//! `yarn import uv`: turns uv projects into Yarn workspaces.

use std::collections::{BTreeMap, BTreeSet};

use clipanion::cli;
use serde_json::{Map, Value, json};
use zpm_primitives::canonicalize_pypi_name;
use zpm_utils::{FromFileString, Path, ToFileString};

use crate::{
    error::Error,
    project::Project,
};

/// Import uv projects as Yarn workspaces
///
/// For each folder containing a `pyproject.toml`, this command writes a
/// `package.json` next to it describing the same dependencies, registers
/// the folder as a workspace of the current project, and adds a venv island
/// for it in `.yarnrc.yml`. The conversion covers:
///
/// - `project.dependencies` → `dependencies` (`pypi:` ranges, extras and
///   markers preserved);
/// - the `dev` dependency group (and legacy `tool.uv.dev-dependencies`) →
///   `devDependencies`; other groups are reported and skipped unless listed
///   with `--group`;
/// - `tool.uv.sources` path / workspace entries → `workspace:^` (the target
///   must be imported too, which happens automatically with `--recursive`);
///   index entries → a `packageRules` entry routing the package to the index;
/// - `tool.uv.override-dependencies` → the workspace's `resolutions`;
/// - `tool.uv.constraint-dependencies` → the island's `pypiConstraints`;
/// - `requires-python` → the island's `pythonVersion` (lowest allowed minor);
/// - `uv.lock` → the island's `pypiSeedLockfile`, so the first install keeps
///   the versions uv had locked;
/// - uv workspace members → workspaces sharing their root's island (uv
///   resolves a workspace as a whole).
///
/// Run with `--dry-run` to print the generated files without writing them.
#[cli::command]
#[cli::path("import", "uv")]
#[cli::category("Project management")]
pub struct ImportUv {
    /// Also import the projects referenced through path sources and uv workspace members
    #[cli::option("-r,--recursive", default = false)]
    recursive: bool,

    /// Print the files that would be written, without writing them
    #[cli::option("--dry-run", default = false)]
    dry_run: bool,

    /// Dependency groups (besides `dev`) to import as devDependencies
    #[cli::option("--group", default = vec![])]
    groups: Vec<String>,

    /// Extras of the imported projects to install in their own venvs, as
    /// `uv sync --extra` does (their requirements become devDependencies)
    #[cli::option("--extra", default = vec![])]
    extras: Vec<String>,

    /// Install all the extras of the imported projects in their own venvs,
    /// as `uv sync --all-extras` does
    #[cli::option("--all-extras", default = false)]
    all_extras: bool,

    /// Folders containing a pyproject.toml
    folders: Vec<Path>,
}

struct ImportedProject {
    folder: Path,
    rel_folder: String,
    name: String,
    manifest: Map<String, Value>,
    island: Map<String, Value>,
    package_rules: Vec<Value>,
    warnings: Vec<String>,
    linked_folders: Vec<Path>,
    /// Members of the uv workspace this project is the root of; they share
    /// its island, as uv resolves them together (single uv.lock)
    workspace_members: Vec<Path>,
}

fn pyproject_name(folder: &Path) -> Option<String> {
    let text
        = folder.with_join_str("pyproject.toml").fs_read_text().ok()?;

    let document
        = text.parse::<toml_edit::DocumentMut>().ok()?;

    document.get("project")?.get("name")?.as_str().map(canonicalize_pypi_name)
}

fn str_array(item: Option<&toml_edit::Item>) -> Vec<String> {
    item.and_then(|item| item.as_array())
        .map(|array| array.iter().filter_map(|value| value.as_str().map(|value| value.to_string())).collect())
        .unwrap_or_default()
}

/// Converts a PEP 508 requirement into a (name, `pypi:` range) pair.
fn requirement_to_range(requirement: &str) -> Option<(String, String)> {
    let parsed
        = pep_508::parse(requirement).ok()?;

    let name
        = canonicalize_pypi_name(parsed.name);

    let specifier = match &parsed.spec {
        None => "*".to_string(),
        Some(pep_508::Spec::Url(_)) => return None,
        Some(pep_508::Spec::Version(specifiers)) => specifiers.iter()
            .map(|specifier| {
                let op = match specifier.comparator {
                    pep_508::Comparator::Lt => "<",
                    pep_508::Comparator::Le => "<=",
                    pep_508::Comparator::Ne => "!=",
                    pep_508::Comparator::Eq => "==",
                    pep_508::Comparator::Ge => ">=",
                    pep_508::Comparator::Gt => ">",
                    pep_508::Comparator::Cp => "~=",
                    pep_508::Comparator::Ae => "===",
                };

                format!("{}{}", op, specifier.version)
            })
            .collect::<Vec<_>>()
            .join(","),
    };

    let mut parameters
        = Vec::new();

    if !parsed.extras.is_empty() {
        let extras
            = parsed.extras.iter().map(|extra| zpm_primitives::normalize_pypi_extra(extra)).collect::<Vec<_>>().join(",");

        parameters.push(format!("extras={}", extras));
    }

    if let Some(marker) = crate::resolvers::pypi::marker_of(requirement) {
        parameters.push(format!("marker={}", zpm_utils::QueryString::encode(&marker)));
    }

    let range = match parameters.is_empty() {
        true => format!("pypi:{}", specifier),
        false => format!("pypi:{}#{}", specifier, parameters.join("&")),
    };

    Some((name, range))
}

/// The lowest Python minor version allowed by `requires-python` among the
/// usual candidates (3.9 → 3.14), preferring 3.12 when it's allowed since
/// it's what most of the projects use.
fn python_version_for(requires_python: Option<&str>) -> Option<String> {
    let specifiers
        = pep440_rs::VersionSpecifiers::from_str_lossy(requires_python?)?;

    let candidates
        = ["3.12", "3.13", "3.11", "3.14", "3.10", "3.9"];

    // A minor is compatible when one of its releases is (`>=3.12.1` still
    // allows Python 3.12, from its 3.12.1 patch release)
    candidates.iter()
        .find(|candidate| {
            (0..=50).any(|patch| {
                let version = pep440_rs::Version::from_str_lossy(&format!("{}.{}", candidate, patch));
                version.map_or(false, |version| specifiers.contains(&version))
            })
        })
        .map(|candidate| candidate.to_string())
}

/// The requirements brought by the extras requested on a local project
/// (`mylib[server]`), read from its pyproject.toml - including
/// self-referencing extras (`mylib[all]` → other mylib extras).
fn local_extra_requirements(target: &Path, requirement: &str) -> Vec<(String, String)> {
    let Ok(parsed) = pep_508::parse(requirement) else {
        return vec![];
    };

    let Ok(text) = target.with_join_str("pyproject.toml").fs_read_text() else {
        return vec![];
    };

    let Ok(document) = text.parse::<toml_edit::DocumentMut>() else {
        return vec![];
    };

    let Some(project_table) = document.get("project") else {
        return vec![];
    };

    let target_name
        = project_table.get("name").and_then(|name| name.as_str()).map(canonicalize_pypi_name).unwrap_or_default();

    let Some(optional) = project_table.get("optional-dependencies").and_then(|value| value.as_table_like()) else {
        return vec![];
    };

    let mut queue
        = parsed.extras.iter().map(|extra| zpm_primitives::normalize_pypi_extra(extra)).collect::<Vec<_>>();

    let mut seen
        = BTreeSet::new();

    let mut result
        = Vec::new();

    while let Some(extra) = queue.pop() {
        if !seen.insert(extra.clone()) {
            continue;
        }

        let Some((_, entries)) = optional.iter().find(|(key, _)| zpm_primitives::normalize_pypi_extra(key) == extra) else {
            continue;
        };

        for entry in str_array(Some(entries)) {
            let Ok(entry_parsed) = pep_508::parse(&entry) else {
                continue;
            };

            if canonicalize_pypi_name(entry_parsed.name) == target_name {
                queue.extend(entry_parsed.extras.iter().map(|extra| zpm_primitives::normalize_pypi_extra(extra)));
                continue;
            }

            if let Some((entry_name, entry_range)) = requirement_to_range(&entry) {
                // Requirements the local project gets from its own path
                // sources are workspaces too (an extra requiring a project that
                // lives in a subfolder)
                let entry_range = match local_path_source(&document, target, &entry_name) {
                    Some(_) => "workspace:^".to_string(),
                    None => entry_range,
                };

                result.push((entry_name, entry_range));
            }
        }
    }

    result
}

/// The folder a project maps a dependency to through a uv path source.
fn local_path_source(document: &toml_edit::DocumentMut, project_folder: &Path, dep_name: &str) -> Option<Path> {
    let sources
        = document.get("tool")?.get("uv")?.get("sources")?.as_table_like()?;

    let (_, source)
        = sources.iter().find(|(key, _)| canonicalize_pypi_name(key) == dep_name)?;

    let path
        = source.get("path")?.as_str()?;

    Some(project_folder.with_join_str(path))
}

/// Merges two `pypi:` ranges for the same package (specifiers intersected,
/// extras unioned); returns `None` when they can't be merged.
fn merge_ranges(a: &str, b: &str) -> Option<String> {
    let split = |range: &str| -> Option<(String, BTreeSet<String>, Option<String>)> {
        let body = range.strip_prefix("pypi:")?;
        let (specifier, parameters) = body.split_once('#').unwrap_or((body, ""));

        let mut extras = BTreeSet::new();
        let mut marker = None;

        for parameter in parameters.split('&').filter(|parameter| !parameter.is_empty()) {
            match parameter.split_once('=') {
                Some(("extras", value)) => extras.extend(value.split(',').map(|extra| extra.to_string())),
                Some(("marker", value)) => marker = Some(value.to_string()),
                _ => return None,
            }
        }

        Some((specifier.to_string(), extras, marker))
    };

    let (spec_a, mut extras, marker_a) = split(a)?;
    let (spec_b, extras_b, marker_b) = split(b)?;

    // Conditional entries for the same package would need marker logic;
    // keep it simple and only merge unconditional ones
    if marker_a.is_some() || marker_b.is_some() {
        return None;
    }

    extras.extend(extras_b);

    let specifier = match (spec_a.as_str(), spec_b.as_str()) {
        ("*", other) | (other, "*") => other.to_string(),
        (a, b) => format!("{},{}", a, b),
    };

    Some(match extras.is_empty() {
        true => format!("pypi:{}", specifier),
        false => format!("pypi:{}#extras={}", specifier, extras.into_iter().collect::<Vec<_>>().join(",")),
    })
}

trait FromStrLossy: Sized {
    fn from_str_lossy(value: &str) -> Option<Self>;
}

impl FromStrLossy for pep440_rs::VersionSpecifiers {
    fn from_str_lossy(value: &str) -> Option<Self> {
        <Self as std::str::FromStr>::from_str(value).ok()
    }
}

impl FromStrLossy for pep440_rs::Version {
    fn from_str_lossy(value: &str) -> Option<Self> {
        <Self as std::str::FromStr>::from_str(value).ok()
    }
}

/// Members of a uv workspace inherit the `[tool.uv.sources]` of the
/// workspace root (their own entries win). Path sources are rewritten
/// relative to the member, as the rest of the import resolves them from it.
fn inherit_workspace_sources(document: &mut toml_edit::DocumentMut, folder: &Path) {
    let Some(root) = find_uv_workspace_root(folder) else {
        return;
    };

    let Ok(root_text) = root.with_join_str("pyproject.toml").fs_read_text() else {
        return;
    };

    let Ok(root_document) = root_text.parse::<toml_edit::DocumentMut>() else {
        return;
    };

    let Some(root_sources) = root_document.get("tool").and_then(|tool| tool.get("uv")).and_then(|uv| uv.get("sources")).and_then(|sources| sources.as_table_like()) else {
        return;
    };

    let member_tool = document.entry("tool").or_insert(toml_edit::table()).as_table_like_mut();
    let Some(member_tool) = member_tool else { return; };
    let member_uv = member_tool.entry("uv").or_insert(toml_edit::table()).as_table_like_mut();
    let Some(member_uv) = member_uv else { return; };
    let member_sources = member_uv.entry("sources").or_insert(toml_edit::table()).as_table_like_mut();
    let Some(member_sources) = member_sources else { return; };

    for (key, source) in root_sources.iter() {
        if member_sources.contains_key(key) {
            continue;
        }

        let mut source = source.clone();

        let path = source.get("path").and_then(|path| path.as_str()).map(|path| path.to_string());

        if let Some(path) = path {
            let absolute = root.with_join_str(&path);
            let relative = absolute.relative_to(folder).to_file_string();

            if let Some(table) = source.as_inline_table_mut() {
                table.insert("path", relative.into());
            } else if let Some(table) = source.as_table_like_mut() {
                table.insert("path", toml_edit::value(relative));
            }
        }

        member_sources.insert(key, source);
    }
}

/// The closest parent folder holding a pyproject.toml whose
/// `[tool.uv.workspace]` lists `folder` as a member.
fn find_uv_workspace_root(folder: &Path) -> Option<Path> {
    let mut current = folder.dirname()?;

    loop {
        if let Ok(text) = current.with_join_str("pyproject.toml").fs_read_text() {
            if let Ok(document) = text.parse::<toml_edit::DocumentMut>() {
                let members = document.get("tool").and_then(|tool| tool.get("uv")).and_then(|uv| uv.get("workspace")).and_then(|workspace| workspace.get("members"));

                if let Some(members) = members.and_then(|members| members.as_array()) {
                    let canonical_folder = folder.fs_canonicalize().unwrap_or(folder.clone());

                    let is_member = members.iter()
                        .filter_map(|member| member.as_str())
                        .flat_map(|member| glob_folders(&current.with_join_str(member).to_file_string()))
                        .any(|candidate| candidate.fs_canonicalize().unwrap_or(candidate) == canonical_folder);

                    if is_member {
                        return Some(current);
                    }
                }
            }
        }

        let parent = current.dirname()?;
        if parent == current {
            return None;
        }

        current = parent;
    }
}

/// Which extras of the imported project its own venv installs
pub enum OwnExtras<'a> {
    Some(&'a [String]),
    All,
}

fn import_project(project: &Project, folder: &Path, groups: &[String], own_extras: OwnExtras<'_>) -> Result<ImportedProject, Error> {
    let pyproject_path
        = folder.with_join_str("pyproject.toml");

    let text
        = pyproject_path.fs_read_text()
            .map_err(|_| Error::InvalidResolution(format!("{} has no pyproject.toml", folder.to_file_string())))?;

    let mut document
        = text.parse::<toml_edit::DocumentMut>()
            .map_err(|err| Error::InvalidResolution(format!("Invalid {}: {}", pyproject_path.to_file_string(), err)))?;

    inherit_workspace_sources(&mut document, folder);

    let rel_folder
        = folder.relative_to(&project.project_cwd).to_file_string();

    let project_table
        = document.get("project");

    let uv
        = document.get("tool").and_then(|tool| tool.get("uv"));

    let mut warnings
        = Vec::new();

    let name = match project_table.and_then(|table| table.get("name")).and_then(|name| name.as_str()) {
        Some(name) => canonicalize_pypi_name(name),
        None => {
            // Virtual uv workspace roots don't have a [project] table
            let fallback = format!("{}-root", folder.basename().unwrap_or("python"));
            warnings.push(format!("no [project] table; using {} as workspace name", fallback));
            fallback
        },
    };

    let sources
        = uv.and_then(|uv| uv.get("sources")).and_then(|sources| sources.as_table_like());

    let mut linked_folders
        = Vec::new();

    let mut package_rules
        = Vec::new();

    let indexes
        = uv.and_then(|uv| uv.get("index")).and_then(|index| index.as_array_of_tables())
            .map(|tables| tables.iter().filter_map(|table| Some((table.get("name")?.as_str()?.to_string(), table.get("url")?.as_str()?.to_string()))).collect::<BTreeMap<_, _>>())
            .unwrap_or_default();

    // Resolves a dependency name to its source override, if any
    let source_range = |dep_name: &str, linked_folders: &mut Vec<Path>, package_rules: &mut Vec<Value>, warnings: &mut Vec<String>| -> Option<String> {
        let (_, source) = sources?.iter().find(|(key, _)| canonicalize_pypi_name(key) == dep_name)?;
        let source = source.as_inline_table().map(|table| table.clone().into_table())
            .or_else(|| source.as_table().cloned())?;

        if let Some(path) = source.get("path").and_then(|path| path.as_str()) {
            let target = folder.with_join_str(path);

            if target.to_file_string().ends_with(".whl") || target.to_file_string().ends_with(".tar.gz") {
                warnings.push(format!("{}: local archive sources aren't supported", dep_name));
                return None;
            }

            linked_folders.push(target.clone());

            if pyproject_name(&target).as_deref() != Some(dep_name) {
                warnings.push(format!("{}: the path source points to a project with another name", dep_name));
            }

            return Some("workspace:^".to_string());
        }

        if source.get("workspace").and_then(|value| value.as_bool()) == Some(true) {
            return Some("workspace:^".to_string());
        }

        if let Some(index) = source.get("index").and_then(|value| value.as_str()) {
            let url = indexes.get(index).cloned().unwrap_or_else(|| index.to_string());

            package_rules.push(json!({
                "ecosystemFilter": "pypi",
                "packageFilter": dep_name,
                "pypiRegistryServer": url,
            }));

            warnings.push(format!("{}: routed to {}; add pypiAuthToken / pypiAuthIdent to the generated packageRules entry (or a sourceRules entry for the index) if it requires credentials", dep_name, url));

            return None;
        }

        if source.get("git").is_some() || source.get("url").is_some() {
            warnings.push(format!("{}: git and url sources aren't supported yet; it will be resolved from the index", dep_name));
        }

        None
    };

    let convert = |requirements: Vec<String>, linked_folders: &mut Vec<Path>, package_rules: &mut Vec<Value>, warnings: &mut Vec<String>| -> Map<String, Value> {
        let mut result
            = Map::new();

        for requirement in requirements {
            let Some((dep_name, mut range)) = requirement_to_range(&requirement) else {
                warnings.push(format!("unsupported requirement: {}", requirement));
                continue;
            };

            // Self-references with extras were expanded beforehand
            if dep_name == name {
                continue;
            }

            if let Some(source) = source_range(&dep_name, linked_folders, package_rules, warnings) {
                // Extras of local projects: their requirements are added to
                // the dependent directly, as workspaces can't be required
                // with extras
                if let Some(target) = linked_folders.last().cloned() {
                    for (extra_name, extra_range) in local_extra_requirements(&target, &requirement) {
                        if !result.contains_key(&extra_name) && extra_name != name {
                            result.insert(extra_name, Value::String(extra_range));
                        }
                    }
                }

                range = source;
            }

            if let Some(Value::String(existing)) = result.get(&dep_name) {
                if existing != &range {
                    match merge_ranges(existing, &range) {
                        Some(merged) => range = merged,
                        None => {
                            warnings.push(format!("{} is listed multiple times ({} and {}); keeping the first", dep_name, existing, range));
                            continue;
                        },
                    }
                }
            }

            result.insert(dep_name, Value::String(range));
        }

        result
    };

    let optional_dependencies
        = project_table.and_then(|table| table.get("optional-dependencies")).and_then(|value| value.as_table_like());

    // `name[extra]` requirements referencing the project itself (typically
    // in its dev group) are replaced by the requirements of those extras
    let expand_self = |requirements: Vec<String>| -> Vec<String> {
        let mut expanded
            = Vec::new();

        let mut queue
            = requirements;

        let mut seen_extras
            = BTreeSet::new();

        while let Some(requirement) = queue.pop() {
            let Ok(parsed) = pep_508::parse(&requirement) else {
                expanded.push(requirement);
                continue;
            };

            if canonicalize_pypi_name(parsed.name) != name {
                expanded.push(requirement);
                continue;
            }

            for extra in parsed.extras {
                let extra = zpm_primitives::normalize_pypi_extra(extra);

                if !seen_extras.insert(extra.clone()) {
                    continue;
                }

                let entries = optional_dependencies.and_then(|table| {
                    table.iter().find(|(key, _)| zpm_primitives::normalize_pypi_extra(key) == extra).map(|(_, value)| str_array(Some(value)))
                });

                queue.extend(entries.unwrap_or_default());
            }
        }

        expanded.reverse();
        expanded
    };

    let dependencies
        = convert(expand_self(str_array(project_table.and_then(|table| table.get("dependencies")))), &mut linked_folders, &mut package_rules, &mut warnings);

    let dependency_groups
        = document.get("dependency-groups").and_then(|groups| groups.as_table_like());

    let mut dev_requirements
        = str_array(uv.and_then(|uv| uv.get("dev-dependencies")));

    let mut wanted_groups
        = BTreeSet::from(["dev".to_string()]);

    wanted_groups.extend(groups.iter().cloned());

    if let Some(dependency_groups) = dependency_groups {
        for (group, entries) in dependency_groups.iter() {
            if !wanted_groups.contains(group) {
                warnings.push(format!("dependency group '{}' skipped (use --group {} to import it)", group, group));
                continue;
            }

            for entry in entries.as_array().into_iter().flatten() {
                if let Some(requirement) = entry.as_str() {
                    dev_requirements.push(requirement.to_string());
                } else if let Some(include) = entry.as_inline_table().and_then(|table| table.get("include-group")).and_then(|value| value.as_str()) {
                    dev_requirements.extend(str_array(dependency_groups.get(include)));
                }
            }
        }
    }

    // The project's own extras requested on the command line are installed
    // in its venv; a self-reference expands them like those of the dev group
    let own_extra_names = match own_extras {
        OwnExtras::All => optional_dependencies.map(|table| table.iter().map(|(key, _)| key.to_string()).collect::<Vec<_>>()).unwrap_or_default(),
        OwnExtras::Some(extras) => extras.to_vec(),
    };

    if !own_extra_names.is_empty() {
        dev_requirements.push(format!("{}[{}]", name, own_extra_names.join(",")));
    }

    let dev_dependencies
        = convert(expand_self(dev_requirements), &mut linked_folders, &mut package_rules, &mut warnings);

    if let Some(optional_dependencies) = optional_dependencies {
        let extras
            = optional_dependencies.iter().map(|(key, _)| key.to_string()).collect::<Vec<_>>();

        if !extras.is_empty() {
            warnings.push(format!("optional dependencies ({}) are only installed when requested by a dependent; they're read from pyproject.toml", extras.join(", ")));
        }
    }

    let mut manifest
        = Map::new();

    manifest.insert("name".to_string(), Value::String(name.clone()));
    manifest.insert("private".to_string(), Value::Bool(true));

    if !dependencies.is_empty() {
        manifest.insert("dependencies".to_string(), Value::Object(dependencies));
    }

    if !dev_dependencies.is_empty() {
        manifest.insert("devDependencies".to_string(), Value::Object(dev_dependencies));
    }

    let overrides
        = str_array(uv.and_then(|uv| uv.get("override-dependencies")));

    if !overrides.is_empty() {
        let mut resolutions
            = Map::new();

        for requirement in overrides {
            match requirement_to_range(&requirement) {
                Some((dep_name, range)) => {
                    resolutions.insert(dep_name, Value::String(range));
                },

                None => {
                    warnings.push(format!("unsupported override: {}", requirement));
                },
            }
        }

        manifest.insert("resolutions".to_string(), Value::Object(resolutions));
    }

    // `exclude-newer-package = { pkg = false }` disables the age gate for pkg
    let exclude_newer_package
        = uv.and_then(|uv| uv.get("exclude-newer-package")).and_then(|value| value.as_table_like());

    if let Some(exclude_newer_package) = exclude_newer_package {
        for (package, value) in exclude_newer_package.iter() {
            if value.as_bool() == Some(false) || value.as_value().and_then(|value| value.as_bool()) == Some(false) {
                package_rules.push(json!({
                    "ecosystemFilter": "pypi",
                    "packageFilter": canonicalize_pypi_name(package),
                    "pypiMinimalAgeGate": "0s",
                }));
            } else {
                warnings.push(format!("exclude-newer-package for {} isn't supported (only `false`)", package));
            }
        }
    }

    let mut island
        = Map::new();

    island.insert("workspaces".to_string(), json!([name]));
    island.insert("linker".to_string(), Value::String("venv".to_string()));

    let requires_python
        = project_table.and_then(|table| table.get("requires-python")).and_then(|value| value.as_str());

    if let Some(python_version) = python_version_for(requires_python) {
        if python_version != project.config.settings.python_version.value {
            island.insert("pythonVersion".to_string(), Value::String(python_version));
        }
    }

    let constraints
        = str_array(uv.and_then(|uv| uv.get("constraint-dependencies")));

    if !constraints.is_empty() {
        island.insert("pypiConstraints".to_string(), json!(constraints));
    }

    if folder.with_join_str("uv.lock").fs_exists() {
        island.insert("pypiSeedLockfile".to_string(), Value::String(format!("{}/uv.lock", rel_folder)));
    }

    // uv workspace members are imported alongside their root
    let members
        = str_array(uv.and_then(|uv| uv.get("workspace")).and_then(|workspace| workspace.get("members")));

    let mut workspace_members
        = Vec::new();

    for member in members {
        let pattern
            = folder.with_join_str(&member).to_file_string();

        for entry in glob_folders(&pattern) {
            if entry.with_join_str("pyproject.toml").fs_exists() {
                workspace_members.push(entry.fs_canonicalize().unwrap_or(entry.clone()));
                linked_folders.push(entry);
            }
        }
    }

    Ok(ImportedProject {
        folder: folder.clone(),
        rel_folder,
        name,
        manifest,
        island,
        package_rules,
        warnings,
        linked_folders,
        workspace_members,
    })
}

/// Expands a trailing `*` in a folder pattern (the only form uv workspaces
/// use in practice).
fn glob_folders(pattern: &str) -> Vec<Path> {
    let Some(prefix) = pattern.strip_suffix("/*") else {
        return Path::from_file_string(pattern).ok().into_iter().collect();
    };

    let Ok(base) = Path::from_file_string(prefix) else {
        return vec![];
    };

    base.fs_read_dir().map(|entries| {
        entries.flatten()
            .filter(|entry| entry.path().is_dir())
            .filter_map(|entry| Path::try_from(entry.path()).ok())
            .collect()
    }).unwrap_or_default()
}

impl ImportUv {
    pub async fn execute(&self) -> Result<(), Error> {
        let project
            = Project::new(None).await?;

        let cwd
            = Path::current_dir()?;

        let mut queue
            = self.folders.iter()
                .map(|folder| cwd.with_join(folder))
                .collect::<Vec<_>>();

        let mut seen
            = BTreeSet::new();

        let mut imported
            = Vec::new();

        while let Some(folder) = queue.pop() {
            let folder
                = folder.fs_canonicalize().unwrap_or(folder);

            if !seen.insert(folder.clone()) {
                continue;
            }

            let result
                = import_project(&project, &folder, &self.groups, match self.all_extras {
                    true => OwnExtras::All,
                    false => OwnExtras::Some(&self.extras),
                })?;

            if self.recursive {
                queue.extend(result.linked_folders.iter().cloned());
            } else {
                for linked in &result.linked_folders {
                    if !self.folders.iter().any(|folder| cwd.with_join(folder).fs_canonicalize().ok().as_ref() == Some(linked)) && !linked.with_join_str("package.json").fs_exists() {
                        println!("{}: depends on {} which isn't imported (use --recursive)", result.rel_folder, linked.relative_to(&project.project_cwd).to_file_string());
                    }
                }
            }

            imported.push(result);
        }

        imported.sort_by(|a, b| a.rel_folder.cmp(&b.rel_folder));

        // Root manifest: register the workspaces
        let root_manifest_path
            = project.project_cwd.with_join_str("package.json");

        let mut root_manifest: Map<String, Value>
            = serde_json::from_str(&root_manifest_path.fs_read_text()?)
                .map_err(|err| Error::InvalidResolution(format!("Invalid root package.json: {}", err)))?;

        let workspaces
            = root_manifest.entry("workspaces".to_string())
                .or_insert_with(|| json!([]));

        let workspace_patterns = match workspaces {
            Value::Array(patterns) => patterns,
            Value::Object(object) => match object.entry("packages".to_string()).or_insert_with(|| json!([])) {
                Value::Array(patterns) => patterns,
                _ => return Err(Error::InvalidResolution("Unsupported workspaces field".to_string())),
            },
            _ => return Err(Error::InvalidResolution("Unsupported workspaces field".to_string())),
        };

        for project_import in &imported {
            if project_import.folder == project.project_cwd {
                continue;
            }

            let already_listed
                = workspace_patterns.iter().any(|pattern| pattern.as_str() == Some(project_import.rel_folder.as_str()));

            let already_matched
                = project.workspaces.iter().any(|workspace| workspace.path == project_import.folder);

            if !already_listed && !already_matched {
                workspace_patterns.push(Value::String(project_import.rel_folder.clone()));
            }
        }

        // Configuration: islands and package rules
        let rc_path
            = project.project_cwd.with_join_str(".yarnrc.yml");

        let rc_text
            = rc_path.fs_read_text().unwrap_or_default();

        let mut rc: serde_yaml::Mapping
            = serde_yaml::from_str(&rc_text).ok().flatten().unwrap_or_default();

        let islands
            = rc.entry(serde_yaml::Value::String("unstableIslands".to_string()))
                .or_insert_with(|| serde_yaml::Value::Mapping(Default::default()));

        // uv workspace members join their root's island
        let mut member_of
            = BTreeMap::<Path, String>::new();

        for project_import in &imported {
            for member in &project_import.workspace_members {
                member_of.insert(member.clone(), project_import.name.clone());
            }
        }

        let mut island_maps
            = BTreeMap::<String, Map<String, Value>>::new();

        for project_import in &imported {
            if member_of.contains_key(&project_import.folder) {
                continue;
            }

            island_maps.insert(project_import.name.clone(), project_import.island.clone());
        }

        for project_import in &imported {
            let Some(root_name) = member_of.get(&project_import.folder) else {
                continue;
            };

            let Some(root_island) = island_maps.get_mut(root_name) else {
                continue;
            };

            if let Some(Value::Array(workspaces)) = root_island.get_mut("workspaces") {
                workspaces.push(Value::String(project_import.name.clone()));
            }

            // Members' constraints apply to the whole uv workspace
            if let Some(Value::Array(constraints)) = project_import.island.get("pypiConstraints") {
                let entry = root_island.entry("pypiConstraints".to_string()).or_insert_with(|| json!([]));

                if let Value::Array(existing) = entry {
                    for constraint in constraints {
                        if !existing.contains(constraint) {
                            existing.push(constraint.clone());
                        }
                    }
                }
            }
        }

        for (name, island) in island_maps {
            let island_value: serde_yaml::Value
                = serde_yaml::to_value(&island)
                    .map_err(|err| Error::InvalidResolution(err.to_string()))?;

            if let serde_yaml::Value::Mapping(islands) = islands {
                islands.insert(serde_yaml::Value::String(name), island_value);
            }
        }

        let mut new_rules
            = imported.iter()
                .flat_map(|project_import| project_import.package_rules.iter().cloned())
                .collect::<Vec<_>>();

        new_rules.sort_by_key(|rule| rule.to_string());
        new_rules.dedup();

        if !new_rules.is_empty() {
            let rules
                = rc.entry(serde_yaml::Value::String("packageRules".to_string()))
                    .or_insert_with(|| serde_yaml::Value::Sequence(Default::default()));

            if let serde_yaml::Value::Sequence(rules) = rules {
                for rule in new_rules {
                    let rule: serde_yaml::Value
                        = serde_yaml::to_value(&rule).map_err(|err| Error::InvalidResolution(err.to_string()))?;

                    if !rules.contains(&rule) {
                        rules.push(rule);
                    }
                }
            }
        }

        let rc_output
            = serde_yaml::to_string(&rc).map_err(|err| Error::InvalidResolution(err.to_string()))?;

        let root_output
            = format!("{}\n", serde_json::to_string_pretty(&root_manifest).unwrap());

        for project_import in &imported {
            let manifest_path
                = project_import.folder.with_join_str("package.json");

            let output
                = format!("{}\n", serde_json::to_string_pretty(&project_import.manifest).unwrap());

            for warning in &project_import.warnings {
                println!("{}: {}", project_import.rel_folder, warning);
            }

            if self.dry_run {
                println!("--- {}\n{}", manifest_path.relative_to(&project.project_cwd).to_file_string(), output);
            } else {
                if manifest_path.fs_exists() && project_import.folder != project.project_cwd {
                    println!("{}: overwriting the existing package.json", project_import.rel_folder);
                }

                manifest_path.fs_write(output)?;
            }
        }

        if self.dry_run {
            println!("--- package.json\n{}", root_output);
            println!("--- .yarnrc.yml\n{}", rc_output);
            return Ok(());
        }

        root_manifest_path.fs_write(root_output)?;
        rc_path.fs_write(rc_output)?;

        println!("Imported {} project(s); run `yarn install` to create their venvs", imported.len());

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn python_version_accepts_patch_level_bounds() {
        assert_eq!(python_version_for(Some(">=3.12.1")), Some("3.12".to_string()));
        assert_eq!(python_version_for(Some(">=3.10")), Some("3.12".to_string()));
        assert_eq!(python_version_for(Some(">=3.13.2,<3.14")), Some("3.13".to_string()));
        assert_eq!(python_version_for(Some("<3.10")), Some("3.9".to_string()));
    }
}
