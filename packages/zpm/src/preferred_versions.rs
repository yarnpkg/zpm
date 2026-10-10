use std::{collections::{BTreeMap, BTreeSet}, sync::{Arc, atomic::{AtomicUsize, Ordering}}};

use dashmap::DashMap;
use tokio::sync::OnceCell;
use zpm_primitives::{Ident, Range};
use zpm_semver::Version;
use zpm_utils::FromFileString;

use crate::{
    install::InstallContext,
    resolvers::npm,
};

/**
 * A package locked by a foreign lockfile, depending on another package
 * through a range we don't know yet (lockfiles such as pnpm's only store
 * the version each dependency got locked to; the range itself has to be
 * read from the registry metadata of the parent).
 */
#[derive(Clone, Debug)]
pub struct LockedDependent {
    pub parent_ident: Ident,
    pub parent_version: Version,

    /// The name under which the parent depends on the package; it differs
    /// from the package name when the dependency is an alias.
    pub dependency_name: Ident,

    pub version: Version,
}

/**
 * How often each version got locked for a given range. Votes from the
 * workspaces come first: they're the ones users see in their manifests,
 * so they win over the packages that happen to share the same range.
 */
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
struct Votes {
    workspaces: usize,
    packages: usize,
}

type VoteTable
    = BTreeMap<zpm_semver::Range, BTreeMap<Version, Votes>>;

#[derive(Debug, Default)]
pub struct PreferredVersionsStats {
    /// Descriptors resolved to a version from the foreign lockfile
    pub reused: AtomicUsize,

    /// Descriptors whose package was locked, but not to any version
    /// satisfying their range (typically because the range got bumped)
    pub unmatched: AtomicUsize,
}

/**
 * Versions locked by another package manager. When a project migrates to
 * Yarn without a `yarn.lock`, the npm resolver prefers them over whatever
 * the registry would otherwise give us, so that migrating doesn't silently
 * upgrade the dependency tree.
 *
 * Unlike a regular lockfile, the preferences aren't keyed by descriptor:
 * foreign lockfiles don't always store the ranges they resolved (pnpm only
 * stores them for the workspaces), and the ranges Yarn resolves may differ
 * anyway once catalogs and resolutions are applied. Instead, a descriptor
 * picks the locked version that satisfies its range; when several of them
 * do, it picks the one that the packages declaring this exact range were
 * locked to, which we find by reading their manifests.
 */
#[derive(Debug, Default)]
pub struct PreferredVersions {
    locked_versions: BTreeMap<Ident, BTreeSet<Version>>,
    declared_votes: BTreeMap<Ident, VoteTable>,
    declared_tags: BTreeMap<(Ident, String), BTreeSet<Version>>,
    dependents: BTreeMap<Ident, Vec<LockedDependent>>,

    vote_tables: DashMap<Ident, Arc<OnceCell<Arc<VoteTable>>>>,
    parent_dependencies: DashMap<(Ident, Version), Arc<OnceCell<Option<Arc<BTreeMap<String, String>>>>>>,

    pub stats: PreferredVersionsStats,
}

impl PreferredVersions {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_empty(&self) -> bool {
        self.locked_versions.is_empty()
    }

    pub fn package_count(&self) -> usize {
        self.locked_versions.values()
            .map(|versions| versions.len())
            .sum()
    }

    pub fn add_locked_version(&mut self, ident: Ident, version: Version) {
        self.locked_versions.entry(ident)
            .or_default()
            .insert(version);
    }

    /// Records that something (typically a workspace) declared the given
    /// range and got the given version for it.
    pub fn add_declared_range(&mut self, ident: Ident, range: zpm_semver::Range, version: Version) {
        self.declared_votes.entry(ident.clone())
            .or_default()
            .entry(range)
            .or_default()
            .entry(version.clone())
            .or_default()
            .workspaces += 1;

        self.add_locked_version(ident, version);
    }

    /// Records that something (typically a workspace) declared the given
    /// dist-tag (`latest`, `next`...) and got the given version for it.
    pub fn add_declared_tag(&mut self, ident: Ident, tag: String, version: Version) {
        self.declared_tags.entry((ident.clone(), tag))
            .or_default()
            .insert(version.clone());

        self.add_locked_version(ident, version);
    }

