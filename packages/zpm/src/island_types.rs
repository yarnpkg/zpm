use std::fmt;

use pubgrub::{Ranges, VersionSet};
use smallvec::SmallVec;
use zpm_primitives::{Descriptor, Ident, Locator, Reference, Registry};
use zpm_semver::pubgrub::PubgrubVersion;
use zpm_utils::ToFileString;

// ---------------------------------------------------------------------------
// IslandPackage — the pubgrub P type
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Eq, PartialEq, Hash, PartialOrd, Ord)]
pub enum IslandRegistry {
    Npm,
    Pypi,
    Workspace,
    Other,
}

impl fmt::Display for IslandRegistry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            IslandRegistry::Npm => write!(f, "npm"),
            IslandRegistry::Pypi => write!(f, "pypi"),
            IslandRegistry::Workspace => write!(f, "workspace"),
            IslandRegistry::Other => write!(f, "other"),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Hash, PartialOrd, Ord)]
pub struct IslandPackageKey {
    /// Canonical source-package identity. Aliases remain dependency-edge
    /// metadata and don't create independent resolution slots.
    pub ident: Ident,
    pub registry: IslandRegistry,
}

impl IslandPackageKey {
    pub fn new(ident: Ident, registry: IslandRegistry) -> Self {
        Self {
            ident,
            registry,
        }
    }

    pub fn from_descriptor(descriptor: &Descriptor) -> Self {
        match descriptor.range.registry(&descriptor.ident) {
            Registry::None => Self::new(descriptor.ident.clone(), IslandRegistry::Other),
            registry => Self::from_registry(registry),
        }
    }

    pub fn from_locator(locator: &Locator) -> Self {
        match locator.reference.physical_reference() {
            Reference::Registry(params) => Self::new(params.ident.clone(), IslandRegistry::Npm),
            Reference::Shorthand(_) => Self::new(locator.ident.clone(), IslandRegistry::Npm),
            Reference::PypiRegistry(params) => Self::new(params.ident.clone(), IslandRegistry::Pypi),
            Reference::PypiShorthand(_) => Self::new(locator.ident.clone(), IslandRegistry::Pypi),
            Reference::WorkspaceIdent(params) => Self::new(params.ident.clone(), IslandRegistry::Workspace),
            Reference::WorkspacePath(_) => Self::new(locator.ident.clone(), IslandRegistry::Workspace),
            _ => Self::new(locator.ident.clone(), IslandRegistry::Other),
        }
    }

    pub fn from_registry(registry: Registry) -> Self {
        match registry {
            Registry::Npm(ident) => Self::new(ident, IslandRegistry::Npm),
            Registry::Pypi(ident) => Self::new(ident, IslandRegistry::Pypi),
            Registry::Workspace(ident) => Self::new(ident, IslandRegistry::Workspace),
            Registry::None => Self::new(Ident::default(), IslandRegistry::Other),
        }
    }
}

impl fmt::Display for IslandPackageKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.registry, self.ident)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Hash, PartialOrd, Ord)]
pub enum IslandPackage {
    Root,
    Named(IslandPackageKey),
    ExtraProxy {
        key: IslandPackageKey,
        extra: String,
    },
    ExtraFeature {
        key: IslandPackageKey,
        extra: String,
    },
}

impl fmt::Display for IslandPackage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            IslandPackage::Root => write!(f, "<root>"),
            IslandPackage::Named(key) => write!(f, "{}", key),
            IslandPackage::ExtraProxy { key, extra } |
            IslandPackage::ExtraFeature { key, extra } => write!(f, "{}[{}]", key, extra),
        }
    }
}

impl IslandPackage {
    pub fn ident(&self) -> Option<&Ident> {
        match self {
            IslandPackage::Root => None,
            IslandPackage::Named(key)
            | IslandPackage::ExtraProxy { key, .. }
            | IslandPackage::ExtraFeature { key, .. } => Some(&key.ident),
        }
    }

