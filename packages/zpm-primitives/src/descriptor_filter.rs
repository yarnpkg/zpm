use zpm_macro_enum::zpm_enum;
use zpm_utils::{impl_file_string_from_str, impl_file_string_serialization, ToFileString, ToHumanString};

use crate::{
    DescriptorError,
    Ident,
};

#[zpm_enum(error = DescriptorError, or_else = |s| Err(DescriptorError::SyntaxError(s.to_string())))]
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[derive_variants(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum FilterDescriptor {
    #[pattern(spec = "(?<ident>@?[^@]+)")]
    Ident {
        ident: Ident,
    },

    #[pattern(spec = "(?<ident>@?[^@]+)@(?<range>.*)")]
    Range {
        ident: Ident,
        range: zpm_semver::Range,
    },
}

impl FilterDescriptor {
    pub fn ident(&self) -> &Ident {
        match self {
            FilterDescriptor::Ident(params) => {
                &params.ident
            },

            FilterDescriptor::Range(params) => {
                &params.ident
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
                params.ident.to_file_string()
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
                params.ident.to_print_string()
            },
        }
    }
}

impl_file_string_from_str!(FilterDescriptor);
impl_file_string_serialization!(FilterDescriptor);
