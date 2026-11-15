use std::{collections::BTreeMap, ops::Range, str::FromStr};

use itertools::Itertools;
use serde::{Deserialize, Serialize, de::DeserializeOwned};

use crate::{document::Document, value::{Indent, IndentStyle}, Error, Path, Value};

pub use serde_yaml as yaml_provider;

#[derive(Debug)]
pub struct YamlSource<T> {
    pub value: T,
}

impl<T: DeserializeOwned> FromStr for YamlSource<T> {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self { value: YamlDocument::hydrate_from_str(s)? })
    }
}

pub struct YamlDocument {
    pub input: Vec<u8>,
    pub paths: BTreeMap<Path, usize>,
    pub changed: bool,
}

impl Document for YamlDocument {
    fn update_path(&mut self, path: &Path, value: Value) -> Result<(), Error> {
        if self.paths.contains_key(path) {
            self.set_path(&path, value)
        } else {
            Ok(())
        }
    }

    fn set_path(&mut self, path: &Path, value: Value) -> Result<(), Error> {
        let key_span
            = self.paths.get(path);

        if value == Value::Undefined {
            if let Some(key_span) = key_span {
                return self.remove_key_at(path, key_span.clone());
            } else {
                return Ok(());
            }
        }

        if let Some(key_span) = key_span {
            self.update_key_at(&path, key_span.clone(), value)
        } else {
            self.insert_key(&path, value)
        }
    }
}

impl YamlDocument {
    pub fn hydrate_from_str<'de, T: Deserialize<'de>>(input: &'de str) -> Result<T, Error> {
        Ok(yaml_provider::from_str(input).map_err(|e| Error::InvalidSyntax(e.to_string()))?)
    }

    pub fn hydrate_from_slice<'de, T: Deserialize<'de>>(input: &'de [u8]) -> Result<T, Error> {
        Ok(yaml_provider::from_slice(input).map_err(|e| Error::InvalidSyntax(e.to_string()))?)
    }

    pub fn to_string<T: Serialize + ?Sized>(input: &T) -> Result<String, Error> {
        Ok(yaml_provider::to_string(input).map_err(|e| Error::InvalidSyntax(e.to_string()))?)
    }

    pub fn new(input: Vec<u8>) -> Result<Self, Error> {
        let mut scanner
            = Scanner::new(&input, 0);

        scanner.path = Some(vec![]);
        scanner.scan_document()?;

        let paths
            = scanner.fields.into_iter().collect();

        Ok(Self {
            input,
            paths,
            changed: false,
        })
    }

    pub fn rescan(&mut self) -> Result<(), Error> {
        let mut scanner
            = Scanner::new(&self.input, 0);

        scanner.path = Some(vec![]);
        scanner.scan_document()?;

        self.paths
            = scanner.fields.into_iter().collect();

        Ok(())
    }

    fn replace_range(&mut self, range: Range<usize>, data: &[u8]) -> Result<(), Error> {
        let (before, after)
            = self.input.split_at(range.start);

        let (_, after)
            = after.split_at(range.end - range.start);

        self.changed = true;

        self.input = [before, data, after].concat();
        self.rescan()?;

        Ok(())
    }

    fn remove_key_at(&mut self, path: &Path, key_offset: usize) -> Result<(), Error> {
        let line_start
            = self.input[0..key_offset]
                .iter()
                .rposition(|&c| c == b'\n')
                .map(|pos| pos + 1)
                .unwrap_or(0);

        let mut scanner
            = Scanner::new(&self.input, key_offset);

        scanner.skip_key()?;
        scanner.skip_char(b':')?;
        scanner.skip_inline_whitespace();

        if scanner.peek() == Some(b'\n') || scanner.peek().is_none() {
            let _ = scanner.skip_char(b'\n');

            let block_indent
                = self.find_indent_at(key_offset)
                    .map(|(i, _)| i)
                    .unwrap_or(0);

            scanner.skip_block_lines(block_indent + 1);
        } else {
            scanner.skip_line();
        }

        let end_offset
            = scanner.offset;

        // Check if this is the only key at this level
        let siblings: Vec<_>
            = self.paths.keys()
                .filter(|p| p.is_direct_child_of(&Path::from_segments(path[0..path.len() - 1].to_vec())))
                .collect();

        if siblings.len() == 1 && path.len() > 1 {
            // This is the only child, remove the parent too
            self.set_path(&Path::from_segments(path[0..path.len() - 1].to_vec()), Value::Undefined)
        } else {
            self.replace_range(line_start..end_offset, b"")
        }
    }

    fn update_key_at(&mut self, path: &Path, key_offset: usize, value: Value) -> Result<(), Error> {
        let mut scanner
            = Scanner::new(&self.input, key_offset);

        let indent
            = self.find_property_indent(path, key_offset)?;

        scanner.skip_key()?;
        scanner.skip_char(b':')?;

        // Remember offset right after colon (before any whitespace)
        let after_colon_offset
            = scanner.offset;

        scanner.skip_inline_whitespace();

        let pre_value_offset
            = scanner.offset;

        let is_block
            = scanner.peek() == Some(b'\n') || scanner.peek().is_none();

        if is_block {
            // Skip block value
            if scanner.peek() == Some(b'\n') {
                scanner.offset += 1;
            }

            let block_indent
                = indent.child_indent.unwrap_or(2);

            // Lines at block_indent are children, not siblings
            scanner.skip_block_lines(block_indent);
        } else {
            // Skip inline value
            scanner.skip_to_eol();
        }

        let post_value_offset
            = scanner.offset;

        let formatted
            = self.format_value(&value, indent);

        // If the new value starts with a newline (block value), start from after the colon
        // to remove any trailing spaces. Otherwise, add a space if needed.
        let (replace_start, replace_end, final_value) = if formatted.starts_with('\n') {
            // The block value we replace includes its trailing newline, but
            // the formatted value doesn't
            let formatted = if is_block && post_value_offset > 0 && self.input[post_value_offset - 1] == b'\n' {
                format!("{}\n", formatted)
            } else {
                formatted
            };

            (after_colon_offset, post_value_offset, formatted)
        } else {
            // Ensure there's a space before inline values
            let prefix
                = if pre_value_offset == after_colon_offset {" "} else {""};

            // When replacing a block value with an inline value, preserve the trailing newline
            let end = if is_block && post_value_offset > 0 && self.input[post_value_offset - 1] == b'\n' {
                post_value_offset - 1
            } else {
                post_value_offset
            };

            (pre_value_offset, end, format!("{}{}", prefix, formatted))
        };

        self.replace_range(replace_start..replace_end, final_value.as_bytes())
    }

