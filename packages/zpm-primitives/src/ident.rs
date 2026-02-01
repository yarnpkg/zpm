use std::{hash::Hash, str::FromStr, sync::LazyLock};

use zpm_ecow::EcoString;
use rkyv::{
    Archive, Deserialize, DeserializeUnsized, Place, Serialize, SerializeUnsized,
};
use rkyv::rancor::{Fallible, Source};
use rkyv::string::{ArchivedString, StringResolver};
use zpm_utils::{impl_file_string_from_str, impl_file_string_serialization, DataType, FromFileString, Path, ToFileString, ToHumanString};

#[derive(thiserror::Error, Clone, Debug)]
pub enum IdentError {
    #[error("Invalid ident: {0}")]
    SyntaxError(String),
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Ident(EcoString);

impl Archive for Ident {
    type Archived = ArchivedString;
    type Resolver = StringResolver;

    fn resolve(&self, resolver: Self::Resolver, out: Place<Self::Archived>) {
        ArchivedString::resolve_from_str(self.0.as_str(), resolver, out);
    }
}

impl<S: Fallible + ?Sized> Serialize<S> for Ident
where
    S::Error: Source,
    str: SerializeUnsized<S>,
{
    fn serialize(&self, serializer: &mut S) -> Result<Self::Resolver, S::Error> {
        ArchivedString::serialize_from_str(self.0.as_str(), serializer)
    }
}

impl<D: Fallible + ?Sized> Deserialize<Ident, D> for ArchivedString
where
    str: DeserializeUnsized<str, D>,
{
    fn deserialize(&self, _: &mut D) -> Result<Ident, D::Error> {
        Ok(Ident(EcoString::from(self.as_str())))
    }
}

impl Ident {
    pub fn new<P: AsRef<str>>(full: P) -> Ident {
        Ident(EcoString::from(full.as_ref()))
    }

    pub fn split(&self) -> (Option<&str>, &str) {
        self.0.split_once('/').map_or(
            (None, self.0.as_str()),
            |(scope, name)| (Some(scope), name),
        )
    }

    pub fn scope(&self) -> Option<&str> {
        self.0.split_once('/').map(|(scope, _)| scope)
    }

    pub fn name(&self) -> &str {
        self.0.split_once('/').map(|(_, name)| name).unwrap_or(self.0.as_str())
    }

    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }

    pub fn slug(&self) -> String {
        self.0.replace("/", "-").into()
    }

    pub fn nm_subdir(&self) -> Path {
        Path::from_str(&format!("node_modules/{}", self.0)).unwrap()
    }

    pub fn type_ident(&self) -> Ident {
        Ident::new(self.scope().map_or_else(
            || format!("@types/{}", self.name()),
            |scope| format!("@types/{}__{}", &scope[1..], self.name())
        ))
    }
}

impl AsRef<str> for Ident {
    fn as_ref(&self) -> &str {
        self.0.as_str()
    }
}

static IDENT_REGEX: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(r"^(?:@[^/]*/)?([^@/]+)$").unwrap()
});

impl FromFileString for Ident {
    type Error = IdentError;

    fn from_file_string(src: &str) -> Result<Self, Self::Error> {
        if !IDENT_REGEX.is_match(src) {
            return Err(IdentError::SyntaxError(src.to_string()));
        }

        Ok(Ident::new(src))
    }
}

impl ToFileString for Ident {
    fn to_file_string(&self) -> String {
        self.as_str().to_string()
    }
}

impl ToHumanString for Ident {
    fn to_print_string(&self) -> String {
        let slash_index
            = self.0.find('/');

        if let Some(slash_index) = slash_index {
            let scope_part
                = &self.0[..=slash_index];
            let name_part
                = &self.0[slash_index + 1..];
            format!("{}{}", DataType::Scope.colorize(scope_part), DataType::Ident.colorize(name_part))
        } else {
            DataType::Ident.colorize(&self.0)
        }
    }
}

impl_file_string_from_str!(Ident);
impl_file_string_serialization!(Ident);

#[cfg(test)]
mod tests {
    use super::Ident;

    #[test]
    fn ident_rkyv_roundtrip() {
        let ident = Ident::new("@scope/name");
        let bytes =
            rkyv::to_bytes::<rkyv::rancor::BoxedError>(&ident).unwrap();
        let decoded =
            rkyv::from_bytes::<Ident, rkyv::rancor::BoxedError>(&bytes)
                .unwrap();
        assert_eq!(ident, decoded);
    }
}
