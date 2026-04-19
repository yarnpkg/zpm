use std::path::PathBuf;

use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use tokio::sync::mpsc;

pub struct FileWatcher {
    watcher: RecommendedWatcher,
    project_cwd: PathBuf,
}

impl FileWatcher {
    pub fn new(
        event_tx: mpsc::UnboundedSender<notify::Event>,
        project_cwd: PathBuf,
    ) -> Self {
        let watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
            if let Ok(event) = res {
                let _ = event_tx.send(event);
            }
        })
        .expect("failed to create file watcher");

        Self {
            watcher,
            project_cwd,
        }
    }

    pub fn register(&mut self, path: &str) {
        let full_path = self.project_cwd.join(path);
        let _ = self
            .watcher
            .watch(&full_path, RecursiveMode::NonRecursive);
    }

    pub fn resolve_event_paths(&self, event: &notify::Event) -> Vec<String> {
        let mut paths = Vec::new();
        for abs_path in &event.paths {
            if let Ok(relative) = abs_path.strip_prefix(&self.project_cwd) {
                if let Some(s) = relative.to_str() {
                    paths.push(s.to_string());
                }
            }
        }
        paths
    }
}