    fn insert_key(&mut self, path: &Path, value: Value) -> Result<(), Error> {
        if path.is_empty() {
            return Ok(());
        }

        // The missing parents are inserted along with the value, as a single
        // nested object under the closest ancestor that already exists
        let mut existing_len
            = path.len() - 1;

        while existing_len > 0 && !self.paths.contains_key(&Path::from_segments(path[0..existing_len].to_vec())) {
            existing_len -= 1;
        }

        let value = path[existing_len + 1..].iter().rev()
            .fold(value, |value, segment| Value::Object(vec![(segment.clone(), value)]));

        if existing_len > 0 {
            self.insert_nested_key(&Path::from_segments(path[0..existing_len].to_vec()), &path[existing_len], value)
        } else {
            self.insert_top_level_key(&path[0], value)
        }
    }

    fn insert_nested_key(&mut self, parent_path: &Path, new_key: &str, value: Value) -> Result<(), Error> {
        let &parent_key_offset
            = self.paths.get(parent_path)
                .expect("A parent key must exist");

        let mut scanner
            = Scanner::new(&self.input, parent_key_offset);

        scanner.skip_key()?;
        scanner.skip_char(b':')?;
        scanner.skip_inline_whitespace();

        // Get parent's indent level and calculate the indent delta
        let parent_indent
            = self.find_indent_at(parent_key_offset).map(|(i, _)| i).unwrap_or(0);

        // The new key must be aligned with its siblings, if any
        let sibling_indent
            = self.paths.iter()
                .filter(|(path, _)| path.is_direct_child_of(parent_path))
                .filter_map(|(_, &offset)| self.find_indent_at(offset))
                .map(|(indent, _)| indent)
                .min();

        // Calculate indent delta by comparing parent's indent with grandparent's indent
        let indent_delta = match sibling_indent {
            Some(sibling_indent) if sibling_indent > parent_indent => {
                sibling_indent - parent_indent
            },

            _ => {
                parent_path.parent()
                    .and_then(|grandparent| self.paths.get(&grandparent))
                    .and_then(|&grandparent_offset| self.find_indent_at(grandparent_offset))
                    .map(|(grandparent_indent, _)| parent_indent.saturating_sub(grandparent_indent))
                    .filter(|delta| *delta > 0)
                    .unwrap_or(2)
            },
        };

        let child_indent
            = parent_indent + indent_delta;

        let property_indent
            = Indent::with_style(Some(child_indent), Some(child_indent + indent_delta), IndentStyle::Spaces);

        self.insert_at(scanner.offset, parent_path, new_key, property_indent, value)
    }

    fn insert_top_level_key(&mut self, new_key: &str, value: Value) -> Result<(), Error> {
        let (self_indent, style)
            = self.paths.iter()
                .filter(|(path, _)| path.is_direct_child_of(&Path::new()))
                .filter_map(|(_, offset)| self.find_indent_at(*offset))
                .min_by_key(|(indent, _)| *indent)
                .unwrap_or((0, IndentStyle::Spaces));

        let top_level_indent
            = Indent::with_style(Some(self_indent), Some(self_indent + 2), style);

        self.insert_at(0, &Path::new(), new_key, top_level_indent, value)
    }

    fn insert_at(&mut self, _offset: usize, parent_path: &Path, new_key: &str, indent: Indent, value: Value) -> Result<(), Error> {
        let (before, after): (Vec<_>, Vec<_>)
            = self.paths.keys()
                .filter(|p| p.is_direct_child_of(parent_path))
                .partition(|p| p.last() < Some(new_key));

        if let Some(insert_path) = after.first() {
            return self.insert_before_property(self.paths[insert_path], new_key, indent, value);
        }

        if let Some(insert_path) = before.last() {
            return self.insert_after_property(self.paths[insert_path], new_key, indent, value);
        }

        // Insert into empty parent
        self.insert_into_empty(parent_path, new_key, indent, value)
    }

    fn insert_before_property(&mut self, next_property_offset: usize, new_key: &str, indent: Indent, value: Value) -> Result<(), Error> {
        // Find line start for the next property
        let line_start
            = self.input[0..next_property_offset]
                .iter()
                .rposition(|&c| c == b'\n')
                .map(|pos| pos + 1)
                .unwrap_or(0);

        let self_indent
            = indent.self_indent.unwrap_or(0);

        let mut injected_content
            = vec![];

        // Add indentation
        for _ in 0..self_indent {
            injected_content.push(b' ');
        }

        push_yaml_key(&mut injected_content, new_key);
        injected_content.push(b':');

        let formatted_value
            = self.format_value(&value, indent.clone());

        if !formatted_value.starts_with('\n') {
            injected_content.push(b' ');
        }

        injected_content.extend_from_slice(formatted_value.as_bytes());
        injected_content.push(b'\n');

        self.replace_range(line_start..line_start, &injected_content)
    }

    fn insert_after_property(&mut self, previous_property_offset: usize, new_key: &str, indent: Indent, value: Value) -> Result<(), Error> {
        let mut scanner
            = Scanner::new(&self.input, previous_property_offset);

        let self_indent
            = indent.self_indent.unwrap_or(0);

        // Skip the previous property's key and value
        scanner.skip_key()?;
        scanner.skip_char(b':')?;
        scanner.skip_inline_whitespace();

        // Skip value
        if scanner.peek() == Some(b'\n') {
            // Block value
            scanner.offset += 1;

            let block_indent
                = indent.child_indent.unwrap_or(self_indent + 2);

            scanner.skip_block_lines(block_indent);
        } else {
            scanner.skip_line();
        }

        let mut injected_content
            = vec![];

        // Add leading newline if the previous line doesn't end with one
        // (happens when document ends without trailing newline)
        if scanner.offset > 0 && self.input[scanner.offset - 1] != b'\n' {
            injected_content.push(b'\n');
        }

        // Add indentation
        for _ in 0..self_indent {
            injected_content.push(b' ');
        }

        push_yaml_key(&mut injected_content, new_key);
        injected_content.push(b':');

        let formatted_value
            = self.format_value(&value, indent.clone());

        if !formatted_value.starts_with('\n') {
            injected_content.push(b' ');
        }

        injected_content.extend_from_slice(formatted_value.as_bytes());
        injected_content.push(b'\n');

        self.replace_range(scanner.offset..scanner.offset, &injected_content)
    }

