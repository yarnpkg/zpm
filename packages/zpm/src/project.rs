use std::{collections::{BTreeMap, HashMap}, fs::Permissions, os::unix::fs::PermissionsExt, sync::Arc};

use arca::{Path, ToArcaPath};
use serde::Deserialize;
use wax::walk::{Entry, FileIterator};
use zpm_macros::track_time;

use crate::{cache::{CompositeCache, DiskCache}, config::Config, error::Error, formats::zip::ZipSupport, install::{InstallContext, InstallManager, InstallState}, lockfile::Lockfile, manifest::{read_manifest, BinField, BinManifest, Manifest}, primitives::{Descriptor, Ident, Locator, Range, Reference}, script::Binary};

pub const LOCKFILE_NAME: &str = "yarn.lock";
pub const MANIFEST_NAME: &str = "package.json";
pub const PNP_CJS_NAME: &str = ".pnp.cjs";
pub const PNP_ESM_NAME: &str = ".pnp.loader.mjs";
pub const PNP_DATA_NAME: &str = ".pnp.data.json";

pub struct Project {
    pub project_cwd: Path,
    pub package_cwd: Path,
    pub shell_cwd: Path,

    pub config: Config,
    pub workspaces: HashMap<Ident, Workspace>,
    pub workspaces_by_rel_path: HashMap<Path, Ident>,

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
            .map(Ok)
            .unwrap_or_else(|| std::env::current_dir().map(|p| p.to_arca()))?;

        let (project_cwd, package_cwd)
            = Project::find_closest_project(shell_cwd.clone())?;

        let config = Config::new(Some(project_cwd.clone()));

        let root_workspace
            = Workspace::from_path(&project_cwd, project_cwd.clone())?;

        let mut workspaces: HashMap<_, _> = root_workspace
            .workspaces()?
            .into_iter()
            .map(|w| (w.locator().ident, w))
            .collect();

        workspaces.insert(
            root_workspace.locator().ident,
            root_workspace.clone(),
        );

        let workspaces_by_rel_path = workspaces.values()
            .map(|w| (w.rel_path.clone(), w.locator().ident))
            .collect::<HashMap<_, _>>();

