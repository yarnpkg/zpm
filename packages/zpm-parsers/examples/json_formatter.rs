use zpm_parsers::{json::{JsonFormatter, JsonValue}, JsonPath};
use zpm_utils::FromFileString;
use indoc::indoc;

fn main() {
    // Example 1: Simple object with 2-space indentation
    let input1 = r#"{
  "name": "John Doe",
  "age": 30,
  "active": true
}"#;

    let mut formatter = JsonFormatter::from(input1).expect("Failed to parse JSON");
    
    // Update existing field
    let path = JsonPath::from_file_string("name").unwrap();
    formatter.set(&path, JsonValue::String("Jane Doe".to_string())).unwrap();
    
    // Add new field (will be added at the end with same formatting)
    let path = JsonPath::from_file_string("email").unwrap();
    formatter.set(&path, JsonValue::String("jane@example.com".to_string())).unwrap();
    
    println!("Example 1 - Modified JSON:");
    println!("{}", formatter.to_string());
    println!();

    // Example 2: Nested object with 4-space indentation
    let input2 = r#"{
    "user": {
        "id": 123,
        "profile": {
            "name": "Alice",
            "role": "admin"
        }
    },
    "settings": {
        "theme": "dark"
    }
}"#;

    let mut formatter2 = JsonFormatter::from(input2).expect("Failed to parse JSON");
    
    // Update nested field using from_file_string
    let path = JsonPath::from_file_string("user.profile.name").unwrap();
    formatter2.set(&path, JsonValue::String("Bob".to_string())).unwrap();
    
    // Add new nested field using vector segments
    let path: JsonPath = vec!["user", "profile", "email"].into();
    formatter2.set(&path, JsonValue::String("bob@example.com".to_string())).unwrap();
    
    // Add array using direct segment creation
    let path: JsonPath = vec!["tags".to_string()].into();
    formatter2.set(&path, JsonValue::Array(vec![
        JsonValue::String("important".to_string()),
        JsonValue::String("verified".to_string()),
    ])).unwrap();
    
    println!("Example 2 - Modified nested JSON:");
    println!("{}", formatter2.to_string());
    println!();

    // Example 3: Array manipulation
    let input3 = r#"{"items": [1, 2, 3], "total": 3}"#;
    
    let mut formatter3 = JsonFormatter::from(input3).expect("Failed to parse JSON");
    
    // Update array element
    let path = JsonPath::from_file_string("items.1").unwrap();
    formatter3.set(&path, JsonValue::Number("42".to_string())).unwrap();
    
    // Extend array (will add null values as needed)
    let path = JsonPath::from_file_string("items.5").unwrap();
    formatter3.set(&path, JsonValue::Number("100".to_string())).unwrap();
    
    println!("Example 3 - Modified array:");
    println!("{}", formatter3.to_string());
    println!();

    // Example 4: Adding new objects and arrays
    let input4 = r#"{"version": "1.0"}"#;
    
    let mut formatter4 = JsonFormatter::from(input4).expect("Failed to parse JSON");
    
    // Add various types of new fields using vector segments
    let path: JsonPath = vec!["metadata"].into();
    formatter4.set(&path, JsonValue::Object(vec![
        ("author".to_string(), JsonValue::String("Example Corp".to_string())),
        ("created".to_string(), JsonValue::String("2024-01-01".to_string())),
    ])).unwrap();
    
    let path: JsonPath = vec!["features"].into();
    formatter4.set(&path, JsonValue::Array(vec![
        JsonValue::String("format-preserving".to_string()),
        JsonValue::String("nested-access".to_string()),
        JsonValue::String("type-safe".to_string()),
    ])).unwrap();
    
    // Using vector segments for nested paths
    let path: JsonPath = vec!["config", "debug"].into();
    formatter4.set(&path, JsonValue::Bool(false)).unwrap();
    let path: JsonPath = vec!["config", "timeout"].into();
    formatter4.set(&path, JsonValue::Number("30".to_string())).unwrap();
    
    println!("Example 4 - Adding new objects and arrays:");
    println!("{}", formatter4.to_string());
    println!();

    // Example 5: Building complex nested structure from empty
    let input5 = r#"{}"#;
    
    let mut formatter5 = JsonFormatter::from(input5).expect("Failed to parse JSON");
    
    // Build a complex structure using vector segments
    let path: JsonPath = vec!["api", "v1", "endpoints"].into();
    formatter5.set(&path, JsonValue::Array(vec![
        JsonValue::Object(vec![
            ("path".to_string(), JsonValue::String("/users".to_string())),
            ("method".to_string(), JsonValue::String("GET".to_string())),
            ("auth".to_string(), JsonValue::Bool(true)),
        ]),
        JsonValue::Object(vec![
            ("path".to_string(), JsonValue::String("/users/{id}".to_string())),
            ("method".to_string(), JsonValue::String("GET".to_string())),
            ("auth".to_string(), JsonValue::Bool(true)),
        ]),
    ])).unwrap();
    
    let path: JsonPath = vec!["api", "v1", "version"].into();
    formatter5.set(&path, JsonValue::String("1.0.0".to_string())).unwrap();
    let path: JsonPath = vec!["api", "v2", "status"].into();
    formatter5.set(&path, JsonValue::String("beta".to_string())).unwrap();
    
    println!("Example 5 - Complex nested structure from empty:");
    println!("{}", formatter5.to_string());
    println!();

    // Example 6: Using JsonPath with bracket notation
    let input6 = r#"{
  "users": [],
  "config": {}
}"#;
    
    let mut formatter6 = JsonFormatter::from(input6).expect("Failed to parse JSON");
    
    // Use bracket notation for array indices
    let path = JsonPath::from_file_string(r#"users[0]"#).unwrap();
    formatter6.set(&path, JsonValue::Object(vec![
        ("name".to_string(), JsonValue::String("Alice".to_string())),
        ("roles".to_string(), JsonValue::Array(vec![])),
    ])).unwrap();
    
    let path = JsonPath::from_file_string(r#"users[0].roles[0]"#).unwrap();
    formatter6.set(&path, JsonValue::String("admin".to_string())).unwrap();
    let path = JsonPath::from_file_string(r#"users[0].roles[1]"#).unwrap();
    formatter6.set(&path, JsonValue::String("user".to_string())).unwrap();
    
    // Use bracket notation for keys with special characters
    let path = JsonPath::from_file_string(r#"config["api-key"]"#).unwrap();
    formatter6.set(&path, JsonValue::String("secret123".to_string())).unwrap();
    let path = JsonPath::from_file_string(r#"config["max-retries"]"#).unwrap();
    formatter6.set(&path, JsonValue::Number("3".to_string())).unwrap();
    let path = JsonPath::from_file_string(r#"config["feature flags"]"#).unwrap();
    formatter6.set(&path, JsonValue::Object(vec![
        ("debug".to_string(), JsonValue::Bool(true)),
        ("new-ui".to_string(), JsonValue::Bool(false)),
    ])).unwrap();
    
    println!("Example 6 - JsonPath with bracket notation:");
    println!("{}", formatter6.to_string());
    println!();

    // Example 7: Removing values
    let input7 = indoc! {r#"{
      "name": "Project",
      "version": "1.0.0",
      "deprecated_field": "old value",
      "config": {
        "api_key": "secret",
        "old_endpoint": "http://old.example.com",
        "timeout": 30
      },
      "features": ["feature1", "deprecated_feature", "feature3"]
    }"#};
    
    let mut formatter7 = JsonFormatter::from(input7).expect("Failed to parse JSON");
    
    // Remove deprecated fields using the convenience method
    let path = JsonPath::from_file_string("deprecated_field").unwrap();
    formatter7.remove(&path).unwrap();
    let path = JsonPath::from_file_string("config.old_endpoint").unwrap();
    formatter7.remove(&path).unwrap();
    
    // Remove array element using set with Undefined
    let path = JsonPath::from_file_string("features[1]").unwrap();
    formatter7.set(&path, JsonValue::Undefined).unwrap();
    
    // Can also remove entire objects/arrays
    let path = JsonPath::from_file_string("config.api_key").unwrap();
    formatter7.remove(&path).unwrap();
    
    println!("Example 7 - Removing values:");
    println!("{}", formatter7.to_string());
} 
