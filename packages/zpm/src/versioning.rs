use std::collections::{BTreeMap, BTreeSet};

use itertools::Itertools;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use zpm_macro_enum::zpm_enum;
use zpm_parsers::{JsonDocument, Document};
use zpm_primitives::Ident;
use zpm_semver::{BumpStrategy, PrereleasePattern};
use zpm_utils::{IoResultExt, Path, ToFileString};

use crate::{error::Error, git_utils::{fetch_branch_base, fetch_changed_files}, project::Project};

#[zpm_enum(error = zpm_utils::EnumError, or_else = |s| Err(zpm_utils::EnumError::NotFound(s.to_string())))]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[derive_variants(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ReleaseStrategy {
    #[literal("major")]
    Major,

    #[literal("minor")]
    Minor,

    #[literal("patch")]
    Patch,

    #[literal("premajor")]
    Premajor,

    #[literal("preminor")]
    Preminor,

    #[literal("prepatch")]
    Prepatch,

    #[literal("prerelease")]
    Prerelease,

    #[pattern(r"(?<version>.*)")]
    #[to_file_string(|params| params.version.to_file_string())]
    #[to_print_string(|params| params.version.to_file_string())]
    Exact {
        version: zpm_semver::Version,
    },
}

impl ReleaseStrategy {
    /// Returns the version a workspace at `current_version` would be bumped
    /// to. Exact versions are used as-is, ignoring the prerelease pattern.
    pub fn apply(&self, current_version: &zpm_semver::Version, prerelease: Option<&PrereleasePattern>) -> zpm_semver::Version {
        let strategy = match self {
            ReleaseStrategy::Major => BumpStrategy::Major,
            ReleaseStrategy::Minor => BumpStrategy::Minor,
            ReleaseStrategy::Patch => BumpStrategy::Patch,
            ReleaseStrategy::Premajor => BumpStrategy::Premajor,
            ReleaseStrategy::Preminor => BumpStrategy::Preminor,
            ReleaseStrategy::Prepatch => BumpStrategy::Prepatch,
            ReleaseStrategy::Prerelease => BumpStrategy::Prerelease,

            ReleaseStrategy::Exact(params) => {
                return params.version.clone();
            },
        };

        current_version.bump(strategy, prerelease)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct VersioningFile {
    #[serde(default)]
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub releases: BTreeMap<Ident, ReleaseStrategy>,

    #[serde(default)]
    #[serde(skip_serializing_if = "BTreeSet::is_empty")]
    pub declined: BTreeSet<Ident>,
}

pub struct Versioning<'a> {
    project: &'a Project,
}

pub struct ResolveOptions<'a> {
    pub prerelease: Option<&'a PrereleasePattern>,
}

impl<'a> Versioning<'a> {
    pub fn new(project: &'a Project) -> Self {
        Self { project }
    }

    fn create_versioning_path(&self) -> Result<Path, Error> {
        let nonce = rand::rng()
            .next_u32();

        let versioning_path
            = self.project.versioning_path().with_join_str(format!("{:08x}.json", nonce));

        Ok(versioning_path)
    }

    pub fn resolve_releases(&self, options: ResolveOptions) -> Result<BTreeMap<Ident, zpm_semver::Version>, Error> {
        let mut releases
            = BTreeMap::new();

        let versioning_dir
            = self.project.versioning_path();

        let versioning_files
            = versioning_dir.fs_read_dir()
                .ok_missing()?;

        let Some(versioning_files) = versioning_files else {
            return Ok(releases);
        };

        for versioning_file in versioning_files {
            let versioning_file = versioning_file?;
            let path = Path::try_from(versioning_file.path())?;

            if !path.fs_is_file() || path.extname() != Some(".json") {
                continue;
            }

            let content
                = path.fs_read_text_prealloc()
                    .ok_missing()?
                    .unwrap_or_else(|| "{}".to_string());

            let versioning_data: VersioningFile
                = JsonDocument::hydrate_from_str(&content)?;

            for (ident, release_strategy) in versioning_data.releases {
                let workspace
                    = self.project.workspace_by_ident(&ident)?;

                let current_version
                    = workspace.manifest.remote.version.as_ref()
                        .ok_or_else(|| Error::NoVersionFoundForWorkspace(ident.clone()))?;

                let resulting_version
                    = release_strategy.apply(current_version, options.prerelease);

                let is_highest_requested_version
                    = releases.get(&ident)
                        .map(|version| version < &resulting_version)
                        .unwrap_or(true);

                if is_highest_requested_version {
                    releases.insert(ident, resulting_version);
                }
            }
        }

        Ok(releases)
    }

