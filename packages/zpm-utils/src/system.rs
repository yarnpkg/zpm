pub enum CiProvider {
    Unknown,
    GitHubActions,
    GitLab,
}

pub fn is_ci() -> bool {
    std::env::var("CI").is_ok()
}

pub fn get_system_string() -> &'static str {
    env!("TARGET")
}
