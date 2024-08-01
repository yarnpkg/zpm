use std::{collections::{BTreeMap, HashMap}, sync::Arc};

use arca::{Path, ToArcaPath};
use serde::Deserialize;
use serde_with::serde_as;
use wax::walk::Entry;

use crate::{cache::{CompositeCache, DiskCache}, config::Config, error::Error, install::InstallState, lockfile::Lockfile, manifest::{read_manifest, Manifest}, primitives::{Descriptor, Ident, Locator, Range, Reference}, zip::ZipSupport};

static LOCKFILE_NAME: &str = "yarn.lock";
static MANIFEST_NAME: &str = "package.json";
static INSTALL_STATE_PATH: &str = ".yarn/install-state.json";

pub struct Project {
    pub project_cwd: Path,
    pub package_cwd: Path,
    pub shell_cwd: Path,

    pub config: Config,
    pub workspaces: HashMap<Ident, Workspace>,

    pub install_state: Option<InstallState>,
}

impl Project {
    pub fn find_closest_project(mut p: Path) -> Result<(Path, Path), Error> {
        let mut closest_pkg = None;

        loop {
            let lock_p = p.with_join_str(LOCKFILE_NAME);
            if lock_p.fs_exists() {
                return Ok((p.clone(), closest_pkg.unwrap_or(p)));
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
    
        let closest_pkg = closest_pkg
            .ok_or(Error::ProjectNotFound(p))?;

        Ok((closest_pkg.clone(), closest_pkg))
    }

    pub fn new(cwd: Option<Path>) -> Result<Project, Error> {
        let shell_cwd = cwd
            .or_else(|| std::env::var("YARN_CWD").ok().map(|s| Path::from(s)))
            .or_else(|| std::env::current_dir().ok().map(|p| p.to_arca()))
            .expect("Failed to determine current working directory");

        let (project_cwd, package_cwd) = Project::find_closest_project(shell_cwd.clone())
            .expect("Failed to find project root");

        let config = Config::new(Some(project_cwd.clone()));

        let root_workspace = Workspace::from_path(&project_cwd, project_cwd.clone())
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
            shell_cwd: shell_cwd.relative_to(&project_cwd),
            package_cwd: package_cwd.relative_to(&project_cwd),
            project_cwd,

            config,
            workspaces,

            install_state: None,
        })
    }

    pub fn manifest_path(&self) -> Path {
        self.project_cwd.with_join_str(MANIFEST_NAME)
    }

    pub fn lockfile_path(&self) -> Path {
        self.project_cwd.with_join_str(LOCKFILE_NAME)
    }

    pub fn pnp_path(&self) -> Path {
        self.project_cwd.with_join_str(".pnp.cjs")
    }

    pub fn pnp_data_path(&self) -> Path {
        self.project_cwd.with_join_str(".pnp.data.json")
    }

    pub fn pnp_loader_path(&self) -> Path {
        self.project_cwd.with_join_str(".pnp.loader.mjs")
    }

    pub fn nm_path(&self) -> Path {
        self.project_cwd.with_join_str("node_modules")
    }

    pub fn lockfile(&self) -> Result<Lockfile, Error> {
        let lockfile_path
            = self.project_cwd.with_join_str(LOCKFILE_NAME);

        if !lockfile_path.fs_exists() {
            return Ok(Lockfile::new());
        }

        let src = lockfile_path
            .fs_read_text()
            .map_err(Arc::new)?;

        if src.is_empty() {
            return Ok(Lockfile::new());
        }

        serde_json::from_str(&src)
            .map_err(|err| Error::LockfileParseError(Arc::new(err)))
    }

    pub fn import_install_state(&mut self) -> Result<&mut Self, Error> {
        let install_state_path
            = self.project_cwd.with_join_str(INSTALL_STATE_PATH);

        if !install_state_path.fs_exists() {
            return Err(Error::InstallStateNotFound);
        }

        let src = install_state_path
            .fs_read_text()
            .map_err(Arc::new)?;

        self.install_state = serde_json::from_str(&src)
            .map_err(|err| Error::InvalidJsonData(Arc::new(err)))?;

        Ok(self)
    }

