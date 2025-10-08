use std::{collections::{BTreeMap, BTreeSet, HashSet}, io::ErrorKind, time::UNIX_EPOCH};

use globset::{GlobBuilder, GlobSetBuilder};
use zpm_config::{Configuration, ConfigurationContext};
use zpm_macro_enum::zpm_enum;
use zpm_parsers::JsonDocument;
use zpm_primitives::{Descriptor, Ident, Locator, Range, Reference, WorkspaceIdentReference, WorkspaceMagicRange, WorkspacePathReference};
use zpm_utils::{impl_file_string_from_str, impl_file_string_serialization, Path, ToFileString, ToHumanString};
use serde::Deserialize;
use zpm_formats::zip::ZipSupport;

use crate::{
    cache::{CompositeCache, DiskCache},
    diff_finder::SaveEntry,
    error::Error,
    git::{detect_git_operation, GitOperation},
    http::HttpClient,
    install::{InstallContext, InstallManager, InstallState},
    lockfile::{from_legacy_berry_lockfile, Lockfile},
    manifest::{helpers::read_manifest_with_size, Manifest},
    manifest_finder::CachedManifestFinder,
    report::{with_report_result, StreamReport, StreamReportConfig},
    script::{Binary, ScriptEnvironment},
    system::System,
};

pub const LOCKFILE_NAME: &str = "yarn.lock";
pub const MANIFEST_NAME: &str = "package.json";
pub const PNP_CJS_NAME: &str = ".pnp.cjs";
pub const PNP_ESM_NAME: &str = ".pnp.loader.mjs";
pub const PNP_DATA_NAME: &str = ".pnp.data.json";

#[zpm_enum(or_else = |s| Err(Error::InvalidInstallMode(s.to_string())))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallMode {
    /// Don't run the build scripts.
    #[pattern(spec = "skip-build")]
    SkipBuild,
}

impl ToFileString for InstallMode {
    fn to_file_string(&self) -> String {
        match self {
            InstallMode::SkipBuild => "skip-build".to_string(),
        }
    }
}

impl_file_string_from_str!(InstallMode);
impl_file_string_serialization!(InstallMode);

#[derive(Default)]
pub struct RunInstallOptions {
    pub check_checksums: bool,
    pub check_resolutions: bool,
    pub enforced_resolutions: BTreeMap<Descriptor, Locator>,
    pub prune_dev_dependencies: bool,
    pub mode: Option<InstallMode>,
    pub refresh_lockfile: bool,
    pub roots: Option<BTreeSet<Ident>>,
    pub silent_or_error: bool,
}

pub struct Project {
    pub project_cwd: Path,
    pub package_cwd: Path,
    pub shell_cwd: Path,

    pub config: Configuration,
    pub workspaces: Vec<Workspace>,
    pub workspaces_by_ident: BTreeMap<Ident, usize>,
    pub workspaces_by_rel_path: BTreeMap<Path, usize>,

    pub last_changed_at: u128,
    pub install_state: Option<InstallState>,
    pub http_client: std::sync::Arc<HttpClient>,
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
        let user_cwd
            = Path::home_dir()?;

        let shell_cwd = cwd
            .map(Ok)
            .unwrap_or_else(|| Path::current_dir())?;

        let (project_cwd, package_cwd)
            = Project::find_closest_project(shell_cwd.clone())?;

        let config = Configuration::load(
            &ConfigurationContext {
                env: std::env::vars().collect(),
                user_cwd: user_cwd.clone(),
                project_cwd: Some(project_cwd.clone()),
                package_cwd: Some(package_cwd.clone()),
            },
        ).unwrap();

        let root_workspace
            = Workspace::from_root_path(&project_cwd)?;

        let (mut workspaces, last_changed_at) = root_workspace
            .workspaces().await?;

        // Add root workspace to the beginning
        workspaces.insert(0, root_workspace);

        let mut workspaces_by_ident
            = BTreeMap::new();
        let mut workspaces_by_rel_path
            = BTreeMap::new();

        for (idx, workspace) in workspaces.iter().enumerate() {
            workspaces_by_ident.insert(workspace.locator().ident.clone(), idx);
            workspaces_by_rel_path.insert(workspace.rel_path.clone(), idx);
        }

        let http_client
            = HttpClient::new(&config)?;

