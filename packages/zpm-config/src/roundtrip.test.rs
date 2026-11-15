//! Checks that every setting of the schema survives a `yarn config set`
//! followed by a configuration load: we hydrate a sample value the same way
//! `yarn config set` does, write it into an rc file through the same YAML
//! editor, load the file back, and compare what `yarn config get` would
//! print. New settings are picked up automatically; new types need a sample
//! value in `sample_value`.

use std::collections::BTreeMap;

use zpm_parsers::{DataDocument, JsonDocument};
use zpm_utils::{Path, RedactionScope};

use super::*;

/// What `yarn config set` gets on its command line for each type. The
/// strings are purposely awkward, to check that they get quoted correctly.
fn sample_value(type_name: &str) -> &'static str {
    match type_name {
        "boolean" => "true",
        "string" => "*weird: 'value' #1",
        "u64" | "usize" => "7",
        "std::time::Duration" => "1.5h",
        "zpm_utils::Path" => "/some/@weird folder",
        "zpm_utils::Secret<String>" => "@secret:token",
        "zpm_utils::Glob" => "*.example.org",
        "zpm_primitives::Ident" => "@scope/name",
        "zpm_primitives::IdentGlob" => "@scope/*",
        "zpm_primitives::PeerRange" => "^1.0.0",
        "zpm_primitives::Range" => "^1.0.0",
        "zpm_primitives::SemverDescriptor" => "@babel/parser@*",
        "zpm_primitives::VersionFilter" => "@scope/name@^1.0.0",
        "zpm_semver::Range" => "^1.0.0",
        "zpm_semver::RangeKind" => "tilde",
        "zpm_formats::CompressionAlgorithm" => "9",
        "crate::ArchitectureFilter<zpm_utils::Cpu>" => "x64,arm64",
        "crate::ArchitectureFilter<zpm_utils::Libc>" => "musl",
        "crate::ArchitectureFilter<zpm_utils::Os>" => "linux,darwin",
        "crate::EcosystemFilter" => "pypi",
        "crate::IslandLinker" => "venv",
        "crate::LazyInstallMode" => "all",
        "crate::NmHoistingLimits" => "workspaces",
        "crate::NmMode" => "hardlinks-local",
        "crate::NodeLinker" => "node-modules",
        "crate::NodePackageMapType" => "loose",
        "crate::NpmPublishAccess" => "restricted",
        "crate::PnpFallbackMode" => "all",
        "crate::PythonPreference" => "system",
        "crate::WinLinkType" => "junctions",
        "crate::types::LogLevel" => "warning",

        _ => panic!("No sample value for the {type_name} type; add one to sample_value so that its settings get tested"),
    }
}

fn main_type(field: &serde_yaml::Value) -> String {
    let types = match &field["type"] {
        serde_yaml::Value::String(type_name) => vec![type_name.clone()],
        serde_yaml::Value::Sequence(types) => types.iter().map(|t| t.as_str().unwrap().to_string()).collect(),
        other => panic!("Unexpected type: {other:?}"),
    };

    types.into_iter()
        .find(|t| t != "null")
        .expect("A field must have a non-null type")
}

/// A leaf setting and how to reach it from the root of the configuration
struct Leaf {
    path: Vec<String>,
    type_name: String,
}

/// A list setting, which can only be set as a whole (with a JSON document)
struct List {
    path: Vec<String>,
    item: serde_yaml::Value,
}

fn collect(field: &serde_yaml::Value, path: Vec<String>, leaves: &mut Vec<Leaf>, lists: &mut Vec<List>) {
    match main_type(field).as_str() {
        "object" => {
            if let serde_yaml::Value::Mapping(properties) = &field["properties"] {
                for (name, property) in properties {
                    let mut property_path
                        = path.clone();

                    property_path.push(name.as_str().unwrap().to_string());
                    collect(property, property_path, leaves, lists);
                }
            } else {
                let key_type = match &field["additionalKeys"] {
                    serde_yaml::Value::Null => "string".to_string(),
                    additional_keys => main_type(additional_keys),
                };

                // A key that doesn't exist yet, since creating entries is
                // what `yarn config set` is often used for
                let key = match key_type.as_str() {
                    "string" => "https://example.org/@weird #path".to_string(),
                    key_type => sample_value(key_type).to_string(),
                };

                let mut entry_path
                    = path.clone();

                entry_path.push(key);
                collect(&field["additionalProperties"], entry_path, leaves, lists);
            }
        },

        "array" => {
            lists.push(List {path, item: field["items"].clone()});
        },

        type_name => {
            leaves.push(Leaf {path, type_name: type_name.to_string()});
        },
    }
}

/// Builds a JSON value covering every field of a list item
fn sample_json(field: &serde_yaml::Value) -> serde_yaml::Value {
    match main_type(field).as_str() {
        "object" => {
            let serde_yaml::Value::Mapping(properties) = &field["properties"] else {
                return serde_yaml::Value::Mapping(Default::default());
            };

            let mut mapping
                = serde_yaml::Mapping::new();

            for (name, property) in properties {
                mapping.insert(name.clone(), sample_json(property));
            }

            serde_yaml::Value::Mapping(mapping)
        },

        "array" => {
            serde_yaml::Value::Sequence(vec![sample_json(&field["items"])])
        },

        type_name => {
            // Every type also accepts its file string (which is what
            // environment variables and interpolations go through)
            serde_yaml::from_str(&JsonDocument::to_string(&sample_value(type_name)).unwrap()).unwrap()
        },
    }
}

