use std::{collections::{BTreeMap, BTreeSet, HashSet}, io::ErrorKind, sync::{Arc, mpsc}, time::{Duration, UNIX_EPOCH}};

use colored::Colorize;
use globset::{GlobBuilder, GlobSetBuilder};
use zpm_config::{Configuration, ConfigurationContext, Source};
use zpm_macro_enum::zpm_enum;
use zpm_parsers::JsonDocument;
use zpm_primitives::{Descriptor, Ident, Locator, Range, Reference, WorkspaceIdentReference, WorkspaceMagicRange, WorkspacePathReference};
use zpm_tasks::{parse as parse_taskfile, ResolvedTasks, TaskFile, TaskId};
use zpm_utils::{DataType, Glob, IoResultExt, LastModifiedAt, Path, ToFileString, ToHumanString, is_terminal, start_progress};
use serde::Deserialize;
use zpm_formats::zip::ZipSupport;

use crate::{
    cache::{CompositeCache, DiskCache},
    content_flags,
    diff_finder::CacheEntry,
    error::Error,
    git::{GitOperation, detect_git_operation},
    http::HttpClient,
    http_npm,
    install::{InstallContext, InstallManager, InstallResult, InstallState},
    lockfile::{Lockfile, from_legacy_berry_lockfile, from_pnpm_node_modules},
    manifest::{Manifest, helpers::read_manifest_with_size},
    manifest_finder::CachedManifestFinder,
    report::{StreamReport, StreamReportConfig, async_section, current_report, with_report_result},
    script::{Binary, ScriptEnvironment},
    tasks::TASK_FILE_NAME,
};

pub const LOCKFILE_NAME: &str = "yarn.lock";
pub const MANIFEST_NAME: &str = "package.json";
pub const PNP_CJS_NAME: &str = ".pnp.cjs";
pub const PNP_ESM_NAME: &str = ".pnp.loader.mjs";
pub const PNP_DATA_NAME: &str = ".pnp.data.json";
const LOCKFILE_DIFF_LINE_LIMIT: usize = 100;
const LOCKFILE_DIFF_TIMEOUT: Duration = Duration::from_secs(3);

fn colorize_diff_line(line: &str) -> String {
    if line.starts_with("+") && !line.starts_with("+++") {
        line.green().to_string()
    } else if line.starts_with("-") && !line.starts_with("---") {
        line.red().to_string()
    } else if line.starts_with("@@") {
        DataType::Info.colorize(line)
    } else if line.starts_with("+++") || line.starts_with("---") {
        DataType::Path.colorize(line)
    } else {
        line.to_string()
    }
}

fn render_lockfile_diff(current: Option<Vec<u8>>, expected: Vec<u8>) -> Option<String> {
    let (tx, rx) = mpsc::channel();

    std::thread::spawn(move || {
        let current_text = current
            .as_deref()
            .map(String::from_utf8_lossy)
            .unwrap_or_else(|| "".into())
            .into_owned();
        let expected_text
            = String::from_utf8_lossy(&expected).into_owned();

        let diff = similar::TextDiff::from_lines(&current_text, &expected_text)
            .unified_diff()
            .header("current yarn.lock", "generated yarn.lock")
            .to_string();

        let mut truncated = false;
        let mut lines = Vec::new();

        for (idx, line) in diff.lines().enumerate() {
            if idx >= LOCKFILE_DIFF_LINE_LIMIT {
                truncated = true;
                break;
            }

            lines.push(colorize_diff_line(line));
        }

        if truncated {
            lines.push(DataType::Code.colorize(&format!(
                "... Diff truncated after {} lines.",
                LOCKFILE_DIFF_LINE_LIMIT,
            )));
        }

        let _ = tx.send(lines.join("\n"));
    });

    rx.recv_timeout(LOCKFILE_DIFF_TIMEOUT).ok()
}

#[zpm_enum(or_else = |s| Err(Error::InvalidInstallMode(s.to_string())))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallMode {
    /// Don't run the build scripts.
    #[pattern("skip-build")]
    #[to_file_string(|| "skip-build".to_string())]
    #[to_print_string(|| "skip-build".to_string())]
    SkipBuild,

    /// Just update the lockfile, skip the fetching and linking.
    #[pattern("update-lockfile")]
    #[to_file_string(|| "update-lockfile".to_string())]
    #[to_print_string(|| "update-lockfile".to_string())]
    UpdateLockfile,
}


