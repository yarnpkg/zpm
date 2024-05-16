use crate::primitives::Ident;

pub fn registry_url_for(_: &Ident) -> String {
    std::env::var("YARN_NPM_REGISTRY_SERVER").unwrap_or_else(|_| "https://registry.npmjs.org".to_string())
}