    pub fn key(&self) -> Option<&IslandPackageKey> {
        match self {
            IslandPackage::Root => None,
            IslandPackage::Named(key)
            | IslandPackage::ExtraProxy { key, .. }
            | IslandPackage::ExtraFeature { key, .. } => Some(key),
        }
    }

    pub fn is_proxy(&self) -> bool {
        matches!(self, IslandPackage::ExtraProxy { .. } | IslandPackage::ExtraFeature { .. })
    }
}

// ---------------------------------------------------------------------------
// IslandVersion — the pubgrub V type, wrapping a Locator
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct IslandVersion(pub Locator);

impl IslandVersion {
    pub fn into_inner(self) -> Locator {
        self.0
    }

    /// Extract a semver version from the inner locator's reference, if it has
    /// one (Shorthand or Registry references). Returns `None` for non-semver
    /// references.
    pub fn version(&self) -> Option<zpm_semver::Version> {
        match self.0.reference.physical_reference() {
            Reference::Shorthand(params) => Some(params.version.clone()),
            Reference::Registry(params) => Some(params.version.clone()),
            _ => None,
        }
    }

    pub fn pypi_version(&self) -> Option<zpm_primitives::PypiVersion> {
        match self.0.reference.physical_reference() {
            Reference::PypiShorthand(params) => Some(params.version.clone()),
            Reference::PypiRegistry(params) => Some(params.version.clone()),
            _ => None,
        }
    }
}

impl fmt::Display for IslandVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0.to_file_string())
    }
}

// ---------------------------------------------------------------------------
// ExactSet — set algebra for non-semver version sets
//
// Two variants cover all cases:
//   OneOf([])        = ∅        (empty)
//   OneOf([v])       = {v}      (singleton)
//   OneOf([a, b])    = {a, b}   (finite set)
//   NoneOf([])       = *        (full / universe)
//   NoneOf([v])      = ¬{v}     (complement of singleton)
//   NoneOf([a, b])   = ¬{a, b}  (everything except a and b)
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExactSet {
    OneOf(SmallVec<[IslandVersion; 1]>),
    NoneOf(SmallVec<[IslandVersion; 1]>),
}

impl ExactSet {
    pub fn one_of(versions: impl IntoIterator<Item = IslandVersion>) -> ExactSet {
        ExactSet::OneOf(versions.into_iter().collect())
    }

    pub fn complement(&self) -> ExactSet {
        match self {
            ExactSet::OneOf(vs) => ExactSet::NoneOf(vs.clone()),
            ExactSet::NoneOf(vs) => ExactSet::OneOf(vs.clone()),
        }
    }

    pub fn intersection(&self, other: &ExactSet) -> ExactSet {
        match (self, other) {
            // {a, b, ...} ∩ {c, d, ...} = elements in both
            (ExactSet::OneOf(a), ExactSet::OneOf(b)) => {
                let vs: SmallVec<_> = a.iter()
                    .filter(|v| b.contains(v))
                    .cloned()
                    .collect();
                ExactSet::OneOf(vs)
            }

            // {a, b, ...} ∩ ¬{c, d, ...} = elements in a not excluded by b
            (ExactSet::OneOf(vs), ExactSet::NoneOf(excluded))
            | (ExactSet::NoneOf(excluded), ExactSet::OneOf(vs)) => {
                let kept: SmallVec<_> = vs.iter()
                    .filter(|v| !excluded.contains(v))
                    .cloned()
                    .collect();
                ExactSet::OneOf(kept)
            }

            // ¬{a, b, ...} ∩ ¬{c, d, ...} = ¬(a ∪ b ∪ c ∪ d ∪ ...)
            (ExactSet::NoneOf(a), ExactSet::NoneOf(b)) => {
                let mut merged = a.clone();
                for v in b {
                    if !merged.contains(v) {
                        merged.push(v.clone());
                    }
                }
                ExactSet::NoneOf(merged)
            }
        }
    }

    pub fn contains(&self, v: &IslandVersion) -> bool {
        match self {
            ExactSet::OneOf(vs) => vs.contains(v),
            ExactSet::NoneOf(vs) => !vs.contains(v),
        }
    }