    /**
     * Returns the version a dist-tag descriptor should resolve to, if the
     * package manager we migrate from locked it. When the tag got locked to
     * several versions (different workspaces installed at different times),
     * the highest one wins.
     */
    pub fn pick_tag(&self, ident: &Ident, tag: &str, is_available: impl Fn(&Version) -> bool) -> Option<Version> {
        let picked
            = self.declared_tags.get(&(ident.clone(), tag.to_string()))?
                .iter()
                .rev()
                .find(|version| is_available(version))
                .cloned();

        let counter = match picked {
            Some(_) => &self.stats.reused,
            None => &self.stats.unmatched,
        };

        counter.fetch_add(1, Ordering::Relaxed);

        picked
    }

    pub fn add_dependent(&mut self, ident: Ident, dependent: LockedDependent) {
        self.add_locked_version(ident.clone(), dependent.version.clone());

        self.dependents.entry(ident)
            .or_default()
            .push(dependent);
    }

    /**
     * Returns the locked version a descriptor for the given package should
     * resolve to, if any. Only versions for which `is_available` returns
     * true are considered (ie. those the registry still knows about).
     */
    pub async fn pick(&self, context: &InstallContext<'_>, ident: &Ident, range: &zpm_semver::Range, is_available: impl Fn(&Version) -> bool) -> Option<Version> {
        let locked_versions
            = self.locked_versions.get(ident)?;

        let candidates
            = locked_versions.iter()
                .filter(|version| range.check(version) || (range.is_wildcard() && range.check_ignore_rc(*version)))
                .filter(|version| is_available(version))
                .collect::<Vec<_>>();

        let picked = match candidates.as_slice() {
            [] => {
                None
            },

            [version] => {
                Some((*version).clone())
            },

            _ => {
                let vote_table
                    = self.vote_table(context, ident).await;

                let votes
                    = vote_table.get(range);

                // Ties (including the case where nobody declared this exact
                // range) go to the highest version, like a regular resolution
                candidates.iter()
                    .max_by_key(|version| (votes.and_then(|votes| votes.get(**version)).copied().unwrap_or_default(), (**version).clone()))
                    .map(|version| (*version).clone())
            },
        };

        let counter = match picked {
            Some(_) => &self.stats.reused,
            None => &self.stats.unmatched,
        };

        counter.fetch_add(1, Ordering::Relaxed);

        picked
    }

    async fn vote_table(&self, context: &InstallContext<'_>, ident: &Ident) -> Arc<VoteTable> {
        let cell
            = self.vote_tables
                .entry(ident.clone())
                .or_default()
                .clone();

        let table = cell.get_or_init(|| async {
            let mut table
                = self.declared_votes.get(ident)
                    .cloned()
                    .unwrap_or_default();

            let dependents
                = self.dependents.get(ident)
                    .map(|dependents| dependents.as_slice())
                    .unwrap_or_default();

            let parent_dependencies
                = futures::future::join_all(dependents.iter().map(|dependent| {
                    self.parent_dependencies(context, &dependent.parent_ident, &dependent.parent_version)
                })).await;

            for (dependent, dependencies) in dependents.iter().zip(parent_dependencies) {
                let declared_range = dependencies.as_ref()
                    .and_then(|dependencies| dependencies.get(dependent.dependency_name.as_str()))
                    .and_then(|declared| parse_declared_range(declared, ident));

                let Some(declared_range) = declared_range else {
                    continue;
                };

                table.entry(declared_range)
                    .or_default()
                    .entry(dependent.version.clone())
                    .or_default()
                    .packages += 1;
            }

            Arc::new(table)
        }).await;

        table.clone()
    }

    async fn parent_dependencies(&self, context: &InstallContext<'_>, ident: &Ident, version: &Version) -> Option<Arc<BTreeMap<String, String>>> {
        let cell
            = self.parent_dependencies
                .entry((ident.clone(), version.clone()))
                .or_default()
                .clone();

        let dependencies = cell.get_or_init(|| async {
            npm::fetch_declared_dependencies(context, ident, version).await
                .ok()
                .map(Arc::new)
        }).await;

        dependencies.clone()
    }
}

/**
 * Extracts the semver range a manifest declared for the given package,
 * following `npm:` aliases.
 */
fn parse_declared_range(declared: &str, ident: &Ident) -> Option<zpm_semver::Range> {
    match Range::from_file_string(declared).ok()? {
        Range::AnonymousSemver(params)
            => Some(params.range),

        Range::RegistrySemver(params) if params.ident.as_ref().map_or(true, |inner| inner == ident)
            => Some(params.range),

        _ => None,
    }
}