    fn insert_into_empty(&mut self, parent_path: &Path, new_key: &str, indent: Indent, value: Value) -> Result<(), Error> {
        if parent_path.is_empty() {
            // Inserting at top level into empty document
            let mut new_content
                = vec![];

            push_yaml_key(&mut new_content, new_key);
            new_content.push(b':');

            let formatted_value
                = self.format_value(&value, indent);

            if !formatted_value.starts_with('\n') {
                new_content.push(b' ');
            }

            new_content.extend_from_slice(formatted_value.as_bytes());
            new_content.push(b'\n');

            return self.replace_range(0..self.input.len(), &new_content);
        }

        // Find parent and insert after the colon
        let &parent_offset
            = self.paths.get(parent_path)
                .expect("Parent path must exist");

        let mut scanner
            = Scanner::new(&self.input, parent_offset);

        scanner.skip_key()?;
        scanner.skip_char(b':')?;

        let self_indent
            = indent.self_indent.unwrap_or(0);

        let mut new_content
            = vec![];

        new_content.push(b'\n');

        for _ in 0..self_indent {
            new_content.push(b' ');
        }

        push_yaml_key(&mut new_content, new_key);
        new_content.push(b':');

        let child_indent_obj
            = Indent::with_style(Some(self_indent), Some(self_indent + 2), indent.style);

        let formatted_value
            = self.format_value(&value, child_indent_obj);

        if !formatted_value.starts_with('\n') {
            new_content.push(b' ');
        }

        new_content.extend_from_slice(formatted_value.as_bytes());

        let insert_offset
            = scanner.offset;

        scanner.skip_inline_whitespace();

        let end_offset = if scanner.peek() == Some(b'\n') || scanner.peek().is_none() {
            insert_offset
        } else {
            // There's an inline value, skip it
            scanner.skip_to_eol();
            scanner.offset
        };

        self.replace_range(insert_offset..end_offset, &new_content)
    }

    fn find_indent_at(&self, offset: usize) -> Option<(usize, IndentStyle)> {
        let mut check_offset
            = offset;
        let mut indent
            = 0;

        while check_offset > 0 && self.input[check_offset - 1] == b' ' {
            indent += 1;
            check_offset -= 1;
        }

        if check_offset == 0 || self.input[check_offset - 1] == b'\n' {
            Some((indent, IndentStyle::Spaces))
        } else {
            None
        }
    }

    fn find_property_indent(&self, path: &Path, offset: usize) -> Result<Indent, Error> {
        let self_indent_info
            = self.find_indent_at(offset);

        let (self_indent, style) = match self_indent_info {
            Some((indent, style)) => (Some(indent), style),
            None => (None, IndentStyle::Spaces),
        };

        // Calculate child indent based on parent relationship
        let suggested_child_indent = match self_indent {
            Some(self_indent_val) => {
                let delta
                    = path.parent()
                        .and_then(|p| self.paths.get(&p))
                        .and_then(|&offset| self.find_indent_at(offset))
                        .map(|(parent_indent, _)| self_indent_val.saturating_sub(parent_indent))
                        .unwrap_or(2);

                Some(self_indent_val + delta)
            },

            None => {
                Some(2)
            },
        };

        let child_indent
            = suggested_child_indent;

        Ok(Indent::with_style(self_indent, child_indent, style))
    }

    fn format_value(&self, value: &Value, indent: Indent) -> String {
        match value {
            Value::Null => "null".to_string(),
            Value::Bool(b) => b.to_string(),
            Value::Number(n) => n.to_string(),
            Value::String(s) => format_yaml_string(s),

            Value::Array(arr) => {
                if arr.is_empty() {
                    return "[]".to_string();
                }

                let child_indent
                    = indent.child_indent.unwrap_or(indent.self_indent.unwrap_or(0) + 2);
                let mut result
                    = String::new();

                for item in arr {
                    result.push('\n');

                    for _ in 0..child_indent {
                        result.push(' ');
                    }

                    result.push_str("- ");

                    let item_str = match item {
                        // Objects start on the same line as their dash, with
                        // their other keys aligned on the first one
                        Value::Object(obj) if !obj.is_empty() => {
                            let item_indent
                                = child_indent + 2;

                            let formatted
                                = self.format_value(item, Indent::with_style(
                                    Some(item_indent),
                                    Some(item_indent),
                                    indent.style,
                                ));

                            formatted[1 + item_indent..].to_string()
                        },

                        _ => {
                            self.format_value(item, Indent::with_style(
                                Some(child_indent + 2),
                                Some(child_indent + 4),
                                indent.style,
                            ))
                        },
                    };

                    result.push_str(&item_str);
                }

                result
            }
            Value::Object(obj) => {
                if obj.is_empty() {
                    return "{}".to_string();
                }

                let child_indent
                    = indent.child_indent.unwrap_or(indent.self_indent.unwrap_or(0) + 2);
                let mut result
                    = String::new();

                for (key, val) in obj {
                    result.push('\n');

                    for _ in 0..child_indent {
                        result.push(' ');
                    }

                    result.push_str(&format_yaml_key(key));
                    result.push(':');

                    let val_str
                        = self.format_value(val, Indent::with_style(
                            Some(child_indent),
                            Some(child_indent + 2),
                            indent.style,
                        ));

                    if !val_str.starts_with('\n') {
                        result.push(' ');
                    }

                    result.push_str(&val_str);
                }

                result
            }
            Value::Undefined => panic!("Undefined value cannot be converted to YAML"),
            Value::Raw(s) => s.clone(),
        }
    }

