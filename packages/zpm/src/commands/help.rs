//! The help of a single command (`yarn <command> --help`).
//!
//! The clipanion revision we use answers it with the command's usage line
//! and description only, as for `yarn --help`. Until we upgrade to a version
//! that prints the whole help of a command itself, we print it here, in the
//! same layout: the description, then the usage, the options, the details,
//! and the examples.

use std::process::ExitCode;

use clipanion::{
    Environment,
    core::{BuiltinCommand, CommandSpec, Component, Info, SelectionResult},
    format::write_fading_title_line,
    prelude::*,
};
use colored::Colorize;

use super::YarnCli;

/// The width the help wraps its text to.
const HELP_WIDTH: usize = 80;

/// A description goes on the line after its option's definition when it
/// would have fewer columns than this next to it.
const MIN_DESCRIPTION_WIDTH: usize = 30;

/// Prints the help of the command `env.argv` asks it for, if it asks for the
/// help of a single command.
pub fn print_command_help(env: &Environment) -> Option<ExitCode> {
    if !env.argv.iter().any(|arg| arg == "-h" || arg == "--help") {
        return None;
    }

    let builder
        = YarnCli::build_cli().ok()?;

    let Ok(SelectionResult::Builtin(BuiltinCommand::Help(commands))) = YarnCli::parse_args(&builder, env) else {
        return None;
    };

    let [command] = commands.as_slice() else {
        return None;
    };

    print!("{}", format_command_help(&env.info, command));

    Some(ExitCode::SUCCESS)
}

struct OptionHelp {
    definition: String,
    description: String,
}

fn option_helps(command_spec: &CommandSpec) -> Vec<OptionHelp> {
    command_spec.components.iter()
        .filter_map(Component::is_option)
        .filter(|option| !option.is_hidden)
        .map(|option| {
            // The usage form of an option is its definition between brackets
            // (`[--mode <arg>]`, or `<...>` when it's required)
            let usage
                = option.to_string();

            let definition
                = usage[1..usage.len() - 1].to_string();

            let description = option.documentation.as_ref()
                .map(|documentation| documentation.description.clone())
                .unwrap_or_default();

            OptionHelp {
                definition,
                description,
            }
        })
        .collect()
}

/// Takes the examples out of a command's details: the paragraphs after an
/// `## Examples` heading, alternating the description of each example and its
/// command, between backticks.
fn split_examples(details: &str) -> (Vec<&str>, Vec<(&str, &str)>) {
    let paragraphs = details.split("\n\n")
        .map(|paragraph| paragraph.trim())
        .collect::<Vec<_>>();

    let Some(heading_index) = paragraphs.iter().position(|paragraph| *paragraph == "## Examples") else {
        return (paragraphs, vec![]);
    };

    let examples = paragraphs[heading_index + 1..].chunks(2)
        .filter_map(|pair| match pair {
            [description, command] => command.strip_prefix('`')
                .and_then(|command| command.strip_suffix('`'))
                .map(|command| (*description, command)),

            _
                => None,
        })
        .collect();

    (paragraphs[..heading_index].to_vec(), examples)
}

fn write_section_header(output: &mut String, info: &Info, title: &str) {
    output.push('\n');

    if info.colorized {
        write_fading_title_line(output, title, (128, 128, 128), HELP_WIDTH, 50);
    } else {
        let header
            = format!("━━━ {} ", title);

        output.push_str(&header);

        for _ in header.chars().count()..HELP_WIDTH {
            output.push('━');
        }

        output.push('\n');
    }

    output.push('\n');
}

