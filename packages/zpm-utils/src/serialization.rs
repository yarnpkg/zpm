#[macro_export]
macro_rules! check_serialize(
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
macro_rules! serialization_protocol {
    ($type:ident, {
        deserialize($deserialize_src:ident) { $($deserialize_body:tt)* }
    }) => {
        impl<'a> std::convert::TryFrom<&'a str> for $type {
            type Error = $crate::Error;

            fn try_from($deserialize_src: &str) -> std::result::Result<Self, Self::Error> {
                $($deserialize_body)*
            }
        }

        impl std::str::FromStr for $type {
            type Err = $crate::Error;

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
            type Error = $crate::Error;
        
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
        serialization_protocol!($type, $color, {
            serialize(&$serialize_src) { $($serialize_body)* }
        });

        serialization_protocol!($type, {
            deserialize($deserialize_src) { $($deserialize_body)* }
        });
    };
}