    pub fn sort_object_keys(&mut self, parent_path: &Path) -> Result<bool, Error> {
        let mut keys_by_position
            = self.paths.iter()
                .filter(|(path, _)| path.is_direct_child_of(parent_path))
                .map(|(path, &offset)| (path.last().unwrap(), offset))
                .collect_vec();

        if keys_by_position.len() <= 1 {
            return Ok(false);
        }

        keys_by_position.sort_by_key(|(_, offset)| *offset);

        // Check if already sorted alphabetically
        if keys_by_position.windows(2).all(|w| w[0].0 <= w[1].0) {
            return Ok(false);
        }

        // For YAML, we need to extract and sort entire key-value blocks
        let mut key_value_pairs: Vec<(&str, Vec<u8>)>
            = vec![];

        for (key_name, offset) in &keys_by_position {
            let line_start
                = self.input[0..*offset]
                    .iter()
                    .rposition(|&c| c == b'\n')
                    .map(|pos| pos + 1)
                    .unwrap_or(0);

            let mut scanner
                = Scanner::new(&self.input, *offset);

            scanner.skip_key()?;
            scanner.skip_char(b':')?;
            scanner.skip_inline_whitespace();

            // Find the end of this value
            if scanner.peek() == Some(b'\n') || scanner.peek().is_none() {
                // Block value
                if scanner.peek() == Some(b'\n') {
                    scanner.offset += 1;
                }

                let key_indent
                    = self.find_indent_at(*offset).map(|(i, _)| i).unwrap_or(0);

                while scanner.offset < self.input.len() {
                    let line_indent
                        = scanner.get_line_indent();

                    if line_indent <= key_indent && !scanner.is_empty_or_comment_line() {
                        break;
                    }

                    scanner.skip_line();
                }
            } else {
                scanner.skip_line();
            }

            key_value_pairs.push((key_name, self.input[line_start..scanner.offset].to_vec()));
        }

        // Sort by key name
        key_value_pairs.sort_by_key(|(key_name, _)| *key_name);

        // Calculate the range to replace
        let first_line_start
            = self.input[0..keys_by_position[0].1]
                .iter()
                .rposition(|&c| c == b'\n')
                .map(|pos| pos + 1)
                .unwrap_or(0);

        let last_offset
            = keys_by_position.last().unwrap().1;

        let mut scanner
            = Scanner::new(&self.input, last_offset);

        scanner.skip_key()?;
        scanner.skip_char(b':')?;
        scanner.skip_inline_whitespace();

        if scanner.peek() == Some(b'\n') || scanner.peek().is_none() {
            if scanner.peek() == Some(b'\n') {
                scanner.offset += 1;
            }

            let key_indent
                = self.find_indent_at(last_offset).map(|(i, _)| i).unwrap_or(0);

            while scanner.offset < self.input.len() {
                let line_indent
                    = scanner.get_line_indent();

                if line_indent <= key_indent && !scanner.is_empty_or_comment_line() {
                    break;
                }

                scanner.skip_line();
            }
        } else {
            scanner.skip_line();
        }

        let content_end
            = scanner.offset;

        // Rebuild content
        let mut sorted_content: Vec<u8>
            = vec![];
        for (_, entry_bytes) in key_value_pairs {
            sorted_content.extend_from_slice(&entry_bytes);
        }

        self.replace_range(first_line_start..content_end, &sorted_content)?;

        Ok(true)
    }
}

/// YAML 1.1 (used by many other tools reading the same files) also reads
/// those as booleans; we quote them so that they stay strings everywhere.
const YAML_1_1_BOOLEANS: &[&str] = &[
    "y", "yes", "n", "no", "on", "off",
];

/// Characters that can't start a plain scalar, as they'd be read as an
/// indicator (anchor, alias, tag, directive, block scalar, flow collection,
/// comment, quote...).
const INDICATOR_CHARS: &[char] = &[
    '-', '?', ':', ',', '[', ']', '{', '}', '#', '&', '*', '!', '|', '>', '\'', '"', '%', '@', '`',
];

/// Checks that `document` (a single `key: value` line) gets read back by
/// the YAML parser as a mapping from `key` to `value`.
fn plain_scalars_roundtrip(document: &str, key: &str, value: &str) -> bool {
    let Ok(yaml_provider::Value::Mapping(mapping)) = yaml_provider::from_str::<yaml_provider::Value>(document) else {
        return false;
    };

    let Some((parsed_key, parsed_value)) = mapping.iter().next() else {
        return false;
    };

    mapping.len() == 1
        && parsed_key.as_str() == Some(key)
        && parsed_value.as_str() == Some(value)
}

/// Whether a scalar must be quoted to be read back as the very same string,
/// be it as a key or as a value.
fn scalar_needs_quotes(s: &str) -> bool {
    if s.is_empty() || s.trim() != s {
        return true;
    }

    if s.starts_with(INDICATOR_CHARS) {
        return true;
    }

    // Line breaks, comments, and mapping separators; we also check for the
    // characters that our own scanner relies on, even if they'd be fine
    // for a regular YAML parser
    if s.contains(['\n', '\r', '#', ':', '\t']) {
        return true;
    }

    let lower
        = s.to_lowercase();

    if YAML_1_1_BOOLEANS.contains(&lower.as_str()) {
        return true;
    }

    // Anything else the parser would read as something else than a string
    // (null, booleans, numbers in their various notations, ...)
    !plain_scalars_roundtrip(&format!("{s}: {s}\n"), s, s)
}

fn quote_yaml_string(s: &str) -> String {
    let mut quoted
        = String::with_capacity(s.len() + 2);

    quoted.push('"');

    for c in s.chars() {
        match c {
            '\\' => quoted.push_str("\\\\"),
            '"' => quoted.push_str("\\\""),
            '\n' => quoted.push_str("\\n"),
            '\r' => quoted.push_str("\\r"),
            '\t' => quoted.push_str("\\t"),
            c if c.is_control() => quoted.push_str(&format!("\\u{:04X}", c as u32)),
            c => quoted.push(c),
        }
    }

    quoted.push('"');
    quoted
}