fn load(text: &str) -> Settings {
    Configuration::validate(text)
        .unwrap_or_else(|e| panic!("The configuration should be valid ({e}):\n{text}"));

    let context = ConfigurationContext {
        env: BTreeMap::new(),
        user_cwd: Some(Path::from_file_string("/home").unwrap()),
        project_cwd: Some(Path::from_file_string("/project").unwrap()),
        package_cwd: None,
    };

    let project
        = serde_yaml::from_str::<intermediate::Settings>(text)
            .unwrap_or_else(|e| panic!("The configuration should be valid ({e}):\n{text}"));

    Settings::merge(&context, Partial::Missing, Partial::Value(project), || panic!("No configuration found"))
}

/// Simulates `yarn config set` (without `--json`) on a document
fn config_set(document: &str, path: &[String], value_str: &str) -> String {
    let segments
        = path.iter().map(|s| s.as_str()).collect::<Vec<_>>();

    let hydrated
        = Settings::hydrate(&segments, value_str)
            .unwrap_or_else(|e| panic!("Failed to hydrate {path:?} from {value_str:?}: {e}"));

    let value
        = zpm_parsers::Value::from_serializable(&hydrated).unwrap();

    DataDocument::update_document_field(document, zpm_parsers::Path::from_segments(path.to_vec()), value)
        .unwrap_or_else(|e| panic!("Failed to write {path:?}: {e}"))
}

fn strip_ansi(s: &str) -> String {
    let mut result
        = String::new();

    let mut chars
        = s.chars();

    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            for c in chars.by_ref() {
                if c.is_ascii_alphabetic() {
                    break;
                }
            }
        } else {
            result.push(c);
        }
    }

    result
}

/// Checks that the values written into the rc file have the shape described
/// by the schema; in particular scalar settings must be written as scalars
/// (`std::time::Duration` used to be written as a `{secs, nanos}` mapping).
fn assert_shape(field: &serde_yaml::Value, value: &serde_yaml::Value, path: &str) {
    let type_name
        = main_type(field);

    match (type_name.as_str(), value) {
        (_, serde_yaml::Value::Null) => {},

        ("object", serde_yaml::Value::Mapping(mapping)) => {
            for (key, child) in mapping {
                let key
                    = key.as_str().unwrap();

                let child_field = match &field["properties"] {
                    serde_yaml::Value::Mapping(properties) => &properties[key],
                    _ => &field["additionalProperties"],
                };

                assert_shape(child_field, child, &format!("{path}.{key}"));
            }
        },

        ("array", serde_yaml::Value::Sequence(items)) => {
            for (index, item) in items.iter().enumerate() {
                assert_shape(&field["items"], item, &format!("{path}[{index}]"));
            }
        },

        (type_name, serde_yaml::Value::Sequence(_)) if type_name.starts_with("crate::ArchitectureFilter<") => {},

        ("object" | "array", value) => {
            panic!("{path} should be a container, got {value:?}");
        },

        (type_name, serde_yaml::Value::Mapping(_) | serde_yaml::Value::Sequence(_)) => {
            panic!("{path} ({type_name}) should be written as a scalar, got {value:?}");
        },

        _ => {},
    }
}

fn written_value(document: &str, path: &[String]) -> serde_yaml::Value {
    let mut value: serde_yaml::Value
        = serde_yaml::from_str(document).unwrap();

    for segment in path {
        value = value[segment.as_str()].clone();
    }

    value
}

const BASE_DOCUMENT: &str = "# A comment that must be preserved\nenableTelemetry: false # this one too\n";

fn schema() -> serde_yaml::Value {
    serde_yaml::from_str(include_str!("../schema.json")).unwrap()
}

/// We write all the settings into the same document, both to check that the
/// editor keeps it valid and because some map entries are only valid once all
/// their required fields are set (`unstableIslands.*.linker`)
fn leaves_document(leaves: &[Leaf]) -> String {
    let mut document
        = BASE_DOCUMENT.to_string();

    for leaf in leaves {
        document = config_set(&document, &leaf.path, sample_value(&leaf.type_name));

        Configuration::validate(&document)
            .unwrap_or_else(|e| panic!("{:?} made the configuration invalid ({e}):\n{document}", leaf.path));
    }

    document
}

