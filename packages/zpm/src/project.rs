use std::{collections::{HashMap, HashSet}, sync::Arc};

use arca::Path;
use cached::proc_macro::once;
use futures::{stream::FuturesUnordered, StreamExt};
use zpm_macros::track_time;

use crate::{error::Error, fetcher::PackageData, hash::Sha256, lockfile::{self, Lockfile}, manifest::{read_manifest, Manifest}, primitives::{Descriptor, Ident, Locator, Range, Reference}, resolver::Resolution, tree_resolver::TreeResolver};

static LOCKFILE_NAME: &str = "yarn.lock";
static MANIFEST_NAME: &str = "package.json";

#[once]
pub fn current_dir() -> Result<Path, Error> {
    let args: Vec<String> = std::env::args().collect();
    Ok(Path::from(&args[1]))
}

pub fn root() -> Result<Path, Error> {
    let mut p = current_dir()?;
    let mut closest_pkg = None;

    loop {
        let lock_p = p.with_join_str(LOCKFILE_NAME);
        if lock_p.fs_exists() {
            return Ok(p);
        }
    
        let pkg_p = p.with_join_str(MANIFEST_NAME);
        if pkg_p.fs_exists() {
            closest_pkg = Some(p.clone());
        }

        if let Some(dirname) = p.dirname() {
            p = dirname;
        } else {
            break
        }
    }

    closest_pkg
        .ok_or(Error::ProjectNotFound(p))
}

#[once]
pub fn workspace_paths() -> Result<Vec<Path>, Error> {
    let project_root = root()?;

    let mut workspaces = vec![project_root.clone()];

    let root_manifest = read_manifest(&project_root.with_join_str(MANIFEST_NAME))?;
    if let Some(patterns) = root_manifest.workspaces {
        for pattern in patterns {
            if !pattern.ends_with("/*") {
                return Err(Error::InvalidWorkspacePattern(pattern.to_string()));
            }

            let slice = &pattern[0..pattern.len() - 2];

            let entries = project_root.with_join_str(slice).to_path_buf().read_dir()
                .map_err(Arc::new)?;

            for entry in entries {
                let entry = entry
                    .map_err(Arc::new)?;

                let entry_path = entry.file_name().into_string().unwrap();

                let mut workspace_p = project_root.with_join_str(slice);
                workspace_p.join_str(entry_path);

                if workspace_p.with_join_str(MANIFEST_NAME).fs_is_file() {
                    workspaces.push(workspace_p);
                }
            }
        }

        workspaces.sort();
    }

    Ok(workspaces)
}

#[derive(Debug, Clone)]
pub struct Workspace {
    pub path: Path,
    pub manifest: Manifest,
}

impl Workspace {
    pub fn descriptor(&self) -> Descriptor {
        Descriptor::new(self.manifest.name.clone(), Range::WorkspaceMagic("^".to_string()))
    }

    pub fn locator(&self) -> Locator {
        Locator::new(self.manifest.name.clone(), Reference::Workspace(self.manifest.name.clone()))
    }
}

#[track_time]
#[once]
pub fn workspaces() -> Result<HashMap<Ident, Workspace>, Error> {
    let mut manifests = HashMap::new();

    for workspace_p in workspace_paths()? {
        let manifest = read_manifest(&workspace_p.with_join_str(MANIFEST_NAME))?;

        manifests.insert(manifest.name.clone(), Workspace {
            path: workspace_p,
            manifest,
        });
    }

    Ok(manifests)
}

#[track_time]
#[once]
pub fn top_level_dependencies() -> Result<HashSet<Descriptor>, Error> {
    let mut all_dependencies = HashSet::new();

    for workspace in workspaces()?.into_values() {
        all_dependencies.insert(workspace.descriptor());

        if let Some(dependencies) = workspace.manifest.dependencies {
            all_dependencies.extend(dependencies.into_values());
        }

        if let Some(dev_dependencies) = workspace.manifest.dev_dependencies {
            all_dependencies.extend(dev_dependencies.into_values());
        }
    }

    Ok(all_dependencies)
}

#[track_time]
#[once]
pub fn lockfile() -> Result<Lockfile, Error> {
    let lockfile_path = root()?.with_join_str(LOCKFILE_NAME);

    if !lockfile_path.fs_exists() {
        return Ok(Lockfile::new());
    }

    let src = std::fs::read_to_string(lockfile_path.to_path_buf())
        .map_err(Arc::new)?;

    Ok(lockfile::deserialize(&src).unwrap_or_default())
}

struct ResolutionManager {
    pub resolutions: HashMap<Descriptor, Resolution>,

    lockfile: Lockfile,
    seen: HashSet<Descriptor>,
    queue: Vec<Descriptor>,
}

impl ResolutionManager {
    pub fn new() -> Self {
        ResolutionManager {
            resolutions: HashMap::new(),

            lockfile: lockfile().unwrap(),
            seen: HashSet::new(),
            queue: Vec::new(),
        }
    }