fn format_yaml_string(s: &str) -> String {
    // Spaces and quotes are fine inside plain scalars, but we've always quoted
    // them; we keep doing so for consistency with the files we already wrote
    if s.contains([' ', '\'', '"']) || scalar_needs_quotes(s) {
        quote_yaml_string(s)
    } else {
        s.to_string()
    }
}

fn format_yaml_key(key: &str) -> String {
    if scalar_needs_quotes(key) {
        quote_yaml_string(key)
    } else {
        key.to_string()
    }
}

fn push_yaml_key(content: &mut Vec<u8>, key: &str) {
    content.extend_from_slice(format_yaml_key(key).as_bytes());
}

#[derive(Clone)]
struct Scanner<'a> {
    input: &'a [u8],
    offset: usize,

    pub path: Option<Vec<String>>,
    pub fields: Vec<(Path, usize)>,
}

impl<'a> Scanner<'a> {
    fn new(input: &'a [u8], offset: usize) -> Self {
        Self {
            input,
            offset,
            path: None,
            fields: vec![],
        }
    }

    fn peek(&self) -> Option<u8> {
        self.input.get(self.offset).copied()
    }

    fn skip_inline_whitespace(&mut self) {
        while self.offset < self.input.len() && (self.input[self.offset] == b' ' || self.input[self.offset] == b'\t') {
            self.offset += 1;
        }
    }

    fn skip_to_eol(&mut self) {
        while self.offset < self.input.len() && self.input[self.offset] != b'\n' {
            self.offset += 1;
        }
    }

    fn skip_line(&mut self) {
        self.skip_to_eol();

        if self.peek() == Some(b'\n') {
            self.offset += 1;
        }
    }

    /// Skips the lines of a block value, which are the ones indented by at
    /// least `min_indent`. The empty lines and the less indented comments
    /// that follow the block aren't part of it (they usually describe the
    /// next key), so we leave them alone.
    fn skip_block_lines(&mut self, min_indent: usize) {
        let mut content_end
            = self.offset;

        while self.offset < self.input.len() {
            let line_indent
                = self.get_line_indent();

            let is_empty_or_comment
                = self.is_empty_or_comment_line();

            if line_indent < min_indent && !is_empty_or_comment {
                break;
            }

            let is_content
                = !is_empty_or_comment || (line_indent >= min_indent && self.input.get(self.offset + line_indent) == Some(&b'#'));

            self.skip_line();

            if is_content {
                content_end = self.offset;
            }
        }

        self.offset = content_end;
    }

    fn get_line_indent(&self) -> usize {
        let mut offset
            = self.offset;

        while offset < self.input.len() && self.input[offset] == b' ' {
            offset += 1;
        }

        offset - self.offset
    }

    fn is_empty_or_comment_line(&self) -> bool {
        let mut offset
            = self.offset;

        while offset < self.input.len() && self.input[offset] == b' ' {
            offset += 1;
        }

        offset >= self.input.len() || self.input[offset] == b'\n' || self.input[offset] == b'#'
    }

    fn skip_char(&mut self, c: u8) -> Result<(), Error> {
        if self.offset < self.input.len() && self.input[self.offset] == c {
            self.offset += 1;
            Ok(())
        } else {
            Err(Error::InvalidSyntax(format!(
                "Expected '{}' at offset {}, got {:?}",
                c as char,
                self.offset,
                self.peek().map(|b| b as char)
            )))
        }
    }

    fn skip_key(&mut self) -> Result<(), Error> {
        self.skip_inline_whitespace();

        // Check for quoted key
        if self.peek() == Some(b'"') || self.peek() == Some(b'\'') {
            let quote
                = self.input[self.offset];

            self.offset += 1;

            let mut escaped
                = false;

            while self.offset < self.input.len() {
                if escaped {
                    escaped = false;
                    self.offset += 1;
                } else if self.input[self.offset] == b'\\' {
                    escaped = true;
                    self.offset += 1;
                } else if self.input[self.offset] == quote {
                    self.offset += 1;
                    return Ok(());
                } else {
                    self.offset += 1;
                }
            }

            return Err(Error::InvalidSyntax("Unterminated quoted key".to_string()));
        }

        // Unquoted key
        while self.offset < self.input.len() && self.input[self.offset] != b':' && self.input[self.offset] != b'\n' {
            self.offset += 1;
        }

        Ok(())
    }

    fn parse_key(&mut self) -> Result<Option<String>, Error> {
        let start
            = self.offset;

        // Check for quoted key
        if self.peek() == Some(b'"') || self.peek() == Some(b'\'') {
            let quote
                = self.input[self.offset];

            self.offset += 1;

            let mut key
                = Vec::new();
            let mut escaped
                = false;

            while self.offset < self.input.len() {
                if escaped {
                    match self.input[self.offset] {
                        b'n' => key.push(b'\n'),
                        b't' => key.push(b'\t'),
                        b'r' => key.push(b'\r'),

                        b'u' if quote == b'"' => {
                            let code_point = self.input.get(self.offset + 1..self.offset + 5)
                                .and_then(|hex| std::str::from_utf8(hex).ok())
                                .and_then(|hex| u32::from_str_radix(hex, 16).ok())
                                .and_then(char::from_u32);

                            let Some(c) = code_point else {
                                return Ok(None);
                            };

                            let mut buffer
                                = [0; 4];

                            key.extend_from_slice(c.encode_utf8(&mut buffer).as_bytes());
                            self.offset += 4;
                        },

                        c => key.push(c),
                    }

                    escaped = false;
                    self.offset += 1;
                } else if self.input[self.offset] == b'\\' {
                    escaped = true;
                    self.offset += 1;
                } else if self.input[self.offset] == quote {
                    self.offset += 1;
                    return Ok(Some(String::from_utf8(key)?));
                } else if self.input[self.offset] == b'\n' {
                    return Ok(None);
                } else {
                    key.push(self.input[self.offset]);
                    self.offset += 1;
                }
            }
            return Ok(None);
        }

        // Unquoted key
        while self.offset < self.input.len() && self.input[self.offset] != b':' && self.input[self.offset] != b'\n' {
            self.offset += 1;
        }

        if self.offset == start || self.peek() != Some(b':') {
            return Ok(None);
        }

        let key
            = std::str::from_utf8(&self.input[start..self.offset])?.trim().to_string();

        Ok(Some(key))
    }

