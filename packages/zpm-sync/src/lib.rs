use std::{borrow::Cow, collections::BTreeMap, os::unix::fs::PermissionsExt, sync::Arc};

use itertools::Itertools;
use serde::Deserialize;
use zpm_formats::{Entry, iter_ext::IterExt};
use zpm_utils::{IoResultExt, Path, PathError, Serialized, ToHumanString};

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "type")]
pub enum SyncTemplate {
    Zip {
        archive_path: Path,
        inner_path: Path,
    },
}

/// What a folder does with on-disk entries that weren't registered in
/// the tree. Registered entries (including `Missing` ones) are always
/// reconciled regardless of this setting.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
pub enum PreserveExtra {
    /// Unregistered entries are removed.
    #[default]
    None,

    /// Unregistered entries whose name starts with a dot are kept.
    Dots,

    /// Unregistered entries are kept; the folder content is managed by
    /// someone else, except for the explicitly registered children.
    All,
}

impl PreserveExtra {
    pub fn preserves(&self, name: &str) -> bool {
        match self {
            PreserveExtra::None => false,
            PreserveExtra::Dots => name.starts_with('.'),
            PreserveExtra::All => true,
        }
    }
}

#[derive(Debug, PartialEq, Eq, Deserialize)]
#[serde(tag = "type")]
pub enum SyncItem<'a> {
    /// Placeholder telling the sync tree "something is supposed to
    /// live here, but I'll write it myself after the sync runs." The
    /// tree skips create/remove and never descends into it, leaving
    /// whatever the caller produced later on its own.
    Any,

    /// The path must not exist; whatever lives there is removed, even
    /// when the parent folder would otherwise preserve it.
    Missing,

    Folder {
        template: Option<SyncTemplate>,

        /// The caller vouches that the on-disk content of this folder
        /// already matches its template; the sync keeps the folder
        /// without expanding the template. Explicitly registered
        /// children are still visited.
        #[serde(default)]
        assume_up_to_date: bool,

        /// Intermediate folders created to host this folder's
        /// descendants inherit this setting.
        #[serde(default)]
        preserve_extra: PreserveExtra,
    },

    Symlink {
        target_path: Path,
    },

    File {
        data: Cow<'a, [u8]>,
        is_exec: bool,
    },
}

#[derive(thiserror::Error, Clone, Debug)]
pub enum SyncError {
    #[error("IO error: {0}")]
    IoError(Arc<std::io::Error>),

    #[error("Path error: {0}")]
    PathError(#[from] PathError),

    #[error(transparent)]
    FormatError(#[from] zpm_formats::Error),

    #[error("Forward path required: {}", .0.to_print_string())]
    ForwardPathRequired(Path),

    #[error("Conflicting path types: {}", .0.to_print_string())]
    ConflictingPathTypes(Path),

    #[error("Expected a folder node")]
    NotAFolder,
}

impl From<std::io::Error> for SyncError {
    fn from(error: std::io::Error) -> Self {
        Self::IoError(Arc::new(error))
    }
}

#[derive(Debug)]
pub struct SyncCheck {
    pub must_remove: bool,
    pub must_create: bool,
    pub exists: bool,
}

pub struct SyncTree<'a> {
    pub dry_run: bool,
    nodes: Vec<SyncNode<'a>>,
}

pub enum FileOp {
    Delete(Path),
    CreateFolder(Path),
    CreateSymlink(Path, Path),
    CreateFile(Path, Vec<u8>),
}

impl std::fmt::Display for FileOp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FileOp::Delete(path) => write!(f, "delete: {}", path.to_print_string()),
            FileOp::CreateFolder(path) => write!(f, "create folder: {}", path.to_print_string()),
            FileOp::CreateSymlink(path, target_path) => write!(f, "create symlink: {} -> {}", path.to_print_string(), target_path.to_print_string()),
            FileOp::CreateFile(path, data) => write!(f, "create file: {} (starting with {})", path.to_print_string(), Serialized::new(String::from_utf8_lossy(data)).to_print_string()),
        }
    }
}

