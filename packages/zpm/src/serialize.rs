use bincode::{Decode, Encode};
use serde::ser::{Serialize, Serializer, SerializeStruct, SerializeTuple, SerializeTupleStruct, SerializeTupleVariant, SerializeMap, SerializeSeq, SerializeStructVariant};
use std::fmt;

use crate::error::Error;

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

#[macro_export]
macro_rules! yarn_check_serialize(
    ($src:expr, $serialized:expr) => {
        {
            let serialized = $serialized;

            use std::str::FromStr;
            let re_parsed = Self::from_str(&serialized).unwrap();

            assert_eq!($src, &re_parsed, "Serialized form of {:?} ({}) did not match the input (re-parsed as {:?} instead)", $src, serialized, re_parsed);

            serialized
        }
    }
);

#[macro_export]
macro_rules! yarn_serialization_protocol {
    ($type:ident, {
        deserialize($deserialize_src:ident) { $($deserialize_body:tt)* }
    }) => {
        impl<'a> std::convert::TryFrom<&'a str> for $type {
            type Error = $crate::error::Error;

            fn try_from($deserialize_src: &str) -> std::result::Result<Self, Self::Error> {
                $($deserialize_body)*
            }
        }

        impl std::str::FromStr for $type {
            type Err = $crate::error::Error;

            fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
                Self::try_from(s)
            }
        }

        impl<'de> serde::Deserialize<'de> for $type {
            fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error> where D: serde::Deserializer<'de> {
                use std::str::FromStr;

                let s = String::deserialize(deserializer)?;
                $type::from_str(&s)
                    .map_err(serde::de::Error::custom)
            }
        }

        impl TryFrom<&str> for Box<$type> {
            type Error = $crate::error::Error;
        
            fn try_from(value: &str) -> Result<Self, Self::Error> {
                use std::str::FromStr;
                Ok(Box::new($type::from_str(value)?))
            }
        }
    };

    ($type:ident, $color:expr, {
        serialize(&$serialize_src:ident) { $($serialize_body:tt)* }
    }) => {
        impl serde::Serialize for $type {
            fn serialize<S>(&$serialize_src, serializer: S) -> std::result::Result<S::Ok, S::Error> where S: serde::Serializer {
                let val = { $($serialize_body)* };
                serializer.serialize_str(&val)
            }
        }

        impl $crate::serialize::Serialized for $type {
            fn serialized(&self) -> Result<String, std::fmt::Error> {
                let mut serializer = $crate::serialize::NoopSerializer::new();

                use serde::ser::Serialize;
                self.serialize(&mut serializer)?;

                Ok(serializer.output)
            }
        }

        impl std::fmt::Display for $type {
            fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                use $crate::serialize::Serialized;
                write!(f, "{}", self.serialized()?)
            }
        }
    };

    ($type:ident, $color:expr, {
        deserialize($deserialize_src:ident) { $($deserialize_body:tt)* }
        serialize(&$serialize_src:ident) { $($serialize_body:tt)* }
    }) => {
        yarn_serialization_protocol!($type, $color, {
            serialize(&$serialize_src) { $($serialize_body)* }
        });

        yarn_serialization_protocol!($type, {
            deserialize($deserialize_src) { $($deserialize_body)* }
        });
    };
}

#[derive(Clone, Debug, Decode, Encode, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct UrlEncoded<T>(pub T);

impl<T> UrlEncoded<T> {
    pub fn new(value: T) -> UrlEncoded<T> {
        UrlEncoded(value)
    }
}

impl<T: for<'t> TryFrom<&'t str, Error = Error>> TryFrom<&str> for UrlEncoded<T> {
    type Error = Error;

    fn try_from(value: &str) -> Result<Self, Error> {
        let url_decoded
            = urlencoding::decode(value).unwrap();
        let converted
            = T::try_from(url_decoded.as_ref()).unwrap();

        Ok(UrlEncoded(converted))
    }
}

impl<T: ToString> std::fmt::Display for UrlEncoded<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{}", urlencoding::encode(&self.0.to_string()))
    }
}

impl<T: for<'t> TryFrom<&'t str, Error = Error>> TryFrom<&str> for Box<UrlEncoded<T>> {
    type Error = Error;

    fn try_from(value: &str) -> Result<Self, Error> {
        UrlEncoded::try_from(value).map(Box::new)
    }
}

