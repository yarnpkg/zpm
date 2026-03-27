use std::fmt;

use pubgrub::{Ranges, VersionSet};
use zpm_primitives::{Locator, Reference};
use zpm_semver::pubgrub::PubgrubVersion;
use zpm_utils::ToFileString;

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
        match &self.0.reference {
            Reference::Shorthand(params) => Some(params.version.clone()),
            Reference::Registry(params) => Some(params.version.clone()),
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
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExactSet {
    Empty,
    Singleton(IslandVersion),
    Full,
    Complement(IslandVersion),
}

impl ExactSet {
    pub fn complement(&self) -> ExactSet {
        match self {
            ExactSet::Empty => ExactSet::Full,
            ExactSet::Full => ExactSet::Empty,
            ExactSet::Singleton(v) => ExactSet::Complement(v.clone()),
            ExactSet::Complement(v) => ExactSet::Singleton(v.clone()),
        }
    }

    pub fn intersection(&self, other: &ExactSet) -> ExactSet {
        match (self, other) {
            (ExactSet::Empty, _) | (_, ExactSet::Empty) => ExactSet::Empty,
            (ExactSet::Full, other) => other.clone(),
            (other, ExactSet::Full) => other.clone(),

            (ExactSet::Singleton(a), ExactSet::Singleton(b)) => {
                if a == b { ExactSet::Singleton(a.clone()) } else { ExactSet::Empty }
            }

            (ExactSet::Singleton(a), ExactSet::Complement(b))
            | (ExactSet::Complement(b), ExactSet::Singleton(a)) => {
                if a == b { ExactSet::Empty } else { ExactSet::Singleton(a.clone()) }
            }

            (ExactSet::Complement(a), ExactSet::Complement(b)) => {
                if a == b {
                    ExactSet::Complement(a.clone())
                } else {
                    panic!(
                        "ExactSet::intersection of two different complements is not representable: {:?} vs {:?}",
                        a, b
                    )
                }
            }
        }
    }

    pub fn contains(&self, v: &IslandVersion) -> bool {
        match self {
            ExactSet::Empty => false,
            ExactSet::Full => true,
            ExactSet::Singleton(inner) => inner == v,
            ExactSet::Complement(inner) => inner != v,
        }
    }

    pub fn is_empty(&self) -> bool {
        matches!(self, ExactSet::Empty)
    }

    pub fn is_full(&self) -> bool {
        matches!(self, ExactSet::Full)
    }
}

impl fmt::Display for ExactSet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ExactSet::Empty => write!(f, "∅"),
            ExactSet::Full => write!(f, "*"),
            ExactSet::Singleton(v) => write!(f, "{{ {} }}", v),
            ExactSet::Complement(v) => write!(f, "¬{{ {} }}", v),
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
        IslandVersionSet::Exact(ExactSet::Singleton(v))
    }

    fn is_semver_empty(&self) -> bool {
        matches!(self, IslandVersionSet::Semver(r) if *r == Ranges::empty())
    }

    fn is_semver_full(&self) -> bool {
        matches!(self, IslandVersionSet::Semver(r) if *r == Ranges::full())
    }

    fn is_exact_empty(&self) -> bool {
        matches!(self, IslandVersionSet::Exact(ExactSet::Empty))
    }

    fn is_exact_full(&self) -> bool {
        matches!(self, IslandVersionSet::Exact(ExactSet::Full))
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
                IslandVersionSet::Exact(ExactSet::Singleton(v))
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
            // Cross-variant: handle trivial cases (empty/full), panic otherwise
            _ => {
                if self.is_logically_empty() || other.is_logically_empty() {
                    IslandVersionSet::empty()
                } else if self.is_logically_full() {
                    other.clone()
                } else if other.is_logically_full() {
                    self.clone()
                } else {
                    panic!(
                        "IslandVersionSet: cross-variant intersection not supported: {:?} ∩ {:?}",
                        self, other
                    )
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