#[derive(Default)]
pub struct RunInstallOptions {
    pub check_checksums: bool,
    pub check_resolutions: bool,
    pub enforced_resolutions: BTreeMap<Descriptor, Option<Locator>>,
    pub prune_dev_dependencies: bool,
    pub mode: Option<InstallMode>,
    pub refresh_lockfile: bool,
    pub roots: Option<BTreeSet<Ident>>,
    pub silent_or_error: bool,
    pub json: bool,
    pub inline_builds: bool,
}

pub struct Project {
    pub project_cwd: Path,
    pub package_cwd: Path,
    pub shell_cwd: Path,

    pub config: Configuration,
    pub workspaces: Vec<Workspace>,
    pub workspaces_by_ident: BTreeMap<Ident, usize>,
    pub workspaces_by_rel_path: BTreeMap<Path, usize>,

    pub last_modified_at: LastModifiedAt,
    pub install_state: Option<InstallState>,
    pub http_client: std::sync::Arc<HttpClient>,
    pub clone_limiter: std::sync::Arc<tokio::sync::Semaphore>,
}

impl Project {
    pub fn find_closest_project(start: Path) -> Result<(Path, Path), Error> {
        let mut p = start.clone();

        let mut closest_pkg = None;
        let mut farthest_pkg = None;

        loop {
            let lock_p
                = p.with_join_str(LOCKFILE_NAME);

            if lock_p.fs_exists() {
                return Ok((p.clone(), closest_pkg.unwrap_or(p)));
            }

            let pkg_p
                = p.with_join_str(MANIFEST_NAME);

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

        let mut last_modified_at
            = LastModifiedAt::new();

        let configuration_context = ConfigurationContext {
            env: std::env::vars().collect(),
            user_cwd: user_cwd.clone(),
            project_cwd: Some(project_cwd.clone()),
            package_cwd: Some(package_cwd.clone()),
        };

        let mut config
            = Configuration::load(&configuration_context, &mut last_modified_at)
                .map_err(|e| Error::ConfigurationParseError(Arc::new(e)))?;

        if config.settings.enable_migration_mode.value {
            config.settings.enable_global_cache.value = true;
            config.settings.enable_global_cache.source = config.settings.enable_migration_mode.source;
        }

        let clone_concurrency
            = config.settings.clone_concurrency.value;

        if clone_concurrency == 0 {
            return Err(Error::InvalidConfigValue("cloneConcurrency".to_string(), "must be >= 1".to_string()));
        }

        let clone_limiter
            = Arc::new(tokio::sync::Semaphore::new(clone_concurrency));

        let root_workspace
            = Workspace::from_root_path(&project_cwd)?;

        let mut workspaces = root_workspace
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

            last_modified_at.update(workspace.last_changed_at);
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

            last_modified_at,
            install_state: None,
            http_client,
            clone_limiter,
        })
    }

    pub fn manifest_path(&self) -> Path {
        self.project_cwd.with_join_str(MANIFEST_NAME)
    }

    pub fn lockfile_path(&self) -> Path {
        if self.config.settings.enable_migration_mode.value {
            self.migration_path().with_join_str(LOCKFILE_NAME)
        } else {
            self.project_cwd.with_join_str(LOCKFILE_NAME)
        }
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

    pub fn versioning_path(&self) -> Path {
        let path
            = &self.config.settings.deferred_version_folder.value;

        self.project_path(path)
    }

    pub fn project_path(&self, path: &Path) -> Path {
        if path.is_relative() {
            self.project_cwd.with_join(path)
        } else {
            path.clone()
        }
    }

    pub fn patch_folder_path(&self) -> Path {
        self.project_path(&self.config.settings.patch_folder.value)
    }

    pub fn migration_path(&self) -> Path {
        self.ignore_path().with_join_str("migration")
    }

    pub fn unplugged_path(&self) -> Path {
        self.project_path(&self.config.settings.pnp_unplugged_folder.value)
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
        self.config.settings.cache_folder.value.clone()
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

        let mut lockfile
            = Project::lockfile_from(&lockfile_path)?;

        if self.config.settings.enable_migration_mode.value {
            let source_lockfile_path
                = self.project_cwd.with_join_str(LOCKFILE_NAME);

            let source_lockfile
                = Project::lockfile_from(&source_lockfile_path)?;

            lockfile.resolutions.extend(source_lockfile.resolutions.into_iter());
        }

        Ok(lockfile)
    }

    fn lockfile_from(lockfile_path: &Path) -> Result<Lockfile, Error> {
        if !lockfile_path.fs_exists() {
            // Check for pnpm node_modules in the same directory
            if let Some(project_cwd) = lockfile_path.dirname() {
                let pnpm_dir
                    = project_cwd.with_join_str("node_modules/.pnpm");

                if pnpm_dir.fs_exists() {
                    return from_pnpm_node_modules(&project_cwd);
                }
            }

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

        let install_state
            = rkyv::from_bytes::<InstallState, rkyv::rancor::BoxedError>(&src)
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
            = rkyv::to_bytes::<rkyv::rancor::BoxedError>(install_state)
                .map_err(|_| Error::InvalidInstallState)?
                .to_vec();

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
            let current_content = lockfile_path
                .fs_read()
                .ok_missing()?;

            if current_content.as_ref().is_some_and(|current| current.as_slice() == contents.as_bytes()) {
                return Ok(());
            }

            let diff
                = render_lockfile_diff(current_content.clone(), contents.into_bytes());

            return Err(match current_content {
                Some(_) => Error::ImmutableLockfileModification { diff },
                None => Error::ImmutableLockfileCreation { diff },
            });
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
        let enable_mirror
            = self.config.settings.enable_mirror.value;

        let enable_immutable_cache
            = self.config.settings.enable_immutable_cache.value;

        let cleanable_local_cache
            = matches!(self.config.settings.cache_folder.source, Source::Default);

        let name_suffix = match compression_algorithm {
            Some(zpm_formats::CompressionAlgorithm::Deflate(_)) => format!("-d{}", compression_algorithm.unwrap().to_file_string()),
            None => "".to_string(),
        };

        let global_cache = (enable_global_cache || enable_mirror)
            .then(|| DiskCache::new(global_cache_path, name_suffix.clone(), enable_immutable_cache, false));

        let local_cache = (!enable_global_cache)
            .then(|| DiskCache::new(local_cache_path, name_suffix, enable_immutable_cache, cleanable_local_cache));

        Ok(CompositeCache::new(
            compression_algorithm,
            global_cache,
            local_cache,
        ))
    }

    pub fn root_workspace(&self) -> &Workspace {
        &self.workspaces[0]
    }

    pub fn root_workspace_mut(&mut self) -> &mut Workspace {
        &mut self.workspaces[0]
    }

    pub fn active_package(&self) -> Result<Locator, Error> {
        let install_state
            = self.install_state.as_ref()
                .ok_or(Error::InstallStateNotFound)?;

        let Some(active_package) = install_state.packages_by_location.get(&self.package_cwd) else {
            let is_workspace
                = self.try_workspace_by_rel_path(&self.package_cwd)?
                    .is_some();

            return Err(if is_workspace {
                Error::WorkspaceNotInstalled
            } else {
                Error::ActivePackageNotFound
            });
        };

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

    /// Rewrites workspace locators to their path form
    /// (`name@workspace:packages/foo`) so same-ident workspaces are
    /// distinguishable. Non-workspace locators pass through.
    pub fn displayable_locator(&self, locator: &Locator) -> Locator {
        match &locator.reference {
            Reference::WorkspaceIdent(params) => {
                self.workspaces_by_ident.get(&params.ident)
                    .map(|&idx| self.workspaces[idx].locator_path())
                    .unwrap_or_else(|| locator.clone())
            },

            _ => locator.clone(),
        }
    }

    pub fn try_workspace_by_locator(&self, locator: &Locator) -> Result<Option<&Workspace>, Error> {
        match &locator.reference {
            Reference::WorkspaceIdent(params) => {
                Ok(Some(self.workspace_by_ident(&params.ident)?))
            },

            Reference::WorkspacePath(params) => {
                Ok(Some(self.workspace_by_rel_path(&params.path)?))
            },

            _ => {
                Ok(None)
            },
        }
    }

    pub fn workspace_by_locator(&self, locator: &Locator) -> Result<&Workspace, Error> {
        self.try_workspace_by_locator(locator)?
            .ok_or(Error::WorkspaceNotFound(locator.ident.clone()))
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

    pub fn try_workspace_by_rel_path(&self, rel_path: &Path) -> Result<Option<&Workspace>, Error> {
        let workspace
            = self.workspaces_by_rel_path.get(rel_path)
                .map(|idx| &self.workspaces[*idx]);

        Ok(workspace)
    }

    pub fn workspace_by_rel_path(&self, rel_path: &Path) -> Result<&Workspace, Error> {
        self.try_workspace_by_rel_path(rel_path)?
            .ok_or(Error::WorkspacePathNotFound(rel_path.clone()))
    }

    pub fn try_workspace_by_file_path(&self, rel_path: &Path) -> Result<Option<&Workspace>, Error> {
        let workspace
            = self.workspaces.iter()
                .find(|w| w.rel_path.contains(rel_path));

        Ok(workspace)
    }

    pub fn package_location(&self, locator: &Locator) -> Result<&Path, Error> {
        if let Some(locator_workspace) = self.try_workspace_by_locator(locator)? {
            return Ok(&locator_workspace.path);
        }

        let install_state = self.install_state.as_ref()
            .ok_or(Error::InstallStateNotFound)?;

        let package_location = install_state.locations_by_package.get(locator)
            .unwrap_or_else(|| panic!("Expected {} to have a package location", locator.to_print_string()));

        Ok(package_location)
    }

    fn package_binaries_at(&self, locator: &Locator, package_location: &Path) -> Result<BTreeMap<String, Binary>, Error> {
        // Link dependencies never have any package.json, so we mustn't even try to read them.
        if matches!(locator.reference, Reference::Link(_)) {
            return Ok(BTreeMap::new());
        }

        let install_state = self.install_state.as_ref()
            .ok_or(Error::InstallStateNotFound)?;

        let content_flags = install_state.content_flags.get(&locator.physical_locator())
            .unwrap_or_else(|| panic!("Expected {} to have content flags", locator.to_print_string()));

        let binaries = content_flags.binaries.iter()
            .map(|(name, binary)| {
                let binary = match binary {
                    content_flags::Binary::Node(path) => {
                        Binary::new_path(self, package_location.with_join(path))
                    },

                    content_flags::Binary::Python {module, object} => {
                        Binary::new_python(
                            name.clone(),
                            self.project_cwd.with_join(package_location),
                            module.clone(),
                            object.clone(),
                        )
                    },
                };

                (name.clone(), binary)
            })
            .collect();

        Ok(binaries)
    }

    pub fn package_self_binaries(&self, locator: &Locator) -> Result<BTreeMap<String, Binary>, Error> {
        let install_state = self.install_state.as_ref()
            .ok_or(Error::InstallStateNotFound)?;

        let package_location = install_state.locations_by_package.get(locator)
            .unwrap_or_else(|| panic!("Expected {} to have a package location", locator.to_print_string()));

        self.package_binaries_at(locator, package_location)
    }

    fn find_visible_dependency_location(&self, issuer_location: &Path, ident: &Ident, locator: &Locator) -> Result<Option<Path>, Error> {
        let install_state = self.install_state.as_ref()
            .ok_or(Error::InstallStateNotFound)?;

        for candidate_issuer_location in issuer_location.iter_path().rev() {
            let candidate = candidate_issuer_location
                .with_join_str("node_modules")
                .with_join_str(ident.as_str());

            if install_state.packages_by_location.get(&candidate) == Some(locator) {
                return Ok(Some(candidate));
            }
        }

        Ok(install_state.locations_by_package.get(locator).cloned())
    }

    pub fn package_visible_binaries(&self, locator: &Locator) -> Result<BTreeMap<String, Binary>, Error> {
        let install_state = self.install_state.as_ref()
            .ok_or(Error::InstallStateNotFound)?;

        let resolution = install_state.resolution_tree.locator_resolutions.get(locator)
            .expect("Expected active package to have a resolution tree");

        let package_location = install_state.locations_by_package.get(locator)
            .unwrap_or_else(|| panic!("Expected {} to have a package location", locator.to_print_string()));

        let mut all_bins
            = BTreeMap::new();

        for (ident, descriptor) in &resolution.dependencies {
            let locator = install_state.resolution_tree.descriptor_to_locator.get(descriptor)
                .expect("Expected resolution to be found in the resolution tree");

            // Packages may be missing from locations_by_package when they
            // haven't been installed due to being unsupported on the current
            // platform. In this case, we ignore its binaries.
            //
            if let Some(dependency_location) = self.find_visible_dependency_location(package_location, ident, locator)? {
                all_bins.extend(self.package_binaries_at(locator, &dependency_location)?);
            }
        }

        all_bins.extend(self.package_binaries_at(locator, package_location)?);

        Ok(BTreeMap::from_iter(all_bins.into_iter()))
    }

    pub fn find_binary(&self, name: &str) -> Result<Binary, Error> {
        let active_package
            = self.active_package()?;

        self.package_visible_binaries(&active_package)?
            .remove(name)
            .ok_or(Error::BinaryNotFound(name.to_string()))
    }

    pub fn find_script(&self, name: &str) -> Result<(Locator, String), Error> {
        let active_package
            = self.active_package()?;

        self.find_package_script(&active_package, name)
    }

    pub fn find_package_script(&self, locator: &Locator, name: &str) -> Result<(Locator, String), Error> {
        #[derive(Debug, Clone, Deserialize)]
        struct ScriptManifest {
            pub scripts: Option<BTreeMap<String, String>>,
        }

        let package_location
            = self.package_location(locator)?;

        let manifest_text = self.project_cwd
            .with_join(&package_location)
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

        let cache_exists = if self.config.settings.enable_global_cache.value {
            self.global_cache_path().fs_exists()
        } else {
            self.local_cache_path().fs_exists()
        };

        if cache_exists {
            if let Some(install_state) = &self.install_state {
                if !self.last_modified_at.has_changed_since(install_state.last_installed_at) {
                    return Ok(());
                }
            }
        }

        let install = self.run_install(RunInstallOptions {
            check_checksums: false,
            check_resolutions: false,
            enforced_resolutions: BTreeMap::new(),
            prune_dev_dependencies: false,
            refresh_lockfile: false,
            silent_or_error: true,
            mode: None,
            roots: None,
            ..Default::default()
        });

        tokio::pin!(install);

        if !is_terminal() {
            install.await?;
            return Ok(());
        }

        tokio::select! {
            result = &mut install => {
                result?;
                return Ok(());
            },

            _ = tokio::time::sleep(Duration::from_millis(200)) => {
            }
        }

        let mut progress_handle
            = start_progress(|_| "Installing dependencies...".to_string());

        let install_result
            = install.await;

        progress_handle.stop();

        install_result?;

        Ok(())
    }

    pub async fn run_install(&mut self, options: RunInstallOptions) -> Result<InstallResult, Error> {
        // Useful for optimization purposes as we can reuse some information such as content flags.
        // Discard errors; worst case scenario we just recompute the whole state from scratch.
        if self.install_state.is_none() {
            let _ = self.import_install_state();
        }

        let report = StreamReport::new(StreamReportConfig {
            include_version: true,
            json: options.json,
            silent_or_error: options.silent_or_error,
            ..StreamReportConfig::from_config(&self.config)
        });

        let systems
            = self.config.settings.supported_architectures.to_systems();

        let background_writes
            = Arc::new(http_npm::BackgroundWrites::new());

        let drain_background_writes
            = background_writes.clone();

        let install_outcome = with_report_result(report, async {
            let package_cache
                = self.package_cache()?;

            let mut lockfile
                = self.lockfile();

            async_section("Project validation", async {
                let report_guard = current_report().await;
                let Some(report) = report_guard.as_ref() else {
                    return;
                };

                for workspace in self.workspaces.iter().skip(1) {
                    if workspace.manifest.resolutions.is_empty() {
                        continue;
                    }

                    report.warn(format!(
                        "{}: Resolutions field will be ignored",
                        workspace.pretty_name(),
                    ));
                }
            }).await;

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

            let install_context
                = InstallContext::default()
                    .with_package_cache(Some(&package_cache))
                    .with_project(Some(self))
                    .set_check_checksums(options.check_checksums)
                    .set_check_resolutions(options.check_resolutions)
                    .set_enforced_resolutions(options.enforced_resolutions)
                    .set_prune_dev_dependencies(options.prune_dev_dependencies)
                    .set_refresh_lockfile(options.refresh_lockfile)
                    .set_mode(options.mode)
                    .set_inline_builds(options.inline_builds)
                    .with_systems(Some(&systems))
                    .with_background_writes(Some(background_writes.clone()));

            let roots
                = self.workspaces.iter()
                    .filter(|w| options.roots.as_ref().map_or(true, |r| r.contains(&w.name)))
                    .map(|w| w.descriptor())
                    .collect();

            let install_result
                = InstallManager::new()
                    .with_context(install_context)
                    .with_lockfile(lockfile?)
                    .with_previous_state(self.install_state.as_ref())
                    .with_roots(roots)
                    .with_constraints_check(!options.silent_or_error && self.config.settings.enable_constraints_checks.value && options.roots.is_none())
                    .with_skip_link_step(options.mode == Some(InstallMode::UpdateLockfile))
                    .with_skip_lockfile_update(options.roots.is_some())
                    .resolve_and_fetch().await?
                    .link_and_build(self).await?;

            Ok(install_result)
        }).await;

        // Always drain so the cache writes don't outlive the runtime.
        drain_background_writes.drain().await;

        install_outcome
    }

    /// Resolve a task and all its dependencies.
    pub fn resolve_task(&self, root_task: &TaskId) -> Result<ResolveTaskResult, Error> {
        let source_files
            = std::cell::RefCell::new(Vec::<Path>::new());

        let get_task_file = |ident: &Ident, path: Option<&str>| {
            let workspace = self.workspace_by_ident(ident).ok()?;
            let task_file_path = match path {
                Some(custom_path) => workspace.path.with_join_str(custom_path),
                None => workspace.taskfile_path(),
            };
            let content = task_file_path.fs_read_text().ok()?;
            let task_file = parse_taskfile(&content).ok()?;
            source_files.borrow_mut().push(task_file_path);
            Some(task_file)
        };

        let resolve_ident_glob = |glob: &zpm_primitives::IdentGlob, context: &Ident| {
            // Get the context workspace to check its dependencies
            let Some(context_ws) = self.workspace_by_ident(context).ok() else {
                return vec![];
            };

            // Only match workspaces that are dependencies of the context workspace
            self.workspaces
                .iter()
                .filter(|ws| {
                    glob.check(&ws.name)
                        && (context_ws.manifest.remote.dependencies.contains_key(&ws.name)
                            || context_ws.manifest.dev_dependencies.contains_key(&ws.name))
                })
                .map(|ws| ws.name.clone())
                .collect()
        };

        let is_dependency = |workspace: &Ident, include_ident: &Ident| {
            let Some(ws) = self.workspace_by_ident(workspace).ok() else {
                return false;
            };

            // Check if include_ident is in workspace's dependencies or devDependencies
            ws.manifest.remote.dependencies.contains_key(include_ident)
                || ws.manifest.dev_dependencies.contains_key(include_ident)
        };

        let resolved = zpm_tasks::resolve(root_task, get_task_file, resolve_ident_glob, is_dependency)
            .map_err(Error::TaskResolveError)?;

        Ok(ResolveTaskResult {
            resolved,
            source_files: source_files.into_inner(),
        })
    }

    /// Get the parsed taskfile and its source file paths for a workspace.
    /// Returns `None` if the taskfile doesn't exist or fails to parse.
    pub fn get_workspace_taskfile(&self, workspace: &Workspace) -> Option<(TaskFile, Vec<Path>)> {
        let task_file_path = workspace.taskfile_path();
        let content = task_file_path.fs_read_text().ok()?;
        let task_file = parse_taskfile(&content).ok()?;

        let mut sources = vec![task_file_path];

        for include in &task_file.includes {
            if let Ok(inc_ws) = self.workspace_by_ident(&include.ident) {
                let inc_path = match &include.path {
                    Some(p) => inc_ws.path.with_join_str(p),
                    None => inc_ws.taskfile_path(),
                };
                sources.push(inc_path);
            }
        }

        Some((task_file, sources))
    }
}

/// Result of resolving a task, including the source files that were read.
pub struct ResolveTaskResult {
    pub resolved: ResolvedTasks,
    pub source_files: Vec<Path>,
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

    /// Berry-compatible `prettyWorkspace`: unnamed workspaces append
    /// a 6-hex sha512 of the relative cwd so collisions are visible.
    pub fn pretty_name(&self) -> String {
        use sha2::{Digest, Sha512};

        if self.manifest.name.is_some() {
            return self.name.to_print_string();
        }

        let rel_input = if self.rel_path == Path::new() {
            ".".to_string()
        } else {
            self.rel_path.to_file_string()
        };

        let digest = Sha512::digest(rel_input.as_bytes());
        let hex_prefix = &hex::encode(&digest)[..6];

        format!("{}-{}", self.name.as_str(), hex_prefix)
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

    pub fn manifest_path(&self) -> Path {
        self.path.with_join_str(MANIFEST_NAME)
    }

    pub fn taskfile_path(&self) -> Path {
        self.path.with_join_str(TASK_FILE_NAME)
    }

    pub async fn workspaces(&self) -> Result<Vec<Workspace>, Error> {
        let mut workspaces = vec![];

        if !self.manifest.workspaces.is_empty() {
            let patterns = &self.manifest.workspaces.packages;
            let roots
                = patterns.iter().filter_map(|pattern| {
                    if pattern.starts_with('!') {
                        return None;
                    }

                    let Ok(glob) = Glob::parse(pattern.as_str()) else {
                        return None;
                    };

                    let Ok(prefix) = glob.prefix() else {
                        return None;
                    };

                    Some(prefix)
                })
                .collect::<Vec<_>>();

            let mut manifest_finder
                = CachedManifestFinder::new(self.path.clone(), roots);

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
                            matches!(entry, CacheEntry::File(_, _) | CacheEntry::Error(_))
                        })
                        .filter(|(p, _)| {
                            let candidate_workspace_rel_dir
                                = p.dirname()
                                    .expect("Expected this path to have a parent directory, since it's supposed to be the relative path to a package.json file");

                            let dir_str
                                = candidate_workspace_rel_dir.as_str();

                            positive_glob_set.is_match(dir_str) && !negative_glob_set.is_match(dir_str)
                        })
                        .collect::<Vec<_>>();

                for (manifest_rel_path, cache_entry) in workspace_paths {
                    match cache_entry {
                        CacheEntry::File(last_changed_at, manifest) => {
                            let workspace_rel_path
                                = manifest_rel_path.dirname().unwrap();

                            if !processed_workspaces.insert(workspace_rel_path.clone()) {
                                continue;
                            }

                            workspaces.push(Workspace::from_info(&self.path, WorkspaceInfo {
                                rel_path: workspace_rel_path.clone(),
                                manifest: manifest.clone(),
                                last_changed_at: *last_changed_at,
                            })?);

                            if !manifest.workspaces.packages.is_empty() {
                                workspace_queue.push((workspace_rel_path, manifest.workspaces.packages.clone()));
                            }
                        },

                        CacheEntry::Error(error) => {
                            return Err(error.as_ref().unwrap().clone());
                        },

                        CacheEntry::Directory(_) => {
                            unreachable!();
                        },
                    }
                }
            }

            workspaces.sort_by(|w1, w2| {
                w1.rel_path.cmp(&w2.rel_path)
            });
        }

        Ok(workspaces)
    }
}
