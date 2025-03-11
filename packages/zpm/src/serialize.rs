use bincode::{Decode, Encode};
use serde::ser::{Serialize, Serializer, SerializeStruct, SerializeTuple, SerializeTupleStruct, SerializeTupleVariant, SerializeMap, SerializeSeq, SerializeStructVariant};
use zpm_utils::{impl_serialization_traits, FromFileString, ToFileString, ToHumanString};
use std::fmt;

use crate::{error::Error, primitives::{Descriptor, Locator}};

pub struct NoopSerializer {
    pub output: String,
}

impl Default for NoopSerializer {
    fn default() -> Self {
        Self::new()
    }
}

impl NoopSerializer {
    pub fn new() -> NoopSerializer {
        NoopSerializer {
            output: String::new(),
        }
    }
}

impl Serializer for &mut NoopSerializer {
    type Ok = ();
    type Error = fmt::Error;

    type SerializeSeq = NoopSubSerializer;
    type SerializeTuple = NoopSubSerializer;
    type SerializeTupleStruct = NoopSubSerializer;
    type SerializeTupleVariant = NoopSubSerializer;
    type SerializeMap = NoopSubSerializer;
    type SerializeStruct = NoopSubSerializer;
    type SerializeStructVariant = NoopSubSerializer;

    fn serialize_bool(self, _v: bool) -> Result<Self::Ok, Self::Error> {
        Ok(())
    }

    fn serialize_i8(self, _v: i8) -> Result<Self::Ok, Self::Error> {
        Ok(())
    }

    fn serialize_i16(self, _v: i16) -> Result<Self::Ok, Self::Error> {
        Ok(())
    }

    fn serialize_i32(self, _v: i32) -> Result<Self::Ok, Self::Error> {
        Ok(())
    }

    fn serialize_i64(self, _v: i64) -> Result<Self::Ok, Self::Error> {
        Ok(())
    }

    fn serialize_u8(self, _v: u8) -> Result<Self::Ok, Self::Error> {
        Ok(())
    }

    fn serialize_u16(self, _v: u16) -> Result<Self::Ok, Self::Error> {
        Ok(())
    }

    fn serialize_u32(self, _v: u32) -> Result<Self::Ok, Self::Error> {
        Ok(())
    }

    fn serialize_u64(self, _v: u64) -> Result<Self::Ok, Self::Error> {
        Ok(())
    }

    fn serialize_f32(self, _v: f32) -> Result<Self::Ok, Self::Error> {
        Ok(())
    }

    fn serialize_f64(self, _v: f64) -> Result<Self::Ok, Self::Error> {
        Ok(())
    }

    fn serialize_char(self, _v: char) -> Result<Self::Ok, Self::Error> {
        Ok(())
    }

    fn serialize_str(self, v: &str) -> Result<Self::Ok, Self::Error> {
        self.output.push_str(v);
        Ok(())
    }

    fn serialize_bytes(self, _v: &[u8]) -> Result<Self::Ok, Self::Error> {
        Ok(())
    }

    fn serialize_none(self) -> Result<Self::Ok, Self::Error> {
        Ok(())
    }

    fn serialize_some<T>(self, __value: &T) -> Result<Self::Ok, Self::Error> where T: Serialize + ?Sized {
        Ok(())
    }

    fn serialize_unit(self) -> Result<Self::Ok, Self::Error> {
        Ok(())
    }

    fn serialize_unit_struct(self, _name: &'static str) -> Result<Self::Ok, Self::Error> {
        Ok(())
    }

    fn serialize_unit_variant(self, _name: &'static str, _variant_index: u32, _variant: &'static str) -> Result<Self::Ok, Self::Error> {
        Ok(())
    }

    fn serialize_newtype_struct<T>(self, _name: &'static str, __value: &T) -> Result<Self::Ok, Self::Error> where T: Serialize + ?Sized {
        Ok(())
    }

    fn serialize_newtype_variant<T>(self, _name: &'static str, _variant_index: u32, _variant: &'static str, __value: &T) -> Result<Self::Ok, Self::Error> where T: Serialize + ?Sized {
        Ok(())
    }

    fn serialize_seq(self, _len: Option<usize>) -> Result<Self::SerializeSeq, Self::Error> {
        Ok(NoopSubSerializer {})
    }