    pub fn is_empty(&self) -> bool {
        matches!(self, ExactSet::OneOf(vs) if vs.is_empty())
    }

    pub fn is_full(&self) -> bool {
        matches!(self, ExactSet::NoneOf(vs) if vs.is_empty())
    }
}

impl fmt::Display for ExactSet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ExactSet::OneOf(vs) if vs.is_empty() => write!(f, "∅"),
            ExactSet::NoneOf(vs) if vs.is_empty() => write!(f, "*"),
            ExactSet::OneOf(vs) | ExactSet::NoneOf(vs) => {
                if matches!(self, ExactSet::NoneOf(_)) {
                    write!(f, "¬")?;
                }
                write!(f, "{{ ")?;
                for (i, v) in vs.iter().enumerate() {
                    if i > 0 { write!(f, ", ")?; }
                    write!(f, "{}", v)?;
                }
                write!(f, " }}")
            }
        }
    }
}

// ---------------------------------------------------------------------------
// IslandVersionSet — the pubgrub VS type
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub enum IslandVersionSet {
    Semver(Ranges<PubgrubVersion>),
    Exact(ExactSet),
}

impl IslandVersionSet {
    pub fn from_semver_range(range: &zpm_semver::Range) -> IslandVersionSet {
        use zpm_semver::pubgrub::ToRanges;
        IslandVersionSet::Semver(range.to_ranges())
    }

    pub fn exact_singleton(v: IslandVersion) -> IslandVersionSet {
        IslandVersionSet::Exact(ExactSet::OneOf(SmallVec::from_elem(v, 1)))
    }

    fn is_semver_empty(&self) -> bool {
        matches!(self, IslandVersionSet::Semver(r) if *r == Ranges::empty())
    }

    fn is_semver_full(&self) -> bool {
        matches!(self, IslandVersionSet::Semver(r) if *r == Ranges::full())
    }

    fn is_exact_empty(&self) -> bool {
        matches!(self, IslandVersionSet::Exact(e) if e.is_empty())
    }

    fn is_exact_full(&self) -> bool {
        matches!(self, IslandVersionSet::Exact(e) if e.is_full())
    }

    fn is_logically_empty(&self) -> bool {
        self.is_semver_empty() || self.is_exact_empty()
    }

    fn is_logically_full(&self) -> bool {
        self.is_semver_full() || self.is_exact_full()
    }
}

impl PartialEq for IslandVersionSet {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (IslandVersionSet::Semver(a), IslandVersionSet::Semver(b)) => a == b,
            (IslandVersionSet::Exact(a), IslandVersionSet::Exact(b)) => a == b,
            // Cross-variant: only equal if both logically empty or both logically full
            _ => {
                (self.is_logically_empty() && other.is_logically_empty())
                    || (self.is_logically_full() && other.is_logically_full())
            }
        }
    }
}

impl Eq for IslandVersionSet {}

impl fmt::Display for IslandVersionSet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            IslandVersionSet::Semver(r) => write!(f, "{}", r),
            IslandVersionSet::Exact(e) => write!(f, "{}", e),
        }
    }
}

impl VersionSet for IslandVersionSet {
    type V = IslandVersion;

    fn empty() -> Self {
        IslandVersionSet::Semver(Ranges::empty())
    }

    fn singleton(v: Self::V) -> Self {
        match v.version() {
            Some(version) => {
                IslandVersionSet::Semver(Ranges::singleton(PubgrubVersion::new(version)))
            }
            None => {
                IslandVersionSet::Exact(ExactSet::OneOf(SmallVec::from_elem(v, 1)))
            }
        }
    }

    fn complement(&self) -> Self {
        match self {
            IslandVersionSet::Semver(r) => IslandVersionSet::Semver(r.complement()),
            IslandVersionSet::Exact(e) => IslandVersionSet::Exact(e.complement()),
        }
    }

