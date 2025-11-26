use zpm_macro_enum::zpm_enum;
use zpm_utils::{ToFileString, ToHumanString, impl_file_string_from_str, impl_file_string_serialization};

use crate::{
    DescriptorError,
    Ident, IdentGlob,
};

#[zpm_enum(error = DescriptorError, or_else = |s| Err(DescriptorError::SyntaxError(s.to_string())))]
#[derive(Debug, Clone,)]
#[derive_variants(Debug, Clone)]
pub enum FilterDescriptor {
    #[pattern(spec = "(?<ident>@?[^@]+)")]
    Ident {
        ident: IdentGlob,
    },

    #[pattern(spec = "(?<ident>@?[^@]+)@(?<range>.*)")]
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

impl ToFileString for FilterDescriptor {
    fn to_file_string(&self) -> String {
        match self {
            FilterDescriptor::Ident(params) => {
                params.ident.to_file_string()
            },

            FilterDescriptor::Range(params) => {
                format!("{}@{}", params.ident.to_file_string(), params.range.to_file_string())
            },
        }
    }
}

impl ToHumanString for FilterDescriptor {
    fn to_print_string(&self) -> String {
        match self {
            FilterDescriptor::Ident(params) => {
                params.ident.to_print_string()
            },

            FilterDescriptor::Range(params) => {
                format!("{}{}", params.ident.to_print_string(), format!("@{}", params.range.to_print_string()))
            },
        }
    }
}

impl_file_string_from_str!(FilterDescriptor);
impl_file_string_serialization!(FilterDescriptor);