    fn serialize_tuple(self, _len: usize) -> Result<Self::SerializeTuple, Self::Error> {
        Ok(NoopSubSerializer {})
    }

    fn serialize_tuple_struct(self, _name: &'static str, _len: usize) -> Result<Self::SerializeTupleStruct, Self::Error> {
        Ok(NoopSubSerializer {})
    }

    fn serialize_tuple_variant(self, _name: &'static str, _variant_index: u32, _variant: &'static str, _len: usize) -> Result<Self::SerializeTupleVariant, Self::Error> {
        Ok(NoopSubSerializer {})
    }

    fn serialize_map(self, _len: Option<usize>) -> Result<Self::SerializeMap, Self::Error> {
        Ok(NoopSubSerializer {})
    }

    fn serialize_struct(self, _name: &'static str, _len: usize) -> Result<Self::SerializeStruct, Self::Error> {
        Ok(NoopSubSerializer {})
    }

    fn serialize_struct_variant(self, _name: &'static str, _variant_index: u32, _variant: &'static str, _len: usize) -> Result<Self::SerializeStructVariant, Self::Error> {
        Ok(NoopSubSerializer {})
    }
}

pub struct NoopSubSerializer {
}

impl SerializeMap for NoopSubSerializer {
    type Ok = ();
    type Error = fmt::Error;

    fn serialize_key<T>(&mut self, _key: &T) -> Result<(), Self::Error> where T: Serialize + ?Sized {
        Ok(())
    }

    fn serialize_value<T>(&mut self, _value: &T) -> Result<(), Self::Error> where T: Serialize + ?Sized {
        Ok(())
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        Ok(())
    }
}

impl SerializeSeq for NoopSubSerializer {
    type Ok = ();
    type Error = fmt::Error;

    fn serialize_element<T>(&mut self, _value: &T) -> Result<(), Self::Error> where T: Serialize + ?Sized {
        Ok(())
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        Ok(())
    }
}

impl SerializeTuple for NoopSubSerializer {
    type Ok = ();
    type Error = fmt::Error;

    fn serialize_element<T>(&mut self, _value: &T) -> Result<(), Self::Error> where T: Serialize + ?Sized {
        Ok(())
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        Ok(())
    }
}

impl SerializeTupleStruct for NoopSubSerializer {
    type Ok = ();
    type Error = fmt::Error;

    fn serialize_field<T>(&mut self, _value: &T) -> Result<(), Self::Error> where T: Serialize + ?Sized {
        Ok(())
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        Ok(())
    }
}

impl SerializeTupleVariant for NoopSubSerializer {
    type Ok = ();
    type Error = fmt::Error;

    fn serialize_field<T>(&mut self, _value: &T) -> Result<(), Self::Error> where T: Serialize + ?Sized {
        Ok(())
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        Ok(())
    }
}

impl SerializeStruct for NoopSubSerializer {
    type Ok = ();
    type Error = fmt::Error;

    fn serialize_field<T>(&mut self, _key: &'static str, _value: &T) -> Result<(), Self::Error> where T: Serialize + ?Sized {
        Ok(())
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        Ok(())
    }
}

impl SerializeStructVariant for NoopSubSerializer {
    type Ok = ();
    type Error = fmt::Error;

    fn serialize_field<T>(&mut self, _key: &'static str, _value: &T) -> Result<(), Self::Error> where T: Serialize + ?Sized {
        Ok(())
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        Ok(())
    }
}

pub trait Serialized {
    fn serialized(&self) -> Result<String, fmt::Error>;
}

#[derive(Clone, Debug, Decode, Encode, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct UrlEncoded<T>(pub T);

impl<T> UrlEncoded<T> {
    pub fn new(value: T) -> UrlEncoded<T> {
        UrlEncoded(value)
    }
}

impl<T: FromFileString<Error = Error>> FromFileString for UrlEncoded<T> {
    type Error = Error;

    fn from_file_string(value: &str) -> Result<Self, Error> {
        let url_decoded
            = urlencoding::decode(value).unwrap();

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

impl_serialization_traits!(UrlEncoded<Descriptor>);
impl_serialization_traits!(UrlEncoded<Locator>);

