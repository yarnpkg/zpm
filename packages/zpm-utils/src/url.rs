use std::{borrow::Cow, collections::BTreeMap, ops::Deref, string::FromUtf8Error};

use rkyv::Archive;
use urlencoding::decode;

use crate::{FromFileString, ToFileString, ToHumanString};

#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Archive, rkyv::Serialize, rkyv::Deserialize)]
#[rkyv(derive(PartialEq, Eq, Hash, PartialOrd, Ord))]
pub enum QueryStringValue {
    String(String),
    True,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Archive, rkyv::Serialize, rkyv::Deserialize)]
#[rkyv(derive(PartialEq, Eq))]
pub struct QueryString {
    pub fields: BTreeMap<String, QueryStringValue>,
}

impl QueryString {
    pub fn from_iter(iter: impl Iterator<Item = (String, QueryStringValue)>) -> Self {
        Self {fields: iter.collect()}
    }

    pub fn encode(value: &'_ str) -> Cow<'_, str> {
        urlencoding::encode(value)
    }
}

impl FromFileString for QueryString {
    type Error = FromUtf8Error;

    fn from_file_string(value: &str) -> Result<Self, Self::Error> {
        let mut fields
            = BTreeMap::new();

        for segment in value.split('&') {
            let eq_index
                = segment.find('=');

            if let Some(eq_index) = eq_index {
                let (key, value) = segment.split_at(eq_index);

                let key
                    = urlencoding::decode(&key)?.to_string();
                let value
                    = urlencoding::decode(&value[1..])?.to_string();

                fields.insert(key, QueryStringValue::String(value));
            } else {
                let key
                    = urlencoding::decode(segment)?.to_string();

                fields.insert(key, QueryStringValue::True);
            }
        }

        Ok(QueryString {fields})
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Archive, rkyv::Serialize, rkyv::Deserialize)]
#[rkyv(archive_bounds(T::Archived: PartialEq + Eq + PartialOrd + Ord + std::hash::Hash))]
#[rkyv(derive(PartialEq, Eq, Hash, PartialOrd, Ord))]
pub struct UrlEncoded<T>(pub T);

impl<T> UrlEncoded<T> {
    pub fn new(value: T) -> UrlEncoded<T> {
        UrlEncoded(value)
    }
}

impl<T: FromFileString> FromFileString for UrlEncoded<T> where T::Error: From<FromUtf8Error> {
    type Error = T::Error;

    fn from_file_string(value: &str) -> Result<Self, Self::Error> {
        let url_decoded
            = decode(value)?;

        Ok(UrlEncoded(T::from_file_string(url_decoded.as_ref())?))
    }
}

impl<T: ToFileString> ToFileString for UrlEncoded<T> {
    fn to_file_string(&self) -> String {
        urlencoding::encode(&self.0.to_file_string()).to_string()
    }
}

impl<T: ToHumanString> ToHumanString for UrlEncoded<T> {
    fn to_print_string(&self) -> String {
        self.0.to_print_string()
    }
}

impl<T> Deref for UrlEncoded<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
