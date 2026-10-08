use globset::{
    GlobBuilder,
    GlobSet,
    GlobSetBuilder,
};
use zpm_utils::Path;

use crate::error::Error;

/// A list of globs as declared in `@inputs(...)` / `@outputs(...)`. Patterns
/// are relative to a base folder (the workspace, or the project root for the
/// global inputs); patterns prefixed by `!` are exclusions.
///
/// A pattern matching a folder also matches everything inside it, so
/// `dist` and `dist/**` are equivalent.
#[derive(Debug, Clone)]
pub struct PatternSet {
    includes: GlobSet,
    excludes: GlobSet,
    roots: Vec<String>,
    is_empty: bool,
}

const GLOB_METACHARS: &[char] = &['*', '?', '[', ']', '{', '}', '\\'];

fn normalize_pattern(pattern: &str) -> &str {
    let pattern
        = pattern.strip_prefix("./").unwrap_or(pattern);

    pattern.trim_end_matches('/')
}

/// Returns the leading components of the pattern that don't contain any
/// glob metacharacter; that's the folder (or file) we need to crawl to find
/// all the files the pattern could match.
fn literal_prefix(pattern: &str) -> String {
    let mut segments
        = Vec::new();

    for segment in pattern.split('/') {
        if segment.contains(GLOB_METACHARS) {
            break;
        }

        segments.push(segment);
    }

    segments.join("/")
}

fn build_glob(pattern: &str) -> Result<globset::Glob, Error> {
    GlobBuilder::new(pattern)
        .literal_separator(true)
        .backslash_escape(true)
        .build()
        .map_err(|err| Error::TaskCacheError(format!("Invalid glob pattern '{}': {}", pattern, err)))
}

impl PatternSet {
    pub fn new<S: AsRef<str>>(patterns: &[S]) -> Result<Self, Error> {
        let mut includes
            = GlobSetBuilder::new();
        let mut excludes
            = GlobSetBuilder::new();

        let mut roots
            = Vec::new();

        let mut is_empty
            = true;

        for raw in patterns {
            let raw
                = raw.as_ref();

            let (is_exclude, pattern)
                = match raw.strip_prefix('!') {
                    Some(pattern) => (true, pattern),
                    None => (false, raw),
                };

            let pattern
                = normalize_pattern(pattern);

            if pattern.is_empty() || pattern.starts_with('/') {
                return Err(Error::TaskCacheError(format!("Invalid glob pattern '{}': patterns must be relative", raw)));
            }

            let target
                = if is_exclude {&mut excludes} else {&mut includes};

            target.add(build_glob(pattern)?);
            target.add(build_glob(&format!("{}/**", pattern))?);

            if !is_exclude {
                is_empty = false;
                roots.push(literal_prefix(pattern));
            }
        }

        // No need to crawl a folder if one of its parents will be crawled anyway
        roots.sort();
        roots.dedup();

        let mut deduped_roots: Vec<String>
            = Vec::new();

        for root in roots {
            let is_covered = deduped_roots.iter().any(|parent| {
                parent.is_empty() || root == *parent || root.starts_with(&format!("{}/", parent))
            });

            if !is_covered {
                deduped_roots.push(root);
            }
        }

        let includes = includes.build()
            .map_err(|err| Error::TaskCacheError(err.to_string()))?;
        let excludes = excludes.build()
            .map_err(|err| Error::TaskCacheError(err.to_string()))?;

        Ok(Self {
            includes,
            excludes,
            roots: deduped_roots,
            is_empty,
        })
    }

    pub fn is_empty(&self) -> bool {
        self.is_empty
    }

    /// The paths (relative to the base folder) that must be crawled to find
    /// all the files that may match the set.
    pub fn roots(&self) -> &[String] {
        &self.roots
    }

    pub fn is_match(&self, rel_path: &str) -> bool {
        self.includes.is_match(rel_path) && !self.is_excluded(rel_path)
    }

    pub fn is_excluded(&self, rel_path: &str) -> bool {
        self.excludes.is_match(rel_path)
    }

    /// Whether every path under a folder matches the include patterns; only
    /// meaningful for exclusion lists (outputs), which have no excludes of
    /// their own to consider. `folder_marker` is a path inside the folder.
    pub fn includes_folder(&self, folder_marker: &str) -> bool {
        self.includes.is_match(folder_marker) && self.excludes.is_empty()
    }

    /// Whether the pattern set may reference files outside of its base
    /// folder (through `..` segments).
    pub fn escapes_base(&self) -> bool {
        self.roots.iter().any(|root| root == ".." || root.starts_with("../"))
    }

    pub fn root_paths(&self, base: &Path) -> Vec<Path> {
        self.roots.iter()
            .map(|root| base.with_join_str(root))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_match() {
        let set = PatternSet::new(&["src/**", "tsconfig.json"]).unwrap();

        assert!(set.is_match("src/index.ts"));
        assert!(set.is_match("src/deep/nested/file.ts"));
        assert!(set.is_match("tsconfig.json"));
        assert!(!set.is_match("tsconfig.build.json"));
        assert!(!set.is_match("dist/index.js"));
    }

    #[test]
    fn test_star_does_not_cross_folders() {
        let set = PatternSet::new(&["src/*.ts"]).unwrap();

        assert!(set.is_match("src/index.ts"));
        assert!(!set.is_match("src/deep/index.ts"));
    }

    #[test]
    fn test_folder_pattern_matches_contents() {
        let set = PatternSet::new(&["dist", "./lib/"]).unwrap();

        assert!(set.is_match("dist/index.js"));
        assert!(set.is_match("dist/a/b.js"));
        assert!(set.is_match("lib/index.js"));
        assert!(!set.is_match("distribution/index.js"));
    }

    #[test]
    fn test_excludes() {
        let set = PatternSet::new(&["src/**", "!src/**/*.test.ts", "!src/fixtures"]).unwrap();

        assert!(set.is_match("src/index.ts"));
        assert!(!set.is_match("src/index.test.ts"));
        assert!(!set.is_match("src/deep/index.test.ts"));
        assert!(!set.is_match("src/fixtures/a.ts"));
    }

    #[test]
    fn test_empty() {
        let set = PatternSet::new::<&str>(&[]).unwrap();

        assert!(set.is_empty());
        assert!(!set.is_match("anything"));
        assert!(set.roots().is_empty());
    }

    #[test]
    fn test_roots() {
        let set = PatternSet::new(&["src/**", "src/generated/*.ts", "tsconfig.json", "!dist", "assets/{a,b}/**"]).unwrap();
        assert_eq!(set.roots(), &["assets".to_string(), "src".to_string(), "tsconfig.json".to_string()]);

        let set = PatternSet::new(&["**/*.ts", "src/**"]).unwrap();
        assert_eq!(set.roots(), &["".to_string()]);
    }

    #[test]
    fn test_parent_patterns() {
        let set = PatternSet::new(&["../shared/config.json"]).unwrap();

        assert!(set.escapes_base());
        assert!(set.is_match("../shared/config.json"));
    }

    #[test]
    fn test_invalid_patterns() {
        assert!(PatternSet::new(&["/etc/passwd"]).is_err());
        assert!(PatternSet::new(&["src/[a"]).is_err());
        assert!(PatternSet::new(&["!"]).is_err());
    }
}
