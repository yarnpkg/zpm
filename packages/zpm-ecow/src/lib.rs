use std::ops::{Deref, DerefMut};

pub extern crate ecow;

use ecow::{EcoString as InnerEcoString, EcoVec as InnerEcoVec};
use rkyv::{
    rancor::{Fallible, Source},
    ser::{Allocator, Writer},
    string::{ArchivedString, StringResolver},
    vec::{ArchivedVec, VecResolver},
    Archive, Deserialize, DeserializeUnsized, Place, Serialize, SerializeUnsized,
};

#[macro_export]
macro_rules! eco_vec {
    () => {
        $crate::EcoVec($crate::ecow::eco_vec![])
    };
    ($elem:expr; $n:expr) => {
        $crate::EcoVec($crate::ecow::eco_vec![$elem; $n])
    };
    ($($value:expr),+ $(,)?) => {
        $crate::EcoVec($crate::ecow::eco_vec![$($value),+])
    };
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct EcoString(pub InnerEcoString);

impl EcoString {
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

impl From<&str> for EcoString {
    fn from(value: &str) -> Self {
        Self(InnerEcoString::from(value))
    }
}

impl From<String> for EcoString {
    fn from(value: String) -> Self {
        Self(InnerEcoString::from(value))
    }
}

impl From<InnerEcoString> for EcoString {
    fn from(value: InnerEcoString) -> Self {
        Self(value)
    }
}

impl From<EcoString> for InnerEcoString {
    fn from(value: EcoString) -> Self {
        value.0
    }
}

impl Deref for EcoString {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        self.0.as_str()
    }
}

impl std::fmt::Display for EcoString {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl Archive for EcoString {
    type Archived = ArchivedString;
    type Resolver = StringResolver;

    fn resolve(&self, resolver: Self::Resolver, out: Place<Self::Archived>) {
        ArchivedString::resolve_from_str(self.0.as_str(), resolver, out);
    }
}

impl<S: Fallible + ?Sized> Serialize<S> for EcoString
where
    S::Error: Source,
    str: SerializeUnsized<S>,
{
    fn serialize(&self, serializer: &mut S) -> Result<Self::Resolver, S::Error> {
        ArchivedString::serialize_from_str(self.0.as_str(), serializer)
    }
}

impl<D: Fallible + ?Sized> Deserialize<EcoString, D> for ArchivedString
where
    str: DeserializeUnsized<str, D>,
{
    fn deserialize(&self, _: &mut D) -> Result<EcoString, D::Error> {
        Ok(EcoString(InnerEcoString::from(self.as_str())))
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct EcoVec<T>(pub InnerEcoVec<T>);

impl<T> EcoVec<T> {
    pub fn new() -> Self {
        Self(InnerEcoVec::new())
    }

    pub fn as_slice(&self) -> &[T] {
        self.0.as_slice()
    }
}

impl<T> Default for EcoVec<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Clone> From<Vec<T>> for EcoVec<T> {
    fn from(value: Vec<T>) -> Self {
        Self(InnerEcoVec::from(value))
    }
}

impl<T: Clone, const N: usize> From<[T; N]> for EcoVec<T> {
    fn from(value: [T; N]) -> Self {
        Self(InnerEcoVec::from(value))
    }
}

impl<T> From<InnerEcoVec<T>> for EcoVec<T> {
    fn from(value: InnerEcoVec<T>) -> Self {
        Self(value)
    }
}

impl<T> From<EcoVec<T>> for InnerEcoVec<T> {
    fn from(value: EcoVec<T>) -> Self {
        value.0
    }
}

impl<T> Deref for EcoVec<T> {
    type Target = InnerEcoVec<T>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<T> DerefMut for EcoVec<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl<'a, T> IntoIterator for &'a EcoVec<T> {
    type IntoIter = core::slice::Iter<'a, T>;
    type Item = &'a T;

    fn into_iter(self) -> Self::IntoIter {
        self.0.as_slice().iter()
    }
}

impl<T: Clone> From<&[T]> for EcoVec<T> {
    fn from(value: &[T]) -> Self {
        Self(InnerEcoVec::from(value))
    }
}

impl<T: Clone> IntoIterator for EcoVec<T> {
    type IntoIter = <InnerEcoVec<T> as IntoIterator>::IntoIter;
    type Item = T;

    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

impl<T: Archive> Archive for EcoVec<T> {
    type Archived = ArchivedVec<T::Archived>;
    type Resolver = VecResolver;

    fn resolve(&self, resolver: Self::Resolver, out: Place<Self::Archived>) {
        ArchivedVec::resolve_from_slice(self.0.as_slice(), resolver, out);
    }
}

impl<T, S> Serialize<S> for EcoVec<T>
where
    T: Archive + Serialize<S>,
    S: Fallible + Allocator + Writer + ?Sized,
{
    fn serialize(&self, serializer: &mut S) -> Result<Self::Resolver, S::Error> {
        ArchivedVec::<T::Archived>::serialize_from_slice(self.0.as_slice(), serializer)
    }
}

impl<T, D> Deserialize<EcoVec<T>, D> for ArchivedVec<T::Archived>
where
    T: Archive + Clone,
    [T::Archived]: DeserializeUnsized<[T], D>,
    D: Fallible + ?Sized,
    D::Error: Source,
{
    fn deserialize(&self, deserializer: &mut D) -> Result<EcoVec<T>, D::Error> {
        let values: Vec<T> = self.deserialize(deserializer)?;
        Ok(EcoVec(InnerEcoVec::from(values)))
    }
}
