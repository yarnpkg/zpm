use std::collections::{BTreeMap, HashMap, HashSet};

use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use tokio::sync::mpsc;
use zpm_primitives::Ident;
use zpm_tasks::TaskFile;
use zpm_utils::{Path, ToFileString};

pub struct TaskfileWatcher {
    watcher: RecommendedWatcher,

    /// For each workspace, the set of file paths that were read
    /// when resolving its taskfile (includes the main taskfile + includes).
    workspace_sources: BTreeMap<Ident, Vec<Path>>,

    /// Reverse index: file path -> set of workspaces that depend on it.
    file_to_workspaces: HashMap<Path, HashSet<Ident>>,

    /// Cached parsed taskfiles per workspace (the raw parse, not resolved).
    cached_taskfiles: BTreeMap<Ident, TaskFile>,
}

impl TaskfileWatcher {
    pub fn new(event_tx: mpsc::UnboundedSender<notify::Event>) -> Self {
        let watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
            if let Ok(event) = res {
                let _ = event_tx.send(event);
            }
        })
        .expect("failed to create taskfile watcher");

        Self {
            watcher,
            workspace_sources: BTreeMap::new(),
            file_to_workspaces: HashMap::new(),
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
                        let _ = self.watcher.unwatch(std::path::Path::new(&path.to_file_string()));
                    }
                }
            }
        }

        // Add new entries
        for path in &sources {
            let is_new = !self.file_to_workspaces.contains_key(path);

            self.file_to_workspaces
                .entry(path.clone())
                .or_default()
                .insert(workspace.clone());

            if is_new {
                let _ = self
                    .watcher
                    .watch(std::path::Path::new(&path.to_file_string()), RecursiveMode::NonRecursive);
            }
        }

        self.workspace_sources.insert(workspace, sources);
    }

    /// Resolve a notify event into the set of workspace idents whose
    /// taskfiles may have changed.
    pub fn resolve_changed_workspaces(&self, event: &notify::Event) -> Vec<Ident> {
        let mut changed: HashSet<Ident> = HashSet::new();

        for abs_path in &event.paths {
            if let Ok(path) = Path::try_from(abs_path.as_path()) {
                if let Some(workspaces) = self.file_to_workspaces.get(&path) {
                    changed.extend(workspaces.iter().cloned());
                }
            }
        }

        changed.into_iter().collect()
    }

    /// Update the cached taskfile for a workspace.
    pub fn update_cached_taskfile(&mut self, workspace: Ident, taskfile: TaskFile) {
        self.cached_taskfiles.insert(workspace, taskfile);
    }

    /// Remove a workspace's cached taskfile.
    pub fn remove_cached_taskfile(&mut self, workspace: &Ident) {
        self.cached_taskfiles.remove(workspace);
    }

    /// Read access to cached taskfiles.
    pub fn cached_taskfiles(&self) -> &BTreeMap<Ident, TaskFile> {
        &self.cached_taskfiles
    }

    /// Number of files being watched.
    pub fn watched_file_count(&self) -> usize {
        self.file_to_workspaces.len()
    }
}
