use std::process::ExitCode;

use clipanion::cli;
use colored::Colorize;
use zpm_utils::{tree, AbstractValue, DataType, ToFileString, ToHumanString};
use zpm_parsers::{document::Document, JsonDocument, Value};

use crate::{constraints::{check_constraints, structs::{ConstraintsOutput, WorkspaceError, WorkspaceOperation}}, error::Error, project::Project};

/// Check constraints
#[cli::command]
#[cli::path("constraints")]
#[cli::category("Dependency management")]
pub struct Constraints {
    #[cli::option("-f,--fix", default = false)]
    fix: bool,

    #[cli::option("--json", default = false)]
    json: bool,
}

impl Constraints {
    pub async fn execute(&self) -> Result<ExitCode, Error> {
        let mut project
            = Project::new(None).await?;

        let max_loops = if self.fix {
            10
        } else {
            1
        };

        for loop_idx in 1..=max_loops {
            project
                .lazy_install().await?;

            let output
                = check_constraints(&project, self.fix).await?;

            for (workspace_rel_path, operations) in &output.all_workspace_operations {
                // Read the current manifest
                let manifest_path = project.project_cwd
                    .with_join(workspace_rel_path)
                    .with_join_str("package.json");

                let manifest_content = manifest_path
                    .fs_read_prealloc()?;

                let mut document
                    = JsonDocument::new(manifest_content)?;

                // Apply each operation
                for operation in operations {
                    match operation {
                        WorkspaceOperation::Set { path, value } => {
                            document.set_path(&zpm_parsers::Path::from_segments(path.clone()), value.into())?;
                        },

                        WorkspaceOperation::Unset { path } => {
                            document.set_path(&zpm_parsers::Path::from_segments(path.clone()), Value::Undefined)?;
                        },
                    }
                }

                // Write the formatted result back
                manifest_path
                    .fs_change(&document.input, false)?;
            }

            let should_break = false
                || output.all_workspace_operations.is_empty()
                || output.all_workspace_errors.is_empty()
                || loop_idx == max_loops;

            if should_break {
                if self.json {
                    println!("{}", String::from_utf8_lossy(&output.raw_json));
                }

                if !output.all_workspace_errors.is_empty() {
                    if !self.json {
                        display_report(&project, &output)?;
                    }
                    return Ok(ExitCode::FAILURE);
                } else {
                    return Ok(ExitCode::SUCCESS);
                }
            }
        }

        unreachable!()
    }
}

fn display_report(project: &Project, output: &ConstraintsOutput) -> Result<(), Error> {
    let are_all_errors_fixable = output.all_workspace_errors.iter().all(|(_, errors)| errors.iter().all(|error| match error {
        WorkspaceError::MissingField { .. } => true,
        WorkspaceError::ExtraneousField { .. } => true,
        WorkspaceError::InvalidField { .. } => true,
        WorkspaceError::ConflictingValues { .. } => false,
        WorkspaceError::UserError { .. } => false,
    }));

    let are_some_errors_fixable = output.all_workspace_errors.iter().any(|(_, errors)| errors.iter().any(|error| match error {
        WorkspaceError::MissingField { .. } => true,
        WorkspaceError::ExtraneousField { .. } => true,
        WorkspaceError::InvalidField { .. } => true,
        WorkspaceError::ConflictingValues { .. } => false,
        WorkspaceError::UserError { .. } => false,
    }));

    if are_all_errors_fixable {
        println!("➤ Those errors can all be fixed by running {}", DataType::Code.colorize("yarn constraints --fix"));
        println!();
    } else if are_some_errors_fixable {
        println!("➤ Errors prefixed by '⚙' can be fixed by running {}", DataType::Code.colorize("yarn constraints --fix"));
        println!();
    }

    let mut root_children
        = vec![];

    let cog
        = "⚙".truecolor(130, 130, 130).to_string();

    for (workspace_rel_path, errors) in &output.all_workspace_errors {
        let workspace
            = project.workspace_by_rel_path(&workspace_rel_path)?;

        let mut report_children
            = vec![];

        for error in errors {
            match error {
                WorkspaceError::MissingField { field_path, expected } => {
                    report_children.push(tree::Node {
                        label: Some(format!("{cog} Missing field {}; expected {}", field_path.to_print_string(), expected)),
                        value: None,
                        children: None,
                    });
                },

                WorkspaceError::ExtraneousField { field_path, current_value } => {
                    report_children.push(tree::Node {
                        label: Some(format!("{cog} Extraneous field {} currently set to {}", field_path.to_print_string(), current_value)),
                        value: None,
                        children: None,
                    });
                },

                WorkspaceError::InvalidField { field_path, expected, current_value } => {
                    report_children.push(tree::Node {
                        label: Some(format!("{cog} Invalid field {}; expected {}, found {}", field_path.to_print_string(), expected, current_value)),
                        value: None,
                        children: None,
                    });
                },

                WorkspaceError::ConflictingValues { field_path, set_values, unset_values } => {
                    let entries = unset_values.as_ref()
                        .map(|unset_values| (DataType::Code.colorize("undefined"), unset_values))
                        .into_iter()
                        .chain(set_values.iter().map(|(value, info)| (value.to_print_string(), info)))
                        .collect::<Vec<_>>();

                    let mut flat_entries = entries.iter()
                        .flat_map(|(value, info)| info.callers.iter().map(|caller| (value.as_str(), caller)))
                        .collect::<Vec<_>>();

                    flat_entries.sort_by_cached_key(|(_, caller)| {
                        caller.to_file_string()
                    });

                    let options = flat_entries.iter()
                        .map(|(value, caller)| format!("{} at {}", value, caller.to_print_string()))
                        .map(|option| tree::Node {label: Some(option), value: None, children: None})
                        .collect::<Vec<_>>();

                    report_children.push(tree::Node {
                        label: Some(format!("Conflict detected in constraint targeting {}; conflicting values are:", field_path.to_print_string())),
                        value: None,
                        children: Some(tree::TreeNodeChildren::Vec(options)),
                    });
                },

                WorkspaceError::UserError { message } => {
                    report_children.push(tree::Node {
                        label: Some(message.to_string()),
                        value: None,
                        children: None,
                    });
                },
            }
        }

        root_children.push(tree::Node {
            label: None,
            value: Some(AbstractValue::new(workspace.locator_path())),
            children: Some(tree::TreeNodeChildren::Vec(report_children)),
        });
    }

    let root = tree::Node {
        label: None,
        value: None,
        children: Some(tree::TreeNodeChildren::Vec(root_children)),
    };

    print!("{}", root.to_string());

    Ok(())
}
