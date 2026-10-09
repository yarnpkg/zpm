//! Task result caching.
//!
//! Tasks declared with the `@cache` attribute get a fingerprint computed
//! right before they'd run (once all their dependencies completed). If an
//! entry exists for this fingerprint in the cache folder, its outputs are
//! restored and its logs replayed instead of running the script; otherwise
//! the script runs and, if it succeeds, its outputs and logs are stored.
//!
//! The fingerprint covers everything that may influence the result of the
//! task; see `TaskCache::compute_fingerprint` for the exact list.

mod files;
mod patterns;
mod store;

use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Mutex,
    time::{SystemTime, UNIX_EPOCH},
};

use serde::Serialize;
use zpm_primitives::Ident;
use zpm_tasks::{
    CacheSpec,
    TaskId,
};
use zpm_utils::{
    Hash64,
    Hash64Writer,
    IoResultExt,
    Path,
    ToFileString,
};

pub use files::{
    ALWAYS_IGNORED_FOLDERS,
    FileHashState,
    FileStamp,
    WalkOptions,
    list_files,
};
pub use patterns::PatternSet;
pub use store::{
    CachedEntry,
    EntryMeta,
    LogLine,
    entry_path,
};

use crate::{
    error::Error,
    lockfile_tree::compute_workspace_tree_hashes,
    project::{
        Project,
        Workspace,
    },
    tasks::TASK_FILE_NAME,
};

/// Bump whenever the fingerprint composition or the entry format changes.
const CACHE_FORMAT: &str = "zpm-task-cache-v1";

const STATE_FILE_NAME: &str = "task-cache-files";
const TREE_HASHES_FILE_NAME: &str = "task-cache-tree-hashes.json";

/// Everything about a task definition that goes into its fingerprint.
#[derive(Debug, Clone)]
pub struct CacheTaskInfo {
    pub task_id: TaskId,
    pub workspace_path: Path,
    pub workspace_rel_path: Path,
    /// Absolute paths of the workspaces nested inside this one; their files
    /// aren't part of the default inputs.
    pub nested_workspaces: Vec<Path>,
    pub script: String,
    pub args: Vec<String>,
    pub spec: CacheSpec,
    /// Whether the task results are stored and restored (`@cache`). Tasks
    /// that aren't cached still get a fingerprint when a cached task
    /// depends on them, so that their changes cascade.
    pub is_cached: bool,
}

impl CacheTaskInfo {
    pub fn new(project: &Project, workspace: &Workspace, task_id: TaskId, script: String, spec: Option<CacheSpec>) -> Self {
        let nested_workspaces = project.workspaces.iter()
            .filter(|other| other.name != workspace.name && workspace.path.contains(&other.path))
            .map(|other| other.path.clone())
            .collect();

        Self {
            task_id,
            workspace_path: workspace.path.clone(),
            workspace_rel_path: workspace.rel_path.clone(),
            nested_workspaces,
            script,
            args: vec![],
            is_cached: spec.is_some(),
            spec: spec.unwrap_or_default(),
        }
    }
}

/// Per-run information supplied by the client that pushed the tasks. The
/// client is authoritative (rather than the daemon, which may have been
/// started with an older configuration).
#[derive(Debug, Clone)]
pub struct CacheRunOptions {
    /// When false, existing entries are ignored (but new ones are written).
    pub read: bool,

    pub cache_folder: Path,

    /// Files (relative to the project root) part of every fingerprint.
    pub global_inputs: Vec<String>,

    /// Hash of each workspace's dependency closure, as described by the
    /// lockfile. Workspaces missing from the map can't be cached.
    pub tree_hashes: BTreeMap<Ident, Hash64>,
}

