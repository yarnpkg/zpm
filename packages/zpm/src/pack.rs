use std::collections::BTreeMap;
use std::collections::HashSet;
use std::sync::LazyLock;

use zpm_parsers::JsonFormatter;
use zpm_parsers::JsonValue;
use zpm_utils::Path;
use globset::GlobBuilder;
use globset::GlobMatcher;
use regex::Regex;
use zpm_utils::ToFileString;

use crate::error::Error;
use crate::manifest::helpers::parse_manifest;
use crate::manifest::Manifest;
use crate::primitives::range::AnonymousSemverRange;
use crate::primitives::Descriptor;
use crate::primitives::PeerRange;
use crate::primitives::Range;
use crate::project::Project;
use crate::project::Workspace;

#[derive(Default)]
struct IgnoreFiles {
    pub gitignore: bool,
    pub npmignore: bool,
}

impl IgnoreFiles {
    pub fn gitignore() -> Self {
        Self {
            gitignore: true,
            npmignore: false,
        }
    }

    pub fn npmignore() -> Self {
        Self {
            gitignore: false,
            npmignore: true,
        }
    }
}

static GLOB_ABS_REGEXP: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(!)?(\.{0,2}/|\.{0,2}$)?(.*)").unwrap()
});

struct PackGlob {
    pub glob_matcher: GlobMatcher,
    pub is_positive: bool,
}

impl std::fmt::Debug for PackGlob {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PackGlob")
            .field("glob_matcher", &self.glob_matcher.glob().to_string())
            .field("is_positive", &self.is_positive)
            .finish()
    }
}

#[derive(Debug)]
struct PackIgnore {
    pub patterns: Vec<PackGlob>,
}

impl PackIgnore {
    pub fn new() -> Self {
        Self {
            patterns: vec![],
        }
    }

    pub fn add_raw(&mut self, pack_glob: PackGlob) {
        self.patterns.push(pack_glob);
    }

    pub fn add(&mut self, from_dir: &Path, pattern: &str) -> Result<(), Error> {
        let captures = GLOB_ABS_REGEXP
            .captures(pattern)
            .expect("Expected the glob regex to match");

        let prefix = &captures.get(1)
            .map(|m| m.as_str())
            .unwrap_or("");

        let is_positive =
            !prefix.contains('!');

        let is_rooted =
            captures.get(2).is_some() || pattern.contains('/');

        let mut rooted_path = match is_rooted {
            true => from_dir.with_join_str(&captures[3]),
            false => from_dir.with_join_str("**").with_join_str(&captures[3]),
        };

        self.push(&rooted_path.as_str(), is_positive);
        rooted_path.join_str("**");
        self.push(&rooted_path.as_str(), is_positive);

        Ok(())
    }

    fn push(&mut self, rooted_path: &str, is_positive: bool) {
        let nested_glob_matcher = GlobBuilder::new(&rooted_path)
            .build()
            .expect("Failed to build glob")
            .compile_matcher();

        self.patterns.push(PackGlob {
            glob_matcher: nested_glob_matcher,
            is_positive,
        });
    }

    pub fn is_ignored(&self, rel_path: &Path) -> bool {
        let last_matching_entry = self.patterns.iter().rev()
            .find(|matcher| matcher.glob_matcher.is_match(rel_path.as_str()));

        last_matching_entry
            .map(|matcher| matcher.is_positive)
            .unwrap_or(false)
    }
}

struct PackList {
    pub root_path: Path,

    pub files: Vec<Path>,
    pub ignore_files: BTreeMap<Path, IgnoreFiles>,

    pub skip_traversal_by_name: HashSet<String>,
    pub skip_traversal_by_rel_path: HashSet<Path>,
}

impl PackList {
    pub fn new(root_path: Path) -> Self {
        Self {
            root_path,

            files: vec![],
            ignore_files: BTreeMap::new(),

            skip_traversal_by_name: HashSet::new(),
            skip_traversal_by_rel_path: HashSet::new(),
        }
    }

