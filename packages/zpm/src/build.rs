use std::collections::{BTreeMap, BTreeSet};

use zpm_parsers::JsonDocument;
use zpm_primitives::Locator;
use zpm_utils::{CollectHash, Hash64, IoResultExt, Path, ToFileString, ToHumanString};
use rkyv::Archive;
use futures::{future::BoxFuture, stream::FuturesUnordered, FutureExt, StreamExt};
use serde::{ser::SerializeMap, Deserialize, Deserializer, Serialize};

use crate::{
    algos,
    diff_finder::{DiffController, DiffFinder},
    error::Error,
    project::Project,
    report::{with_context_result, ReportContext},
    script::{ScriptEnvironment, ScriptResult},
};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Archive, rkyv::Serialize, rkyv::Deserialize)]
#[serde(tag = "type")]
pub enum Command {
    Program {
        name: String,
        args: Vec<String>
    },

    Script {
        event: Option<String>,
        script: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BuildSkip {
    Disabled,
    Incompatible,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BuildStep {
    Skip(BuildSkip),
    Commands(Vec<Command>),
}

impl BuildStep {
    pub fn is_noop(&self) -> bool {
        matches!(self, Self::Commands(commands) if commands.is_empty())
    }
}

pub struct ArtifactFinder;

impl DiffController for ArtifactFinder {
    type Data = ();

    fn get_file_data(_path: &Path, _metadata: &std::fs::Metadata) -> Result<Self::Data, Error> {
        Ok(())
    }

    fn is_relevant_entry(file_name: &str, file_type: &std::fs::FileType) -> bool {
        if file_type.is_dir() {
            return file_name != "node_modules";
        }

        file_type.is_file()
    }
}

#[derive(Debug, Clone)]
pub struct BuildRequest {
    pub cwd: Path,
    pub locator: Locator,
    pub build_step: BuildStep,
    pub allowed_to_fail: bool,
    pub force_rebuild: bool,
    pub inline_builds: bool,
}

impl BuildRequest {
    fn persists_build_state(&self) -> bool {
        matches!(self.build_step, BuildStep::Commands(_) | BuildStep::Skip(BuildSkip::Disabled))
    }

    pub async fn run(self, project: &Project, hash: Hash64) -> Result<ScriptResult, Error> {
        let commands = match self.build_step {
            BuildStep::Skip(BuildSkip::Incompatible) => {
                return Ok(ScriptResult::new_success());
            },

            BuildStep::Skip(BuildSkip::Disabled) => {
                crate::report::if_active(|report| {
                    report.warn(format!(
                        "{} lists build scripts, but its build has been explicitly disabled through configuration.",
                        self.locator.to_print_string(),
                    ));
                });

                return Ok(ScriptResult::new_success());
            },

            BuildStep::Commands(commands) => commands,
        };

        let cwd_abs = project.project_cwd
            .with_join(&self.cwd);

        let mut script_env = ScriptEnvironment::new()?
            .with_project(project)
            .enable_trust_check()
            .with_package(project, &self.locator)?
            .with_env_variable("INIT_CWD", cwd_abs.as_str())
            .with_cwd(cwd_abs.clone());

        crate::report::if_active(|report| {
            report.info(format!(
                "{} must be built because it never has been before",
                self.locator.to_print_string(),
            ));
        });

        let inline_builds = self.inline_builds;
        let locator = self.locator.clone();

        let res = with_context_result(ReportContext::Locator(self.locator.clone()), async {
            let build_cache_folder = if self.locator.reference.is_disk_reference() {
                None
            } else {
                let build_cache_folder = project.project_cwd
                    .with_join_str(".yarn/ignore/builds")
                    .with_join_str(format!("{}-{}", self.locator.slug(), hash.short()));

                Some(build_cache_folder)
            };

            let roots
                = vec![Path::new()];

            let mut artifact_finder
                = DiffFinder::<ArtifactFinder>::new(cwd_abs, roots, Default::default());

            if build_cache_folder.is_some() {
                artifact_finder.rsync()?;
            }

            let mut combined_stdout = Vec::<u8>::new();
            let mut combined_stderr = Vec::<u8>::new();

            for command in commands.iter() {
                let script_result = match command {
                    Command::Program {name, args} => {
                        script_env.run_exec(name, args).await?
                    },

                    Command::Script {event, script} => {
                        if let Some(event) = event {
                            script_env = script_env
                                .with_env_variable("npm_lifecycle_event", event);
                        }

                        script_env.run_script(script, Vec::<&str>::new()).await?
                    },
                };

                if inline_builds {
                    match &script_result {
                        ScriptResult::Success(output) | ScriptResult::Failure(output, _, _) => {
                            combined_stdout.extend_from_slice(&output.stdout);
                            combined_stderr.extend_from_slice(&output.stderr);
                        },
                    }
                }

                if !script_result.success() {
                    return match self.allowed_to_fail {
                        true => {
                            // Error is suppressed; emit the captured
                            // log explicitly so inline-builds output
                            // isn't lost.
                            if inline_builds {
                                emit_success_log(&locator, &combined_stdout, &combined_stderr);
                            }
                            Ok(ScriptResult::new_success())
                        },

                        false => {
                            let script_result = if inline_builds {
                                match script_result {
                                    ScriptResult::Failure(mut output, program, shell_line) => {
                                        output.stdout = combined_stdout;
                                        output.stderr = combined_stderr;
                                        ScriptResult::Failure(output, program, shell_line)
                                    },

                                    ScriptResult::Success(_) => unreachable!(),
                                }
                            } else {
                                script_result
                            };

                            // `script_result.ok()` writes the captured log
                            // itself; emitting a success log here would print
                            // it twice.
                            Err(script_result.ok().unwrap_err())
                        },
                    };
                }
            }

            if inline_builds {
                emit_success_log(&locator, &combined_stdout, &combined_stderr);
            }

            if let Some(build_cache_folder) = build_cache_folder {
                let (_has_changed, diff_list)
                    = artifact_finder.rsync()?;

                build_cache_folder
                    .fs_rm()
                    .ok_missing()?;

                build_cache_folder
                    .fs_create_parent()?
                    .fs_write_text(format!("{:#?}", diff_list))?;
            }

            Ok(ScriptResult::new_success())
        }).await?;

        Ok(res)
    }

    pub fn key(&self) -> (Locator, Path) {
        (self.locator.clone(), self.cwd.clone())
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BuildState {
    pub entries: BTreeMap<Locator, BTreeMap<Path, Hash64>>,
}

impl<'de> Deserialize<'de> for BuildState {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error> where D: Deserializer<'de> {
        let map
            = BTreeMap::deserialize(deserializer)?;

        Ok(Self {
            entries: map,
        })
    }
}

impl Serialize for BuildState {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error> where S: serde::Serializer {
        let mut map
            = serializer.serialize_map(None)?;

        for (locator, paths) in &self.entries {
            if !paths.is_empty() {
                map.serialize_entry(locator, paths)?;
            }
        }

        map.end()
    }
}

impl BuildState {
    pub fn from_entries(entries: BTreeMap<Locator, BTreeMap<Path, Hash64>>) -> Self {
        Self { entries }
    }

    pub async fn load(project: &Project) -> Self {
        let build_state_path = project
            .build_state_path();

        let build_state_text = build_state_path
            .fs_read_text_async()
            .await
            .unwrap_or_else(|_| "{}".to_owned());

        JsonDocument::hydrate_from_str::<Self>(&build_state_text)
            .expect("Failed to parse the build state")
    }

    pub fn save(&self, project: &Project) -> Result<(), Error> {
        let build_state_path = project
            .build_state_path();

        build_state_path
            .fs_create_parent()?;

        let build_state_text
            = JsonDocument::to_string(self)?;

        build_state_path
            .fs_change(build_state_text, false)?;

        Ok(())
    }
}

#[derive(Debug)]
pub struct BuildRequests {
    pub entries: Vec<BuildRequest>,
    pub dependencies: BTreeMap<usize, BTreeSet<usize>>,
}

pub struct Build {
    pub build_errors: BTreeSet<(Locator, Path)>,
}

pub struct BuildManager<'a> {
    pub requests: BuildRequests,
    pub dependents: BTreeMap<usize, BTreeSet<usize>>,
    pub tree_hashes: BTreeMap<Locator, Hash64>,
    pub queued: Vec<usize>,
    pub running: FuturesUnordered<BoxFuture<'a, (usize, Hash64, Result<ScriptResult, Error>)>>,
    pub build_errors: BTreeSet<(Locator, Path)>,
    pub build_state_out: BuildState,
}

impl<'a> BuildManager<'a> {
    pub fn new(requests: BuildRequests) -> Self {
        let mut dependents
            = BTreeMap::new();

        let sccs
            = algos::scc_tarjan_pearce(&requests.dependencies);

        for scc in sccs {
            let dependencies = scc.iter()
                .flat_map(|&idx| requests.dependencies.get(&idx).unwrap().iter())
                .filter(|&dep_idx| !scc.contains(dep_idx))
                .cloned()
                .collect::<BTreeSet<_>>();

            for &idx in scc.iter() {
                for &dependency in dependencies.iter() {
                    dependents.entry(dependency)
                        .or_insert_with(BTreeSet::new)
                        .insert(idx);
                }
            }
        }

        Self {
            requests,
            dependents,
            tree_hashes: BTreeMap::new(),
            queued: Vec::new(),
            running: FuturesUnordered::new(),
            build_errors: BTreeSet::new(),
            build_state_out: BuildState::default(),
        }
    }

    fn record(&mut self, idx: usize, hash: Hash64, script_result: ScriptResult) {
        let request
            = &self.requests.entries[idx];

        if !script_result.success() {
            if self.build_errors.insert(request.key()) {
                emit_build_failure_warning(&request.locator);
            }
        } else {
            if request.persists_build_state() {
                self.build_state_out.entries.entry(request.locator.clone())
                    .or_insert_with(BTreeMap::new)
                    .insert(request.cwd.clone(), hash);
            }

            if let Some(dependents) = self.dependents.get_mut(&idx) {
                for &dependent_idx in dependents.iter() {
                    let dependencies
                        = self.requests.dependencies.get_mut(&dependent_idx)
                            .expect("Expected this package to have dependencies, since it's listed as a dependent");

                    dependencies.remove(&idx);

                    if dependencies.is_empty() {
                        self.queued.push(dependent_idx);
                    }
                }
            }
        }
    }

    fn trigger(&mut self, project: &'a Project, build_state: &BuildState) {
        while self.running.len() < 5 {
            if let Some(idx) = self.queued.pop() {
                let req
                    = self.requests.entries[idx].clone();

                let force_rebuild
                    = req.force_rebuild;

                let tree_hash
                    = self.get_hash(project, &req.locator);

                if req.persists_build_state() && !force_rebuild {
                    let existing_hash = build_state.entries
                        .get(&req.locator)
                        .and_then(|entries| entries.get(&req.cwd));

                    if existing_hash == Some(&tree_hash) {
                        self.record(idx, tree_hash.clone(), ScriptResult::new_success());
                        continue;
                    }
                }

                self.build_state_out.entries.get_mut(&req.locator)
                    .and_then(|entries| entries.remove(&req.cwd));

                let future
                    = req.run(project, tree_hash.clone())
                        .map(move |res| (idx, tree_hash, res));

                self.running.push(Box::pin(future));
            } else {
                break;
            }
        }
    }

    fn get_hash_impl<'b>(tree_hashes: &'b mut BTreeMap<Locator, Hash64>, project: &'a Project, locator: &Locator) -> Hash64 {
        let hash
            = tree_hashes.get(locator);

        if let Some(hash) = hash {
            return hash.clone();
        }

        // To avoid the case where one dependency depends on itself somehow
        tree_hashes.insert(locator.clone(), Hash64::from_string(&"<recursive>"));

        let install_state = project.install_state.as_ref()
            .expect("Expected the install state to be present");

        let resolution = install_state.resolution_tree.locator_resolutions.get(locator)
            .expect("Expected package to have a resolution");

        let self_hash
            = Hash64::from_string(&locator.to_file_string());

        let hashes = resolution.dependencies.values()
            .map(|descriptor| &install_state.resolution_tree.descriptor_to_locator[descriptor])
            .map(|dependency| BuildManager::get_hash_impl(tree_hashes, project, dependency))
            .chain(Some(self_hash))
            .collect::<Vec<_>>();

        let hash = hashes.iter()
            .collect_hash();

        tree_hashes.insert(locator.clone(), hash.clone());

        hash
    }

    fn get_hash(&mut self, project: &'a Project, locator: &Locator) -> Hash64 {
        BuildManager::get_hash_impl(&mut self.tree_hashes, project, locator)
    }

    pub async fn run(mut self, project: &'a mut Project) -> Result<Build, Error> {
        let locators_to_build = self.requests.entries.iter()
            .map(|req| req.locator.clone())
            .collect::<BTreeSet<_>>();

        let build_state_in =
            BuildState::load(&project).await;

        self.build_state_out = BuildState::from_entries(
            build_state_in.entries.iter()
                .filter(|(l, _)| locators_to_build.contains(l))
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect()
        );

        for idx in 0..self.requests.entries.len() {
            if let Some(set) = self.requests.dependencies.get(&idx) {
                if !set.is_empty() {
                    continue;
                }
            }

            self.queued.push(idx);
        }

        self.trigger(project, &build_state_in);

        let mut current_build_state_out
            = self.build_state_out.clone();

        while let Some((idx, hash, result)) = self.running.next().await {
            let request
                = &self.requests.entries[idx];

            match result {
                Ok(exit_status) => {
                    self.record(idx, hash, exit_status);
                }

                Err(_) => {
                    if self.build_errors.insert(request.key()) {
                        emit_build_failure_warning(&request.locator);
                    }
                }
            }

            self.trigger(project, &build_state_in);

            if current_build_state_out != self.build_state_out {
                self.build_state_out
                    .save(&project)?;

                current_build_state_out = self.build_state_out.clone();
            }
        }

        Ok(Build {
            build_errors: self.build_errors,
        })
    }
}

fn emit_build_failure_warning(locator: &Locator) {
    crate::report::if_active(|report| {
        report.warn(format!(
            "{} couldn't be built successfully; please check the logs above for more information.",
            locator.to_print_string(),
        ));
    });
}

/// Dumps the build's stdout/stderr to a temp log and surfaces it via
/// the active report. Mirrors the failure-path `ChildProcessFailedWithLog`
/// flow so `--inline-builds` reads the same for successful builds.
fn emit_success_log(locator: &Locator, stdout: &[u8], stderr: &[u8]) {
    let Ok(temp_dir) = Path::temp_dir() else {
        return;
    };

    let log_path = temp_dir
        .with_join_str(format!("{}-build.log", locator.slug()));

    let body = format!(
        "=== BUILD {} ===\n\n=== STDOUT ===\n\n{}\n=== STDERR ===\n\n{}",
        locator.to_file_string(),
        String::from_utf8_lossy(stdout),
        String::from_utf8_lossy(stderr),
    );

    if log_path.fs_write_text(&body).is_err() {
        return;
    }

    crate::report::if_active(|report| {
        report.add_log_file(log_path);
    });
}