    fn scan_document(&mut self) -> Result<(), Error> {
        self.scan_block(0)?;
        Ok(())
    }

    fn scan_block(&mut self, expected_indent: usize) -> Result<(), Error> {
        while self.offset < self.input.len() {
            self.skip_empty_lines();

            if self.offset >= self.input.len() {
                break;
            }

            let line_indent
                = self.get_line_indent();

            // If indent decreased, we're done with this block
            if line_indent < expected_indent {
                break;
            }

            // Skip indentation
            self.offset += line_indent;

            // Check for list item
            if self.peek() == Some(b'-') && self.offset + 1 < self.input.len() && self.input[self.offset + 1] == b' ' {
                // Skip list items for now - we don't track them in paths
                self.skip_line();
                continue;
            }

            let key_start
                = self.offset;
            let key
                = self.parse_key()?;

            if let Some(key) = key {
                if let Some(ref mut path) = self.path {
                    path.push(key.clone());
                    self.fields.push((Path::from_segments(path.clone()), key_start));
                }

                if self.peek() == Some(b':') {
                    self.offset += 1;
                }

                self.skip_inline_whitespace();

                if self.peek() != Some(b'\n') && self.peek().is_some() {
                    self.skip_to_eol();
                }

                if self.peek() == Some(b'\n') {
                    self.offset += 1;
                }

                self.skip_empty_lines();

                if self.offset < self.input.len() {
                    let next_indent
                        = self.get_line_indent();

                    if next_indent > line_indent {
                        self.scan_block(next_indent)?;
                    }
                }

                if let Some(ref mut path) = self.path {
                    path.pop();
                }
            } else {
                self.skip_line();
            }
        }

        Ok(())
    }

