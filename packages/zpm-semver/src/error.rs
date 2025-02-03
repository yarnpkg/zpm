#[derive(thiserror::Error, Clone, Debug)]
pub enum Error {
    #[error("Invalid semver range: {0}")]
    InvalidRange(String),

    #[error("Invalid semver version: {0}")]
    InvalidVersion(String),
}
