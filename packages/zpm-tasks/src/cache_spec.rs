use serde::Serialize;

use crate::{
    ast::Task,
    error::Error,
};

pub const CACHE_ATTRIBUTE: &str = "cache";
pub const INPUTS_ATTRIBUTE: &str = "inputs";
pub const OUTPUTS_ATTRIBUTE: &str = "outputs";
pub const ENV_ATTRIBUTE: &str = "env";
pub const LONG_LIVED_ATTRIBUTE: &str = "long-lived";

/// Token of `@inputs(...)` standing for the default input set, similar to
/// Turborepo's `$TURBO_DEFAULT$`.
pub const DEFAULT_INPUTS_TOKEN: &str = "@default";

/// Caching configuration of a task, as declared through its attributes:
///
/// ```text
/// @cache
/// @inputs(src/** tsconfig.json)
/// @outputs(dist/**)
/// @env(NODE_ENV API_URL)
/// build: ^build
///   tsc
/// ```
///
/// Values are whitespace-separated lists; an attribute may be repeated, in
/// which case the lists are concatenated. Patterns prefixed by `!` are
/// exclusions.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct CacheSpec {
    /// Globs (relative to the workspace) selecting the input files. When
    /// `None`, all the files of the workspace that aren't ignored by git
    /// are used.
    pub inputs: Option<Vec<String>>,

    /// Whether the default input set (all the files of the workspace that
    /// aren't ignored by git) is included in addition to `inputs`. Set by
    /// the `@default` token in `@inputs(...)`, which lets a task add files
    /// from outside its workspace without listing its own files.
    pub default_inputs: bool,

    /// Globs (relative to the workspace) selecting the files to store in
    /// the cache and to restore on cache hits.
    pub outputs: Vec<String>,

    /// Names of the environment variables whose values are part of the
    /// cache key. A trailing `*` matches any variable with that prefix.
    pub env: Vec<String>,
}

fn split_list(value: &Option<String>) -> Vec<String> {
    value.as_deref()
        .unwrap_or_default()
        .split_whitespace()
        .map(|s| s.to_string())
        .collect()
}

impl Task {
    pub fn has_attribute(&self, name: &str) -> bool {
        self.attributes.iter().any(|attr| attr.name == name)
    }

    pub fn is_long_lived(&self) -> bool {
        self.has_attribute(LONG_LIVED_ATTRIBUTE)
    }

    /// Returns the caching configuration of the task, or `None` if the
    /// task isn't cached. Fails when the attributes are inconsistent.
    pub fn cache_spec(&self) -> Result<Option<CacheSpec>, Error> {
        let mut is_cached
            = false;

        let mut spec
            = CacheSpec::default();

        let mut has_cache_details
            = false;

        for attribute in &self.attributes {
            match attribute.name.as_str() {
                CACHE_ATTRIBUTE => {
                    if attribute.value.is_some() {
                        return Err(Error::InvalidAttribute("@cache doesn't accept a value".to_string()));
                    }

                    is_cached = true;
                },

                INPUTS_ATTRIBUTE => {
                    has_cache_details = true;

                    let inputs
                        = spec.inputs.get_or_insert_with(Vec::new);

                    for input in split_list(&attribute.value) {
                        match input.as_str() {
                            DEFAULT_INPUTS_TOKEN => {
                                spec.default_inputs = true;
                            },

                            _ => {
                                inputs.push(input);
                            },
                        }
                    }
                },

                OUTPUTS_ATTRIBUTE => {
                    has_cache_details = true;
                    spec.outputs.extend(split_list(&attribute.value));
                },

                ENV_ATTRIBUTE => {
                    has_cache_details = true;
                    spec.env.extend(split_list(&attribute.value));
                },

                _ => {},
            }
        }

        if !is_cached {
            if has_cache_details {
                return Err(Error::InvalidAttribute("@inputs, @outputs, and @env require @cache".to_string()));
            }

            return Ok(None);
        }

        if self.is_long_lived() {
            return Err(Error::InvalidAttribute("long-lived tasks cannot be cached".to_string()));
        }

        Ok(Some(spec))
    }
}

#[cfg(test)]
mod tests {
    use crate::parse;

    use super::*;

    #[test]
    fn test_no_cache() {
        let tf = parse("build:\n  tsc").unwrap();
        assert_eq!(tf.tasks["build"].cache_spec().unwrap(), None);
    }

    #[test]
    fn test_cache_default_inputs_token() {
        let tf = parse("@cache\n@inputs(@default ../shared/**)\nbuild:\n  tsc").unwrap();

        assert_eq!(tf.tasks["build"].cache_spec().unwrap(), Some(CacheSpec {
            inputs: Some(vec!["../shared/**".to_string()]),
            default_inputs: true,
            ..CacheSpec::default()
        }));
    }

    #[test]
    fn test_cache_defaults() {
        let tf = parse("@cache\nbuild:\n  tsc").unwrap();
        assert_eq!(tf.tasks["build"].cache_spec().unwrap(), Some(CacheSpec::default()));
    }

    #[test]
    fn test_cache_full() {
        let tf = parse("@cache\n@inputs(src/** !src/**/*.test.ts)\n@inputs(tsconfig.json)\n@outputs(dist/** lib/**)\n@env(NODE_ENV NEXT_PUBLIC_*)\nbuild:\n  tsc").unwrap();

        assert_eq!(tf.tasks["build"].cache_spec().unwrap(), Some(CacheSpec {
            inputs: Some(vec!["src/**".to_string(), "!src/**/*.test.ts".to_string(), "tsconfig.json".to_string()]),
            outputs: vec!["dist/**".to_string(), "lib/**".to_string()],
            env: vec!["NODE_ENV".to_string(), "NEXT_PUBLIC_*".to_string()],
            default_inputs: false,
        }));
    }

    #[test]
    fn test_empty_inputs() {
        let tf = parse("@cache\n@inputs()\nbuild:\n  tsc").unwrap();
        assert_eq!(tf.tasks["build"].cache_spec().unwrap().unwrap().inputs, Some(vec![]));
    }

    #[test]
    fn test_details_without_cache() {
        let tf = parse("@outputs(dist/**)\nbuild:\n  tsc").unwrap();
        assert!(tf.tasks["build"].cache_spec().is_err());
    }

    #[test]
    fn test_long_lived_cannot_be_cached() {
        let tf = parse("@cache\n@long-lived\ndev:\n  vite").unwrap();
        assert!(tf.tasks["dev"].cache_spec().is_err());
    }
}