    fn skip_empty_lines(&mut self) {
        while self.offset < self.input.len() {
            let start
                = self.offset;

            self.skip_inline_whitespace();

            if self.peek() == Some(b'#') {
                self.skip_line();
            } else if self.peek() == Some(b'\n') {
                self.offset += 1;
            } else {
                self.offset = start;
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use rstest::rstest;

    use super::*;

    #[rstest]
    // Basic value updates
    #[case(b"test: value\n", vec!["test"], Value::String("foo".to_string()), b"test: foo\n")]
    #[case(b"test: 42\n", vec!["test"], Value::Number("100".to_string()), b"test: 100\n")]
    #[case(b"test: true\n", vec!["test"], Value::Bool(false), b"test: false\n")]
    #[case(b"test: null\n", vec!["test"], Value::String("not null".to_string()), b"test: \"not null\"\n")]

    // Nested value updates
    #[case(b"parent:\n  child: old\n", vec!["parent", "child"], Value::String("new".to_string()), b"parent:\n  child: new\n")]
    #[case(b"a:\n  b:\n    c: value\n", vec!["a", "b", "c"], Value::String("updated".to_string()), b"a:\n  b:\n    c: updated\n")]

    // Insert new top-level keys
    #[case(b"existing: value\n", vec!["new_key"], Value::String("another".to_string()), b"existing: value\nnew_key: another\n")]
    #[case(b"        existing: value\n", vec!["new_key"], Value::String("another".to_string()), b"        existing: value\n        new_key: another\n")]

    // Insert nested keys
    #[case(b"parent:\n  existing: value\n", vec!["parent", "new_child"], Value::String("new".to_string()), b"parent:\n  existing: value\n  new_child: new\n")]

    // Insert into empty parent
    #[case(b"parent:\n", vec!["parent", "child"], Value::String("value".to_string()), b"parent:\n  child: value\n")]
    #[case(b"level1:\n  level2:\n", vec!["level1", "level2", "level3"], Value::String("deep".to_string()), b"level1:\n  level2:\n    level3: deep\n")]
    // Insert into empty parent with 4-space indentation
    #[case(b"parent:\n    child:\n", vec!["parent", "child", "grandchild"], Value::String("value".to_string()), b"parent:\n    child:\n        grandchild: value\n")]

    // Delete operations
    #[case(b"keep: this\ndelete: me\n", vec!["delete"], Value::Undefined, b"keep: this\n")]
    #[case(b"first: v1\nsecond: v2\nthird: v3\n", vec!["second"], Value::Undefined, b"first: v1\nthird: v3\n")]

    // Delete key at EOF without trailing newline
    #[case(b"keep: this\ndelete: me", vec!["delete"], Value::Undefined, b"keep: this\n")]
    #[case(b"only:", vec!["only"], Value::Undefined, b"")]

    // Array values
    #[case(b"arr: []\n", vec!["arr"], Value::Array(vec![Value::Number("1".to_string()), Value::Number("2".to_string())]), b"arr:\n  - 1\n  - 2\n")]

    // Object values
    #[case(b"obj: {}\n", vec!["obj"], Value::Object(vec![("inner".to_string(), Value::String("value".to_string()))]), b"obj:\n  inner: value\n")]

    // String quoting
    #[case(b"test: simple\n", vec!["test"], Value::String("has space".to_string()), b"test: \"has space\"\n")]
    #[case(b"test: old\n", vec!["test"], Value::String("true".to_string()), b"test: \"true\"\n")]

    // Insert after property without trailing newline
    #[case(b"existing: value", vec!["new_key"], Value::String("new".to_string()), b"existing: value\nnew_key: new\n")]

    // Update parent key that has nested children
    #[case(b"parent:\n  child: old\n", vec!["parent"], Value::String("new".to_string()), b"parent: new\n")]
    #[case(b"a:\n  b:\n    c: value\n", vec!["a"], Value::String("replaced".to_string()), b"a: replaced\n")]
    #[case(b"a:\n  b:\n    c: value\n", vec!["a", "b"], Value::String("replaced".to_string()), b"a:\n  b: replaced\n")]

    // Replace a block value followed by a sibling
    #[case(b"a:\n  - x\nb: 1\n", vec!["a"], Value::Array(vec![Value::String("z".to_string())]), b"a:\n  - z\nb: 1\n")]
    #[case(b"a:\n  x: 1\nb: 1\n", vec!["a"], Value::Object(vec![("z".to_string(), Value::Number("2".to_string()))]), b"a:\n  z: 2\nb: 1\n")]

    // Comments and empty lines after a block value belong to the next key
    #[case(b"a:\n  x: 1\n\n# About b\nb: 1\n", vec!["a"], Value::Object(vec![("z".to_string(), Value::Number("2".to_string()))]), b"a:\n  z: 2\n\n# About b\nb: 1\n")]
    #[case(b"a:\n  x: 1\n  # About x\n\n# About b\nb: 1\n", vec!["a"], Value::String("new".to_string()), b"a: new\n\n# About b\nb: 1\n")]
    #[case(b"a:\n  x: 1\n\n# About b\nb: 1\n", vec!["a"], Value::Undefined, b"\n# About b\nb: 1\n")]
    #[case(b"a:\n  x: 1\n\n# About c\nc: 1\n", vec!["d"], Value::Number("2".to_string()), b"a:\n  x: 1\n\n# About c\nc: 1\nd: 2\n")]
    #[case(b"a:\n  x: 1\n\n# About c\nc: 1\n", vec!["b"], Value::Number("2".to_string()), b"a:\n  x: 1\n\n# About c\nb: 2\nc: 1\n")]
    #[case(b"c:\n  x: 1\n\n# About d\nd: 1\n", vec!["c", "z"], Value::Number("2".to_string()), b"c:\n  x: 1\n  z: 2\n\n# About d\nd: 1\n")]

    // Missing parents are created along with the value
    #[case(b"other: 1\n", vec!["a", "b", "c"], Value::Bool(true), b"a:\n  b:\n    c: true\nother: 1\n")]
    #[case(b"a:\n    existing: 1\n", vec!["a", "b", "c"], Value::Bool(true), b"a:\n    b:\n        c: true\n    existing: 1\n")]

    fn test_update_document(#[case] document: &[u8], #[case] path: Vec<&str>, #[case] value: Value, #[case] expected: &[u8]) {
        let mut document
            = YamlDocument::new(document.to_vec()).unwrap();

        document.set_path(&Path::from_segments(path.into_iter().map(|s| s.to_string()).collect()), value).unwrap();
        assert_eq!(String::from_utf8(document.input).unwrap(), String::from_utf8(expected.to_vec()).unwrap());
    }

    #[test]
    fn test_hydrate_from_str() {
        let yaml
            = "name: test\nversion: \"1.0\"\n";
        let result: serde_yaml::Value
            = YamlDocument::hydrate_from_str(yaml).unwrap();

        assert_eq!(result["name"], serde_yaml::Value::String("test".to_string()));
        assert_eq!(result["version"], serde_yaml::Value::String("1.0".to_string()));
    }

    #[test]
    fn test_to_string() {
        use std::collections::HashMap;

        let mut map
            = HashMap::new();
        map.insert("key", "value");

        let yaml
            = YamlDocument::to_string(&map).unwrap();
        assert!(yaml.contains("key:"));
        assert!(yaml.contains("value"));
    }

    #[rstest]
    // Sort unsorted keys at top level
    #[case(b"zebra: z\napple: a\nmango: m\n", vec![], b"apple: a\nmango: m\nzebra: z\n", true)]

    // Already sorted - no change
    #[case(b"apple: a\nmango: m\nzebra: z\n", vec![], b"apple: a\nmango: m\nzebra: z\n", false)]

    // Single key - no change
    #[case(b"only: key\n", vec![], b"only: key\n", false)]

    // Sort nested object keys
    #[case(b"deps:\n  zebra: '1.0'\n  apple: '2.0'\n", vec!["deps"], b"deps:\n  apple: '2.0'\n  zebra: '1.0'\n", true)]

    fn test_sort_object_keys(#[case] document: &[u8], #[case] path: Vec<&str>, #[case] expected: &[u8], #[case] expected_sorted: bool) {
        let mut document
            = YamlDocument::new(document.to_vec()).unwrap();

        let sorted
            = document.sort_object_keys(&Path::from_segments(path.into_iter().map(|s| s.to_string()).collect())).unwrap();

        assert_eq!(sorted, expected_sorted, "sort_object_keys return value mismatch");
        assert_eq!(String::from_utf8(document.input).unwrap(), String::from_utf8(expected.to_vec()).unwrap());
    }

    const AWKWARD_SCALARS: &[&str] = &[
        "plain", "@babel/parser@*", "@scope/name", "*foo", "&anchor", "!tag", "%directive", "`backtick", "|block", ">folded",
        "?question", "? question", "-dash", "- dash", ",comma", "[bracket", "]bracket", "{brace", "}brace", "#comment",
        "a #b", "a#b", "a: b", "a:b", "https://example.org", "//npm.example.com/", " leading", "trailing ", "  ",
        "1", "1.0", "-1", "+1", "0x10", "0o17", "1e3", "1_000", ".5", ".inf", "-.inf", ".nan", "inf", "nan",
        "true", "false", "True", "FALSE", "yes", "no", "No", "on", "OFF", "y", "n", "null", "Null", "~", "",
        "multi\nline", "trailing newline\n", "carriage\rreturn", "tab\tchar", "back\\slash", "quote\"inside", "\"quoted\"",
        "it's", "'single'", "foo@1.0 || 2.0", "lodash@^1.0.0", "a=b", "a,b", "日本語", "bell\u{7}",
    ];

    #[test]
    fn test_awkward_scalars_roundtrip_as_values() {
        for scalar in AWKWARD_SCALARS {
            let mut document
                = YamlDocument::new(b"before: 1\nafter: 2\n".to_vec()).unwrap();

            document.set_path(&Path::from_segments(vec!["between".to_string()]), Value::String(scalar.to_string())).unwrap();

            let text
                = String::from_utf8(document.input.clone()).unwrap();

            let parsed: BTreeMap<String, serde_yaml::Value>
                = serde_yaml::from_str(&text).unwrap_or_else(|e| panic!("{scalar:?} produced an invalid document ({e}):\n{text}"));

            assert_eq!(parsed.get("between").and_then(|v| v.as_str()), Some(*scalar), "{scalar:?} didn't round-trip:\n{text}");
            assert_eq!(parsed.len(), 3, "{scalar:?} corrupted the document:\n{text}");
        }
    }

    #[test]
    fn test_awkward_scalars_roundtrip_as_keys() {
        for scalar in AWKWARD_SCALARS {
            let mut document
                = YamlDocument::new(b"parent:\n  existing: 1\n".to_vec()).unwrap();

            let path
                = Path::from_segments(vec!["parent".to_string(), scalar.to_string(), "child".to_string()]);

            document.set_path(&path, Value::Bool(true)).unwrap();

            let text
                = String::from_utf8(document.input.clone()).unwrap();

            let parsed: BTreeMap<String, BTreeMap<String, serde_yaml::Value>>
                = serde_yaml::from_str(&text).unwrap_or_else(|e| panic!("{scalar:?} produced an invalid document ({e}):\n{text}"));

            let child
                = parsed["parent"].get(*scalar)
                    .unwrap_or_else(|| panic!("{scalar:?} didn't round-trip:\n{text}"));

            assert_eq!(child["child"], serde_yaml::Value::Bool(true), "{scalar:?} didn't round-trip:\n{text}");
            assert_eq!(parsed["parent"].len(), 2, "{scalar:?} corrupted the document:\n{text}");

            // Our own scanner must also find the key again, so that it can
            // be updated or removed later on
            assert!(document.paths.contains_key(&path), "{scalar:?} can't be found by the scanner:\n{text}");

            document.set_path(&path, Value::Undefined).unwrap();
            assert_eq!(String::from_utf8(document.input).unwrap(), "parent:\n  existing: 1\n", "{scalar:?} couldn't be removed");
        }
    }

    #[rstest]
    #[case("@babel/parser@*", "\"@babel/parser@*\"")]
    #[case("foo@*", "foo@*")]
    #[case("https://example.org", "\"https://example.org\"")]
    #[case("*foo", "\"*foo\"")]
    #[case("1.0", "\"1.0\"")]
    #[case("no", "\"no\"")]
    #[case("~", "\"~\"")]
    #[case("", "\"\"")]
    #[case("0x10", "\"0x10\"")]
    #[case(".inf", "\".inf\"")]
    #[case("a #b", "\"a #b\"")]
    #[case("multi\nline", "\"multi\\nline\"")]
    #[case("back\\slash", "back\\slash")]
    fn test_key_quoting(#[case] key: &str, #[case] expected: &str) {
        assert_eq!(format_yaml_key(key), expected);
    }

    #[rstest]
    #[case("3h", "3h")]
    #[case("**", "\"**\"")]
    #[case("0x10", "\"0x10\"")]
    #[case(".inf", "\".inf\"")]
    #[case("yes", "\"yes\"")]
    #[case("Off", "\"Off\"")]
    #[case("tab\tchar", "\"tab\\tchar\"")]
    #[case("bell\u{7}", "\"bell\\u0007\"")]
    fn test_value_quoting(#[case] value: &str, #[case] expected: &str) {
        assert_eq!(format_yaml_string(value), expected);
    }

    #[test]
    fn test_insert_object_in_array() {
        let mut document
            = YamlDocument::new(b"enableScripts: false\n".to_vec()).unwrap();

        document.set_path(&Path::from_segments(vec!["packageRules".to_string()]), Value::Array(vec![
            Value::Object(vec![
                ("packageFilter".to_string(), Value::String("@scoped/*".to_string())),
                ("npmMinimalAgeGate".to_string(), Value::String("3h".to_string())),
                ("dependencies".to_string(), Value::Object(vec![("x".to_string(), Value::String("*".to_string()))])),
            ]),
            Value::Object(vec![
                ("packageFilter".to_string(), Value::String("lodash".to_string())),
            ]),
        ])).unwrap();

        assert_eq!(String::from_utf8(document.input).unwrap(), concat!(
            "enableScripts: false\n",
            "packageRules:\n",
            "  - packageFilter: \"@scoped/*\"\n",
            "    npmMinimalAgeGate: 3h\n",
            "    dependencies:\n",
            "      x: \"*\"\n",
            "  - packageFilter: lodash\n",
        ));
    }

    #[test]
    fn test_untouched_parts_are_preserved() {
        let original = concat!(
            "# Top comment\n",
            "\n",
            "enableScripts:   false   # aligned comment\n",
            "npmScopes:\n",
            "    # indented comment\n",
            "    'single-quoted':\n",
            "        npmAlwaysAuth: \"true\"\n",
            "    zzz:\n",
            "        npmRegistryServer: 'https://example.org'\n",
            "\n",
            "# Trailing comment\n",
            "packageExtensions:\n",
            "    \"foo@*\":\n",
            "        dependencies: {bar: '*'}\n",
        );

        let mut document
            = YamlDocument::new(original.as_bytes().to_vec()).unwrap();

        document.set_path(&Path::from_segments(vec!["npmScopes".to_string(), "@new".to_string(), "npmAlwaysAuth".to_string()]), Value::Bool(true)).unwrap();
        document.set_path(&Path::from_segments(vec!["npmMinimalAgeGate".to_string()]), Value::String("3h".to_string())).unwrap();

        assert_eq!(String::from_utf8(document.input.clone()).unwrap(), concat!(
            "# Top comment\n",
            "\n",
            "enableScripts:   false   # aligned comment\n",
            "npmMinimalAgeGate: 3h\n",
            "npmScopes:\n",
            "    # indented comment\n",
            "    \"@new\":\n",
            "        npmAlwaysAuth: true\n",
            "    'single-quoted':\n",
            "        npmAlwaysAuth: \"true\"\n",
            "    zzz:\n",
            "        npmRegistryServer: 'https://example.org'\n",
            "\n",
            "# Trailing comment\n",
            "packageExtensions:\n",
            "    \"foo@*\":\n",
            "        dependencies: {bar: '*'}\n",
        ));

        document.set_path(&Path::from_segments(vec!["npmScopes".to_string(), "@new".to_string(), "npmAlwaysAuth".to_string()]), Value::Undefined).unwrap();
        document.set_path(&Path::from_segments(vec!["npmMinimalAgeGate".to_string()]), Value::Undefined).unwrap();

        assert_eq!(String::from_utf8(document.input).unwrap(), original);
    }
}