impl CacheRunOptions {
    pub fn to_ipc(&self) -> crate::daemon::TaskCacheOptions {
        crate::daemon::TaskCacheOptions {
            read: self.read,
            cache_folder: self.cache_folder.to_file_string(),
            global_inputs: self.global_inputs.clone(),
            tree_hashes: self.tree_hashes.iter().map(|(ident, hash)| (ident.to_file_string(), hash.to_file_string())).collect(),
        }
    }

    pub fn from_ipc(options: &crate::daemon::TaskCacheOptions) -> Result<Self, Error> {
        let tree_hashes = options.tree_hashes.iter()
            .map(|(ident, hash)| {
                let hash = zpm_utils::FromFileString::from_file_string(hash)
                    .map_err(|_| Error::TaskCacheError(format!("Invalid tree hash for {}", ident)))?;

                Ok((Ident::new(ident), hash))
            })
            .collect::<Result<_, Error>>()?;

        Ok(Self {
            read: options.read,
            cache_folder: zpm_utils::FromFileString::from_file_string(&options.cache_folder)?,
            global_inputs: options.global_inputs.clone(),
            tree_hashes,
        })
    }

    pub fn from_project(project: &Project, read: bool) -> Result<Self, Error> {
        Ok(Self {
            read,
            cache_folder: project.project_path(&project.config.settings.task_cache_folder.value),
            global_inputs: project.config.settings.task_cache_global_inputs.iter().map(|setting| setting.value.clone()).collect(),
            tree_hashes: compute_tree_hashes(project)?,
        })
    }
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FingerprintDetails {
    pub task: String,
    pub fingerprint: String,
    pub components: BTreeMap<String, String>,
    /// Hashes of the declared environment variables (values are hashed so
    /// that secrets don't end up in logs)
    pub env: BTreeMap<String, String>,
    pub global_inputs: BTreeMap<String, String>,
    pub inputs: BTreeMap<String, String>,
    pub dependencies: BTreeMap<String, String>,
}

pub struct Fingerprint {
    pub hash: Hash64,
    pub details: FingerprintDetails,
}

struct HashBuilder {
    writer: Hash64Writer,
}

impl HashBuilder {
    fn new() -> Self {
        Self {writer: Hash64Writer::new()}
    }

    /// Length-prefixed so that no two different sequences of fields can
    /// produce the same byte stream.
    fn field(&mut self, label: &str, value: impl AsRef<[u8]>) {
        let value
            = value.as_ref();

        self.writer.update((label.len() as u64).to_le_bytes());
        self.writer.update(label.as_bytes());
        self.writer.update((value.len() as u64).to_le_bytes());
        self.writer.update(value);
    }

    fn finalize(self) -> Hash64 {
        self.writer.finalize()
    }
}

fn env_value_hash(value: Option<&str>) -> String {
    match value {
        Some(value) => Hash64::from_data(value.as_bytes()).mini(),
        None => "<unset>".to_string(),
    }
}

/// Returns the environment variables matching the declared names; a name
/// ending with `*` matches all the variables with that prefix.
fn collect_env(spec_env: &[String]) -> BTreeMap<String, Option<String>> {
    let mut values
        = BTreeMap::new();

    let mut prefixes
        = Vec::new();

    for name in spec_env {
        match name.strip_suffix('*') {
            Some(prefix) => {
                prefixes.push(prefix.to_string());
            },

            None => {
                values.insert(name.clone(), std::env::var(name).ok());
            },
        }
    }

    if !prefixes.is_empty() {
        for (key, value) in std::env::vars() {
            if prefixes.iter().any(|prefix| key.starts_with(prefix.as_str())) {
                values.insert(key, Some(value));
            }
        }

        // Record the prefix itself so that `FOO_*` matching nothing differs
        // from not declaring it at all
        for prefix in prefixes {
            values.entry(format!("{}*", prefix)).or_insert(None);
        }
    }

    values
}

fn find_in_path(binary: &str) -> Option<Path> {
    let path_var
        = std::env::var_os("PATH")?;

    for folder in std::env::split_paths(&path_var) {
        let candidate
            = folder.join(binary);

        if candidate.is_file() {
            return Path::try_from(candidate).ok();
        }
    }

    None
}

/// Long-lived part of the task cache (one per daemon): the memoized file
/// hashes and node version.
pub struct TaskCache {
    pub project_cwd: Path,
    pub file_state: FileHashState,
    node_version: Mutex<Option<(Path, FileStamp, String)>>,
}

impl TaskCache {
    pub fn new(project: &Project) -> Self {
        Self {
            project_cwd: project.project_cwd.clone(),
            file_state: FileHashState::load(task_cache_state_path(project)),
            node_version: Mutex::new(None),
        }
    }

