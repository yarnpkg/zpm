use std::{collections::{BTreeMap, BTreeSet}, fs::Permissions, os::unix::fs::PermissionsExt};

use zpm_utils::{OkMissing, Path};
use bincode::{Decode, Encode};
use futures::{future::BoxFuture, stream::FuturesUnordered, FutureExt, StreamExt};
use serde::{Deserialize, Serialize};
use sha2::Digest;
use zpm_utils::ToFileString;

use crate::{diff_finder::{DiffController, DiffFinder}, error::Error, hash::Blake2b80, primitives::Locator, project::Project, report::{with_context_result, ReportContext}, script::{ScriptEnvironment, ScriptResult}, tree_resolver::ResolutionTree};

#[derive(Clone, Debug, Decode, Encode, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type")]
pub enum Command {
    Program {
        name: String,
        args: Vec<String>
    },

    Script {
        script: String,
    },
}

pub struct ArtifactFinder;

impl DiffController for ArtifactFinder {
    type Data = ();

    fn get_file_data(_path: &Path, _metadata: &std::fs::Metadata) -> Result<Self::Data, Error> {
        Ok(())
    }

    fn is_relevant_entry(entry: &std::fs::DirEntry, file_type: &std::fs::FileType) -> bool {
        if file_type.is_dir() {
            return entry.file_name() != "node_modules";
        }

        file_type.is_file()
    }
}

#[derive(Debug, Clone)]
pub struct BuildRequest {
    pub cwd: Path,
    pub locator: Locator,
    pub tree_hash: String,
    pub commands: Vec<Command>,
    pub allowed_to_fail: bool,
    pub force_rebuild: bool,
}

impl BuildRequest {
    pub async fn run(self, project: &Project) -> Result<ScriptResult, Error> {
        let cwd_abs = project.project_cwd
            .with_join(&self.cwd);

        let mut script_env = ScriptEnvironment::new()?
            .with_project(project)
            .with_package(project, &self.locator)?
            .with_env_variable("INIT_CWD", cwd_abs.as_str())
            .with_cwd(cwd_abs.clone());

        with_context_result(ReportContext::Locator(self.locator.clone()), async {
            let build_cache_folder = project.project_cwd
                .with_join_str(".yarn/ignore/builds")
                .with_join_str(format!("{}-{}", self.locator.slug(), self.tree_hash));

            let mut artifact_finder
                = DiffFinder::<ArtifactFinder>::new(cwd_abs, Default::default())?;

            artifact_finder.rsync()?;

            for command in self.commands.iter() {
                let script_result = match command {
                    Command::Program {name, args} =>
                        script_env.run_exec(name, args).await,
                    Command::Script {script} =>
                        script_env.run_script(script, Vec::<&str>::new()).await,
                };

                if !script_result.success() {
                    return match self.allowed_to_fail {
                        true => Ok(ScriptResult::new_success()),
                        false => Err(script_result.ok().unwrap_err()),
                    };
                }
            }

            let (_has_changed, diff_list)
                = artifact_finder.rsync()?;

            build_cache_folder
                .fs_rm()
                .ok_missing()?;

            build_cache_folder
                .fs_create_parent()?
                .fs_write_text(format!("{:#?}", diff_list))?;

            Ok(ScriptResult::new_success())
        }).await
    }

    pub fn key(&self) -> (Locator, Path) {
        (self.locator.clone(), self.cwd.clone())
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
    pub tree_hashes: BTreeMap<Locator, String>,
    pub queued: Vec<usize>,
    pub running: FuturesUnordered<BoxFuture<'a, (usize, Option<String>, Result<ScriptResult, Error>)>>,
    pub build_errors: BTreeSet<(Locator, Path)>,
    pub build_state_out: BTreeMap<Path, String>,
}

impl<'a> BuildManager<'a> {
    pub fn new(requests: BuildRequests) -> Self {
        let mut dependents = BTreeMap::new();

        for (idx, set) in requests.dependencies.iter() {
            for &dep_idx in set.iter() {
                dependents.entry(dep_idx)
                    .or_insert_with(BTreeSet::new)
                    .insert(*idx);
            }
        }

        Self {
            requests,
            dependents,
            tree_hashes: BTreeMap::new(),
            queued: Vec::new(),
            running: FuturesUnordered::new(),
            build_errors: BTreeSet::new(),
            build_state_out: BTreeMap::new(),
        }
    }

    fn find_acyclic_locators(&self, project: &'a Project, root: &Locator) -> Vec<Locator> {
        struct TraversalState<'a> {
            resolution_tree: &'a ResolutionTree,
            visited: BTreeMap<&'a Locator, VisitationState>,
            in_cycle: BTreeSet<&'a Locator>,
            result: Vec<&'a Locator>,
            stack: Vec<&'a Locator>,
        }

        enum VisitationState {
            Visiting,
            Visited,
        }
        