    pub fn traverse(&mut self, rel_path: &Path) -> Result<(), Error> {
        let abs_path = self.root_path
            .with_join(rel_path);

        let directory_entries = abs_path.fs_read_dir()?
            .into_iter()
            .collect::<Result<Vec<_>, _>>()?;

        for entry in directory_entries {
            let file_type
                = entry.file_type()?;

            let file_name
                = entry.file_name()
                    .into_string()
                    .map_err(|_| Error::NonUtf8Path)?;

            let entry_rel_path = rel_path
                .with_join_str(&file_name);

            if file_type.is_dir() {
                if !self.skip_traversal_by_name.contains(&file_name) && !self.skip_traversal_by_rel_path.contains(&entry_rel_path) {
                    self.traverse(&entry_rel_path)?;
                }
            }

            if file_type.is_file() {
                if file_name == ".gitignore" {
                    self.ignore_files.entry(rel_path.clone())
                        .and_modify(|f| f.gitignore = true)
                        .or_insert(IgnoreFiles::gitignore());
                }

                if file_name == ".npmignore" {
                    self.ignore_files.entry(rel_path.clone())
                        .and_modify(|f| f.npmignore = true)
                        .or_insert(IgnoreFiles::npmignore());
                }

                self.files.push(entry_rel_path);
            }
        }

        Ok(())
    }

    pub fn load_ignore(&self) -> Result<Vec<String>, Error> {
        let mut patterns = vec![];

        for (path, ignore_files) in &self.ignore_files {
            let ignore_name = if ignore_files.npmignore {
                Some(".npmignore")
            } else if ignore_files.gitignore {
                Some(".gitignore")
            } else {
                None
            };

            if let Some(ignore_name) = ignore_name {
                let abs_glob_root = self.root_path
                    .with_join(&path);

                let ignore_file = abs_glob_root
                    .with_join_str(&ignore_name)
                    .fs_read_text_prealloc()?;

                let ignore_list = ignore_file
                    .split('\n')
                    .map(|line| line.trim())
                    .filter(|line| !line.is_empty() && !line.starts_with('#'))
                    .map(|line| line.to_string())
                    .collect::<Vec<_>>();

                patterns.extend(ignore_list);
            }
        }

        Ok(patterns)
    }
}