    pub fn schedule(&mut self, descriptor: Descriptor) {
        if !self.seen.insert(descriptor.clone()) {
            return;
        }

        if let Some(locator) = self.lockfile.resolutions.remove(&descriptor) {
            let entry = self.lockfile.entries.get(&locator)
                .expect("Expected a matching resolution to be found in the lockfile for any resolved locator.");

            self.insert(descriptor, entry.resolution.clone());
        } else {
            self.queue.push(descriptor);
        }
    }

    pub fn insert(&mut self, descriptor: Descriptor, resolution: Resolution) {
        let transitive_dependencies = resolution.dependencies
            .values()
            .cloned();

        for descriptor in transitive_dependencies {
            self.schedule(descriptor);
        }

        self.resolutions.insert(descriptor, resolution);
    }

    pub async fn run(&mut self) {
        let mut wait = FuturesUnordered::new();

        let mut resolution_limit = 5000;
        let mut total_resolutions = 0;
    
        while wait.len() < resolution_limit {
            if let Some(descriptor) = self.queue.pop() {
                wait.push(descriptor.resolve_with_descriptor());
            } else {
                break;
            }
        }
    
        while let Some((descriptor, result)) = wait.next().await {
            total_resolutions = total_resolutions + 1;
    
            match result {
                Ok(resolution) => {
                    self.insert(descriptor, resolution)
                }
                
                Err(err) => {
                    if let Error::RemoteRegistryError(err) = &err {
                        if err.is_connect() {
                            self.queue.push(descriptor);
                            resolution_limit -= 1;
    
                            continue;
                        }
                    }
    
                    println!("{} - resolve failed: {:?}", descriptor, err)
                }
            }
    
            while wait.len() < resolution_limit {
                if let Some(descriptor) = self.queue.pop() {
                    wait.push(descriptor.resolve_with_descriptor());
                } else {
                    break;
                }
            }
        }
    }
}

#[track_time]
#[once]
pub async fn resolutions() -> Result<HashMap<Descriptor, Resolution>, Error> {
    let mut manager = ResolutionManager::new();

    for descriptor in top_level_dependencies()? {
        manager.schedule(descriptor);
    }

    manager.run().await;

    Ok(manager.resolutions)
}

#[track_time]
#[once]
pub async fn resolution_checksums() -> Result<HashMap<Locator, Option<Sha256>>, Error> {
    let cache = cache().await?;

    let mut checksum_map = HashMap::new();

    for (locator, data) in cache {
        checksum_map.insert(locator, data.checksum());
    }

    Ok(checksum_map)
}

pub async fn persist_lockfile() -> Result<(), Error> {
    let lockfile_path = root()?.with_join_str(LOCKFILE_NAME);

    let resolutions
        = resolutions().await?;
    let checksums
        = resolution_checksums().await?;

    let mut entries = HashMap::new();

    for (descriptor, resolution) in resolutions {
        let locator = resolution.locator.clone();

        let entry = lockfile::LockfileEntry {
            resolution,
            checksum: checksums.get(&locator).unwrap().clone(),
        };

        entries.insert(descriptor, entry);
    }

    let serialized
        = lockfile::serialize(entries)?;

    std::fs::write(lockfile_path.to_path_buf(), serialized)
        .map_err(Arc::new)?;

    Ok(())
}

#[track_time]
#[once]
pub async fn cache() -> Result<HashMap<Locator, PackageData>, Error> {
    let mut queue: Vec<_> = resolutions().await?
        .into_values()
        .map(|resolution| resolution.locator)
        .collect();
    
    let mut wait = FuturesUnordered::new();

    let mut fetch_limit = 5000;

    let mut cache = HashMap::new();

    let fetch_with_locator = |locator: Locator| async move {
        (locator.clone(), locator.fetch().await)
    };

    while wait.len() < fetch_limit {
        if let Some(locator) = queue.pop() {
            wait.push(fetch_with_locator(locator));
        } else {
            break;
        }
    }

    while let Some((locator, result)) = wait.next().await {
        match result {
            Ok(data) => {
                cache.insert(locator, data);
            }

            Err(err) => {
                if let Error::RemoteRegistryError(err) = &err {
                    if err.is_connect() {
                        queue.push(locator);
                        fetch_limit -= 1;

                        continue;
                    }
                }

                if let Error::IoError(err) = &err {
                    if err.raw_os_error() == Some(24) {
                        queue.push(locator);
                        fetch_limit -= 1;

                        continue;
                    }
                }

                println!("{} - fetch failed: {:?}", locator, err)
            }
        }

        if let Some(locator) = queue.pop() {
            wait.push(fetch_with_locator(locator));
        }
    }

    Ok(cache)
}

#[track_time]
#[once]
pub async fn tree() -> Result<TreeResolver, Error> {
    let root_descriptors: Vec<_> = workspaces()?.values()
        .map(|w| w.descriptor())
        .collect();

    Ok(TreeResolver::new(
        resolutions().await?,
        root_descriptors,
    ))
}

