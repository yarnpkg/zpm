use zpm_utils::{FromFileString, Path};

use crate::ConfigurationContext;

pub fn compute_cache_folder(context: &ConfigurationContext) -> Path {
    context.preferred_cwd()
        .map(|cwd| cwd.with_join_str(".yarn").with_join_str("cache"))
        .unwrap_or_else(Path::new)
}

/// The folder every release candidate used as `globalFolder`, regardless of
/// the platform conventions.
fn legacy_global_folder(home: &Path) -> Path {
    home.with_join_str(".yarn/zpm")
}

/// Where the global folder should live according to the platform conventions
/// (same rules as Berry's `getDefaultGlobalFolder`, with `zpm` instead of
/// `berry`). The environment values are passed explicitly so that every
/// platform can be tested from any other.
pub fn preferred_global_folder(is_windows: bool, home: Option<&Path>, local_app_data: Option<&Path>, xdg_data_home: Option<&Path>) -> Option<Path> {
    if is_windows {
        let base = local_app_data.cloned()
            .or_else(|| home.map(|home| home.with_join_str("AppData/Local")))?;

        return Some(base.with_join_str("Yarn/zpm"));
    }

    if let Some(xdg_data_home) = xdg_data_home {
        return Some(xdg_data_home.with_join_str("yarn/zpm"));
    }

    home.map(legacy_global_folder)
}

/// The default `globalFolder`. When the preferred location doesn't exist yet
/// but the one used by the release candidates (`~/.yarn/zpm`) does, we keep
/// using the latter so that upgrading doesn't silently orphan its content
/// (package cache, content-addressed store, Python builds, ...).
pub fn default_global_folder(is_windows: bool, home: Option<&Path>, local_app_data: Option<&Path>, xdg_data_home: Option<&Path>) -> Option<Path> {
    let preferred
        = preferred_global_folder(is_windows, home, local_app_data, xdg_data_home)?;

    if let Some(home) = home {
        let legacy
            = legacy_global_folder(home);

        if preferred != legacy && !preferred.fs_exists() && legacy.fs_exists() {
            return Some(legacy);
        }
    }

    Some(preferred)
}

/// Reads an absolute path from the environment; empty and relative values
/// are ignored (the XDG specification requires it for `XDG_DATA_HOME`).
fn env_absolute_path(context: &ConfigurationContext, name: &str) -> Option<Path> {
    context.env.get(name)
        .filter(|value| !value.is_empty())
        .and_then(|value| Path::from_file_string(value).ok())
        .filter(|path| path.is_absolute())
}

/// Computes the default `globalFolder` from the configuration environment.
/// Returns an empty path when no home directory can be found, which
/// `Configuration::load` reports as an error unless the setting is
/// explicitly set.
pub fn compute_global_folder(context: &ConfigurationContext) -> Path {
    let is_windows
        = cfg!(windows);

    // Same lookup order as `Path::home_dir`
    let home = env_absolute_path(context, "HOME")
        .or_else(|| is_windows.then(|| env_absolute_path(context, "USERPROFILE")).flatten());

    let local_app_data
        = env_absolute_path(context, "LOCALAPPDATA");
    let xdg_data_home
        = env_absolute_path(context, "XDG_DATA_HOME");

    default_global_folder(is_windows, home.as_ref(), local_app_data.as_ref(), xdg_data_home.as_ref())
        .unwrap_or_else(Path::empty)
}

pub fn default_task_concurrency() -> usize {
    std::thread::available_parallelism()
        .map_or(4, |n| n.get())
}

pub fn check_tsconfig(context: &ConfigurationContext) -> bool {
    // Probe project root and active package — monorepo workspaces
    // usually own the `tsconfig.json`, the root often doesn't.
    context.project_cwd.iter()
        .chain(context.package_cwd.iter())
        .any(|cwd| cwd.with_join_str("tsconfig.json").fs_exists())
}

/// True when running in a public-repo GitHub Actions PR workflow.
/// Drives the auto-enable of `enableHardenedMode` against fork PRs.
/// Reads from `context.env` so tests can stub without mutating the
/// process environment.
pub fn is_public_pr_ci(context: &ConfigurationContext) -> bool {
    if context.env.get("GITHUB_ACTIONS").map(String::as_str) != Some("true") {
        return false;
    }

    if context.env.get("GITHUB_EVENT_NAME").map(String::as_str) != Some("pull_request") {
        return false;
    }

    let Some(event_path) = context.env.get("GITHUB_EVENT_PATH") else {
        return false;
    };

    #[derive(serde::Deserialize)]
    struct Event {
        repository: Option<Repository>,
    }

    #[derive(serde::Deserialize)]
    struct Repository {
        private: Option<bool>,
    }

    let Ok(text) = std::fs::read_to_string(event_path) else {
        return false;
    };

    let Ok(event) = zpm_parsers::JsonDocument::hydrate_from_str::<Event>(&text) else {
        return false;
    };

    event.repository.and_then(|r| r.private) == Some(false)
}


