use std::{collections::BTreeMap, fs::File, io::Write, path::Path};

use convert_case::{Case, Casing};
use serde::Deserialize;
use serde_with::{serde_as, OneOrMany};

#[derive(Debug, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
enum Type {
    Null,
    Object,
    String,
    Boolean,
    Array,
    #[serde(untagged)]
    Custom(String),
}

enum InternalTypeKind {
    Native(String),
    Array(Box<InternalType>),
    Map(Box<InternalType>),
    Struct(String),
}

struct InternalType {
    kind: InternalTypeKind,
    nullable: bool,
}

impl InternalType {
    fn new(kind: InternalTypeKind, nullable: bool) -> Self {
        Self {kind, nullable}
    }

    fn to_intermediate_type_string(&self) -> String {
        let kind = match &self.kind {
            InternalTypeKind::Native(name) => name,
            InternalTypeKind::Array(inner) => &format!("Vec<{}>", inner.to_intermediate_type_string()),
            InternalTypeKind::Map(inner) => &format!("BTreeMap<String, {}>", inner.to_intermediate_type_string()),
            InternalTypeKind::Struct(name) => &name,
        };

        let kind = if matches!(self.kind, InternalTypeKind::Native(_)) {
            &format!("Interpolated<{}>", kind)
        } else {
            kind
        };

        if self.nullable {
            format!("Option<{}>", kind)
        } else {
            format!("{}", kind)
        }
    }

