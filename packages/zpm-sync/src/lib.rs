use std::{borrow::Cow, collections::BTreeMap, sync::Arc};

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

#[derive(Debug, PartialEq, Eq, Deserialize)]
#[serde(tag = "type")]
pub enum SyncItem<'a> {
    Folder {
        template: Option<SyncTemplate>,
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
                children: BTreeMap::new(),
            }],
        }
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

        if let SyncNode::Folder {template: existing_template, ..} = existing_node {
            if let SyncItem::Folder {template: new_template, ..} = &entry {
                *existing_template = new_template.clone();
                return Ok(());
            }
        }

        if existing_node != &entry.into() {
            return Err(SyncError::ConflictingPathTypes(rel_path.clone()));
        }

        Ok(())
    }

    pub fn run(&self, root_path: Path) -> Result<Vec<FileOp>, SyncError> {
        let mut file_ops
            = Vec::new();

        let mut queue
            = vec![(root_path, 0)];

        while let Some((path, node_idx)) = queue.pop() {
            let next_tasks
                = self.process_node(path, node_idx, &mut file_ops)?;

            queue.extend(next_tasks);
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

            let SyncNode::Folder {children, ..} = current_node else {
                return Err(SyncError::NotAFolder);
            };

            let existing_next
                = children.get(segment);

            if let Some(existing_next) = existing_next {
                current_idx = *existing_next;
                continue;
            }

            current_idx = candidate_next;
            children.insert(segment.to_string(), current_idx);

            self.nodes.push(SyncNode::Folder {
                template: None,
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
            });
        }

        let Some(metadata) = path.fs_symlink_metadata().ok_missing()? else {
            return Ok(SyncCheck {
                must_remove: false,
                must_create: true,
            });
        };

        match node {
            SyncNode::Any => {
                unreachable!("We already checked earlier for Any");
            },

            SyncNode::Folder {template, ..} => {
                Ok(SyncCheck {
                    must_remove: !metadata.is_dir(),
                    must_create: !metadata.is_dir() && template.is_none(),
                })
            },

            SyncNode::File {..} => {
                Ok(SyncCheck {
                    must_remove: !metadata.is_file(),
                    must_create: !metadata.is_file(),
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
                })
            },
        }
    }

    fn process_node(&self, path: Path, node_idx: usize, file_ops: &mut Vec<FileOp>) -> Result<Vec<(Path, usize)>, SyncError> {
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
            SyncNode::Any => {
                // Nothing to do here
                Ok(vec![])
            },

            SyncNode::Folder {template, children} => {
                if check.must_create {
                    if self.dry_run {
                        file_ops.push(FileOp::CreateFolder(path.clone()));
                    } else {
                        path.fs_create_dir()?;
                    }
                }

                if let Some(template) = &template {
                    match template {
                        SyncTemplate::Zip {archive_path, inner_path} => {
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

                            // We must instruct the template tree to ignore the entries
                            // that our side of the tree expects to handle
                            for segment in children.keys() {
                                template_tree.ignore_root_entry(segment.clone())?;
                            }

                            let inner_file_ops
                                = template_tree.run(path.clone())?;

                            file_ops.extend(inner_file_ops);
                        },
                    }
                } else {
                    if !check.must_create {
                        let extraneous_entries = path.fs_read_dir()
                            .ok_missing()?
                            .map(|read_dir| read_dir.collect::<Result<Vec<_>, _>>())
                            .transpose()?
                            .unwrap_or_default()
                            .into_iter()
                            .flat_map(|entry| entry.file_name().into_string().ok())
                            .filter(|file_name| !children.contains_key(file_name))
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

            SyncNode::File {data, ..} => {
                if check.must_create {
                    if self.dry_run {
                        file_ops.push(FileOp::CreateFile(path.clone(), data[..data.len().min(20)].to_vec()));
                    } else {
                        path.fs_write(data)?;
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

    Folder {
        template: Option<SyncTemplate>,
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
            SyncItem::Folder {template} => SyncNode::Folder {
                template,
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