    /// Returns the version of the `node` binary the tasks would use. The
    /// result is memoized until the binary changes.
    fn node_version(&self) -> String {
        let Some(node_path) = find_in_path("node") else {
            return "<none>".to_string();
        };

        let real_path = node_path.fs_canonicalize()
            .unwrap_or(node_path);

        let Ok(metadata) = real_path.fs_metadata() else {
            return "<none>".to_string();
        };

        let stamp
            = FileStamp::from_metadata(&metadata);

        let mut memo
            = self.node_version.lock().unwrap();

        if let Some((known_path, known_stamp, version)) = memo.as_ref() {
            if *known_path == real_path && *known_stamp == stamp {
                return version.clone();
            }
        }

        let version = std::process::Command::new(real_path.to_path_buf())
            .arg("--version")
            .output()
            .ok()
            .filter(|output| output.status.success())
            .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
            // If we can't run it, its identity is the best we have
            .unwrap_or_else(|| format!("{}@{}", real_path.as_str(), stamp.mtime_ns));

        *memo = Some((real_path, stamp, version.clone()));

        version
    }

    /// Lists the files matching the output globs of a task (relative to
    /// its workspace).
    pub fn list_outputs(&self, info: &CacheTaskInfo) -> Result<Vec<String>, Error> {
        let outputs
            = PatternSet::new(&info.spec.outputs)?;

        if outputs.is_empty() {
            return Ok(vec![]);
        }

        if outputs.escapes_base() {
            return Err(Error::TaskCacheError(format!("{}: output globs must be within the workspace", info.task_id.to_file_string())));
        }

        list_files(&info.workspace_path, Some(&outputs), None, &WalkOptions {
            gitignore: false,
            ignored_folder_names: ALWAYS_IGNORED_FOLDERS,
            excluded_files: &[],
            excluded_folders: &info.nested_workspaces,
        })
    }

    fn list_inputs(&self, info: &CacheTaskInfo, options: &CacheRunOptions) -> Result<Vec<String>, Error> {
        let outputs
            = PatternSet::new(&info.spec.outputs)?;

        let mut excluded_folders
            = info.nested_workspaces.clone();

        excluded_folders.push(options.cache_folder.clone());

        let explicit_inputs = match &info.spec.inputs {
            Some(inputs) if !inputs.is_empty() || !info.spec.default_inputs => {
                Some(PatternSet::new(inputs)?)
            },

            _ => {
                None
            },
        };

        let use_default_inputs
            = info.spec.inputs.is_none() || info.spec.default_inputs;

        let mut files
            = Vec::new();

        if let Some(inputs) = &explicit_inputs {
            // Explicit inputs are taken verbatim; gitignore rules only
            // apply to the default input set
            files.extend(list_files(&info.workspace_path, Some(inputs), Some(&outputs), &WalkOptions {
                gitignore: false,
                ignored_folder_names: ALWAYS_IGNORED_FOLDERS,
                excluded_files: &[],
                excluded_folders: &excluded_folders,
            })?);
        }

        if use_default_inputs {
            // The task definition is part of the key already; no need to
            // invalidate everything when an unrelated task changes
            let default_files = list_files(&info.workspace_path, None, Some(&outputs), &WalkOptions {
                gitignore: true,
                ignored_folder_names: ALWAYS_IGNORED_FOLDERS,
                excluded_files: &[TASK_FILE_NAME],
                excluded_folders: &excluded_folders,
            })?;

            // Exclusions listed next to `@default` apply to it too
            files.extend(default_files.into_iter().filter(|file| {
                explicit_inputs.as_ref().map_or(true, |inputs| !inputs.is_excluded(file))
            }));

            files.sort();
            files.dedup();
        }

        // The manifest is always an input: its scripts may be called by
        // the task, and the dependencies it lists are what the tree hash
        // is computed from
        if !files.iter().any(|file| file == "package.json") {
            files.push("package.json".to_string());
            files.sort();
        }

        Ok(files)
    }

