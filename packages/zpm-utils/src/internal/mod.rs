use serde::Serialize;

pub fn to_json_string<T: Serialize>(value: &T) -> String {
    serde_json::to_string(value).unwrap()
}
