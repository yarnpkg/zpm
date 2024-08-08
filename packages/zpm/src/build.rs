use std::collections::{HashMap, HashSet};

use arca::Path;
use futures::{future::BoxFuture, stream::FuturesUnordered, StreamExt};

use crate::{error, primitives::Locator, project::Project, script::ScriptEnvironment};

#[derive(Debug, Clone)]
pub enum Command {
    Program(String, Vec<String>),
    Script(String),
}

#[derive(Debug, Clone)]
pub struct Entry {
    pub cwd: Path,
    pub locator: Locator,
    pub commands: Vec<Command>,
}

impl Entry {
    async fn run_impl(&self, project: &Project) -> error::Result<i32> {
        let mut script_env = ScriptEnvironment::new()
            .with_project(&project)
            .with_package(&project, &self.locator)?
            .with_cwd(self.cwd.clone());

        for command in self.commands.iter() {
            match command {
                Command::Program(program, args) => {
                    script_env.run_exec(program, args).await;
                },
    
                Command::Script(script) => {
                    script_env.run_script(script).await;
                },
            }
        }

        Ok(0)
    }

    pub async fn run(self, idx: usize, context: &Project) -> (usize, error::Result<i32>) {
        (idx, self.run_impl(context).await)
    }
}

pub struct Build {
    pub entries: Vec<Entry>,
    pub dependencies: HashMap<usize, HashSet<usize>>,
}

pub struct BuildManager<'a> {
    pub build: Build,
    pub dependents: HashMap<usize, HashSet<usize>>,
    pub queued: Vec<usize>,
    pub running: FuturesUnordered<BoxFuture<'a, (usize, error::Result<i32>)>>,
}

impl<'a> BuildManager<'a> {
    pub fn new(build: Build) -> Self {
        let mut dependents = HashMap::new();
        dependents.reserve(build.entries.len());

        for (idx, set) in build.dependencies.iter() {
            for &dep_idx in set.iter() {
                dependents.entry(dep_idx)
                    .or_insert_with(HashSet::new)
                    .insert(*idx);
            }
        }

        Self {
            build,
            dependents,
            queued: Vec::new(),
            running: FuturesUnordered::new(),
        }
    }

    fn trigger(&mut self, project: &'a Project) {
        while self.running.len() < 100 {
            if let Some(idx) = self.queued.pop() {
                let entry
                    = self.build.entries[idx].clone();

                let future
                    = entry.run(idx, project);

                self.running.push(Box::pin(future));
            } else {
                break;
            }
        }
    }

    pub async fn run(mut self, project: &'a Project) -> error::Result<()> {
        let mut errors
            = HashSet::new();

        for idx in 0..self.build.entries.len() {
            if let Some(set) = self.build.dependencies.get(&idx) {
                if !set.is_empty() {
                    continue;
                }
            }

            self.queued.push(idx);
        }

        self.trigger(project);

        while let Some((idx, result)) = self.running.next().await {
            let entry = &self.build.entries[idx];

            match result {
                Ok(exit_code) => {
                    if exit_code != 0 {
                        errors.insert(entry.locator.clone());
                    } else {
                        if let Some(dependents) = self.dependents.get_mut(&idx) {
                            for &dependent_idx in dependents.iter() {
                                let dependencies
                                    = self.build.dependencies.get_mut(&dependent_idx)
                                        .expect("Expected this package to have dependencies, since it's listed as a dependent");

                                dependencies.remove(&idx);

                                if dependencies.is_empty() {
                                    self.queued.push(dependent_idx);
                                }
                            }
                        }
                    }
                }

                Err(err) => {
                    println!("Error: {:?}", err);
                    errors.insert(entry.locator.clone());
                }
            }

            self.trigger(project);
        }

        errors.is_empty()
            .then(|| ())
            .ok_or(error::Error::BuildScriptsFailedToRun)
    }
}
