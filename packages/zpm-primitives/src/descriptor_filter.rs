use zpm_macro_enum::zpm_enum;

use crate::{
    DescriptorError,
    Ident, IdentGlob,
};

#[zpm_enum(error = DescriptorError, or_else = |s| Err(DescriptorError::SyntaxError(s.to_string())))]
#[derive(Debug, Clone,)]
#[derive_variants(Debug, Clone)]
pub enum FilterDescriptor {
    #[pattern("(?<ident>@?[^@]+)")]
    #[to_file_string(|params| params.ident.to_file_string())]
    #[to_print_string(|params| params.ident.to_print_string())]
    Ident {
        ident: IdentGlob,
    },

    #[pattern("(?<ident>@?[^@]+)@(?<range>.*)")]
    #[to_file_string(|params| format!("{}@{}", params.ident.to_file_string(), params.range.to_file_string()))]
    #[to_print_string(|params| format!("{}@{}", params.ident.to_print_string(), params.range.to_print_string()))]
    Range {
        ident: IdentGlob,
        range: zpm_semver::Range,
    },
}

impl FilterDescriptor {
    pub fn check(&self, ident: &Ident, version: &zpm_semver::Version) -> bool {
        match self {
            FilterDescriptor::Ident(params) => {
                params.ident.check(ident)
            },

            FilterDescriptor::Range(params) => {
                params.ident.check(ident) && params.range.check(version)
            },
        }
    }
}