impl<'a> SyncTree<'a> {
    pub fn from_entries(entries: &[Entry<'a>]) -> Result<Self, SyncError> {
        let mut sync_tree
            = Self::new();

        for entry in entries {
            sync_tree.register_entry(entry.name.clone(), SyncItem::File {
                data: entry.data.clone(),
                is_exec: entry.mode & 0o111 != 0,
            })?;
        }

        Ok(sync_tree)
    }

    pub fn new() -> Self {
        Self {
            dry_run: true,
            nodes: vec![SyncNode::Folder {
                template: None,
                assume_up_to_date: false,
                preserve_extra: PreserveExtra::None,
                children: BTreeMap::new(),
            }],
        }
    }

    pub fn set_root_preserve_extra(&mut self, new_preserve_extra: PreserveExtra) -> Result<(), SyncError> {
        let SyncNode::Folder {preserve_extra, ..} = &mut self.nodes[0] else {
            return Err(SyncError::NotAFolder);
        };

        *preserve_extra = new_preserve_extra;

        Ok(())
    }

    pub fn root_entries(&self) -> Result<impl Iterator<Item = &String>, SyncError> {
        let node
            = &self.nodes[0];

        let SyncNode::Folder {children, ..} = node else {
            return Err(SyncError::NotAFolder);
        };

        Ok(children.keys())
    }

    pub fn ignore_root_entry(&mut self, name: String) -> Result<(), SyncError> {
        let candidate_idx
            = self.nodes.len();

        let node
            = &mut self.nodes[0];

        let SyncNode::Folder {children, ..} = node else {
            return Err(SyncError::NotAFolder);
        };

        children.insert(name, candidate_idx);
        self.nodes.push(SyncNode::Any);

        Ok(())
    }

    pub fn is_node_filtered_out(&self, node_idx: usize) -> bool {
        if node_idx == 0 {
            return false;
        }

        let node
            = &self.nodes[node_idx];

        // A preserving folder is meaningful even without children: it
        // keeps content the tree doesn't know about.
        matches!(node, SyncNode::Folder {template: None, preserve_extra: PreserveExtra::None, children, ..} if children.is_empty())
    }

    /// Folders whose only descendants are `Missing` entries don't need to
    /// be created: the entries are already absent if the folder is.
    fn is_vacuous(&self, node_idx: usize) -> bool {
        match &self.nodes[node_idx] {
            SyncNode::Missing => true,
            SyncNode::Folder {template: None, children, ..} => !children.is_empty() && children.values().all(|&child_idx| self.is_vacuous(child_idx)),
            _ => false,
        }
    }

    pub fn register_entry(&mut self, rel_path: Path, entry: SyncItem<'a>) -> Result<(), SyncError> {
        if !rel_path.is_forward() {
            return Err(SyncError::ForwardPathRequired(rel_path.clone()));
        }

        let mut segments_it
            = rel_path.segments();

        let basename
            = segments_it.next_back()
                .expect("Expected the entry to have a path");

        let parent_idx
            = self.ensure_folder(segments_it)?;

        let candidate_idx
            = self.nodes.len();

        let parent_node
            = &mut self.nodes[parent_idx];

        let SyncNode::Folder {children, ..} = parent_node else {
            return Err(SyncError::NotAFolder);
        };

        let Some(existing_node_idx) = children.get(basename).copied() else {
            children.insert(basename.to_string(), candidate_idx);
            self.nodes.push(entry.into());

            return Ok(());
        };

        let existing_node
            = &mut self.nodes[existing_node_idx];

        if let SyncNode::Folder {template: existing_template, assume_up_to_date: existing_assume, preserve_extra: existing_preserve, ..} = existing_node {
            if let SyncItem::Folder {template: new_template, assume_up_to_date: new_assume, preserve_extra: new_preserve} = &entry {
                *existing_template = new_template.clone();
                *existing_assume = *new_assume;
                *existing_preserve = *new_preserve;
                return Ok(());
            }
        }

        if existing_node != &entry.into() {
            return Err(SyncError::ConflictingPathTypes(rel_path.clone()));
        }

        Ok(())
    }

    pub fn run(&self, root_path: Path) -> Result<Vec<FileOp>, SyncError> {
        use rayon::prelude::*;

        let mut file_ops
            = Vec::new();

        // Nodes are processed level by level so parents always exist
        // before their children; within a level every node is
        // independent and can run in parallel.
        let mut current_level
            = vec![(root_path, 0)];

        while !current_level.is_empty() {
            let results = current_level
                .into_par_iter()
                .map(|(path, node_idx)| {
                    let mut ops = Vec::new();
                    let next_tasks = self.process_node(path, node_idx, &mut ops)?;

                    Ok((ops, next_tasks))
                })
                .collect::<Result<Vec<_>, SyncError>>()?;

            current_level = Vec::new();

            for (ops, next_tasks) in results {
                file_ops.extend(ops);
                current_level.extend(next_tasks);
            }
        }

        Ok(file_ops)
    }

    fn ensure_folder<'b>(&mut self, segments_it: impl Iterator<Item = &'b str>) -> Result<usize, SyncError> {
        let mut current_idx
            = 0usize;

        for segment in segments_it {
            let candidate_next
                = self.nodes.len();

            let current_node
                = &mut self.nodes[current_idx];

            let SyncNode::Folder {children, preserve_extra, ..} = current_node else {
                return Err(SyncError::NotAFolder);
            };

            let existing_next
                = children.get(segment);

            if let Some(existing_next) = existing_next {
                current_idx = *existing_next;
                continue;
            }

            let preserve_extra
                = *preserve_extra;

            current_idx = candidate_next;
            children.insert(segment.to_string(), current_idx);

            self.nodes.push(SyncNode::Folder {
                template: None,
                assume_up_to_date: false,
                preserve_extra,
                children: BTreeMap::new(),
            });
        }

        Ok(current_idx)
    }

