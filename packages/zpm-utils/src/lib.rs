use colored::Colorize;

pub mod serialization;

pub trait FromFileString {
    type Error;

    fn from_file_string(s: &str) -> Result<Self, Self::Error>
        where Self: Sized;
}

pub trait ToFileString {
    fn to_file_string(&self) -> String;
}

pub trait ToHumanString {
    fn to_print_string(&self) -> String;
}

impl FromFileString for String {
    type Error = std::convert::Infallible;

    fn from_file_string(s: &str) -> Result<Self, Self::Error> {
        Ok(s.to_string())
    }
}

impl ToFileString for String {
    fn to_file_string(&self) -> String {
        sonic_rs::to_string(self).unwrap()
    }
}

impl ToHumanString for String {
    fn to_print_string(&self) -> String {
        self.to_file_string().truecolor(0, 153, 0).to_string()
    }
}

#[macro_export]
macro_rules! impl_serialization_traits(($type:ty) => {
    impl std::str::FromStr for $type {
        type Err = <$type as zpm_utils::FromFileString>::Error;

        fn from_str(s: &str) -> Result<Self, Self::Err> {
            <$type as zpm_utils::FromFileString>::from_file_string(s)
        }
    }

    impl std::convert::TryFrom<&str> for $type {
        type Error = <$type as zpm_utils::FromFileString>::Error;

        fn try_from(value: &str) -> Result<Self, Self::Error> {
            Ok(<$type as zpm_utils::FromFileString>::from_file_string(value)?)
        }
    }

    impl std::convert::TryFrom<&str> for Box<$type> {
        type Error = <$type as zpm_utils::FromFileString>::Error;

        fn try_from(value: &str) -> Result<Self, Self::Error> {
            Ok(Box::new(<$type as zpm_utils::FromFileString>::from_file_string(value)?))
        }
    }

    impl serde::Serialize for $type {
        fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error> where S: serde::Serializer {
            serializer.serialize_str(&self.to_file_string())
        }
    }

    impl<'de> serde::Deserialize<'de> for $type {
        fn deserialize<D>(deserializer: D) -> Result<Self, D::Error> where D: serde::Deserializer<'de> {
            let s = String::deserialize(deserializer)?;
            <$type as zpm_utils::FromFileString>::from_file_string(&s).map_err(serde::de::Error::custom)
        }
    }

    impl std::fmt::Display for $type {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "{}", <$type as zpm_utils::ToHumanString>::to_print_string(self))
        }
    }
});
