use globset::{Glob, GlobBuilder, GlobMatcher};
use zpm_utils::{DataType, FromFileString, ToFileString, ToHumanString, impl_file_string_from_str, impl_file_string_serialization};

use crate::Ident;

#[derive(Debug, Clone)]
pub struct IdentGlob {
    pub glob: Glob,
    pub matcher: GlobMatcher,
}

impl IdentGlob {
    pub fn new(raw: &str) -> Result<Self, globset::Error> {
        let glob = GlobBuilder::new(raw)
            .literal_separator(false)
            .build()?;

        let matcher
            = glob.compile_matcher();

        Ok(Self {glob, matcher})
    }

    pub fn check(&self, ident: &Ident) -> bool {
        self.matcher.is_match(ident.as_str())
    }
}

impl FromFileString for IdentGlob {
    type Error = globset::Error;

    fn from_file_string(src: &str) -> Result<Self, Self::Error> {
        Ok(Self::new(src)?)
    }
}

impl ToFileString for IdentGlob {
    fn to_file_string(&self) -> String {
        self.glob.glob().to_string()
    }
}

impl ToHumanString for IdentGlob {
    fn to_print_string(&self) -> String {
        DataType::Ident.colorize(&self.to_file_string())
    }
}

impl_file_string_from_str!(IdentGlob);
impl_file_string_serialization!(IdentGlob);