        Ok(Project {
            shell_cwd: shell_cwd.relative_to(&project_cwd),
            package_cwd: package_cwd.relative_to(&project_cwd),
            project_cwd,

            config,
            workspaces,
            workspaces_by_rel_path,

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
        self.project_cwd.with_join_str(PNP_CJS_NAME)
    }

    pub fn pnp_data_path(&self) -> Path {
        self.project_cwd.with_join_str(PNP_DATA_NAME)
    }

    pub fn pnp_loader_path(&self) -> Path {
        self.project_cwd.with_join_str(PNP_ESM_NAME)
    }

    pub fn nm_path(&self) -> Path {
        self.project_cwd.with_join_str("node_modules")
    }

    pub fn install_state_path(&self) -> Path {
        self.project_cwd.with_join_str(".yarn/install-state.json")
    }

    pub fn build_state_path(&self) -> Path {
        self.project_cwd.with_join_str(".yarn/build-state.json")
    }

    pub fn lockfile(&self) -> Result<Lockfile, Error> {
        let lockfile_path
            = self.project_cwd.with_join_str(LOCKFILE_NAME);

        if !lockfile_path.fs_exists() {
            return Ok(Lockfile::new());
        }

        let src = lockfile_path
            .fs_read_text()?;

        if src.is_empty() {
            return Ok(Lockfile::new());
        }

        serde_json::from_str(&src)
            .map_err(|err| Error::LockfileParseError(Arc::new(err)))
    }

    #[track_time]
    pub fn import_install_state(&mut self) -> Result<&mut Self, Error> {
        let install_state_path
            = self.install_state_path();

        if !install_state_path.fs_exists() {
            return Err(Error::InstallStateNotFound);
        }

        let src = install_state_path
            .fs_read_text()?;

        self.install_state = serde_json::from_str(&src)?;

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
            = self.install_state_path();

        let contents
            = serde_json::to_string(&install_state)?;

        let re_parsed: InstallState
            = serde_json::from_str(&contents)?;

        if re_parsed != *install_state {
            // We'll print all fields' equality checks here
            println!("install_state.lockfile == re_parsed.lockfile: {}", install_state.lockfile == re_parsed.lockfile);
            println!("install_state.resolution_tree == re_parsed.resolution_tree: {}", install_state.resolution_tree == re_parsed.resolution_tree);
            println!("install_state.packages_by_location == re_parsed.packages_by_location: {}", install_state.packages_by_location == re_parsed.packages_by_location);
            println!("install_state.locations_by_package == re_parsed.locations_by_package: {}", install_state.locations_by_package == re_parsed.locations_by_package);
            println!("install_state.optional_packages == re_parsed.optional_packages: {}", install_state.optional_packages == re_parsed.optional_packages);
            println!("install_state.disabled_locators == re_parsed.disabled_locators: {}", install_state.disabled_locators == re_parsed.disabled_locators);
            println!("install_state.conditional_locators == re_parsed.conditional_locators: {}", install_state.conditional_locators == re_parsed.conditional_locators);

            println!("Pretty lockfile: {:#?} {:#?}", install_state.lockfile, re_parsed.lockfile);
            println!("Pretty resolution tree: {:#?} {:#?}", install_state.resolution_tree, re_parsed.resolution_tree);
        }

        assert_eq!(&re_parsed, install_state);

        link_info_path
            .fs_create_parent()?
            .fs_change(contents, Permissions::from_mode(0o644))?;

        Ok(())
    }

    fn write_lockfile(&self, lockfile: &Lockfile) -> Result<(), Error> {
        let lockfile_path
            = self.project_cwd.with_join_str(LOCKFILE_NAME);

        let contents
            = serde_json::to_string_pretty(lockfile)
                .map_err(|err| Error::LockfileGenerationError(Arc::new(err)))?;

        lockfile_path
            .fs_change(contents, Permissions::from_mode(0o644))?;

        Ok(())
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

    pub fn active_workspace(&self) -> Result<&Workspace, Error> {
        let active_package = self.active_package()?;

        match &active_package.reference {
            Reference::Workspace(ident) => self.workspaces.get(&ident)
                .ok_or(Error::WorkspaceNotFound(ident.clone())),

            _ => {
                Err(Error::ActivePackageNotWorkspace)
            },
        }
    }

    pub fn package_self_binaries(&self, locator: &Locator) -> Result<BTreeMap<String, Binary>, Error> {
        let install_state = self.install_state.as_ref()
            .ok_or(Error::InstallStateNotFound)?;

        let location = install_state.locations_by_package.get(locator)
            .expect("Expected locator to have a location");

        let manifest_text = self.project_cwd
            .with_join(location)
            .with_join_str(MANIFEST_NAME)
            .fs_read_text_with_zip()?;

        let manifest = serde_json::from_str::<BinManifest>(&manifest_text)
            .map_err(|err| Error::InvalidJsonData(Arc::new(err)))?;

        Ok(match manifest.bin {
            Some(BinField::String(bin)) => {
                if let Some(name) = manifest.name {
                    BTreeMap::from_iter([(name.name().to_string(), Binary::new(self, location.with_join(&bin)))])
                } else {
                    BTreeMap::new()
                }
            }

            Some(BinField::Map(bins)) => bins
                .into_iter()
                .map(|(name, path)| (name, Binary::new(self, location.with_join(&path))))
                .collect(),

            None => BTreeMap::new(),
        })
    }

    #[track_time]
    pub fn package_visible_binaries(&self, locator: &Locator) -> Result<BTreeMap<String, Binary>, Error> {
        let install_state = self.install_state.as_ref()
            .ok_or(Error::InstallStateNotFound)?;

        let resolution = install_state.resolution_tree.locator_resolutions.get(locator)
            .expect("Expected active package to have a resolution tree");

        let mut all_bins = HashMap::new();

        for descriptor in resolution.dependencies.values() {
            let locator = install_state.resolution_tree.descriptor_to_locator.get(descriptor)
                .expect("Expected resolution to be found in the resolution tree");

            // Packages may be missing from locations_by_package when they
            // haven't been installed due to being unsupported on the current
            // platform. In this case, we ignore its binaries.
            //
            if install_state.locations_by_package.contains_key(locator) {
                all_bins.extend(self.package_self_binaries(locator)?);
            }
        }

        all_bins.extend(self.package_self_binaries(locator)?);

        Ok(BTreeMap::from_iter(all_bins.into_iter()))
    }

    pub fn find_binary(&self, name: &str) -> Result<Binary, Error> {
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

        let manifest_text = self.project_cwd
            .with_join(&self.package_cwd)
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
            .find_map(|w| w.manifest.scripts.get(name).map(|s| (w.locator(), s.clone())));

        if script_match.is_none() {
            return Err(Error::GlobalScriptNotFound(name.to_string()));
        }

        if iterator.any(|w| w.manifest.scripts.contains_key(name)) {
            return Err(Error::AmbiguousScriptName(name.to_string()));
        }

        Ok(script_match.unwrap())
    }

    pub async fn run_install(&mut self) -> Result<(), Error> {
        let package_cache
            = self.package_cache();

        let install_context = InstallContext::default()
            .with_package_cache(Some(&package_cache))
            .with_project(Some(self));

        let mut lockfile = self.lockfile()?;
        lockfile.forget_transient_resolutions();

        InstallManager::new()
            .with_context(install_context)
            .with_lockfile(lockfile)
            .with_roots_iter(self.workspaces.values().map(|w| w.descriptor()))
            .resolve_and_fetch().await?
            .finalize(self).await?;

        Ok(())
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
            .relative_to(root);

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
        Locator::new(self.name.clone(), Reference::Workspace(self.name.clone()))
    }

    pub fn workspaces(&self) -> Result<Vec<Workspace>, Error> {
        let mut workspaces = vec![];

        if let Some(patterns) = &self.manifest.workspaces {
            let (negated_patterns, regular_patterns): (Vec<_>, Vec<_>) = patterns.iter()
                .partition(|p| p.starts_with("!"));

            let regular_globs = regular_patterns.iter()
                .map(|p| wax::Glob::new(p).map_err(|_| Error::InvalidWorkspacePattern(p.to_string())))
                .collect::<Result<Vec<_>, _>>()?;

            let negated_globs = negated_patterns.iter()
                .map(|p| wax::Glob::new(&p[1..]).map_err(|_| Error::InvalidWorkspacePattern(p.to_string())))
                .collect::<Result<Vec<_>, _>>()?;

            let aggregated_negated_glob
                = wax::any(negated_globs)
                    .unwrap();

            for glob in regular_globs {
                let it = glob
                    .walk(self.path.to_path_buf())
                    .not(aggregated_negated_glob.clone())
                    .unwrap();

                for entry in it {
                    let path = entry
                        .unwrap()
                        .path()
                        .to_arca();

                    if path.with_join_str(MANIFEST_NAME).fs_is_file() {
                        workspaces.push(Workspace::from_path(&self.path, path)?);
                    }
                }
            }

            // for pattern in patterns {
            //     let glob = wax::Glob::new(pattern)
            //         .map_err(|_| Error::InvalidWorkspacePattern(pattern.to_string()))?;

            //     for entry in glob.walk(self.path.to_path_buf()) {
            //         let path = entry
            //             .unwrap()
            //             .path()
            //             .to_arca();

            //         if path.with_join_str(MANIFEST_NAME).fs_is_file() {
            //             workspaces.push(Workspace::from_path(&self.path, path)?);
            //         }
            //     }
            // }
        }

        Ok(workspaces)
    }

    pub fn write_manifest(&self) -> Result<(), Error> {
        let serialized = serde_json::to_string_pretty(&self.manifest)?;

        self.path
            .with_join_str(MANIFEST_NAME)
            .fs_change(serialized, Permissions::from_mode(0o644))?;

        Ok(())
    }
}
