use std::convert::Infallible;

use zpm_macro_enum::zpm_enum;
use zpm_utils::FromFileString;

use crate::{
    locator::Locator,
    reference::{ShorthandReference, WorkspaceIdentReference},
    Ident,
};

#[macro_export]
macro_rules! dependency_map {
    ($($locator:expr),+ $(,)?) => {
        BTreeMap::from_iter([$(($locator.ident.clone(), $locator)),+])
    };
}

pub fn i(name: &str) -> Ident {
    Ident::new(name)
}

pub fn l(name: &str) -> Locator {
    #[zpm_enum(error = Infallible)]
    enum NamePattern {
        #[pattern(spec = r"(?<ident>.*)@(?<version>[0-9]+)")]
        Locator {
            ident: Ident,
            version: String,
        },

        #[pattern(spec = r"(?<ident>workspace-.*)")]
        WorkspaceIdent {
            ident: Ident,
        },

        #[pattern(spec = r"(?<ident>.*)")]
        Ident {
            ident: Ident,
        },
    }

    let name_pattern: NamePattern
        = NamePattern::from_file_string(name)
            .unwrap();

    let locator = match name_pattern {
        NamePattern::Locator(params) => {
            Locator::new(params.ident.clone(), ShorthandReference {
                version: zpm_semver::Version::new_from_components(params.version.parse().unwrap(), 0, 0, None),
            }.into())
        },

        NamePattern::WorkspaceIdent(params) => {
            Locator::new(params.ident.clone(), WorkspaceIdentReference {
                ident: params.ident.clone(),
            }.into())
        },

        NamePattern::Ident(params) => {
            Locator::new(params.ident.clone(), ShorthandReference {
                version: zpm_semver::Version::default(),
            }.into())
        },
    };

    locator
}