        fn dfs<'a>(
            traversal_state: &mut TraversalState<'a>,
            node: &'a Locator,
        ) {
            if let Some(visitation_state) = traversal_state.visited.get(node) {
                match visitation_state {
                    VisitationState::Visiting => {
                        // Detected a cycle
                        if let Some(pos) = traversal_state.stack.iter().position(|&n| n == node) {
                            for &n in &traversal_state.stack[pos..] {
                                traversal_state.in_cycle.insert(n);
                            }
                        }
                    }
                    VisitationState::Visited => (),
                }
            } else {
                traversal_state.visited.insert(node, VisitationState::Visiting);
                traversal_state.stack.push(node);
        
                let resolution = traversal_state.resolution_tree.locator_resolutions.get(node)
                    .expect("Expected package to have a resolution");

                for dependency in resolution.dependencies.values() {
                    let dependency_locator = traversal_state.resolution_tree.descriptor_to_locator.get(dependency)
                        .expect("Expected dependency to have a locator");

                    dfs(traversal_state, dependency_locator);
                }
        
                traversal_state.visited.insert(node, VisitationState::Visited);
                traversal_state.stack.pop();
        
                if !traversal_state.in_cycle.contains(node) {
                    traversal_state.result.push(node);
                }
            }
        }

        let install_state = project.install_state.as_ref()
            .expect("Expected the install state to be present");

        let mut state = TraversalState {
            resolution_tree: &install_state.resolution_tree,
            visited: BTreeMap::new(),
            in_cycle: BTreeSet::new(),
            result: Vec::new(),
            stack: Vec::new(),
        };
    
        dfs(&mut state, root);
        state.result.into_iter()
            .map(|l| (*l).clone())
            .collect::<Vec<_>>()
    }

    fn record(&mut self, idx: usize, hash: Option<String>, script_result: ScriptResult) {
        let request = &self.requests.entries[idx];

        if !script_result.success() {
            self.build_errors.insert(request.key());
        } else {
            if let Some(hash) = hash {
                self.build_state_out.insert(request.cwd.clone(), hash);
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

    fn trigger(&mut self, project: &'a Project, build_state: &BTreeMap<Path, String>) {
        while self.running.len() < 5 {
            if let Some(idx) = self.queued.pop() {
                let req
                    = self.requests.entries[idx].clone();

                let hash
                    = self.get_hash(project, &req.locator);

                if hash.is_none() {
                    println!("Package {} was queued for build but is the root of a dependency cycle; it'll always be rebuilt", req.locator.to_file_string());
                }

                if let Some(hash) = &hash {
                    if build_state.get(&req.cwd) == Some(hash) && !req.force_rebuild {
                        self.record(idx, Some(hash.to_string()), ScriptResult::new_success());
                        continue;
                    }
                }

                self.build_state_out.remove(&req.cwd);

                let future = req.run(project).map(move |res| {
                    (idx, hash, res)
                });

                self.running.push(Box::pin(future));
            } else {
                break;
            }
        }
    }

    fn get_hash(&mut self, project: &'a Project, locator: &Locator) -> Option<String> {
        let install_state = project.install_state.as_ref()
            .expect("Expected the install state to be present");

        let acyclic_locators = self.find_acyclic_locators(project, locator);

        let locators_to_hash = acyclic_locators.iter()
            .filter(|l| !self.tree_hashes.contains_key(l))
            .cloned()
            .collect::<Vec<_>>();

        for locator in locators_to_hash.iter() {
            let mut hasher
                = Blake2b80::new();

            let resolution = install_state.resolution_tree.locator_resolutions.get(locator)
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
    }

    pub async fn run(mut self, project: &'a mut Project) -> Result<Build, Error> {
        let build_state_path = project
            .build_state_path();

        let build_state_text_in = build_state_path
            .fs_read_text()
            .unwrap_or_else(|_| "{}".to_string());

        let paths_to_build = self.requests.entries.iter()
            .map(|req| req.cwd.clone())
            .collect::<BTreeSet<_>>();

        let build_state_in =
            sonic_rs::from_str::<BTreeMap<Path, String>>(&build_state_text_in)?;

        self.build_state_out = build_state_in.iter()
            .filter(|(p, _)| paths_to_build.contains(p))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect::<BTreeMap<_, _>>();

        for idx in 0..self.requests.entries.len() {
            if let Some(set) = self.requests.dependencies.get(&idx) {
                if !set.is_empty() {
                    continue;
                }
            }

            self.queued.push(idx);
        }

        self.trigger(project, &build_state_in);
        
        let mut current_build_state_out = self.build_state_out.clone();

        while let Some((idx, hash, result)) = self.running.next().await {
            let request
                = &self.requests.entries[idx];

            match result {
                Ok(exit_status) => {
                    self.record(idx, hash, exit_status);
                }

                Err(_) => {
                    self.build_errors.insert(request.key());
                }
            }

            self.trigger(project, &build_state_in);

            if current_build_state_out != self.build_state_out {
                let build_state_text_out
                    = sonic_rs::to_string(&self.build_state_out)?;

                build_state_path
                    .fs_change(build_state_text_out, Permissions::from_mode(0o644))?;

                current_build_state_out = self.build_state_out.clone();
            }
        }

        Ok(Build {
            build_errors: self.build_errors,
        })
    }
}