        Ok(Project {
            shell_cwd: shell_cwd.relative_to(&project_cwd),
            package_cwd: package_cwd.relative_to(&project_cwd),
            project_cwd,

            config,
            workspaces,
            workspaces_by_ident,
            workspaces_by_rel_path,

            last_changed_at,
            install_state: None,
            http_client,
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

    pub fn unplugged_path(&self) -> Path {
        self.ignore_path().with_join_str("unplugged")
    }

    pub fn install_state_path(&self) -> Path {
        self.ignore_path().with_join_str("install")
    }

    pub fn build_state_path(&self) -> Path {
        self.ignore_path().with_join_str("build")
    }

    pub fn global_cache_path(&self) -> Path {
        self.config.settings.global_folder.value
            .with_join_str("cache")
    }

    pub fn local_cache_path(&self) -> Path {
        self.project_cwd
            .with_join_str(".yarn")
            .with_join_str(&self.config.settings.local_cache_folder_name.value)
    }

    pub fn preferred_cache_path(&self) -> Path {
        if self.config.settings.enable_global_cache.value {
            self.global_cache_path()
        } else {
            self.local_cache_path()
        }
    }

    pub fn lockfile(&self) -> Result<Lockfile, Error> {
        let lockfile_path
            = self.lockfile_path();

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

        let lockfile: Lockfile
            = JsonDocument::hydrate_from_str(&src)
                .map_err(|e| Error::LockfileParseError(e))?;

        Ok(lockfile)
    }

    pub fn import_install_state(&mut self) -> Result<&mut Self, Error> {
        let install_state_path
            = self.install_state_path();

        if !install_state_path.fs_exists() {
            return Err(Error::InstallStateNotFound);
        }

        let src = install_state_path
            .fs_read()?;

        let (install_state, _): (InstallState, _)
            = bincode::decode_from_slice(src.as_slice(), bincode::config::standard())
                .map_err(|_| Error::InvalidInstallState)?;

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
            .fs_change(contents, false)?;

        Ok(())
    }

    pub fn write_lockfile(&self, lockfile: &Lockfile) -> Result<(), Error> {
        let lockfile_path
            = self.lockfile_path();

        let contents
            = JsonDocument::to_string_pretty(lockfile)?;

        if self.config.settings.enable_immutable_installs.value {
            lockfile_path.fs_expect(contents, false)?;
        } else {
            lockfile_path.fs_change(contents, false)?;
        }

        Ok(())
    }

    pub fn package_cache(&self) -> Result<CompositeCache, Error> {
        let global_cache_path
            = self.global_cache_path();
        let local_cache_path
            = self.local_cache_path();

        global_cache_path.fs_create_dir_all()?;

        if !self.config.settings.enable_global_cache.value {
            if !self.config.settings.enable_immutable_cache.value {
                local_cache_path.fs_create_dir_all()?;
            } else if !local_cache_path.fs_exists() {
                return Err(Error::MissingCacheFolder(local_cache_path));
            }
        }

        let compression_algorithm
            = self.config.settings.compression_level.value;

        let enable_global_cache
            = self.config.settings.enable_global_cache.value;

        let enable_immutable_cache
            = self.config.settings.enable_immutable_cache.value;

        let name_suffix = match compression_algorithm {
            Some(zpm_formats::CompressionAlgorithm::Deflate(_)) => format!("-d{}", compression_algorithm.unwrap().to_file_string()),
            None => "".to_string(),
        };

        let global_cache
            = Some(DiskCache::new(global_cache_path, name_suffix.clone(), enable_immutable_cache));

        let local_cache = (!enable_global_cache)
            .then(|| DiskCache::new(local_cache_path, name_suffix, enable_immutable_cache));

        Ok(CompositeCache {
            compression_algorithm,
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
        let idx
            = self.active_workspace_idx()?;

        Ok(&self.workspaces[idx])
    }

    pub fn active_workspace_mut(&mut self) -> Result<&mut Workspace, Error> {
        let idx
            = self.active_workspace_idx()?;

        Ok(&mut self.workspaces[idx])
    }

    pub fn workspace_by_ident(&self, ident: &Ident) -> Result<&Workspace, Error> {
        let idx = self.workspaces_by_ident.get(ident)
            .ok_or_else(|| Error::WorkspaceNotFound(ident.clone()))?;

        Ok(&self.workspaces[*idx])
    }

    pub fn try_workspace_by_descriptor(&self, descriptor: &Descriptor) -> Result<Option<&Workspace>, Error> {
        match &descriptor.range {
            Range::WorkspaceIdent(params) => {
                Ok(Some(self.workspace_by_ident(&params.ident)?))
            },

            Range::WorkspacePath(params) => {
                Ok(Some(self.workspace_by_rel_path(&params.path)?))
            },

            Range::WorkspaceSemver(_) => {
                Ok(Some(self.workspace_by_ident(&descriptor.ident)?))
            },

            Range::WorkspaceMagic(_) => {
                Ok(Some(self.workspace_by_ident(&descriptor.ident)?))
            },

            Range::RegistryTag(_) if self.config.settings.enable_transparent_workspaces.value => {
                let workspace
                    = self.workspaces_by_ident.get(&descriptor.ident)
                        .map(|idx| &self.workspaces[*idx]);

                Ok(workspace)
            },

            Range::RegistrySemver(params) if self.config.settings.enable_transparent_workspaces.value => {
                let workspace
                    = self.workspaces_by_ident.get(params.ident.as_ref().unwrap_or(&descriptor.ident))
                        .map(|idx| &self.workspaces[*idx]);

                if let Some(workspace) = workspace {
                    if params.range.check(&workspace.manifest.remote.version.clone().unwrap_or_default()) {
                        return Ok(Some(workspace));
                    }
                }

                Ok(None)
            },

            _ => {
                Ok(None)
            },
        }
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

        let package_location = install_state.locations_by_package.get(locator)
            .unwrap_or_else(|| panic!("Expected {} to have a package location", locator.to_print_string()));

        let content_flags = install_state.content_flags.get(&locator.physical_locator())
            .unwrap_or_else(|| panic!("Expected {} to have content flags", locator.to_print_string()));

        let binaries = content_flags.binaries.iter()
            .map(|(name, path)| (name.clone(), Binary::new(self, package_location.with_join(&path))))
            .collect();

        Ok(binaries)
    }

    pub fn package_visible_binaries(&self, locator: &Locator) -> Result<BTreeMap<String, Binary>, Error> {
        let install_state = self.install_state.as_ref()
            .ok_or(Error::InstallStateNotFound)?;

        let resolution = install_state.resolution_tree.locator_resolutions.get(locator)
            .expect("Expected active package to have a resolution tree");

        let mut all_bins
            = BTreeMap::new();

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

        let manifest: ScriptManifest
            = JsonDocument::hydrate_from_str(&manifest_text)?;

        if let Some(script) = manifest.scripts.as_ref().and_then(|s| s.get(name)) {
            return Ok((locator.clone(), script.clone()));
        }

        if !name.contains(':') {
            return Err(Error::ScriptNotFound(name.to_string()));
        }

        let mut iterator
            = self.workspaces.iter();

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
            Ok(_) => {},

            Err(Error::InstallStateNotFound | Error::InvalidInstallState) => {
                // Don't use stale install states.
                self.install_state = None;
            }

            Err(e) => {
                return Err(e);
            },
        };

        if let Some(install_state) = &self.install_state {
            if self.last_changed_at <= install_state.last_installed_at {
                return Ok(());
            }
        }

        self.run_install(RunInstallOptions {
            check_checksums: false,
            check_resolutions: false,
            enforced_resolutions: BTreeMap::new(),
            prune_dev_dependencies: false,
            refresh_lockfile: false,
            silent_or_error: true,
            mode: None,
            roots: None,
        }).await
    }

    pub async fn run_install(&mut self, options: RunInstallOptions) -> Result<(), Error> {
        // Useful for optimization purposes as we can reuse some information such as content flags.
        // Discard errors; worst case scenario we just recompute the whole state from scratch.
        if self.install_state.is_none() {
            let _ = self.import_install_state();
        }

        let report = StreamReport::new(StreamReportConfig {
            include_version: true,
            silent_or_error: options.silent_or_error,
            ..StreamReportConfig::from_config(&self.config)
        });

        let systems
            = System::from_supported_architectures(&self.config.settings.supported_architectures);

        with_report_result(report, async {
            let package_cache
                = self.package_cache()?;

            let mut lockfile
                = self.lockfile();

            if let Err(Error::LockfileParseError(_)) = lockfile {
                let lockfile_path
                    = self.lockfile_path();

                let lockfile_content = lockfile_path
                    .fs_read_text()?;

                if lockfile_content.contains("<<<<<<<") {
                    if self.config.settings.enable_immutable_installs.value {
                        return Err(Error::ImmutableLockfileAutofix);
                    }

                    let git_operation
                        = detect_git_operation(&self.project_cwd)
                            .await?
                            .unwrap_or(GitOperation::Merge);

                    ScriptEnvironment::new()?
                        .with_cwd(self.project_cwd.clone())
                        .run_exec("git", vec!["checkout", git_operation.true_theirs(), lockfile_path.as_str()])
                        .await?
                        .ok()
                        .map_err(|e| Error::LockfileAutofixGitError(e.to_string()))?;

                    lockfile
                        = self.lockfile();
                }
            }

            let install_context = InstallContext::default()
                .with_package_cache(Some(&package_cache))
                .with_project(Some(self))
                .set_check_checksums(options.check_checksums)
                .set_enforced_resolutions(options.enforced_resolutions)
                .set_prune_dev_dependencies(options.prune_dev_dependencies)
                .set_refresh_lockfile(options.refresh_lockfile)
                .set_mode(options.mode)
                .with_systems(Some(&systems));

            let roots
                = self.workspaces.iter()
                    .filter(|w| options.roots.as_ref().map_or(true, |r| r.contains(&w.name)))
                    .map(|w| w.descriptor())
                    .collect();

            InstallManager::new()
                .with_context(install_context)
                .with_lockfile(lockfile?)
                .with_previous_state(self.install_state.as_ref())
                .with_roots(roots)
                .with_constraints_check(!options.silent_or_error && self.config.settings.enable_constraints_checks.value && options.roots.is_none())
                .with_skip_lockfile_update(options.roots.is_some())
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
            Some(ErrorKind::NotFound) | Some(ErrorKind::NotADirectory) => Error::ManifestNotFound(manifest_path.clone()),
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
        Descriptor::new(self.name.clone(), WorkspaceMagicRange {
            magic: zpm_semver::RangeKind::Caret,
        }.into())
    }

    pub fn locator(&self) -> Locator {
        Locator::new(self.name.clone(), WorkspaceIdentReference {
            ident: self.name.clone(),
        }.into())
    }

    pub fn locator_path(&self) -> Locator {
        Locator::new(self.name.clone(), WorkspacePathReference {
            path: self.rel_path.clone(),
        }.into())
    }

    pub async fn workspaces(&self) -> Result<(Vec<Workspace>, u128), Error> {
        let mut workspaces = vec![];
        let mut project_last_changed_at = self.last_changed_at;

        if let Some(patterns) = &self.manifest.workspaces {
            let mut manifest_finder
                = CachedManifestFinder::new(self.path.clone());

            manifest_finder.rsync()?;

            let lookup_state
                = manifest_finder.into_state();

            let mut workspace_queue = vec![
                (Path::new(), patterns.clone()),
            ];

            let mut processed_workspaces
                = HashSet::new();

            while let Some((base_path, current_patterns)) = workspace_queue.pop() {
                let glob_patterns
                    = current_patterns.into_iter()
                        .map(|pattern| {
                            let (pattern, is_positive)
                                = if pattern.starts_with('!') {
                                    (&pattern[1..], false)
                                } else {
                                    (pattern.as_ref(), true)
                                };

                            let pattern_path
                                = base_path.with_join_str(pattern);

                            GlobBuilder::new(pattern_path.as_str())
                                    .literal_separator(true)
                                    .build()
                                    .map(|glob| (glob, is_positive))
                        })
                        .collect::<Result<Vec<_>, _>>()?;

                let (positive_patterns, negative_patterns): (Vec<_>, Vec<_>)
                    = glob_patterns.into_iter()
                        .partition(|(_, is_positive)| *is_positive);

                let mut positive_builder
                    = GlobSetBuilder::new();
                for (glob, _) in positive_patterns {
                    positive_builder.add(glob);
                }

                let mut negative_builder
                    = GlobSetBuilder::new();
                for (glob, _) in negative_patterns {
                    negative_builder.add(glob);
                }

                let positive_glob_set
                    = positive_builder.build()?;
                let negative_glob_set
                    = negative_builder.build()?;

                let workspace_paths
                    = lookup_state.cache.iter()
                        .filter(|(_, entry)| {
                            matches!(entry, SaveEntry::File(_, _))
                        })
                        .filter(|(p, _)| {
                            let candidate_workspace_rel_dir = p.dirname()
                                .expect("Expected this path to have a parent directory, since it's supposed to be the relative path to a package.json file");

                            let dir_str = candidate_workspace_rel_dir.as_str();

                            // Important: If there are no positive patterns, nothing matches.
                            positive_glob_set.is_match(dir_str) && !negative_glob_set.is_match(dir_str)
                        })
                        .collect::<Vec<_>>();

                for (manifest_rel_path, save_entry) in workspace_paths {
                    if let SaveEntry::File(last_changed_at, manifest) = save_entry {
                        let workspace_rel_path = manifest_rel_path.dirname().unwrap();

                        // Skip if we've already processed this workspace
                        if processed_workspaces.contains(&workspace_rel_path) {
                            continue;
                        }

                        processed_workspaces.insert(workspace_rel_path.clone());

                        workspaces.push(Workspace::from_info(&self.path, WorkspaceInfo {
                            rel_path: workspace_rel_path.clone(),
                            manifest: manifest.clone(),
                            last_changed_at: *last_changed_at,
                        })?);

                        if *last_changed_at > project_last_changed_at {
                            project_last_changed_at = *last_changed_at;
                        }

                        // If this workspace has its own workspaces field, add it to the queue
                        if let Some(nested_patterns) = &manifest.workspaces {
                            workspace_queue.push((workspace_rel_path, nested_patterns.clone()));
                        }
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