pub fn pack_manifest(project: &Project, workspace: &Workspace) -> Result<String, Error> {
    let manifest_path = workspace.path
        .with_join_str("package.json");

    let manifest_content = manifest_path
        .fs_read_text_prealloc()?;

    let manifest: Manifest
        = parse_manifest(&manifest_content)?;

    let mut formatter
        = JsonFormatter::from(&manifest_content)?;

    if let Some(type_) = &manifest.publish_config.type_ {
        formatter.set(
            &vec!["publishConfig".to_string(), "type".to_string()].into(),
            JsonValue::String(type_.clone()),
        )?;
    }

    if let Some(main) = &manifest.publish_config.main {
        formatter.set(
            &vec!["main".to_string()].into(),
            JsonValue::String(main.clone()),
        )?;
    }

    if let Some(exports) = &manifest.publish_config.exports {
        formatter.set(
            &vec!["exports".to_string()].into(),
            JsonValue::from(&sonic_rs::to_value(exports)?),
        )?;
    }

    if let Some(imports) = &manifest.publish_config.imports {
        formatter.set(
            &vec!["imports".to_string()].into(),
            JsonValue::from(&sonic_rs::to_value(imports)?),
        )?;
    }

    if let Some(module) = &manifest.publish_config.module {
        formatter.set(
            &vec!["module".to_string()].into(),
            JsonValue::String(module.clone()),
        )?;
    }

    if let Some(browser) = &manifest.publish_config.browser {
        formatter.set(
            &vec!["browser".to_string()].into(),
            JsonValue::from(&sonic_rs::to_value(browser)?),
        )?;
    }

    if let Some(bin) = &manifest.publish_config.bin {
        formatter.set(
            &vec!["bin".to_string()].into(),
            JsonValue::from(&sonic_rs::to_value(bin)?),
        )?;
    }

    let hard_dependencies = vec![
        ("dependencies", manifest.remote.dependencies.iter()),
        ("devDependencies", manifest.dev_dependencies.iter()),
        ("optionalDependencies", manifest.remote.optional_dependencies.iter()),
    ];

    let updated_dependencies = hard_dependencies.into_iter().flat_map(|(field_name, iter)| {
        iter.filter_map(move |(ident, descriptor)| {
            match &descriptor.range {
                Range::WorkspaceSemver(params) => {
                    Some((field_name, Ok(Descriptor::new(ident.clone(), Range::AnonymousSemver(AnonymousSemverRange {
                        range: params.range.clone()
                    })))))
                },
    
                Range::WorkspaceMagic(params) => {
                    let workspace
                        = project.workspace_by_ident(&descriptor.ident);
    
                    Some((field_name, workspace.map(|workspace| Descriptor::new(ident.clone(), Range::AnonymousSemver(AnonymousSemverRange {
                        range: workspace.manifest.remote.version.clone().unwrap_or_default().to_range(params.magic),
                    })))))
                },
    
                Range::WorkspaceIdent(params) => {
                    let workspace
                        = project.workspace_by_ident(&params.ident);
                    
                    Some((field_name, workspace.map(|workspace| Descriptor::new(ident.clone(), Range::AnonymousSemver(AnonymousSemverRange {
                        range: workspace.manifest.remote.version.clone().unwrap_or_default().to_range(zpm_semver::RangeKind::Exact),
                    })))))
                },
    
                Range::WorkspacePath(params) => {
                    let workspace
                        = project.workspace_by_rel_path(&params.path);
    
                    Some((field_name, workspace.map(|workspace| Descriptor::new(ident.clone(), Range::AnonymousSemver(AnonymousSemverRange {
                        range: workspace.manifest.remote.version.clone().unwrap_or_default().to_range(zpm_semver::RangeKind::Exact),
                    })))))
                },
    
                _ => {
                    None
                },
            }
        })
    });

    for (field_name, new_descriptor_result) in updated_dependencies {
        let new_descriptor
            = new_descriptor_result?;

        formatter.set(
            &vec![field_name.to_string(), new_descriptor.ident.to_file_string()].into(),
            JsonValue::String(new_descriptor.range.to_file_string()),
        )?;
    }

    let updated_peer_dependencies = manifest.remote.peer_dependencies.iter().filter_map(|(ident, peer_range)| {
        match peer_range {
            PeerRange::WorkspaceMagic(params) => {
                let workspace
                    = project.workspace_by_ident(ident);

                Some(workspace.and_then(move |workspace| {
                    Ok(Descriptor::new(ident.clone(), Range::AnonymousSemver(AnonymousSemverRange {
                        range: workspace.manifest.remote.version.clone().unwrap_or_default().to_range(params.magic),
                    })))
                }))
            },

            PeerRange::WorkspacePath(params) => {
                let workspace
                    = project.workspace_by_rel_path(&params.path);

                Some(workspace.and_then(move |workspace| {
                    Ok(Descriptor::new(ident.clone(), Range::AnonymousSemver(AnonymousSemverRange {
                        range: workspace.manifest.remote.version.clone().unwrap_or_default().to_range(zpm_semver::RangeKind::Exact),
                    })))
                }))
            },

            PeerRange::WorkspaceSemver(params) => {
                Some(Ok(Descriptor::new(ident.clone(), Range::AnonymousSemver(AnonymousSemverRange {
                    range: params.range.clone(),
                }))))
            },

            _ => {
                None
            },
        }
    });

    for new_descriptor_result in updated_peer_dependencies {
        let new_descriptor
            = new_descriptor_result?;

        formatter.set(
            &vec!["peerDependencies".to_string(), new_descriptor.ident.to_file_string()].into(),
            JsonValue::String(new_descriptor.range.to_file_string())
        )?;
    }

    Ok(formatter.to_string())
}