#[test]
fn every_leaf_setting_should_roundtrip_through_config_set() {
    let _scope
        = RedactionScope::new(false);

    let mut leaves
        = Vec::new();
    let mut lists
        = Vec::new();

    collect(&schema(), vec![], &mut leaves, &mut lists);

    assert!(leaves.len() > 50, "The schema walker should have found most settings");

    let document
        = leaves_document(&leaves);

    assert!(document.starts_with(BASE_DOCUMENT.lines().next().unwrap()), "The comments weren't preserved:\n{document}");
    assert!(document.contains("enableTelemetry: false # this one too\n"), "The comments weren't preserved:\n{document}");

    let settings
        = load(&document);

    for leaf in &leaves {
        let segments
            = leaf.path.iter().map(|s| s.as_str()).collect::<Vec<_>>();

        let expected
            = Settings::hydrate(&segments, sample_value(&leaf.type_name)).unwrap().export(true);

        let entry
            = settings.get(&segments)
                .unwrap_or_else(|e| panic!("{:?} isn't in the loaded configuration ({e}):\n{document}", leaf.path));

        assert!(matches!(entry.source, Source::Project), "{:?} wasn't read from the file:\n{document}", leaf.path);

        assert_shape(&serde_yaml::Value::Mapping(serde_yaml::Mapping::from_iter([(serde_yaml::Value::from("type"), serde_yaml::Value::from(leaf.type_name.as_str()))])), &written_value(&document, &leaf.path), &leaf.path.join("."));
        assert_eq!(entry.value.export(true), expected, "{:?} didn't round-trip:\n{document}", leaf.path);

        // What `yarn config get` prints must be accepted by `yarn config set`
        // and yield the same value
        let printed
            = strip_ansi(&settings.get(&segments).unwrap().value.export(false));

        let reparsed
            = Settings::hydrate(&segments, &printed)
                .unwrap_or_else(|e| panic!("{:?}: `config get` printed {printed:?}, which `config set` rejects ({e})", leaf.path));

        assert_eq!(reparsed.export(true), expected, "{:?}: `config get` printed {printed:?}, which `config set` reads differently", leaf.path);
    }
}

#[test]
fn every_list_setting_should_roundtrip_through_config_get_json() {
    let _scope
        = RedactionScope::new(false);

    let mut leaves
        = Vec::new();
    let mut lists
        = Vec::new();

    collect(&schema(), vec![], &mut leaves, &mut lists);

    assert!(lists.len() > 10, "The schema walker should have found most list settings");

    let base_document
        = leaves_document(&leaves);

    for list in lists {
        let segments
            = list.path.iter().map(|s| s.as_str()).collect::<Vec<_>>();

        let json
            = JsonDocument::to_string(&serde_yaml::Value::Sequence(vec![sample_json(&list.item)])).unwrap();

        let document
            = config_set(&base_document, &list.path, &json);

        let first_export
            = load(&document).get(&segments).unwrap().value.export(true);

        // `yarn config get --json` must print something that `yarn config set`
        // accepts back, without changing the value
        let document
            = config_set(&base_document, &list.path, &first_export);

        assert_shape(&serde_yaml::Value::Mapping(serde_yaml::Mapping::from_iter([
            (serde_yaml::Value::from("type"), serde_yaml::Value::from("array")),
            (serde_yaml::Value::from("items"), list.item.clone()),
        ])), &written_value(&document, &list.path), &list.path.join("."));

        let second_export
            = load(&document).get(&segments).unwrap().value.export(true);

        assert_eq!(first_export, second_export, "{:?} didn't round-trip:\n{document}", list.path);
        assert_ne!(first_export, "[]", "{:?} wasn't set:\n{document}", list.path);
    }
}

#[test]
fn durations_should_be_written_with_an_explicit_unit() {
    let document
        = config_set("", &["npmMinimalAgeGate".to_string()], "3h");

    assert_eq!(document, "npmMinimalAgeGate: 3h\n");

    let document
        = config_set("", &["npmMinimalAgeGate".to_string()], "0");

    assert_eq!(document, "npmMinimalAgeGate: 0s\n");

    let document
        = config_set("", &["packageRules".to_string()], r#"[{"packageFilter": "lodash", "npmMinimalAgeGate": "1d"}]"#);

    let settings
        = load(&document);

    assert_eq!(settings.get(&["packageRules"]).unwrap().value.export(true), r#"[{"ecosystemFilter":null,"npmAlwaysAuth":null,"npmAuthIdent":null,"npmAuthToken":null,"npmMinimalAgeGate":"1d","npmPublishRegistry":null,"npmRegistryServer":null,"packageFilter":"lodash","pypiAuthIdent":null,"pypiAuthToken":null,"pypiMinimalAgeGate":null,"pypiRegistryServer":null}]"#);
}

#[test]
fn unknown_settings_should_still_be_rejected() {
    assert!(matches!(Settings::hydrate(&["notASetting"], "true"), Err(HydrateError::KeyNotFound(_))));
    assert!(matches!(Settings::hydrate(&["npmScopes", "foo", "notASetting"], "true"), Err(HydrateError::KeyNotFound(_))));
    assert!(matches!(Settings::hydrate(&["packageRules", "0", "npmMinimalAgeGate"], "1d"), Err(HydrateError::ListItem(_))));
    assert!(matches!(Settings::hydrate(&["npmScopes", "foo", "npmAlwaysAuth"], "maybe"), Err(HydrateError::InvalidValue(_))));
}