    pub fn attach_install_state(&mut self, install_state: InstallState) -> Result<(), Error> {
        self.write_install_state(&install_state)?;
        self.write_lockfile(&install_state.lockfile)?;

        self.install_state = Some(install_state);

        Ok(())
    }

    fn write_install_state(&mut self, install_state: &InstallState) -> Result<(), Error> {
            let link_info_path
            = self.project_cwd.with_join_str(INSTALL_STATE_PATH);

        let contents
            = serde_json::to_string(&install_state)
                .map_err(|err| Error::LockfileGenerationError(Arc::new(err)))?;

        crate::misc::change_file(link_info_path.to_path_buf(), contents, 0o644)
            .map_err(|err| Error::LockfileWriteError(Arc::new(err)))?;

        Ok(())
    }

    fn write_lockfile(&self, lockfile: &Lockfile) -> Result<(), Error> {
        let lockfile_path
            = self.project_cwd.with_join_str(LOCKFILE_NAME);

        let contents
            = serde_json::to_string_pretty(lockfile)
                .map_err(|err| Error::LockfileGenerationError(Arc::new(err)))?;

        crate::misc::change_file(lockfile_path.to_path_buf(), contents, 0o644)
            .map_err(|err| Error::LockfileWriteError(Arc::new(err)))
    }

    pub fn package_cache(&self) -> CompositeCache {
        let global_cache_path = self.config.project.global_folder.value
            .with_join_str("cache");

        let local_cache_path
            = self.project_cwd.with_join_str(".yarn/cache");

        let global_cache
            = Some(DiskCache::new(global_cache_path));

        let local_cache = (!self.config.project.enable_global_cache.value)
            .then(|| DiskCache::new(local_cache_path));

        CompositeCache {
            global_cache,
            local_cache,
        }
    }

    pub fn active_package(&self) -> Result<Locator, Error> {
        let install_state = self.install_state.as_ref()
            .ok_or(Error::InstallStateNotFound)?;

        let active_package = install_state.packages_by_location.get(&self.package_cwd)
            .ok_or(Error::ActivePackageNotFound)?;

        Ok(active_package.clone())
    }