    fn check(&self, path: &Path, node: &SyncNode<'a>) -> Result<SyncCheck, SyncError> {
        if matches!(node, SyncNode::Any) {
            return Ok(SyncCheck {
                must_remove: false,
                must_create: false,
                exists: true,
            });
        }

        let Some(metadata) = path.fs_symlink_metadata().ok_missing()? else {
            return Ok(SyncCheck {
                must_remove: false,
                must_create: !matches!(node, SyncNode::Missing),
                exists: false,
            });
        };

        match node {
            SyncNode::Any => {
                unreachable!("We already checked earlier for Any");
            },

            SyncNode::Missing => {
                Ok(SyncCheck {
                    must_remove: true,
                    must_create: false,
                    exists: true,
                })
            },

            SyncNode::Folder {template, ..} => {
                let is_dir
                    = path.fs_is_dir();

                Ok(SyncCheck {
                    must_remove: !is_dir,
                    must_create: !is_dir && template.is_none(),
                    exists: true,
                })
            },

            SyncNode::File {data, is_exec} => {
                let expected_x
                    = if *is_exec {0o111} else {0o000};

                let is_file_up_to_date
                    = metadata.is_file()
                        && (metadata.permissions().mode() & 0o111) == expected_x
                        && metadata.len() == data.len() as u64
                        && data == &path.fs_read_with_size(metadata.len())?;

                Ok(SyncCheck {
                    must_remove: !is_file_up_to_date,
                    must_create: !is_file_up_to_date,
                    exists: true,
                })
            },

            SyncNode::Symlink {target_path} => {
                let symlink_target
                    = metadata.is_symlink()
                        .then(|| path.fs_read_link())
                        .transpose()?;

                let is_symlink_up_to_date
                    = symlink_target.as_ref() == Some(target_path);

                Ok(SyncCheck {
                    must_remove: !is_symlink_up_to_date,
                    must_create: !is_symlink_up_to_date,
                    exists: true,
                })
            },
        }
    }

