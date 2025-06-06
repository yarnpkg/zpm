use std::{collections::{BTreeMap, BTreeSet}, fs::Permissions, io::ErrorKind, os::unix::fs::PermissionsExt, sync::Arc, time::UNIX_EPOCH};

use zpm_utils::Path;
use globset::GlobBuilder;
use serde::Deserialize;
use zpm_formats::zip::ZipSupport;
use zpm_macros::track_time;

use crate::{cache::{CompositeCache, DiskCache}, config::Config, diff_finder::SaveEntry, error::Error, install::{InstallContext, InstallManager, InstallState}, lockfile::{from_legacy_berry_lockfile, Lockfile}, manifest::{bin::BinField, helpers::read_manifest_with_size, resolutions::ResolutionSelector, BinManifest, Manifest}, manifest_finder::CachedManifestFinder, primitives::{range, reference, Descriptor, Ident, Locator, Range, Reference}, report::{with_report_result, StreamReport, StreamReportConfig}, script::Binary};

pub const LOCKFILE_NAME: &str = "yarn.lock";
pub const MANIFEST_NAME: &str = "package.json";
pub const PNP_CJS_NAME: &str = ".pnp.cjs";
pub const PNP_ESM_NAME: &str = ".pnp.loader.mjs";
pub const PNP_DATA_NAME: &str = ".pnp.data.json";

#[derive(Default)]
pub struct RunInstallOptions {
    pub check_resolutions: bool,
    pub refresh_lockfile: bool,
    pub silent_or_error: bool,
}

pub struct Project {
    pub project_cwd: Path,
    pub package_cwd: Path,
    pub shell_cwd: Path,

    pub config: Config,
    pub workspaces: Vec<Workspace>,
    pub workspaces_by_ident: BTreeMap<Ident, usize>,
    pub workspaces_by_rel_path: BTreeMap<Path, usize>,
    pub resolution_overrides: BTreeMap<Ident, Vec<(ResolutionSelector, Range)>>,

    pub last_changed_at: u128,
    pub install_state: Option<InstallState>,
}

impl Project {
    pub fn find_closest_project(start: Path) -> Result<(Path, Path), Error> {
        let mut p = start.clone();

        let mut closest_pkg = None;
        let mut farthest_pkg = None;

        loop {
            let lock_p = p.with_join_str(LOCKFILE_NAME);
            if lock_p.fs_exists() {
                return Ok((p.clone(), closest_pkg.unwrap_or(p)));
            }
        
            let pkg_p = p.with_join_str(MANIFEST_NAME);
            if pkg_p.fs_exists() {
                farthest_pkg = Some(p.clone());

                if closest_pkg.is_none() {
                    closest_pkg = Some(p.clone());
                }
            }
    
            if let Some(dirname) = p.dirname() {
                p = dirname;
            } else {
                break
            }
        }
    
        let farthest_pkg = farthest_pkg
            .ok_or(Error::ProjectNotFound(start))?;

        Ok((farthest_pkg, closest_pkg.unwrap()))
    }