pub fn pack_list(project: &Project, workspace: &Workspace, manifest: &Manifest) -> Result<Vec<zpm_utils::Path>, Error> {
    let mut pack_list = PackList::new(workspace.path.clone());

    pack_list.skip_traversal_by_name.insert(".git".to_string());
    pack_list.skip_traversal_by_name.insert(".github".to_string());
    pack_list.skip_traversal_by_name.insert(".hg".to_string());
    pack_list.skip_traversal_by_name.insert(".vscode".to_string());
    pack_list.skip_traversal_by_name.insert(".yarn".to_string());
    pack_list.skip_traversal_by_name.insert("node_modules".to_string());
    pack_list.skip_traversal_by_name.insert("target".to_string());

    for workspace in &project.workspaces {
        pack_list.skip_traversal_by_rel_path.insert(workspace.rel_path.clone());
    }

    pack_list.traverse(&Path::new())?;
    pack_list.files.sort();

    let mut glob_ignore = PackIgnore::new();

    if let Some(files) = &workspace.manifest.files {
        pack_list.ignore_files.remove(&Path::new());

        glob_ignore.add(&Path::new(), "*")?;

        for pattern in files {
            if pattern.starts_with('!') {
                glob_ignore.add(&Path::new(), &pattern[1..])?;
            } else {
                glob_ignore.add(&Path::new(), &format!("!{}", pattern))?;
            }
        }
    }

    let user_patterns = pack_list
        .load_ignore()?;

    for pattern in &user_patterns {
        glob_ignore.add(&Path::new(), pattern)?;
    }

    let always_ignored = GlobBuilder::new("{.#*,.DS_Store,.gitignore,.npmignore,.pnp.*,.yarnrc,yarn.lock,*.tsbuildinfo}")
        .build()
        .expect("Failed to build glob")
        .compile_matcher();

    glob_ignore.add_raw(PackGlob {
        glob_matcher: always_ignored,
        is_positive: true,
    });

    let always_allowed = GlobBuilder::new("package.json")
        .build()
        .expect("Failed to build glob")
        .compile_matcher();

    glob_ignore.add_raw(PackGlob {
        glob_matcher: always_allowed,
        is_positive: false,
    });

    let misc_files = GlobBuilder::new("{readme,licence,license,changelog}{,.*}")
        .empty_alternates(true)
        .case_insensitive(true)
        .build()
        .expect("Failed to build glob")
        .compile_matcher();

    glob_ignore.add_raw(PackGlob {
        glob_matcher: misc_files,
        is_positive: false,
    });

    if let Some(main) = &manifest.main {
        glob_ignore.add(&Path::new(), &format!("!/{}", main))?;
    }

    if let Some(exports) = &manifest.exports {
        for export_path in exports.paths() {
            glob_ignore.add(&Path::new(), &format!("!/{}", export_path.path.to_file_string()))?;
        }
    }

    if let Some(imports) = &manifest.imports {
        for import_path in imports.paths() {
            glob_ignore.add(&Path::new(), &format!("!/{}", import_path.path.to_file_string()))?;
        }
    }

    if let Some(browser) = &manifest.browser {
        for import_path in browser.paths() {
            glob_ignore.add(&Path::new(), &format!("!/{}", import_path.to_file_string()))?;
        }
    }

    if let Some(module) = &manifest.module {
        glob_ignore.add(&Path::new(), &format!("!/{}", module))?;
    }

    if let Some(bin) = &manifest.bin {
        for path in bin.paths() {
            glob_ignore.add(&Path::new(), &format!("!/{}", path.to_file_string()))?;
        }
    }

    let final_list = pack_list
        .files
        .into_iter()
        .filter(|path| !glob_ignore.is_ignored(path))
        .collect();

    Ok(final_list)
}