    pub fn package_self_binaries(&self, locator: &Locator) -> Result<Vec<(String, Path)>, Error> {
        let install_state = self.install_state.as_ref()
            .ok_or(Error::InstallStateNotFound)?;

        let location = install_state.locations_by_package.get(locator)
            .expect("Expected locator to have a location");

        #[serde_as]
        #[derive(Debug, Clone, Deserialize)]
        #[serde(untagged)]
        enum BinField {
            String(Path),
            Map(#[serde_as(as = "HashMap<_, _>")] Vec<(String, Path)>),
        }

        #[derive(Debug, Clone, Deserialize)]
        struct BinManifest {
            pub name: Option<Ident>,
            pub bin: Option<BinField>,
        }

        let manifest_text = location
            .with_join_str(MANIFEST_NAME)
            .fs_read_text_with_zip()?;

        let manifest = serde_json::from_str::<BinManifest>(&manifest_text)
            .map_err(|err| Error::InvalidJsonData(Arc::new(err)))?;

        Ok(match manifest.bin {
            Some(BinField::String(bin)) => {
                if let Some(name) = manifest.name {
                    vec![(name.name().to_string(), location.with_join(&bin))]
                } else {
                    vec![]
                }
            }

            Some(BinField::Map(bins)) => bins
                .iter()
                .map(|(name, path)| (name.clone(), location.with_join(path)))
                .collect(),

            None => vec![]
        })
    }

    pub fn package_visible_binaries(&self, locator: &Locator) -> Result<BTreeMap<String, Path>, Error> {
        let install_state = self.install_state.as_ref()
            .ok_or(Error::InstallStateNotFound)?;

        let resolution = install_state.resolution_tree.locator_resolutions.get(locator)
            .expect("Expected active package to have a resolution tree");

        let mut all_bins = HashMap::new();

        for descriptor in resolution.dependencies.values() {
            let locator = install_state.resolution_tree.descriptor_to_locator.get(descriptor)
                .expect("Expected resolution to be found in the resolution tree");

            all_bins.extend(self.package_self_binaries(locator)?);
        }

        all_bins.extend(self.package_self_binaries(locator)?);

        Ok(BTreeMap::from_iter(all_bins.into_iter()))
    }

    pub fn find_binary(&self, name: &str) -> Result<Path, Error> {
        let active_package = self.active_package()?;

        self.package_visible_binaries(&active_package)?
            .remove(name)
            .ok_or(Error::BinaryNotFound(name.to_string()))
    }

    pub fn find_script(&self, name: &str) -> Result<(Locator, String), Error> {
        let active_package = self.active_package()?;

        #[derive(Debug, Clone, Deserialize)]
        struct ScriptManifest {
            pub scripts: Option<HashMap<String, String>>,
        }

        let manifest_text = self.package_cwd
            .with_join_str(MANIFEST_NAME)
            .fs_read_text_with_zip()?;

        let manifest = serde_json::from_str::<ScriptManifest>(&manifest_text)
            .map_err(|err| Error::InvalidJsonData(Arc::new(err)))?;

        if let Some(script) = manifest.scripts.as_ref().and_then(|s| s.get(name)) {
            return Ok((active_package.clone(), script.clone()));
        }

        if !name.contains(':') {
            return Err(Error::ScriptNotFound(name.to_string()));
        }

        let mut iterator = self.workspaces.values();

        let script_match = iterator
            .find_map(|w| w.manifest.scripts.as_ref().and_then(|s| s.get(name).map(|s| (w.locator(), s.clone()))));

        if script_match.is_none() {
            return Err(Error::GlobalScriptNotFound(name.to_string()));
        }

        if iterator.any(|w| w.manifest.scripts.as_ref().map(|s| s.contains_key(name)).unwrap_or(false)) {
            return Err(Error::AmbiguousScriptName(name.to_string()));
        }

        Ok(script_match.unwrap())
    }
}

#[derive(Debug, Clone)]
pub struct Workspace {
    pub name: Ident,
    pub path: Path,
    pub rel_path: Path,
    pub manifest: Manifest,
}

impl Workspace {
    pub fn from_path(root: &Path, path: Path) -> Result<Workspace, Error> {
        let manifest = read_manifest(&path.with_join_str(MANIFEST_NAME))?;

        let name = manifest.name.clone().unwrap_or_else(|| {
            Ident::new(if root == &path {
                "root-workspace".to_string()
            } else {
                path.basename().map_or_else(|| "unnamed-workspace".to_string(), |b| b.to_string())
            })
        });

        let rel_path = path
            .relative_to(&root);

        Ok(Workspace {
            name,
            path,
            rel_path,
            manifest,
        })
    }

    pub fn descriptor(&self) -> Descriptor {
        Descriptor::new(self.name.clone(), Range::WorkspaceMagic("^".to_string()))
    }

    pub fn locator(&self) -> Locator {
        Locator::new(self.name.clone(), Reference::Workspace(self.rel_path.to_string()))
    }

    pub fn workspaces(&self) -> Result<Vec<Workspace>, Error> {
        let mut workspaces = vec![];

        if let Some(patterns) = &self.manifest.workspaces {
            for pattern in patterns {
                let glob = wax::Glob::new(&pattern)
                    .map_err(|_| Error::InvalidWorkspacePattern(pattern.to_string()))?;

                for entry in glob.walk(self.path.to_path_buf()) {
                    let path = entry
                        .unwrap()
                        .path()
                        .to_arca();

                    if path.with_join_str(MANIFEST_NAME).fs_is_file() {
                        workspaces.push(Workspace::from_path(&self.path, path)?);
                    }
                }
            }
        }

        Ok(workspaces)
    }
}