    pub async fn new(cwd: Option<Path>) -> Result<Project, Error> {
        let shell_cwd = cwd
            .map(Ok)
            .unwrap_or_else(|| Path::current_dir())?;

        let (project_cwd, package_cwd)
            = Project::find_closest_project(shell_cwd.clone())?;

        let config = Config::new(
            Some(project_cwd.clone()),
            Some(package_cwd.clone()),
        )?;

        let root_workspace
            = Workspace::from_root_path(&project_cwd)?;

        let (mut workspaces, last_changed_at) = root_workspace
            .workspaces().await?;

        let mut resolutions_overrides: BTreeMap<Ident, Vec<(ResolutionSelector, Range)>>
            = BTreeMap::new();

        for (resolution, range) in &root_workspace.manifest.resolutions {
            resolutions_overrides.entry(resolution.target_ident().clone())
               .or_default()
               .push((resolution.clone(), range.clone()));
        }

        // Add root workspace to the beginning
        workspaces.insert(0, root_workspace);

        let mut workspaces_by_ident = BTreeMap::new();
        let mut workspaces_by_rel_path = BTreeMap::new();

        for (idx, workspace) in workspaces.iter().enumerate() {
            workspaces_by_ident.insert(workspace.locator().ident.clone(), idx);
            workspaces_by_rel_path.insert(workspace.rel_path.clone(), idx);
        }

        Ok(Project {
            shell_cwd: shell_cwd.relative_to(&project_cwd),
            package_cwd: package_cwd.relative_to(&project_cwd),
            project_cwd,

            config,
            workspaces,
            workspaces_by_ident,
            workspaces_by_rel_path,
            resolution_overrides: resolutions_overrides,

            last_changed_at,
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

    pub fn ignore_path(&self) -> Path {
        self.project_cwd.with_join_str(".yarn/ignore")
    }

    pub fn install_state_path(&self) -> Path {
        self.ignore_path().with_join_str("install")
    }

    pub fn build_state_path(&self) -> Path {
        self.ignore_path().with_join_str("build")
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

        if src.starts_with('#') {
            return from_legacy_berry_lockfile(&src);
        }

        sonic_rs::from_str(&src)
            .map_err(|err| Error::LockfileParseError(Arc::new(err)))
    }

    pub fn resolution_overrides(&self, ident: &Ident) -> Option<&Vec<(ResolutionSelector, Range)>> {
        self.resolution_overrides.get(ident)
    }

    #[track_time]
    pub fn import_install_state(&mut self) -> Result<&mut Self, Error> {
        let install_state_path
            = self.install_state_path();

        if !install_state_path.fs_exists() {
            return Err(Error::InstallStateNotFound);
        }

        let src = install_state_path
            .fs_read()?;

        let (install_state, _): (InstallState, _)
            = bincode::decode_from_slice(src.as_slice(), bincode::config::standard()).unwrap();

        self.install_state
            = Some(install_state);

        Ok(self)
    }

    pub fn attach_install_state(&mut self, install_state: InstallState) -> Result<(), Error> {
        if self.install_state.as_ref().map(|s| *s != install_state).unwrap_or(true) {
            self.write_install_state(&install_state)?;
        }

        self.install_state = Some(install_state);

        Ok(())
    }

    fn write_install_state(&mut self, install_state: &InstallState) -> Result<(), Error> {
        let link_info_path
            = self.install_state_path();

        let contents
            = bincode::encode_to_vec(install_state, bincode::config::standard()).unwrap();

        // let re_parsed: InstallState
        //     = serde_json::from_str(&contents)?;

        // if re_parsed != *install_state {
        //     let install_state_formatted = format!("{:#?}", install_state);
        //     let re_parsed_formatted = format!("{:#?}", re_parsed);

        //     Path::from("/tmp/zpm-install-state-before.json")
        //         .fs_write_text(install_state_formatted)?;
        //     Path::from("/tmp/zpm-install-state-after.json")
        //         .fs_write_text(re_parsed_formatted)?;

        //     panic!("The generated install state does not match the original install state. See /tmp/zpm-install-state-{{before,after}}.json for details.");
        // }

        link_info_path
            .fs_create_parent()?
            .fs_change(contents, Permissions::from_mode(0o644))?;

        Ok(())
    }

    pub fn write_lockfile(&self, lockfile: &Lockfile) -> Result<(), Error> {
        let lockfile_path
            = self.project_cwd.with_join_str(LOCKFILE_NAME);

        let contents
            = sonic_rs::to_string_pretty(lockfile)
                .map_err(|err| Error::LockfileGenerationError(Arc::new(err)))?;

        if self.config.project.enable_immutable_installs.value {
            lockfile_path.fs_expect(contents, Permissions::from_mode(0o644))?;
        } else {
            lockfile_path.fs_change(contents, Permissions::from_mode(0o644))?;
        }

        Ok(())
    }

    pub fn package_cache(&self) -> Result<CompositeCache, Error> {
        let global_cache_path = self.config.project.global_folder.value
            .with_join_str("cache");

        let local_cache_path
            = self.project_cwd
                .with_join_str(".yarn")
                .with_join_str(&self.config.project.local_cache_folder_name.value);

        global_cache_path.fs_create_dir_all()?;

        if !self.config.project.enable_global_cache.value {
            if !self.config.project.enable_immutable_cache.value {
                local_cache_path.fs_create_dir_all()?;
            } else if !local_cache_path.fs_exists() {
                return Err(Error::MissingCacheFolder(local_cache_path));
            }
        }

        let global_cache
            = Some(DiskCache::new(global_cache_path, self.config.project.enable_immutable_cache.value));

        let local_cache = (!self.config.project.enable_global_cache.value)
            .then(|| DiskCache::new(local_cache_path, self.config.project.enable_immutable_cache.value));

        Ok(CompositeCache {
            global_cache,
            local_cache,
        })
    }

    pub fn root_workspace(&self) -> &Workspace {
        &self.workspaces[0]
    }

    pub fn root_workspace_mut(&mut self) -> &mut Workspace {
        &mut self.workspaces[0]
    }

    pub fn active_package(&self) -> Result<Locator, Error> {
        let install_state = self.install_state.as_ref()
            .ok_or(Error::InstallStateNotFound)?;

        let active_package = install_state.packages_by_location.get(&self.package_cwd)
            .ok_or(Error::ActivePackageNotFound)?;

        Ok(active_package.clone())
    }

    fn active_workspace_locator(&self) -> Result<Locator, Error> {
        let active_package = if self.install_state.is_some() {
            self.active_package()?
        } else {
            self.workspaces.iter().find(|w| w.rel_path == self.package_cwd)
                .ok_or(Error::ActivePackageNotWorkspace)?
                .locator()
        };

        let Reference::WorkspaceIdent(_) = &active_package.reference else {
            return Err(Error::ActivePackageNotWorkspace);
        };

        Ok(active_package)
    }

    pub fn active_workspace_idx(&self) -> Result<usize, Error> {
        let active_package = self.active_workspace_locator()?;

        let Reference::WorkspaceIdent(params) = &active_package.reference else {
            return Err(Error::ActivePackageNotWorkspace);
        };

        let &idx = self.workspaces_by_ident.get(&params.ident)
            .ok_or_else(|| Error::WorkspaceNotFound(params.ident.clone()))?;

        Ok(idx)
    }

    pub fn active_workspace(&self) -> Result<&Workspace, Error> {
        let idx = self.active_workspace_idx()?;

        Ok(&self.workspaces[idx])
    }

    pub fn active_workspace_mut(&mut self) -> Result<&mut Workspace, Error> {
        let idx = self.active_workspace_idx()?;

        Ok(&mut self.workspaces[idx])
    }

    pub fn workspace_by_ident(&self, ident: &Ident) -> Result<&Workspace, Error> {
        let idx = self.workspaces_by_ident.get(ident)
            .ok_or_else(|| Error::WorkspaceNotFound(ident.clone()))?;

        Ok(&self.workspaces[*idx])
    }

    pub fn workspace_by_rel_path(&self, rel_path: &Path) -> Result<&Workspace, Error> {
        let idx = self.workspaces_by_rel_path.get(rel_path)
            .ok_or_else(|| Error::WorkspacePathNotFound(rel_path.clone()))?;

        Ok(&self.workspaces[*idx])
    }

    pub fn package_self_binaries(&self, locator: &Locator) -> Result<BTreeMap<String, Binary>, Error> {
        // Link dependencies never have any package.json, so we mustn't even try to read them.
        if matches!(locator.reference, Reference::Link(_)) {
            return Ok(BTreeMap::new());
        }

        let install_state = self.install_state.as_ref()
            .ok_or(Error::InstallStateNotFound)?;

        let location = install_state.locations_by_package.get(locator)
            .expect("Expected locator to have a location");

        let manifest_text = self.project_cwd
            .with_join(location)
            .with_join_str(MANIFEST_NAME)
            .fs_read_text_with_zip()?;

        let manifest
            = sonic_rs::from_str::<BinManifest>(&manifest_text)
                .map_err(|_| Error::ManifestParseError(location.clone()))?;

        Ok(match manifest.bin {
            Some(BinField::String(bin)) => {
                if let Some(name) = manifest.name {
                    BTreeMap::from_iter([(name.name().to_string(), Binary::new(self, location.with_join(&bin.path)))])
                } else {
                    BTreeMap::new()
                }
            }

            Some(BinField::Map(bins)) => bins
                .into_iter()
                .map(|(name, path)| (name, Binary::new(self, location.with_join(&path.path))))
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

        let mut all_bins = BTreeMap::new();

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

        self.find_package_script(&active_package, name)
    }

    pub fn find_package_script(&self, locator: &Locator, name: &str) -> Result<(Locator, String), Error> {
        #[derive(Debug, Clone, Deserialize)]
        struct ScriptManifest {
            pub scripts: Option<BTreeMap<String, String>>,
        }

        let manifest_text = self.project_cwd
            .with_join(&self.package_cwd)
            .with_join_str(MANIFEST_NAME)
            .fs_read_text_with_zip()?;

        let manifest
            = sonic_rs::from_str::<ScriptManifest>(&manifest_text)?;

        if let Some(script) = manifest.scripts.as_ref().and_then(|s| s.get(name)) {
            return Ok((locator.clone(), script.clone()));
        }

        if !name.contains(':') {
            return Err(Error::ScriptNotFound(name.to_string()));
        }

        let mut iterator = self.workspaces.iter();

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

    pub async fn lazy_install(&mut self) -> Result<(), Error> {
        match self.import_install_state() {
            Ok(_) | Err(Error::InstallStateNotFound) => (),
            Err(e) => return Err(e),
        };

        if let Some(install_state) = &self.install_state {
            if self.last_changed_at <= install_state.last_installed_at {
                return Ok(());
            }
        }

        self.run_install(RunInstallOptions {
            check_resolutions: false,
            refresh_lockfile: false,
            silent_or_error: true,
        }).await
    }

    pub async fn run_install(&mut self, options: RunInstallOptions) -> Result<(), Error> {
        let report = StreamReport::new(StreamReportConfig {
            enable_timers: true,
            silent_or_error: options.silent_or_error,
        });

        with_report_result(report, async {
            let package_cache
                = self.package_cache()?;

            let install_context = InstallContext::default()
                .with_package_cache(Some(&package_cache))
                .with_project(Some(self))
                .set_refresh_lockfile(options.refresh_lockfile);

            InstallManager::new()
                .with_context(install_context)
                .with_lockfile(self.lockfile()?)
                .with_roots_iter(self.workspaces.iter().map(|w| w.descriptor()))
                .resolve_and_fetch().await?
                .finalize(self).await?;

            Ok(())
        }).await?;

        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct Workspace {
    pub name: Ident,
    pub path: Path,
    pub rel_path: Path,
    pub manifest: Manifest,
    pub last_changed_at: u128,
}

pub struct WorkspaceInfo {
    pub rel_path: Path,
    pub manifest: Manifest,
    pub last_changed_at: u128,
}

impl Workspace {
    pub fn from_root_path(root: &Path) -> Result<Workspace, Error> {
        let manifest_path = root
            .with_join_str(MANIFEST_NAME);

        let manifest_meta = manifest_path.fs_metadata().map_err(|err| match err.io_kind() {
            Some(ErrorKind::NotFound) | Some(ErrorKind::NotADirectory) => Error::ManifestNotFound,
            _ => err.into(),
        })?;

        let last_changed_at = manifest_meta.modified()?
            .duration_since(UNIX_EPOCH).unwrap()
            .as_nanos();

        let manifest
            = read_manifest_with_size(&manifest_path, manifest_meta.len())?;

        Workspace::from_info(root, WorkspaceInfo {
            rel_path: Path::new(),
            manifest,
            last_changed_at,
        })
    }

    pub fn from_info(root: &Path, info: WorkspaceInfo) -> Result<Workspace, Error> {
        let path = root
            .with_join(&info.rel_path);

        let name = info.manifest.name.clone().unwrap_or_else(|| {
            Ident::new(if info.rel_path == Path::new() {
                "root-workspace".to_string()
            } else {
                info.rel_path.basename().map_or_else(|| "unnamed-workspace".to_string(), |b| b.to_string())
            })
        });

        Ok(Workspace {
            name,
            path,
            rel_path: info.rel_path,
            manifest: info.manifest,
            last_changed_at: info.last_changed_at,
        })
    }

    pub fn descriptor(&self) -> Descriptor {
        Descriptor::new(self.name.clone(), range::WorkspaceMagicRange {
            magic: zpm_semver::RangeKind::Caret,
        }.into())
    }

    pub fn locator(&self) -> Locator {
        Locator::new(self.name.clone(), reference::WorkspaceIdentReference {
            ident: self.name.clone(),
        }.into())
    }

    pub fn locator_path(&self) -> Locator {
        Locator::new(self.name.clone(), reference::WorkspacePathReference {
            path: self.rel_path.clone(),
        }.into())
    }

    pub async fn workspaces(&self) -> Result<(Vec<Workspace>, u128), Error> {
        let mut workspaces = vec![];
        let mut project_last_changed_at = self.last_changed_at;

        if let Some(patterns) = &self.manifest.workspaces {
            let normalized_patterns = patterns.iter()
                .map(|p| Path::try_from(p.as_str()))
                .collect::<Result<Vec<_>, _>>()?;

            let glob_patterns = normalized_patterns.into_iter()
                .map(|p| GlobBuilder::new(p.as_str()).literal_separator(true).build())
                .collect::<Result<Vec<_>, _>>()?;

            let pattern_matchers = glob_patterns.into_iter()
                .map(|g| g.compile_matcher())
                .collect::<Vec<_>>();

            let mut manifest_finder
                = CachedManifestFinder::new(self.path.clone())?;

            manifest_finder.rsync()?;

            let lookup_state
                = manifest_finder.into_state();

            let workspace_paths = lookup_state.cache.into_iter()
                .filter(|(_, entry)| matches!(entry, SaveEntry::File(_, _)))
                .filter(|(p, _)| {
                    let candidate_workspace_rel_dir = p.dirname()
                        .expect("Expected this path to have a parent directory, since it's supposed to be the relative path to a package.json file");

                    pattern_matchers.iter().any(|m| {
                        m.is_match(candidate_workspace_rel_dir.as_str())
                    })
                })
                .collect::<Vec<_>>();

            for (manifest_rel_path, save_entry) in workspace_paths {
                if let SaveEntry::File(last_changed_at, manifest) = save_entry {
                    workspaces.push(Workspace::from_info(&self.path, WorkspaceInfo {
                        rel_path: manifest_rel_path.dirname().unwrap(),
                        manifest,
                        last_changed_at,
                    })?);

                    if last_changed_at > project_last_changed_at {
                        project_last_changed_at = last_changed_at;
                    }
                }
            }

            workspaces.sort_by(|w1, w2| {
                w1.rel_path.cmp(&w2.rel_path)
            });
        }

        Ok((workspaces, project_last_changed_at))
    }
}
