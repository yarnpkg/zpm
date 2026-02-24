use std::collections::{BTreeMap, HashSet};

use winnow::combinator::{delimited, opt};
use winnow::prelude::*;
use winnow::token::{take_till, take_while};

use zpm_primitives::{Ident, IdentGlob};
use zpm_utils::ToFileString;

use crate::ast::{Attribute, Dependency, Include, Task, TaskFile, TaskName};
use crate::error::Error;

pub fn parse(input: &str) -> Result<TaskFile, Error> {
    let mut tasks: BTreeMap<TaskName, Task>
        = BTreeMap::new();

    let mut includes: Vec<Include>
        = Vec::new();

    let mut pending_attributes: Vec<Attribute>
        = Vec::new();

    let mut current_task: Option<(TaskName, Task)>
        = None;

    for (line_num, line) in input.lines().enumerate() {
        let line_num
            = line_num + 1;

        if line.trim().is_empty() {
            continue;
        }

        if line.trim_start().starts_with('#') {
            continue;
        }

        let is_indented
            = line.starts_with(' ') || line.starts_with('\t');

        if is_indented {
            if let Some((_, ref mut task)) = current_task {
                let script_line
                    = strip_indent(line);

                task.script.push(script_line.to_string());
            } else {
                return Err(Error::UnexpectedIndent(line_num));
            }
        } else {
            if let Some((name, task)) = current_task.take() {
                tasks.insert(name, task);
            }

            let trimmed
                = line.trim();

            if trimmed.starts_with('@') {
                let attr
                    = parse_attribute_line(trimmed)
                        .map_err(|e| Error::ParseError {
                            line: line_num,
                            message: e,
                        })?;

                pending_attributes.push(attr);
            } else if let Some(include_arg) = trimmed.strip_prefix("include ") {
                if !pending_attributes.is_empty() {
                    return Err(Error::ParseError {
                        line: line_num,
                        message: "Attributes cannot be applied to include directives".to_string(),
                    });
                }

                let include
                    = parse_include(include_arg.trim())
                        .map_err(|e| Error::ParseError {
                            line: line_num,
                            message: e,
                        })?;

                includes.push(include);
            } else {
                let (name, dependencies)
                    = parse_task_header(trimmed)
                        .map_err(|e| Error::ParseError {
                            line: line_num,
                            message: e,
                        })?;

                current_task = Some((name, Task {
                    attributes: std::mem::take(&mut pending_attributes),
                    dependencies,
                    script: Vec::new(),
                }));
            }
        }
    }

    if let Some((name, task)) = current_task.take() {
        tasks.insert(name, task);
    }

    if !pending_attributes.is_empty() {
        return Err(Error::OrphanedAttributes);
    }

    Ok(TaskFile { includes, tasks })
}

fn parse_include(input: &str) -> Result<Include, String> {
    if input.is_empty() {
        return Err("Include directive requires an ident".to_string());
    }

    let slash_pos
        = if input.starts_with('@') {
            input.find('/').and_then(|first| {
                input[first + 1..].find('/').map(|second| first + 1 + second)
            })
        } else {
            input.find('/')
        };

    if let Some(pos) = slash_pos {
        let ident_str
            = &input[..pos];

        let path_str
            = &input[pos + 1..];

        if ident_str.is_empty() {
            return Err("Include ident cannot be empty".to_string());
        }

        if path_str.is_empty() {
            return Err("Include path cannot be empty after '/'".to_string());
        }

        let ident
            = Ident::new(ident_str);

        Ok(Include {
            ident,
            path: Some(path_str.to_string()),
        })
    } else {
        let ident
            = Ident::new(input);

        Ok(Include {
            ident,
            path: None,
        })
    }
}

fn strip_indent(line: &str) -> &str {
    line.trim_start_matches(|c| c == ' ' || c == '\t')
}

fn parse_attribute_line(input: &str) -> Result<Attribute, String> {
    let mut input = input;
    parse_attribute
        .parse_next(&mut input)
        .map_err(|e| e.to_string())
}