    fn process_node(&self, path: Path, node_idx: usize, file_ops: &mut Vec<FileOp>) -> Result<Vec<(Path, usize)>, SyncError> {
        if self.is_node_filtered_out(node_idx) {
            return Ok(vec![]);
        }

        let node
            = &self.nodes[node_idx];

        let check
            = self.check(&path, node)?;

        if check.must_remove {
            if self.dry_run {
                file_ops.push(FileOp::Delete(path.clone()));
            } else {
                path.fs_rm()?;
            }
        }

        match node {
            SyncNode::Any | SyncNode::Missing => {
                // Nothing to do here
                Ok(vec![])
            },

            SyncNode::Folder {template, assume_up_to_date, preserve_extra, children} => {
                if check.must_create && !check.exists && self.is_vacuous(node_idx) {
                    return Ok(vec![]);
                }

                if check.must_create {
                    if self.dry_run {
                        file_ops.push(FileOp::CreateFolder(path.clone()));
                    } else {
                        path.fs_create_dir()?;
                    }
                }

                // An assumed folder that turns out to be missing (or of
                // the wrong type) self-heals through the regular
                // template expansion.
                let expand_template
                    = !assume_up_to_date || !check.exists || check.must_remove;

                if let Some(template) = &template {
                    match template {
                        SyncTemplate::Zip {archive_path, inner_path} if expand_template => {
                            let zip_buffer
                                = archive_path.fs_read()?;

                            let zip_entries
                                = zpm_formats::zip::entries_from_zip(&zip_buffer)?
                                    .into_iter()
                                    .strip_path_prefix(inner_path)
                                    .collect_vec();

                            let mut template_tree
                                = SyncTree::from_entries(&zip_entries)?;

                            template_tree.dry_run = self.dry_run;
                            template_tree.set_root_preserve_extra(*preserve_extra)?;

                            // We must instruct the template tree to ignore the entries
                            // that our side of the tree expects to handle
                            for segment in children.keys() {
                                template_tree.ignore_root_entry(segment.clone())?;
                            }

                            let inner_file_ops
                                = template_tree.run(path.clone())?;

                            file_ops.extend(inner_file_ops);
                        },

                        SyncTemplate::Zip {..} => {
                            // Assumed up-to-date; leave the folder as is.
                        },
                    }
                } else {
                    // Under `All` nothing can be extraneous; skip the read.
                    if !check.must_create && *preserve_extra != PreserveExtra::All {
                        let extraneous_entries = path.fs_read_dir()
                            .ok_missing()?
                            .map(|read_dir| read_dir.collect::<Result<Vec<_>, _>>())
                            .transpose()?
                            .unwrap_or_default()
                            .into_iter()
                            .flat_map(|entry| entry.file_name().into_string().ok())
                            .filter(|file_name| match children.get(file_name) {
                                Some(&child_idx) => self.is_node_filtered_out(child_idx),
                                None => !preserve_extra.preserves(file_name),
                            })
                            .collect_vec();

                        for entry in extraneous_entries {
                            let entry_path
                                = path.with_join_str(&entry);

                            if self.dry_run {
                                file_ops.push(FileOp::Delete(entry_path));
                            } else {
                                entry_path.fs_rm()?;
                            }
                        }
                    }
                }

                let next_tasks
                    = children.iter()
                        .map(|(segment, child_idx)| (path.with_join_str(segment), *child_idx))
                        .collect_vec();

                Ok(next_tasks)
            },

            SyncNode::File {data, is_exec} => {
                if check.must_create {
                    if self.dry_run {
                        file_ops.push(FileOp::CreateFile(path.clone(), data[..data.len().min(20)].to_vec()));
                    } else {
                        path.fs_write(data)?;

                        if *is_exec {
                            path.fs_set_permissions(std::fs::Permissions::from_mode(0o755))?;
                        }
                    }
                }

                Ok(vec![])
            },

            SyncNode::Symlink {target_path} => {
                if check.must_create {
                    if self.dry_run {
                        file_ops.push(FileOp::CreateSymlink(path.clone(), target_path.clone()));
                    } else {
                        path.fs_symlink(target_path)?;
                    }
                }

                Ok(vec![])
            },
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum SyncNode<'a> {
    Any,

    Missing,

    Folder {
        template: Option<SyncTemplate>,
        assume_up_to_date: bool,
        preserve_extra: PreserveExtra,
        children: BTreeMap<String, usize>,
    },

    File {
        data: Cow<'a, [u8]>,
        is_exec: bool,
    },

    Symlink {
        target_path: Path,
    },
}

impl<'a> From<SyncItem<'a>> for SyncNode<'a> {
    fn from(entry: SyncItem<'a>) -> Self {
        match entry {
            SyncItem::Any => SyncNode::Any,

            SyncItem::Missing => SyncNode::Missing,

            SyncItem::Folder {template, assume_up_to_date, preserve_extra} => SyncNode::Folder {
                template,
                assume_up_to_date,
                preserve_extra,
                children: BTreeMap::new(),
            },

            SyncItem::File {data, is_exec} => SyncNode::File {
                data,
                is_exec,
            },

            SyncItem::Symlink {target_path} => SyncNode::Symlink {
                target_path,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::*;

    fn p(path: &str) -> Path {
        Path::from_str(path).unwrap()
    }

    fn setup(paths: &[&str]) -> Path {
        let root
            = Path::temp_dir().unwrap();

        for path in paths {
            root.with_join_str(path)
                .fs_create_parent().unwrap()
                .fs_write(b"").unwrap();
        }

        root
    }

    fn run(root: &Path, configure: impl FnOnce(&mut SyncTree)) {
        let mut tree
            = SyncTree::new();

        tree.dry_run = false;
        configure(&mut tree);

        tree.run(root.clone()).unwrap();
    }

    fn folder(preserve_extra: PreserveExtra) -> SyncItem<'static> {
        SyncItem::Folder {template: None, assume_up_to_date: false, preserve_extra}
    }

    fn symlink(target_path: &str) -> SyncItem<'static> {
        SyncItem::Symlink {target_path: p(target_path)}
    }

    #[test]
    fn removes_unregistered_entries_by_default() {
        let root = setup(&["extra", ".dot"]);

        run(&root, |tree| {
            tree.register_entry(p("link"), symlink("target")).unwrap();
        });

        assert!(!root.with_join_str("extra").fs_exists());
        assert!(!root.with_join_str(".dot").fs_exists());
        assert_eq!(root.with_join_str("link").fs_read_link().unwrap(), p("target"));
    }

    #[test]
    fn preserve_dots_keeps_unregistered_dot_entries() {
        let root = setup(&["extra", ".dot", ".cache/file"]);

        run(&root, |tree| {
            tree.set_root_preserve_extra(PreserveExtra::Dots).unwrap();
            tree.register_entry(p("link"), symlink("target")).unwrap();
        });

        assert!(!root.with_join_str("extra").fs_exists());
        assert!(root.with_join_str(".dot").fs_exists());
        assert!(root.with_join_str(".cache/file").fs_exists());
    }

    #[test]
    fn preserve_dots_still_syncs_registered_dot_entries() {
        let root = setup(&[".dot"]);

        run(&root, |tree| {
            tree.set_root_preserve_extra(PreserveExtra::Dots).unwrap();
            tree.register_entry(p(".dot"), symlink("target")).unwrap();
        });

        assert_eq!(root.with_join_str(".dot").fs_read_link().unwrap(), p("target"));
    }

    #[test]
    fn preserve_all_keeps_content_and_syncs_registered_children() {
        let root = setup(&["pkg/index.js", "pkg/lib/util.js", "pkg/node_modules/bundled/index.js"]);

        run(&root, |tree| {
            tree.register_entry(p("pkg"), folder(PreserveExtra::All)).unwrap();
            tree.register_entry(p("pkg/node_modules/self"), symlink("../../other")).unwrap();
        });

        assert!(root.with_join_str("pkg/index.js").fs_exists());
        assert!(root.with_join_str("pkg/lib/util.js").fs_exists());

        // The intermediate `node_modules` inherits `All`
        assert!(root.with_join_str("pkg/node_modules/bundled/index.js").fs_exists());
        assert_eq!(root.with_join_str("pkg/node_modules/self").fs_read_link().unwrap(), p("../../other"));
    }

    #[test]
    fn preserve_all_folder_without_children_is_kept() {
        let root = setup(&["pkg/index.js"]);

        run(&root, |tree| {
            tree.register_entry(p("pkg"), folder(PreserveExtra::All)).unwrap();
        });

        assert!(root.with_join_str("pkg/index.js").fs_exists());
    }

    #[test]
    fn preserve_all_folder_is_created_when_missing() {
        let root = setup(&[]);

        run(&root, |tree| {
            tree.register_entry(p("pkg"), folder(PreserveExtra::All)).unwrap();
        });

        assert!(root.with_join_str("pkg").fs_is_real_dir());
    }

    #[test]
    fn missing_removes_entries_even_when_preserved() {
        let root = setup(&[".bin/stale", ".cache/file", "pkg/remove-me", "pkg/keep-me"]);

        run(&root, |tree| {
            tree.set_root_preserve_extra(PreserveExtra::Dots).unwrap();
            tree.register_entry(p(".bin"), SyncItem::Missing).unwrap();
            tree.register_entry(p("pkg"), folder(PreserveExtra::All)).unwrap();
            tree.register_entry(p("pkg/remove-me"), SyncItem::Missing).unwrap();
        });

        assert!(!root.with_join_str(".bin").fs_exists());
        assert!(root.with_join_str(".cache/file").fs_exists());
        assert!(!root.with_join_str("pkg/remove-me").fs_exists());
        assert!(root.with_join_str("pkg/keep-me").fs_exists());
    }

    #[test]
    fn missing_does_not_create_anything() {
        let root = setup(&[]);

        run(&root, |tree| {
            tree.register_entry(p(".bin"), SyncItem::Missing).unwrap();
        });

        assert!(!root.with_join_str(".bin").fs_exists());
    }

    #[test]
    fn missing_does_not_create_its_parent_folders() {
        let root = setup(&[]);

        run(&root, |tree| {
            tree.register_entry(p("a/b/.bin"), SyncItem::Missing).unwrap();
        });

        assert!(!root.with_join_str("a").fs_exists());
    }

    #[test]
    fn missing_reports_a_delete_in_dry_runs() {
        let root = setup(&[".bin/stale"]);

        let mut tree
            = SyncTree::new();

        tree.set_root_preserve_extra(PreserveExtra::Dots).unwrap();
        tree.register_entry(p(".bin"), SyncItem::Missing).unwrap();

        let ops
            = tree.run(root.clone()).unwrap();

        assert!(matches!(ops.as_slice(), [FileOp::Delete(path)] if path == &root.with_join_str(".bin")));
        assert!(root.with_join_str(".bin/stale").fs_exists());
    }

    #[test]
    fn missing_conflicts_with_other_entries() {
        let mut tree
            = SyncTree::new();

        tree.register_entry(p("a"), SyncItem::Missing).unwrap();
        tree.register_entry(p("a"), SyncItem::Missing).unwrap();

        assert!(matches!(tree.register_entry(p("a"), symlink("target")), Err(SyncError::ConflictingPathTypes(_))));
        assert!(matches!(tree.register_entry(p("a/b"), symlink("target")), Err(SyncError::NotAFolder)));

        let mut tree
            = SyncTree::new();

        tree.register_entry(p("a/b"), symlink("target")).unwrap();

        assert!(matches!(tree.register_entry(p("a"), SyncItem::Missing), Err(SyncError::ConflictingPathTypes(_))));
    }

    #[test]
    fn symlinks_with_stale_targets_are_rewritten() {
        let root = setup(&[]);

        root.with_join_str("link").fs_symlink(&p("old")).unwrap();

        run(&root, |tree| {
            tree.register_entry(p("link"), symlink("new")).unwrap();
        });

        assert_eq!(root.with_join_str("link").fs_read_link().unwrap(), p("new"));
    }
}
