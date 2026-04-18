use std::collections::{BTreeMap, HashMap, HashSet};
use std::time::SystemTime;

use zpm_primitives::Ident;
use zpm_tasks::TaskFile;
use zpm_utils::Path;

/// Tracks which files contribute to each workspace's taskfile
/// and caches their modification times for change detection.
pub struct TaskfileWatcher {
    /// For each workspace, the set of file paths that were read
    /// when resolving its taskfile (includes the main taskfile + includes).
    workspace_sources: BTreeMap<Ident, Vec<Path>>,

    /// Reverse index: file path -> set of workspaces that depend on it.
    file_to_workspaces: HashMap<Path, HashSet<Ident>>,

    /// Last known modification time for each watched file.
    /// `None` means the file did not exist at last check.
    file_mtimes: HashMap<Path, Option<SystemTime>>,

    /// Cached parsed taskfiles per workspace (the raw parse, not resolved).
    cached_taskfiles: BTreeMap<Ident, TaskFile>,
}

impl TaskfileWatcher {
    pub fn new() -> Self {
        Self {
            workspace_sources: BTreeMap::new(),
            file_to_workspaces: HashMap::new(),
            file_mtimes: HashMap::new(),
            cached_taskfiles: BTreeMap::new(),
        }
    }

    /// Register the source files for a workspace's taskfile.
    /// Replaces any previous registration for this workspace.
    pub fn register_sources(&mut self, workspace: Ident, sources: Vec<Path>) {
        // Remove old entries from the reverse index
        if let Some(old_sources) = self.workspace_sources.get(&workspace) {
            for path in old_sources {
                if let Some(ws_set) = self.file_to_workspaces.get_mut(path) {
                    ws_set.remove(&workspace);
                    if ws_set.is_empty() {
                        self.file_to_workspaces.remove(path);
                        self.file_mtimes.remove(path);
                    }
                }
            }
        }

        // Add new entries
        for path in &sources {
            self.file_to_workspaces
                .entry(path.clone())
                .or_default()
                .insert(workspace.clone());

            // Snapshot mtime if not already tracked
            self.file_mtimes
                .entry(path.clone())
                .or_insert_with(|| get_mtime(path));
        }

        self.workspace_sources.insert(workspace, sources);
    }

    /// Update the cached taskfile for a workspace.
    pub fn update_cached_taskfile(&mut self, workspace: Ident, taskfile: TaskFile) {
        self.cached_taskfiles.insert(workspace, taskfile);
    }

    /// Remove a workspace's cached taskfile.
    pub fn remove_cached_taskfile(&mut self, workspace: &Ident) {
        self.cached_taskfiles.remove(workspace);
    }

    /// Check all watched files for modification time changes.
    /// Returns the set of workspace idents whose taskfiles need reloading.
    pub fn poll_changes(&mut self) -> Vec<Ident> {
        let mut changed_workspaces: HashSet<Ident>
            = HashSet::new();

        for (path, old_mtime) in &mut self.file_mtimes {
            let current_mtime
                = get_mtime(path);

            if current_mtime != *old_mtime {
                *old_mtime = current_mtime;

                if let Some(workspaces) = self.file_to_workspaces.get(path) {
                    changed_workspaces.extend(workspaces.iter().cloned());
                }
            }
        }

        changed_workspaces.into_iter().collect()
    }

    /// Read access to cached taskfiles.
    pub fn cached_taskfiles(&self) -> &BTreeMap<Ident, TaskFile> {
        &self.cached_taskfiles
    }

    /// Number of files being watched.
    pub fn watched_file_count(&self) -> usize {
        self.file_mtimes.len()
    }
}

/// Get the modification time of a file, or `None` if it doesn't exist.
fn get_mtime(path: &Path) -> Option<SystemTime> {
    path.fs_metadata().ok().and_then(|m| m.modified().ok())
}