    fn intersection(&self, other: &Self) -> Self {
        match (self, other) {
            (IslandVersionSet::Semver(a), IslandVersionSet::Semver(b)) => {
                IslandVersionSet::Semver(a.intersection(b))
            }
            (IslandVersionSet::Exact(a), IslandVersionSet::Exact(b)) => {
                IslandVersionSet::Exact(a.intersection(b))
            }
            // Cross-variant: semver and exact sets never share values (see
            // is_disjoint), so the intersection is empty unless one side is
            // the full set (in which case we return the other side).
            _ => {
                if self.is_logically_empty() || other.is_logically_empty() {
                    IslandVersionSet::empty()
                } else if self.is_logically_full() {
                    other.clone()
                } else if other.is_logically_full() {
                    self.clone()
                } else {
                    IslandVersionSet::empty()
                }
            }
        }
    }

    fn contains(&self, v: &Self::V) -> bool {
        match self {
            IslandVersionSet::Semver(r) => {
                match v.version() {
                    Some(version) => r.contains(&PubgrubVersion::new(version)),
                    None => false,
                }
            }
            IslandVersionSet::Exact(e) => e.contains(v),
        }
    }

    fn full() -> Self {
        IslandVersionSet::Semver(Ranges::full())
    }

    fn is_disjoint(&self, other: &Self) -> bool {
        match (self, other) {
            (IslandVersionSet::Semver(a), IslandVersionSet::Semver(b)) => a.is_disjoint(b),
            (IslandVersionSet::Exact(a), IslandVersionSet::Exact(b)) => {
                a.intersection(b).is_empty()
            }
            // Cross-variant: semver and exact never share values
            _ => true,
        }
    }

    fn subset_of(&self, other: &Self) -> bool {
        match (self, other) {
            (IslandVersionSet::Semver(a), IslandVersionSet::Semver(b)) => a.subset_of(b),
            (IslandVersionSet::Exact(a), IslandVersionSet::Exact(b)) => {
                *a == a.intersection(b)
            }
            // Cross-variant: only if self is empty
            _ => self.is_logically_empty(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zpm_utils::{FromFileString, Hash64, ToFileString};

    #[test]
    fn test_package_keys_are_registry_aware() {
        let npm
            = Descriptor::from_file_string("foo@npm:^1.0.0").unwrap();
        let pypi
            = Descriptor::from_file_string("foo@pypi:>=1.0.0").unwrap();

        assert_eq!(IslandRegistry::Npm, IslandPackageKey::from_descriptor(&npm).registry);
        assert_eq!(IslandRegistry::Pypi, IslandPackageKey::from_descriptor(&pypi).registry);
        assert_ne!(IslandPackageKey::from_descriptor(&npm), IslandPackageKey::from_descriptor(&pypi));
    }

    #[test]
    fn test_aliased_package_keys_round_trip_through_locators() {
        for (descriptor, locator) in [
            ("alias@npm:foo@^1.0.0", "alias@npm:foo@1.2.0"),
            ("alias@pypi:foo@>=1.0.0", "alias@pypi:foo@1.2.0"),
        ] {
            let descriptor
                = Descriptor::from_file_string(descriptor).unwrap();
            let locator
                = Locator::from_file_string(locator).unwrap();

            assert_eq!(
                IslandPackageKey::from_descriptor(&descriptor),
                IslandPackageKey::from_locator(&locator),
            );
        }
    }

    #[test]
    fn test_aliases_share_their_canonical_source_key() {
        let first
            = Descriptor::from_file_string("first@pypi:foo@>=1.0.0").unwrap();
        let second
            = Descriptor::from_file_string("second@pypi:foo@<2.0.0").unwrap();

        assert_eq!(
            IslandPackageKey::from_descriptor(&first),
            IslandPackageKey::from_descriptor(&second),
        );
    }

    #[test]
    fn test_pypi_version_uses_physical_env_locator() {
        let hash
            = Hash64::from_data("fork");
        let locator
            = Locator::from_file_string(&format!("foo@env:{}#pypi:foo@1.2.0", hash.to_file_string())).unwrap();
        let version
            = IslandVersion(locator);

        assert_eq!("1.2.0", version.pypi_version().unwrap().to_file_string());
        assert_eq!(None, version.version());
    }
}