/// Writes `paragraphs`, wrapped to `width` columns, with an empty line between
/// them if `spaced`. Every line but the first one starts with `indent`, and the
/// lines that follow the first one of a list item are indented two more
/// columns.
fn write_paragraphs<'a>(output: &mut String, paragraphs: impl IntoIterator<Item = &'a str>, indent: &str, width: usize, spaced: bool) {
    for (index, paragraph) in paragraphs.into_iter().enumerate() {
        if index > 0 {
            if spaced {
                output.push('\n');
            }

            output.push_str(indent);
        }

        let is_list_item
            = paragraph.starts_with("- ") || paragraph.starts_with("* ");

        let continuation_indent = match is_list_item {
            true => format!("{}  ", indent),
            false => indent.to_string(),
        };

        let available_width
            = width.saturating_sub(indent.chars().count());

        let mut line_width
            = 0;

        for word in paragraph.split_whitespace() {
            let word_width
                = word.chars().count();

            if line_width > 0 && line_width + 1 + word_width > available_width {
                output.push('\n');
                output.push_str(&continuation_indent);
                line_width = continuation_indent.chars().count() - indent.chars().count();
            } else if line_width > 0 {
                output.push(' ');
                line_width += 1;
            }

            output.push_str(word);
            line_width += word_width;
        }

        output.push('\n');
    }
}

fn format_command_help(info: &Info, command_spec: &CommandSpec) -> String {
    let mut output
        = String::new();

    if let Some(documentation) = &command_spec.documentation {
        let description = match info.colorized {
            true => documentation.description.bold().to_string(),
            false => documentation.description.clone(),
        };

        output.push_str(&description);
        output.push('\n');
    }

    write_section_header(&mut output, info, "Usage");

    output.push_str("  ");
    output.push_str(&command_spec.usage().oneliner(info));
    output.push('\n');

    let options
        = option_helps(command_spec);

    if !options.is_empty() {
        write_section_header(&mut output, info, "Options");

        let definition_width = options.iter()
            .map(|option| option.definition.chars().count())
            .max()
            .unwrap_or(0);

        let description_column
            = 2 + definition_width + 4;

        let is_inline
            = description_column + MIN_DESCRIPTION_WIDTH <= HELP_WIDTH;

        let description_indent = match is_inline {
            true => " ".repeat(description_column),
            false => " ".repeat(6),
        };

        for option in &options {
            let definition = match info.colorized {
                true => option.definition.bold().to_string(),
                false => option.definition.clone(),
            };

            output.push_str("  ");
            output.push_str(&definition);

            if option.description.is_empty() {
                output.push('\n');
                continue;
            }

            if is_inline {
                output.push_str(&" ".repeat(description_column - 2 - option.definition.chars().count()));
            } else {
                output.push('\n');
                output.push_str(&description_indent);
            }

            write_paragraphs(&mut output, option.description.split("\n\n"), &description_indent, HELP_WIDTH, false);
        }
    }

    let (details, examples) = command_spec.documentation.as_ref()
        .and_then(|documentation| documentation.details.as_deref())
        .map(split_examples)
        .unwrap_or_default();

    if !details.is_empty() {
        write_section_header(&mut output, info, "Details");
        write_paragraphs(&mut output, details, "", HELP_WIDTH, true);
    }

    let examples = command_spec.examples.iter()
        .map(|example| (example.description.as_str(), example.command.as_str()))
        .chain(examples)
        .collect::<Vec<_>>();

    if !examples.is_empty() {
        write_section_header(&mut output, info, "Examples");

        for (index, (description, command)) in examples.into_iter().enumerate() {
            if index > 0 {
                output.push('\n');
            }

            write_paragraphs(&mut output, [description], "", HELP_WIDTH, false);

            let command = match info.colorized {
                true => format!("› {}", command).bright_white().to_string(),
                false => format!("› {}", command),
            };

            output.push_str("  ");
            output.push_str(&command);
            output.push('\n');
        }
    }

    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_the_examples_out_of_the_details() {
        let (details, examples) = split_examples(concat!(
            "This command adds packages.\n\n",
            "## Examples\n\n",
            "Add a regular package\n\n",
            "`yarn add lodash`\n\n",
            "Add a development dependency\n\n",
            "`yarn add -D typescript`",
        ));

        assert_eq!(details, vec!["This command adds packages."]);
        assert_eq!(examples, vec![
            ("Add a regular package", "yarn add lodash"),
            ("Add a development dependency", "yarn add -D typescript"),
        ]);
    }

    #[test]
    fn keeps_the_details_without_examples() {
        let (details, examples)
            = split_examples("First paragraph.\n\nSecond paragraph.");

        assert_eq!(details, vec!["First paragraph.", "Second paragraph."]);
        assert!(examples.is_empty());
    }
}