fn parse_attribute(input: &mut &str) -> winnow::ModalResult<Attribute> {
    '@'.parse_next(input)?;

    let name: &str
        = take_while(1.., |c: char| c.is_alphanumeric() || c == '_' || c == '-')
            .parse_next(input)?;

    let value: Option<&str>
        = opt(delimited('(', take_till(1.., ')'), ')'))
            .parse_next(input)?;

    Ok(Attribute {
        name: name.to_string(),
        value: value.map(|s| s.to_string()),
    })
}

fn parse_task_header(input: &str) -> Result<(TaskName, Vec<Dependency>), String> {
    let Some(colon_pos) = input.find(':') else {
        return Err(format!("Missing ':' in task header: {}", input));
    };

    let name_str
        = input[..colon_pos].trim();

    if name_str.is_empty() {
        return Err("Task name cannot be empty".to_string());
    }

    let name
        = TaskName::new(name_str).map_err(|e| e.to_string())?;

    let deps_str
        = input[colon_pos + 1..].trim();

    let dependencies
        = if deps_str.is_empty() {
            Vec::new()
        } else {
            parse_dependencies(deps_str)?
        };

    let mut seen: HashSet<String>
        = HashSet::new();
    for dep in &dependencies {
        let dep_key = match dep {
            Dependency::Local { name, .. } => name.as_str().to_string(),
            Dependency::External { ident_glob, task_name, .. } => {
                format!("{}:{}", ident_glob.to_file_string(), task_name.as_str())
            }
        };
        if !seen.insert(dep_key.clone()) {
            return Err(format!("Duplicate dependency: {}", dep_key));
        }
    }

    Ok((name, dependencies))
}

fn parse_dependencies(input: &str) -> Result<Vec<Dependency>, String> {
    let mut dependencies
        = Vec::new();

    for token in input.split_whitespace() {
        let dep
            = parse_dependency(token)?;

        dependencies.push(dep);
    }

    Ok(dependencies)
}

