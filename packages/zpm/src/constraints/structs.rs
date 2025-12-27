use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_with::serde_as;
use zpm_primitives::{Ident, Locator, Range};
use zpm_utils::{ColoredJsonValue, DataType, Path, ToFileString, ToHumanString};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
pub struct Caller {
    pub file: Option<String>,
    pub method_name: Option<String>,
    pub arguments: Vec<String>,
    pub line: Option<u32>,
    pub column: Option<u32>,
}

impl ToFileString for Caller {
    fn to_file_string(&self) -> String {
        let mut parts
            = vec![];

        if let Some(method_name) = &self.method_name {
            parts.push(method_name.clone());
        }

        if let Some(file) = &self.file {
            let mut file_parts
                = vec![];

            file_parts.push(file.clone());

            if let Some(line) = &self.line {
                file_parts.push(line.to_string());

                if let Some(column) = &self.column {
                    file_parts.push(column.to_string());
                }
            }

            parts.push(format!("({})", file_parts.join(":")));
        }

        parts.join(" ")
    }
}

impl ToHumanString for Caller {
    fn to_print_string(&self) -> String {
        let mut parts
            = vec![];

        if let Some(method_name) = &self.method_name {
            parts.push(DataType::Code.colorize(method_name));
        }

        if let Some(file) = &self.file {
            let mut file_parts
                = vec![];

            file_parts.push(DataType::Path.colorize(file));

            if let Some(line) = &self.line {
                file_parts.push(DataType::Number.colorize(&line.to_string()));

                if let Some(column) = &self.column {
                    file_parts.push(DataType::Number.colorize(&column.to_string()));
                }
            }

            parts.push(format!("({})", file_parts.join(&DataType::Code.colorize(":"))));
        }

        parts.join(" ")
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PerValueInfo {
    pub callers: Vec<Caller>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
#[serde(rename_all = "camelCase")]
#[serde(rename_all_fields = "camelCase")]
pub enum WorkspaceError {
    MissingField {
        field_path: zpm_parsers::Path,
        expected: ColoredJsonValue,
    },

    ExtraneousField {
        field_path: zpm_parsers::Path,
        current_value: ColoredJsonValue,
    },

    InvalidField {
        field_path: zpm_parsers::Path,
        expected: ColoredJsonValue,
        current_value: ColoredJsonValue,
    },

    ConflictingValues {
        field_path: zpm_parsers::Path,
        set_values: Vec<(ColoredJsonValue, PerValueInfo)>,
        unset_values: Option<PerValueInfo>,
    },

    UserError {
        message: String,
    },
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
#[serde(rename_all = "camelCase")]
#[serde(rename_all_fields = "camelCase")]
pub enum WorkspaceOperation {
    Set {
        path: Vec<String>,
        value: serde_json::Value,
    },
    Unset {
        path: Vec<String>,
    },
}

#[serde_as]
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
pub struct ConstraintsOutput {
    #[serde(skip)]
    pub raw_json: Vec<u8>,
    #[serde_as(as = "Vec<(_, _)>")]
    pub all_workspace_operations: BTreeMap<Path, Vec<WorkspaceOperation>>,
    #[serde_as(as = "Vec<(_, _)>")]
    pub all_workspace_errors: BTreeMap<Path, Vec<WorkspaceError>>,
}

impl ConstraintsOutput {
    pub fn is_empty(&self) -> bool {
        self.all_workspace_operations.is_empty() && self.all_workspace_errors.is_empty()
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
pub struct ConstraintsContext<'a> {
    pub workspaces: Vec<ConstraintsWorkspace>,
    pub packages: Vec<ConstraintsPackage<'a>>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConstraintsDependency {
    pub ident: Ident,
    pub range: Range,
    pub dependency_type: String,
    pub resolution: Option<Locator>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConstraintsWorkspace {
    pub cwd: Path,
    pub ident: Ident,
    pub dependencies: Vec<ConstraintsDependency>,
    pub peer_dependencies: Vec<ConstraintsDependency>,
    pub dev_dependencies: Vec<ConstraintsDependency>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConstraintsPackage<'a> {
    pub locator: Locator,
    pub workspace: Option<Path>,
    pub ident: Ident,
    pub version: zpm_semver::Version,
    pub dependencies: Vec<(&'a Ident, &'a Locator)>,
    pub peer_dependencies: Vec<(&'a Ident, &'a Locator)>,
}
