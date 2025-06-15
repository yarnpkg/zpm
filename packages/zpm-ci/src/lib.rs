use serde::Serialize;

#[derive(Serialize)]
pub enum Provider {
    GitHubActions,
    GitLab,
    Unknown,
}

pub fn is_ci() -> Option<Provider> {
    if std::env::var("GITHUB_ACTIONS").is_ok() {
        Some(Provider::GitHubActions)
    } else if std::env::var("GITLAB_CI").is_ok() {
        Some(Provider::GitLab)
    } else if std::env::var("CI").is_ok() {
        Some(Provider::Unknown)
    } else {
        None
    }
}