fn parse_dependency(token: &str) -> Result<Dependency, String> {
    let (token, parallel)
        = if let Some(stripped) = token.strip_suffix('&') {
            (stripped, true)
        } else {
            (token, false)
        };

    if let Some(colon_pos) = token.rfind(':') {
        let ident_glob_str
            = &token[..colon_pos];

        let task_name_str
            = &token[colon_pos + 1..];

        if ident_glob_str.is_empty() || task_name_str.is_empty() {
            return Err(format!("Invalid dependency format: {}", token));
        }

        let ident_glob
            = IdentGlob::new(ident_glob_str)
                .map_err(|e| format!("Invalid ident glob '{}': {}", ident_glob_str, e))?;

        let task_name
            = TaskName::new(task_name_str)
                .map_err(|e| format!("Invalid task name '{}': {}", task_name_str, e))?;

        Ok(Dependency::External {
            ident_glob,
            task_name,
            parallel,
        })
    } else {
        let name
            = TaskName::new(token)
                .map_err(|e| format!("Invalid task name '{}': {}", token, e))?;

        Ok(Dependency::Local { name, parallel })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_task() {
        let input = "build:\n  npm run build";
        let result = parse(input).unwrap();
        assert_eq!(result.tasks.len(), 1);
        assert!(result.tasks.contains_key("build"));
        assert_eq!(result.tasks["build"].script, vec!["npm run build"]);
    }

    #[test]
    fn test_parse_task_with_dependencies() {
        let input = "build: lint typecheck\n  npm run build";
        let result = parse(input).unwrap();
        assert_eq!(result.tasks.len(), 1);
        assert_eq!(result.tasks["build"].dependencies.len(), 2);
        match &result.tasks["build"].dependencies[0] {
            Dependency::Local { name, parallel } => {
                assert_eq!(name, "lint");
                assert!(!parallel);
            }
            _ => panic!("Expected local dependency"),
        }
    }

    #[test]
    fn test_parse_with_attributes() {
        let input = "@parallel\n@timeout(30s)\nbuild: lint\n  npm run build";
        let result = parse(input).unwrap();
        assert_eq!(result.tasks["build"].attributes.len(), 2);
        assert_eq!(result.tasks["build"].attributes[0].name, "parallel");
        assert_eq!(result.tasks["build"].attributes[0].value, None);
        assert_eq!(result.tasks["build"].attributes[1].name, "timeout");
        assert_eq!(result.tasks["build"].attributes[1].value, Some("30s".to_string()));
    }

    #[test]
    fn test_parse_multiline_script() {
        let input = "build:\n  npm run build\n  npm run post-build";
        let result = parse(input).unwrap();
        assert_eq!(result.tasks["build"].script.len(), 2);
        assert_eq!(result.tasks["build"].script[0], "npm run build");
        assert_eq!(result.tasks["build"].script[1], "npm run post-build");
    }

    #[test]
    fn test_parse_external_dependency() {
        let input = "deploy: @my-lib/*:build\n  deploy.sh";
        let result = parse(input).unwrap();
        assert_eq!(result.tasks["deploy"].dependencies.len(), 1);
        match &result.tasks["deploy"].dependencies[0] {
            Dependency::External { ident_glob, task_name, parallel } => {
                assert_eq!(task_name, "build");
                assert!(ident_glob.check(&"@my-lib/foo".parse().unwrap()));
                assert!(!parallel);
            }
            _ => panic!("Expected external dependency"),
        }
    }

    #[test]
    fn test_parse_multiple_tasks() {
        let input = "lint:\n  eslint .\n\ntypecheck:\n  tsc --noEmit";
        let result = parse(input).unwrap();
        assert_eq!(result.tasks.len(), 2);
        assert!(result.tasks.contains_key("lint"));
        assert!(result.tasks.contains_key("typecheck"));
    }

    #[test]
    fn test_parse_with_comments() {
        let input = "# This is a comment\nbuild:\n  npm run build\n  # inline comment";
        let result = parse(input).unwrap();
        assert_eq!(result.tasks.len(), 1);
        assert_eq!(result.tasks["build"].script.len(), 1);
    }

    #[test]
    fn test_parse_tab_indent() {
        let input = "build:\n\tnpm run build";
        let result = parse(input).unwrap();
        assert_eq!(result.tasks["build"].script, vec!["npm run build"]);
    }

    #[test]
    fn test_unexpected_indent_error() {
        let input = "  indented line without task";
        let result = parse(input);
        assert!(matches!(result, Err(Error::UnexpectedIndent(1))));
    }

    #[test]
    fn test_orphaned_attributes_error() {
        let input = "@parallel\n@timeout(30s)";
        let result = parse(input);
        assert!(matches!(result, Err(Error::OrphanedAttributes)));
    }

    #[test]
    fn test_duplicate_dependency_error() {
        let input = "build: lint lint\n  npm run build";
        let result = parse(input);
        assert!(matches!(result, Err(Error::ParseError { line: 1, .. })));
    }

    #[test]
    fn test_parse_parallel_local_dependencies() {
        let input = "build: lint& typecheck& format\n  npm run build";
        let result = parse(input).unwrap();
        assert_eq!(result.tasks["build"].dependencies.len(), 3);

        match &result.tasks["build"].dependencies[0] {
            Dependency::Local { name, parallel } => {
                assert_eq!(name, "lint");
                assert!(parallel);
            }
            _ => panic!("Expected local dependency"),
        }

        match &result.tasks["build"].dependencies[1] {
            Dependency::Local { name, parallel } => {
                assert_eq!(name, "typecheck");
                assert!(parallel);
            }
            _ => panic!("Expected local dependency"),
        }

        match &result.tasks["build"].dependencies[2] {
            Dependency::Local { name, parallel } => {
                assert_eq!(name, "format");
                assert!(!parallel);
            }
            _ => panic!("Expected local dependency"),
        }
    }

    #[test]
    fn test_parse_parallel_external_dependency() {
        let input = "deploy: @my-lib/*:build&\n  deploy.sh";
        let result = parse(input).unwrap();
        assert_eq!(result.tasks["deploy"].dependencies.len(), 1);
        match &result.tasks["deploy"].dependencies[0] {
            Dependency::External { ident_glob, task_name, parallel } => {
                assert_eq!(task_name, "build");
                assert!(ident_glob.check(&"@my-lib/foo".parse().unwrap()));
                assert!(parallel);
            }
            _ => panic!("Expected external dependency"),
        }
    }

    #[test]
    fn test_parse_mixed_parallel_dependencies() {
        let input = "build: lint& @pkg/*:test& typecheck\n  npm run build";
        let result = parse(input).unwrap();
        assert_eq!(result.tasks["build"].dependencies.len(), 3);

        match &result.tasks["build"].dependencies[0] {
            Dependency::Local { name, parallel } => {
                assert_eq!(name, "lint");
                assert!(parallel);
            }
            _ => panic!("Expected local dependency"),
        }

        match &result.tasks["build"].dependencies[1] {
            Dependency::External { task_name, parallel, .. } => {
                assert_eq!(task_name, "test");
                assert!(parallel);
            }
            _ => panic!("Expected external dependency"),
        }

        match &result.tasks["build"].dependencies[2] {
            Dependency::Local { name, parallel } => {
                assert_eq!(name, "typecheck");
                assert!(!parallel);
            }
            _ => panic!("Expected local dependency"),
        }
    }

    #[test]
    fn test_parse_include_simple() {
        let input = "include my-lib\n\nbuild:\n  npm run build";
        let result = parse(input).unwrap();

        assert_eq!(result.includes.len(), 1);
        assert_eq!(result.includes[0].ident.as_str(), "my-lib");
        assert_eq!(result.includes[0].path, None);
        assert_eq!(result.tasks.len(), 1);
    }

    #[test]
    fn test_parse_include_with_path() {
        let input = "include my-lib/tasks/build.tasks\n\nbuild:\n  npm run build";
        let result = parse(input).unwrap();

        assert_eq!(result.includes.len(), 1);
        assert_eq!(result.includes[0].ident.as_str(), "my-lib");
        assert_eq!(result.includes[0].path, Some("tasks/build.tasks".to_string()));
    }

    #[test]
    fn test_parse_include_scoped_ident() {
        let input = "include @my-scope/my-lib\n\nbuild:\n  npm run build";
        let result = parse(input).unwrap();

        assert_eq!(result.includes.len(), 1);
        assert_eq!(result.includes[0].ident.as_str(), "@my-scope/my-lib");
        assert_eq!(result.includes[0].path, None);
    }

    #[test]
    fn test_parse_include_scoped_with_path() {
        let input = "include @my-scope/my-lib/custom-taskfile\n\nbuild:\n  npm run build";
        let result = parse(input).unwrap();

        assert_eq!(result.includes.len(), 1);
        assert_eq!(result.includes[0].ident.as_str(), "@my-scope/my-lib");
        assert_eq!(result.includes[0].path, Some("custom-taskfile".to_string()));
    }

    #[test]
    fn test_parse_multiple_includes() {
        let input = "include lib-a\ninclude lib-b/tasks\n\nbuild:\n  npm run build";
        let result = parse(input).unwrap();

        assert_eq!(result.includes.len(), 2);
        assert_eq!(result.includes[0].ident.as_str(), "lib-a");
        assert_eq!(result.includes[0].path, None);
        assert_eq!(result.includes[1].ident.as_str(), "lib-b");
        assert_eq!(result.includes[1].path, Some("tasks".to_string()));
    }

    #[test]
    fn test_parse_include_empty_error() {
        let input = "include \n\nbuild:\n  npm run build";
        let result = parse(input);
        assert!(matches!(result, Err(Error::ParseError { line: 1, .. })));
    }

    #[test]
    fn test_parse_include_with_attributes_error() {
        let input = "@parallel\ninclude my-lib\n\nbuild:\n  npm run build";
        let result = parse(input);
        assert!(matches!(result, Err(Error::ParseError { line: 2, .. })));
    }
}