    fn to_type_string(&self) -> String {
        let kind = match &self.kind {
            InternalTypeKind::Native(name) => &name,
            InternalTypeKind::Array(inner) => &format!("Vec<{}>", inner.to_type_string()),
            InternalTypeKind::Map(inner) => &format!("BTreeMap<String, {}>", inner.to_type_string()),
            InternalTypeKind::Struct(name) => &name,
        };

        let kind = if self.nullable {
            format!("Option<{}>", kind)
        } else {
            format!("{}", kind)
        };

        if matches!(self.kind, InternalTypeKind::Native(_)) {
            format!("Setting<{}>", kind)
        } else {
            kind
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum Expression {
    String(String),
    Bool(bool),
    Number(usize),
}

#[serde_as]
#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
struct Field {
    #[serde(rename = "type")]
    #[serde_as(as = "OneOrMany<_>")]
    types: Vec<Type>,
    title: Option<String>,
    default: Option<Expression>,
    properties: Option<BTreeMap<String, Field>>,
    additional_properties: Option<Box<Field>>,
    items: Option<Box<Field>>,
}

impl Field {
    fn send_to(&self, generator: &mut Generator) {
        let type_
            = self.get_type();

        if let InternalTypeKind::Struct(name) = &type_.kind {
            let fields
                = generator.ensure_struct(name);

            if let Some(properties) = &self.properties {
                for (name, field) in properties.iter() {
                    let field_default = match field.default.as_ref() {
                        Some(Expression::String(default)) if default.contains("::")
                            => format!("|| Setting::new({}, Source::Default)", default),

                        Some(Expression::String(default))
                            => format!("|| Setting::new(FromFileString::from_file_string({}).unwrap(), Source::Default)", sonic_rs::to_string(default).unwrap()),

                        Some(Expression::Bool(default))
                            => format!("|| Setting::new({}, Source::Default)", default),

                        Some(Expression::Number(default))
                            => format!("|| Setting::new({}, Source::Default)", default),

                        None if field.types.contains(&Type::Null)
                            => "|| Setting::new(None, Source::Default)".to_string(),

                        None
                            => "|| panic!(\"No default value available\")".to_string(),
                    };

                    fields.push(GeneratorField {
                        name: name.to_string(),
                        type_: field.get_type(),
                        default: field_default,
                    });
                }

                for field in properties.values() {
                    field.send_to(generator);
                }
            }
        }

        if let InternalTypeKind::Map(_) = &type_.kind {
            if let Some(additional_property) = &self.additional_properties {
                additional_property.send_to(generator);
            }
        }

        if let InternalTypeKind::Array(_) = &type_.kind {
            if let Some(items) = &self.items {
                items.send_to(generator);
            }
        }
    }

    fn get_type(&self) -> InternalType {
        let (types, nullable): (Vec<_>, Vec<_>)
            = self.types.iter()
                .partition(|t| **t != Type::Null);

        assert!(types.len() == 1, "Properties must have exactly one type");

        let main_type
            = types.first()
                .expect("No type found");

        let is_nullable
            = nullable.len() > 0;

        match main_type {
            Type::Boolean => {
                InternalType::new(InternalTypeKind::Native("bool".to_string()), is_nullable)
            },

            Type::String => {
                InternalType::new(InternalTypeKind::Native("String".to_string()), is_nullable)
            },

            Type::Array => {
                InternalType::new(InternalTypeKind::Array(Box::new(self.items.as_ref().expect("Array properties must have an item").get_type())), is_nullable)
            },

            Type::Object => match (self.properties.as_ref(), self.additional_properties.as_ref()) {
                (None, Some(additional_properties)) => {
                    InternalType::new(InternalTypeKind::Map(Box::new(additional_properties.get_type())), is_nullable)
                },

                (Some(_), None) => {
                    InternalType::new(InternalTypeKind::Struct(self.title.as_ref().expect("Object properties must have a title").clone()), is_nullable)
                },

                (Some(_), Some(_)) => {
                    panic!("Object properties cannot have both properties and additional properties");
                },

                (None, None) => {
                    panic!("Object properties must have either properties or additional properties");
                },
            },

            Type::Custom(name) => {
                InternalType::new(InternalTypeKind::Native(name.clone()), is_nullable)
            },

            Type::Null => {
                panic!("The null type cannot be a main type");
            },
        }
    }
}

struct GeneratorField {
    name: String,
    type_: InternalType,
    default: String,
}

struct Generator {
    structs: BTreeMap<String, Vec<GeneratorField>>,
}

impl Generator {
    pub fn new() -> Self {
        Self {
            structs: BTreeMap::new(),
        }
    }

    pub fn ensure_struct(&mut self, name: &str) -> &mut Vec<GeneratorField> {
        self.structs.entry(name.to_string()).or_insert(Vec::new())
    }

    pub fn generate<T: Write>(&self, writer: &mut T) {
        writeln!(writer, "mod intermediate {{").unwrap();
        writeln!(writer, "    use super::*;").unwrap();

        for (name, fields) in &self.structs {
            writeln!(writer).unwrap();
            writeln!(writer, "    #[derive(Default, Deserialize)]").unwrap();
            writeln!(writer, "    pub struct {} {{", name).unwrap();

            for field in fields {
                let lc_snake_name
                    = field.name.to_case(Case::Snake);
                let type_
                    = &field.type_;

                writeln!(writer, "        #[serde(default)] pub {lc_snake_name}: Partial<{}>,", type_.to_intermediate_type_string()).unwrap();
            }

            writeln!(writer, "    }}").unwrap();
        }

        writeln!(writer, "}}").unwrap();
        writeln!(writer).unwrap();

        for (name, fields) in &self.structs {
            writeln!(writer).unwrap();
            writeln!(writer, "pub struct {name} {{").unwrap();

            for field in fields {
                let lc_snake_name
                    = field.name.to_case(Case::Snake);
                let type_
                    = &field.type_;

                writeln!(writer, "    pub {lc_snake_name}: {},", type_.to_type_string()).unwrap();
            }

            writeln!(writer, "}}").unwrap();
            writeln!(writer).unwrap();
            writeln!(writer, "impl MergeSettings for {name} {{").unwrap();
            writeln!(writer, "    type Intermediate = intermediate::{name};").unwrap();
            writeln!(writer).unwrap();
            writeln!(writer, "    fn merge<F: FnOnce() -> Self>(context: &Context, prefix: Option<&str>, user: Partial<Self::Intermediate>, project: Partial<Self::Intermediate>, _default: F) -> Self {{").unwrap();
            writeln!(writer, "        let user = user.unwrap_or_default();").unwrap();
            writeln!(writer, "        let project = project.unwrap_or_default();").unwrap();
            writeln!(writer).unwrap();
            writeln!(writer, "        Self {{").unwrap();

            for field in fields {
                let name = &field.name;
                let default = &field.default;

                let lc_snake_name
                    = name.to_case(Case::Snake);
                let uc_snake_name
                    = name.to_case(Case::UpperSnake);

                writeln!(writer, "            {lc_snake_name}: MergeSettings::merge(context, prefix.map(|p| format!(\"{{}}_{uc_snake_name}\", p)).as_deref(), user.{lc_snake_name}, project.{lc_snake_name}, {default}),").unwrap();
            }

            writeln!(writer, "        }}").unwrap();
            writeln!(writer, "    }}").unwrap();
            writeln!(writer, "}}").unwrap();
        }
    }
}

fn main() {
    let schema_content
        = include_str!("schema.json");
    let schema
        = sonic_rs::from_str::<Field>(schema_content)
            .expect("Failed to parse schema");

    let mut generator
        = Generator::new();

    schema.send_to(&mut generator);

    let out_dir
        = std::env::var_os("OUT_DIR")
            .expect("OUT_DIR must be set");

    let out_file
        = Path::new(&out_dir)
            .join("schema.rs");

    let mut file
        = File::create(out_file)
            .expect("Failed to create schema.rs");

    generator.generate(&mut file);
}