    /// Computes the fingerprint of a task given the fingerprints of all the
    /// tasks it depends on. Returns `None` when the task can't be cached in
    /// this run (for example if its dependency tree is unknown).
    pub fn compute_fingerprint(&self, info: &CacheTaskInfo, options: &CacheRunOptions, dependencies: &BTreeMap<String, Hash64>) -> Result<Option<Fingerprint>, Error> {
        let Some(tree_hash) = options.tree_hashes.get(&info.task_id.workspace) else {
            return Ok(None);
        };

        let mut details = FingerprintDetails {
            task: info.task_id.to_file_string(),
            ..Default::default()
        };

        let mut builder
            = HashBuilder::new();

        let mut component = |builder: &mut HashBuilder, label: &str, value: String| {
            builder.field(label, &value);
            details.components.insert(label.to_string(), value);
        };

        component(&mut builder, "format", CACHE_FORMAT.to_string());
        component(&mut builder, "yarnVersion", zpm_switch::get_bin_version());
        component(&mut builder, "platform", format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH));
        component(&mut builder, "nodeVersion", self.node_version());
        component(&mut builder, "task", info.task_id.to_file_string());
        component(&mut builder, "workspacePath", info.workspace_rel_path.to_file_string());
        component(&mut builder, "script", info.script.clone());
        component(&mut builder, "args", serde_json::to_string(&info.args).unwrap());
        component(&mut builder, "isCached", info.is_cached.to_string());
        component(&mut builder, "inputGlobs", serde_json::to_string(&info.spec.inputs).unwrap());
        component(&mut builder, "defaultInputs", info.spec.default_inputs.to_string());
        component(&mut builder, "outputGlobs", serde_json::to_string(&info.spec.outputs).unwrap());
        component(&mut builder, "envNames", serde_json::to_string(&info.spec.env).unwrap());
        component(&mut builder, "globalInputGlobs", serde_json::to_string(&options.global_inputs).unwrap());
        component(&mut builder, "treeHash", tree_hash.to_file_string());

        for (name, value) in collect_env(&info.spec.env) {
            builder.field("env", format!("{}={}", name, value.as_deref().map_or("\0unset", |v| v)));
            details.env.insert(name, env_value_hash(value.as_deref()));
        }

        let global_inputs
            = PatternSet::new(&options.global_inputs)?;

        if !global_inputs.is_empty() {
            let global_files = list_files(&self.project_cwd, Some(&global_inputs), None, &WalkOptions {
                gitignore: false,
                ignored_folder_names: ALWAYS_IGNORED_FOLDERS,
                excluded_files: &[],
                excluded_folders: &[],
            })?;

            for (rel_path, hashed) in self.file_state.hash_files(&self.project_cwd, global_files)? {
                let value
                    = format!("{}:{}", hashed.hash.to_file_string(), hashed.mode & 0o111 != 0);

                builder.field("globalInput", format!("{}\0{}", rel_path, value));
                details.global_inputs.insert(rel_path, hashed.hash.short());
            }
        }

        let input_files
            = self.list_inputs(info, options)?;

