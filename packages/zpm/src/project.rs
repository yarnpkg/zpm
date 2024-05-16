use std::{collections::HashMap, sync::Arc};

use arca::{Path, ToArcaPath};

use crate::{cache::DiskCache, error::Error, lockfile::Lockfile, manifest::{read_manifest, Manifest}, primitives::{Descriptor, Ident, Locator, Range, Reference}};

static LOCKFILE_NAME: &str = "yarn.lock";
static MANIFEST_NAME: &str = "package.json";

pub struct Project {
    pub cwd: Path,
    pub root: Path,

    pub workspaces: HashMap<Ident, Workspace>,
}

impl Project {
    pub fn find_closest_project(mut p: Path) -> Result<Path, Error> {
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

    pub fn new(cwd: Option<Path>) -> Result<Project, Error> {
        let cwd = cwd
            .or_else(|| std::env::var("YARN_CWD").ok().map(|s| Path::from(s)))
            .or_else(|| std::env::current_dir().ok().map(|p| p.to_arca()));

        let cwd = cwd
            .expect("Failed to determine current working directory");

        let root = Project::find_closest_project(cwd.clone())
            .expect("Failed to find project root");

        let root_workspace = Workspace::from_path(root.clone())
            .expect("Failed to read root workspace");

        let mut workspaces: HashMap<_, _> = root_workspace
            .workspaces()?
            .into_iter()
            .map(|w| (w.locator().ident, w))
            .collect();

        workspaces.insert(
            root_workspace.locator().ident,
            root_workspace.clone(),
        );

        Ok(Project {
            cwd,
            root,
            workspaces,
        })
    }

    pub fn manifest_path(&self) -> Path {
        self.root.with_join_str(MANIFEST_NAME)
    }

    pub fn lockfile_path(&self) -> Path {
        self.root.with_join_str(LOCKFILE_NAME)
    }

    pub fn pnp_path(&self) -> Path {
        self.root.with_join_str(".pnp.cjs")
    }

    pub fn lockfile(&self) -> Result<Lockfile, Error> {
        let lockfile_path
            = self.root.with_join_str(LOCKFILE_NAME);

        if !lockfile_path.fs_exists() {
            return Ok(Lockfile::new());
        }

        let lockfile_path_buf =
            lockfile_path.to_path_buf();

        let src = std::fs::read_to_string(&lockfile_path_buf)
            .map_err(Arc::new)?;

        serde_json::from_str(&src)
            .map_err(|err| Error::LockfileParseError(Arc::new(err)))
    }

    pub fn write_lockfile(&self, lockfile: &Lockfile) -> Result<(), Error> {
        let lockfile_path
            = self.root.with_join_str(LOCKFILE_NAME);

        let lockfile_path_buf
            = lockfile_path.to_path_buf();

        let contents
            = serde_json::to_string_pretty(lockfile)
                .map_err(|err| Error::LockfileGenerationError(Arc::new(err)))?;

        let current_content = std::fs::read_to_string(&lockfile_path_buf);
        if let Ok(current_content) = current_content {
            if current_content == contents {
                return Ok(());
            }
        }

        std::fs::write(&lockfile_path_buf, contents)
            .map_err(|err| Error::LockfileWriteError(Arc::new(err)))
    }

    pub fn package_cache(&self) -> DiskCache {
        let cache_path
            = self.root.with_join_str("node_modules/.zpm");

        DiskCache::new(cache_path)
    }
}

#[derive(Debug, Clone)]
pub struct Workspace {
    pub path: Path,
    pub manifest: Manifest,
}

impl Workspace {
    pub fn from_path(path: Path) -> Result<Workspace, Error> {
        let manifest = read_manifest(&path.with_join_str(MANIFEST_NAME))?;

        Ok(Workspace {
            path,
            manifest,
        })
    }

    pub fn descriptor(&self) -> Descriptor {
        Descriptor::new(self.manifest.name.clone(), Range::WorkspaceMagic("^".to_string()))
    }

    pub fn locator(&self) -> Locator {
        Locator::new(self.manifest.name.clone(), Reference::Workspace(self.manifest.name.clone()))
    }

    pub fn workspaces(&self) -> Result<Vec<Workspace>, Error> {
        let mut workspaces = vec![];

        if let Some(patterns) = &self.manifest.workspaces {
            for pattern in patterns {
                if !pattern.ends_with("/*") {
                    return Err(Error::InvalidWorkspacePattern(pattern.to_string()));
                }

                let slice = &pattern[0..pattern.len() - 2];

                let entries = self.path.with_join_str(slice).to_path_buf().read_dir()
                    .map_err(Arc::new)?;

                for entry in entries {
                    let entry = entry
                        .map_err(Arc::new)?;

                    let entry_path = entry.file_name().into_string().unwrap();

                    let mut path = self.path.with_join_str(slice);
                    path.join_str(entry_path);

                    if path.with_join_str(MANIFEST_NAME).fs_is_file() {
                        workspaces.push(Workspace::from_path(path)?);
                    }
                }
            }
        }

        Ok(workspaces)
    }
}
