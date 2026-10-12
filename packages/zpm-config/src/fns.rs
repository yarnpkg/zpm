use zpm_utils::Path;

use crate::ConfigurationContext;

pub fn compute_cache_folder(context: &ConfigurationContext) -> Path {
    context.preferred_cwd()
        .map(|cwd| cwd.with_join_str(".yarn").with_join_str("cache"))
        .unwrap_or_else(Path::new)
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

