use std::collections::{HashMap, HashSet};

use arca::Path;
use futures::{future::BoxFuture, stream::FuturesUnordered, FutureExt, StreamExt};
use sha2::Digest;

use crate::{error, hash::Blake2b80, misc::change_file, primitives::Locator, project::Project, script::ScriptEnvironment};

#[derive(Debug, Clone)]
pub enum Command {
    Program(String, Vec<String>),
    Script(String),
}

#[derive(Debug, Clone)]
pub struct BuildRequest {
    pub cwd: Path,
    pub locator: Locator,
    pub commands: Vec<Command>,
    pub allowed_to_fail: bool,
}

impl BuildRequest {
    pub async fn run(self, project: &Project) -> error::Result<i32> {
        let cwd_abs = project.project_cwd
            .with_join(&self.cwd);

        let mut script_env = ScriptEnvironment::new()
            .with_project(&project)
            .with_package(&project, &self.locator)?
            .with_cwd(cwd_abs);

        for command in self.commands.iter() {
            let exit_code = match command {
                Command::Program(program, args) =>
                    script_env.run_exec(program, args).await,
                Command::Script(script) =>
                    script_env.run_script(script, Vec::<&str>::new()).await,
            };

            if exit_code != 0 {
                return Ok(match self.allowed_to_fail {
                    true => 0,
                    false => exit_code,
                });
            }
        }

        Ok(0)
    }

    pub fn key(&self) -> (Locator, Path) {
        (self.locator.clone(), self.cwd.clone())
    }
}

#[derive(Debug)]
pub struct BuildRequests {
    pub entries: Vec<BuildRequest>,
    pub dependencies: HashMap<usize, HashSet<usize>>,
}

pub struct Build {
    pub build_errors: HashSet<(Locator, Path)>,
}

pub struct BuildManager<'a> {
    pub requests: BuildRequests,
    pub dependents: HashMap<usize, HashSet<usize>>,
    pub tree_hashes: HashMap<Locator, String>,
    pub queued: Vec<usize>,
    pub running: FuturesUnordered<BoxFuture<'a, (usize, String, error::Result<i32>)>>,
    pub build_errors: HashSet<(Locator, Path)>,
    pub build_state_out: HashMap<Path, String>,
}

impl<'a> BuildManager<'a> {
    pub fn new(requests: BuildRequests) -> Self {
        let mut dependents = HashMap::new();
        dependents.reserve(requests.entries.len());

        for (idx, set) in requests.dependencies.iter() {
            for &dep_idx in set.iter() {
                dependents.entry(dep_idx)
                    .or_insert_with(HashSet::new)
                    .insert(*idx);
            }
        }

        Self {
            requests,
            dependents,
            tree_hashes: HashMap::new(),
            queued: Vec::new(),
            running: FuturesUnordered::new(),
            build_errors: HashSet::new(),
            build_state_out: HashMap::new(),
        }
    }

    fn record(&mut self, idx: usize, hash: String, exit_code: i32) {
        let request = &self.requests.entries[idx];

        if exit_code != 0 {
            self.build_errors.insert(request.key());
        } else {
            self.build_state_out.insert(request.cwd.clone(), hash.to_string());

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

    fn trigger(&mut self, project: &'a Project, build_state: &HashMap<Path, String>) {
        while self.running.len() < 100 {
            if let Some(idx) = self.queued.pop() {
                let req
                    = self.requests.entries[idx].clone();

                let hash
                    = self.get_hash(project, &req.locator);

                if build_state.get(&req.cwd) == Some(&hash) {
                    self.record(idx, hash, 0);
                    continue;
                }

                let future = req.run(project).map(move |res| {
                    (idx, hash, res)
                });

                self.running.push(Box::pin(future));
            } else {
                break;
            }
        }
    }

    fn get_hash(&mut self, project: &'a Project, locator: &Locator) -> String {
        let install_state = project.install_state.as_ref()
            .expect("Expected the install state to be present");

        let mut traversal_queue = vec![locator.clone()];
        let mut dependencies_to_hash = vec![locator.clone()];

        // First we locate all dependencies that haven't been hashed yet
        while let Some(locator) = traversal_queue.pop() {
            let resolution = install_state.resolution_tree.locator_resolutions.get(&locator)
                .expect("Expected package to have a resolution");

            for dependency in resolution.dependencies.values() {
                let dependency_locator = install_state.resolution_tree.descriptor_to_locator.get(dependency)
                    .expect("Expected dependency to have a locator");

                if !self.tree_hashes.contains_key(&dependency_locator) {
                    traversal_queue.push(dependency_locator.clone());
                }
            }

            dependencies_to_hash.push(locator);
        }

        // We can reverse the list; since we know there are no cycles, we are
        // guaranteed that all dependencies will be hashed before their dependents
        dependencies_to_hash.reverse();

        for locator in dependencies_to_hash.iter() {
            let mut hasher
                = Blake2b80::new();

            let resolution = install_state.resolution_tree.locator_resolutions.get(&locator)
                .expect("Expected package to have a resolution");

            for dependency in resolution.dependencies.values() {
                let dependency_locator = install_state.resolution_tree.descriptor_to_locator.get(dependency)
                    .expect("Expected dependency to have a locator");

                let hash = self.tree_hashes.get(dependency_locator)
                    .cloned()
                    .unwrap_or_else(|| "".to_string());

                hasher.update(hash);
            }

            let hash = format!("{:x}", hasher.finalize());
            self.tree_hashes.insert(locator.clone(), hash);
        }

        self.tree_hashes.get(locator)
            .cloned()
            .unwrap()
    }

    pub async fn run(mut self, project: &'a mut Project) -> error::Result<Build> {
        let build_state_path = project
            .build_state_path();

        let build_state_text_in = build_state_path
            .fs_read_text()
            .unwrap_or_else(|_| "{}".to_string());

        let build_state_in =
            serde_json::from_str::<HashMap<Path, String>>(&build_state_text_in)?;

        for idx in 0..self.requests.entries.len() {
            if let Some(set) = self.requests.dependencies.get(&idx) {
                if !set.is_empty() {
                    continue;
                }
            }

            self.queued.push(idx);
        }

        self.trigger(project, &build_state_in);

        while let Some((idx, hash, result)) = self.running.next().await {
            let request
                = &self.requests.entries[idx];

            match result {
                Ok(exit_code) => {
                    self.record(idx, hash, exit_code);
                }

                Err(_) => {
                    self.build_errors.insert(request.key());
                }
            }

            self.trigger(project, &build_state_in);
        }

        let build_state_text_out =
            serde_json::to_string(&self.build_state_out)?;

        change_file(&build_state_path.to_path_buf(), &build_state_text_out, 0o644)?;

        Ok(Build {
            build_errors: self.build_errors,
        })
    }
}