        for (rel_path, hashed) in self.file_state.hash_files(&info.workspace_path, input_files)? {
            let value
                = format!("{}:{}", hashed.hash.to_file_string(), hashed.mode & 0o111 != 0);

            builder.field("input", format!("{}\0{}", rel_path, value));
            details.inputs.insert(rel_path, hashed.hash.short());
        }

        for (task, hash) in dependencies {
            builder.field("dependency", format!("{}\0{}", task, hash.to_file_string()));
            details.dependencies.insert(task.clone(), hash.short());
        }

        let hash
            = builder.finalize();

        details.fingerprint = hash.to_file_string();

        Ok(Some(Fingerprint {hash, details}))
    }

    /// Fingerprint of a task without script (it only aggregates other
    /// tasks); no I/O involved.
    pub fn aggregate_fingerprint(&self, task_id: &TaskId, dependencies: &BTreeMap<String, Hash64>) -> Hash64 {
        let mut builder
            = HashBuilder::new();

        builder.field("format", CACHE_FORMAT);
        builder.field("aggregate", task_id.to_file_string());

        for (task, hash) in dependencies {
            builder.field("dependency", format!("{}\0{}", task, hash.to_file_string()));
        }

        builder.finalize()
    }

    pub fn read_entry(&self, options: &CacheRunOptions, fingerprint: &Hash64) -> Result<Option<CachedEntry>, Error> {
        store::read_entry(&options.cache_folder, fingerprint)
    }

    /// Restores the outputs of a cache entry, removing the stale ones
    /// first. Returns whether anything had to be written.
    pub fn restore(&self, info: &CacheTaskInfo, entry: &CachedEntry) -> Result<bool, Error> {
        let current
            = self.list_outputs(info)?;

        if entry.matches_disk(&info.workspace_path, &current, &self.file_state)? {
            return Ok(false);
        }

        entry.restore(&info.workspace_path, &current, &self.file_state)?;

        Ok(true)
    }

    pub fn store(&self, info: &CacheTaskInfo, options: &CacheRunOptions, fingerprint: &Hash64, logs: Vec<LogLine>) -> Result<u64, Error> {
        let output_files
            = self.list_outputs(info)?;

        let outputs
            = store::collect_outputs(&info.workspace_path, &output_files)?;

        let created_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let meta = EntryMeta {
            format: CACHE_FORMAT.to_string(),
            task: info.task_id.to_file_string(),
            fingerprint: fingerprint.clone(),
            created_at,
            logs,
            outputs: outputs.iter().map(|output| output.file.clone()).collect(),
        };

        store::write_entry(&options.cache_folder, &meta, outputs)
    }

    pub fn save_state(&self) {
        if let Err(err) = self.file_state.save_if_dirty() {
            eprintln!("Failed to save the task cache file state: {}", err);
        }
    }
}

pub fn task_cache_state_path(project: &Project) -> Path {
    project.ignore_path().with_join_str(STATE_FILE_NAME)
}

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct TreeHashesMemo {
    key: String,
    hashes: BTreeMap<String, String>,
}