    pub async fn versioning_path(&self) -> Result<Path, Error> {
        let Some(base) = fetch_branch_base(self.project).await.ok() else {
            return self.create_versioning_path();
        };

        let changed_files
            = fetch_changed_files(self.project, Some(&base)).await?;

        let versioning_path
            = self.project.versioning_path();

        let mut versioning_files
            = changed_files.into_iter()
                .filter(|file| versioning_path.contains(file))
                .collect_vec();

        if versioning_files.is_empty() {
            return self.create_versioning_path();
        }

        if versioning_files.len() > 1 {
            return Err(Error::MultipleVersioningFilesFound);
        }

        Ok(versioning_files.pop().unwrap())
    }

    pub fn discard_workspace_from_release(&self, workspace_ident: &Ident) -> Result<(), Error> {
        let versioning_dir
            = self.project.versioning_path();

        let versioning_files
            = versioning_dir.fs_read_dir()?;

        for versioning_file in versioning_files {
            let versioning_file = versioning_file?;
            let path = Path::try_from(versioning_file.path())?;

            if !path.fs_is_file() || path.extname() != Some(".json") {
                continue;
            }

            let content
                = path.fs_read_text_prealloc()
                    .ok_missing()?
                    .unwrap_or_else(|| "{}".to_string());

            let mut versioning_data: VersioningFile
                = JsonDocument::hydrate_from_str(&content)?;

            if versioning_data.releases.contains_key(workspace_ident) {
                versioning_data.releases.remove(workspace_ident);
                versioning_data.declined.insert(workspace_ident.clone());
            }

            if versioning_data.releases.is_empty() {
                path.fs_rm_file()?;
                continue;
            }

            let versioning_content
                = format!("{}\n", JsonDocument::to_string_pretty(&versioning_data)?);

            path.fs_change(&versioning_content, false)?;
        }

        Ok(())
    }

    pub fn set_manifest_version(&self, workspace_ident: &Ident, version: &zpm_semver::Version) -> Result<(), Error> {
        let manifest_path
            = self.project.workspace_by_ident(workspace_ident)?
                .manifest_path();

        let manifest_content = manifest_path
            .fs_read_prealloc()?;

        let mut document
            = JsonDocument::new(manifest_content)?;

        document.set_path(
            &zpm_parsers::Path::from_segments(vec!["version".to_string()]),
            zpm_parsers::Value::String(version.to_file_string()),
        )?;

        // The `stableVersion` field isn't used to compute bumps anymore; we
        // remove it once the workspace reaches a stable version, but leave it
        // alone on prereleases for the tools that may still read it.
        if version.rc.is_none() {
            document.set_path(
                &zpm_parsers::Path::from_segments(vec!["stableVersion".to_string()]),
                zpm_parsers::Value::Undefined,
            )?;
        }

        manifest_path
            .fs_change(&document.input, false)?;

        Ok(())
    }

    pub async fn set_workspace_release_strategy(&self, workspace_ident: &Ident, release_strategy: Option<ReleaseStrategy>) -> Result<(), Error> {
        let versioning_path
            = self.versioning_path().await?;

        let versioning_content
            = versioning_path.fs_read_text_prealloc()
                .ok_missing()?
                .unwrap_or_else(|| "{}".to_string());

        let mut versioning_data: VersioningFile
            = JsonDocument::hydrate_from_str(&versioning_content)?;

        if let Some(release_strategy) = release_strategy {
            versioning_data.releases.insert(workspace_ident.clone(), release_strategy);
            versioning_data.declined.remove(workspace_ident);
        } else {
            versioning_data.releases.remove(workspace_ident);
            versioning_data.declined.insert(workspace_ident.clone());
        }

        let versioning_content
            = format!("{}\n", JsonDocument::to_string_pretty(&versioning_data)?);

        versioning_path
            .fs_create_parent()?
            .fs_change(&versioning_content, false)?;

        Ok(())
    }
}