#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use zpm_utils::{FromFileString, Path, ToFileString};

    use crate::ConfigurationContext;

    use super::{compute_global_folder, default_global_folder, preferred_global_folder};

    fn path(value: &str) -> Path {
        Path::from_file_string(value).unwrap()
    }

    #[test]
    fn it_should_default_to_the_home_folder() {
        let home
            = path("/home/user");

        assert_eq!(preferred_global_folder(false, Some(&home), None, None), Some(path("/home/user/.yarn/zpm")));
    }

    #[test]
    fn it_should_honour_xdg_data_home() {
        let home
            = path("/home/user");
        let xdg_data_home
            = path("/home/user/.local/share");

        assert_eq!(preferred_global_folder(false, Some(&home), None, Some(&xdg_data_home)), Some(path("/home/user/.local/share/yarn/zpm")));
        assert_eq!(preferred_global_folder(false, None, None, Some(&xdg_data_home)), Some(path("/home/user/.local/share/yarn/zpm")));
    }

    #[test]
    fn it_should_use_local_app_data_on_windows() {
        let home
            = path("/C:/Users/user");
        let local_app_data
            = path("/D:/AppData");
        let xdg_data_home
            = path("/C:/Users/user/.local/share");

        assert_eq!(preferred_global_folder(true, Some(&home), Some(&local_app_data), None), Some(path("/D:/AppData/Yarn/zpm")));
        assert_eq!(preferred_global_folder(true, None, Some(&local_app_data), None), Some(path("/D:/AppData/Yarn/zpm")));

        // XDG_DATA_HOME isn't a Windows convention
        assert_eq!(preferred_global_folder(true, Some(&home), Some(&local_app_data), Some(&xdg_data_home)), Some(path("/D:/AppData/Yarn/zpm")));
    }

    #[test]
    fn it_should_fallback_to_the_home_app_data_on_windows() {
        let home
            = path("/C:/Users/user");

        assert_eq!(preferred_global_folder(true, Some(&home), None, None), Some(path("/C:/Users/user/AppData/Local/Yarn/zpm")));
    }

    #[test]
    fn it_should_return_nothing_without_a_home_folder() {
        assert_eq!(preferred_global_folder(false, None, None, None), None);
        assert_eq!(preferred_global_folder(true, None, None, None), None);
        assert_eq!(default_global_folder(false, None, None, None), None);
    }

    #[test]
    fn it_should_keep_using_the_legacy_folder_until_the_preferred_one_exists() {
        let home
            = Path::temp_dir().unwrap();
        let xdg_data_home
            = home.with_join_str(".local/share");

        let legacy
            = home.with_join_str(".yarn/zpm");
        let preferred
            = xdg_data_home.with_join_str("yarn/zpm");

        // Nothing exists yet: we use the preferred location
        assert_eq!(default_global_folder(false, Some(&home), None, Some(&xdg_data_home)), Some(preferred.clone()));

        // A release candidate created the legacy folder: we keep using it
        legacy.fs_create_dir_all().unwrap();
        assert_eq!(default_global_folder(false, Some(&home), None, Some(&xdg_data_home)), Some(legacy.clone()));

        // Same thing on Windows
        let local_app_data
            = home.with_join_str("AppData/Local");
        assert_eq!(default_global_folder(true, Some(&home), Some(&local_app_data), None), Some(legacy.clone()));

        // Once the preferred folder exists, it wins
        preferred.fs_create_dir_all().unwrap();
        assert_eq!(default_global_folder(false, Some(&home), None, Some(&xdg_data_home)), Some(preferred.clone()));

        // And without XDG_DATA_HOME the legacy folder is the preferred one anyway
        assert_eq!(default_global_folder(false, Some(&home), None, None), Some(legacy.clone()));

        home.fs_rm().unwrap();
    }

    fn context(env: &[(&str, &str)]) -> ConfigurationContext {
        ConfigurationContext {
            env: env.iter().map(|(key, value)| (key.to_string(), value.to_string())).collect::<BTreeMap<_, _>>(),
            user_cwd: None,
            project_cwd: None,
            package_cwd: None,
        }
    }

    // The environment is read using the conventions of the current platform
    #[cfg(not(windows))]
    #[test]
    fn it_should_read_the_configuration_environment() {
        let home
            = Path::temp_dir().unwrap();
        let home_str
            = home.to_file_string();
        let xdg_data_home_str
            = format!("{home_str}/data");

        assert_eq!(compute_global_folder(&context(&[("HOME", &home_str)])), home.with_join_str(".yarn/zpm"));
        assert_eq!(compute_global_folder(&context(&[("HOME", &home_str), ("XDG_DATA_HOME", &xdg_data_home_str)])), home.with_join_str("data/yarn/zpm"));

        // Empty or relative XDG_DATA_HOME values are ignored
        assert_eq!(compute_global_folder(&context(&[("HOME", &home_str), ("XDG_DATA_HOME", "")])), home.with_join_str(".yarn/zpm"));
        assert_eq!(compute_global_folder(&context(&[("HOME", &home_str), ("XDG_DATA_HOME", "data")])), home.with_join_str(".yarn/zpm"));

        // No home folder means no default
        assert!(compute_global_folder(&context(&[])).is_empty());
        assert!(compute_global_folder(&context(&[("HOME", "")])).is_empty());

        home.fs_rm().unwrap();
    }
}