/// Computes the dependency tree hash of each workspace. The result only
/// depends on the lockfile and the workspace manifests, so it's memoized on
/// disk keyed by the lockfile content and the manifests' mtimes.
pub fn compute_tree_hashes(project: &Project) -> Result<BTreeMap<Ident, Hash64>, Error> {
    let lockfile_path
        = project.lockfile_path();

    let Some(lockfile_data) = lockfile_path.fs_read().ok_missing()? else {
        // No lockfile means no dependencies at all; the manifests (always
        // part of the inputs) describe the whole tree
        return Ok(project.workspaces.iter()
            .map(|workspace| (workspace.name.clone(), Hash64::from_data(b"<no-lockfile>")))
            .collect());
    };

    let mut key_builder
        = HashBuilder::new();

    key_builder.field("format", CACHE_FORMAT);
    key_builder.field("yarnVersion", zpm_switch::get_bin_version());
    key_builder.field("lockfile", &lockfile_data);

    // The workspace dependencies come from the manifests rather than from
    // the lockfile, so they must invalidate the memo too
    for workspace in &project.workspaces {
        key_builder.field("workspace", format!("{}\0{}\0{}", workspace.name.to_file_string(), workspace.rel_path.to_file_string(), workspace.last_changed_at));
    }

    let key
        = key_builder.finalize().to_file_string();

    let memo_path
        = project.ignore_path().with_join_str(TREE_HASHES_FILE_NAME);

    let memo = memo_path.fs_read_text().ok()
        .and_then(|text| serde_json::from_str::<TreeHashesMemo>(&text).ok())
        .filter(|memo| memo.key == key);

    if let Some(memo) = memo {
        let hashes: Option<BTreeMap<Ident, Hash64>> = memo.hashes.iter()
            .map(|(ident, hash)| Some((Ident::new(ident), zpm_utils::FromFileString::from_file_string(hash).ok()?)))
            .collect();

        if let Some(hashes) = hashes {
            return Ok(hashes);
        }
    }

    let hashes
        = compute_workspace_tree_hashes(project, &project.lockfile()?);

    let memo = TreeHashesMemo {
        key,
        hashes: hashes.iter().map(|(ident, hash)| (ident.to_file_string(), hash.to_file_string())).collect(),
    };

    if let Ok(text) = serde_json::to_string(&memo) {
        let _ = memo_path.fs_create_parent()
            .and_then(|path| path.fs_write_atomic(|tmp_path| tmp_path.fs_write_text(&text).map(|_| ())));
    }

    Ok(hashes)
}

/// Returns the tasks whose fingerprint may be needed: the cached ones and
/// all the tasks they depend on (directly or not). Long-lived tasks never
/// have a fingerprint, which makes the tasks depending on them uncacheable.
pub fn tasks_needing_fingerprint<'a>(resolved: &'a zpm_tasks::ResolvedTasks, task_ids: impl IntoIterator<Item = &'a TaskId>) -> Result<BTreeSet<TaskId>, Error> {
    let mut needed
        = BTreeSet::new();

    for task_id in task_ids {
        let Some(task) = resolved.task_files.get(&task_id.workspace).and_then(|tf| tf.tasks.get(task_id.task_name.as_str())) else {
            continue;
        };

        let spec = task.cache_spec()
            .map_err(|err| Error::TaskCacheError(format!("{}: {}", task_id.to_file_string(), err)))?;

        if spec.is_some() {
            needed.insert(task_id.clone());
            needed.extend(resolved.tasks.get(task_id).into_iter().flatten().cloned());
        }
    }

    needed.retain(|task_id| {
        let task
            = resolved.task_files.get(&task_id.workspace).and_then(|tf| tf.tasks.get(task_id.task_name.as_str()));

        !task.map_or(false, |task| task.is_long_lived())
    });

    Ok(needed)
}

/// Orders the tasks so that each one comes after all its dependencies.
pub fn topological_order(tasks: &BTreeMap<TaskId, Vec<TaskId>>) -> Vec<TaskId> {
    // The prerequisite lists are transitive, so a task always has strictly
    // more prerequisites than any of its prerequisites
    let mut order: Vec<&TaskId>
        = tasks.keys().collect();

    order.sort_by_key(|task_id| (tasks[*task_id].len(), (*task_id).clone()));

    order.into_iter().cloned().collect()
}

pub fn dependency_fingerprints(prerequisites: &[TaskId], get: impl Fn(&TaskId) -> Option<Hash64>) -> Option<BTreeMap<String, Hash64>> {
    let mut fingerprints
        = BTreeMap::new();

    let unique: BTreeSet<&TaskId>
        = prerequisites.iter().collect();

    for prerequisite in unique {
        fingerprints.insert(prerequisite.to_file_string(), get(prerequisite)?);
    }

    Some(fingerprints)
}
