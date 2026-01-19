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
        let key_span = self.paths.get(path);

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
        let mut scanner = Scanner::new(&input, 0);

        scanner.path = Some(vec![]);
        scanner.scan_document()?;

        let paths = scanner.fields.into_iter().collect();

        Ok(Self {
            input,
            paths,
            changed: false,
        })
    }

    pub fn rescan(&mut self) -> Result<(), Error> {
        let mut scanner = Scanner::new(&self.input, 0);

        scanner.path = Some(vec![]);
        scanner.scan_document()?;

        self.paths = scanner.fields.into_iter().collect();

        Ok(())
    }

    fn replace_range(&mut self, range: Range<usize>, data: &[u8]) -> Result<(), Error> {
        let (before, after) = self.input.split_at(range.start);
        let (_, after) = after.split_at(range.end - range.start);

        self.changed = true;

        self.input = [before, data, after].concat();
        self.rescan()?;

        Ok(())
    }

    fn remove_key_at(&mut self, path: &Path, key_offset: usize) -> Result<(), Error> {
        // Find the line containing this key
        let line_start = self.input[0..key_offset]
            .iter()
            .rposition(|&c| c == b'\n')
            .map(|pos| pos + 1)
            .unwrap_or(0);

        let mut scanner = Scanner::new(&self.input, key_offset);

        // Skip the key
        scanner.skip_key()?;
        scanner.skip_char(b':')?;
        scanner.skip_inline_whitespace();

        // Skip the value (could be inline or block)
        let _value_start = scanner.offset;

        if scanner.peek() == Some(b'\n') || scanner.peek().is_none() {
            // Block value - need to find the end of the block
            scanner.offset += 1;
            let block_indent = self.find_indent_at(key_offset).map(|(i, _)| i).unwrap_or(0);

            while scanner.offset < self.input.len() {
                let line_indent = scanner.get_line_indent();
                if line_indent <= block_indent && !scanner.is_empty_or_comment_line() {
                    break;
                }
                scanner.skip_line();
            }
        } else {
            // Inline value - skip to end of line
            scanner.skip_to_eol();
            if scanner.peek() == Some(b'\n') {
                scanner.offset += 1;
            }
        }

        let end_offset = scanner.offset;

        // Check if this is the only key at this level
        let siblings: Vec<_> = self.paths.keys()
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
        let mut scanner = Scanner::new(&self.input, key_offset);

        let indent = self.find_property_indent(path, key_offset)?;

        // Skip key
        scanner.skip_key()?;
        scanner.skip_char(b':')?;

        // Remember offset right after colon (before any whitespace)
        let after_colon_offset = scanner.offset;

        scanner.skip_inline_whitespace();

        let pre_value_offset = scanner.offset;

        // Determine if the current value is a block or inline
        let is_block = scanner.peek() == Some(b'\n') || scanner.peek().is_none();

        if is_block {
            // Skip block value
            if scanner.peek() == Some(b'\n') {
                scanner.offset += 1;
            }
            let block_indent = indent.child_indent.unwrap_or(2);

            while scanner.offset < self.input.len() {
                let line_indent = scanner.get_line_indent();
                // Check if we hit a line with less or equal indent (and it's not empty/comment)
                if line_indent < block_indent && !scanner.is_empty_or_comment_line() {
                    break;
                }
                if line_indent == block_indent && !scanner.is_empty_or_comment_line() {
                    // Same indent means sibling key, only break if it looks like a key
                    let saved = scanner.offset;
                    scanner.skip_inline_whitespace();
                    if scanner.peek_key() {
                        scanner.offset = saved;
                        break;
                    }
                    scanner.offset = saved;
                }
                scanner.skip_line();
            }
        } else {
            // Skip inline value
            scanner.skip_to_eol();
        }

        let post_value_offset = scanner.offset;

        // Format the new value
        let formatted = self.format_value(&value, indent);

        // If the new value starts with a newline (block value), start from after the colon
        // to remove any trailing spaces. Otherwise, add a space if needed.
        let (replace_start, final_value) = if formatted.starts_with('\n') {
            (after_colon_offset, formatted)
        } else {
            // Ensure there's a space before inline values
            let prefix = if pre_value_offset == after_colon_offset { " " } else { "" };
            (pre_value_offset, format!("{}{}", prefix, formatted))
        };

        self.replace_range(replace_start..post_value_offset, final_value.as_bytes())
    }

    fn insert_key(&mut self, path: &Path, value: Value) -> Result<(), Error> {
        let parent_path = Path::from_segments(path[0..path.len() - 1].to_vec());

        if path.len() > 1 {
            self.insert_nested_key(&parent_path, &path[path.len() - 1], value)
        } else if path.len() == 1 {
            self.insert_top_level_key(&path[path.len() - 1], value)
        } else {
            Ok(())
        }
    }

    fn ensure_object_key(&mut self, path: &Path) -> Result<(), Error> {
        if self.paths.contains_key(path) {
            return Ok(());
        }

        self.insert_key(&path, Value::Object(vec![]))?;

        Ok(())
    }

    fn insert_nested_key(&mut self, parent_path: &Path, new_key: &str, value: Value) -> Result<(), Error> {
        self.ensure_object_key(&parent_path)?;

        let &parent_key_offset = self.paths.get(parent_path)
            .expect("A parent key must exist");

        let mut scanner = Scanner::new(&self.input, parent_key_offset);

        scanner.skip_key()?;
        scanner.skip_char(b':')?;
        scanner.skip_inline_whitespace();

        // Get parent's indent level
        let parent_indent = self.find_indent_at(parent_key_offset).map(|(i, _)| i).unwrap_or(0);
        let child_indent = parent_indent + 2;

        let property_indent = Indent::with_style(Some(child_indent), Some(child_indent + 2), IndentStyle::Spaces);

        self.insert_at(scanner.offset, parent_path, new_key, property_indent, value)
    }

    fn insert_top_level_key(&mut self, new_key: &str, value: Value) -> Result<(), Error> {
        let top_level_indent = Indent::with_style(Some(0), Some(2), IndentStyle::Spaces);

        self.insert_at(0, &Path::new(), new_key, top_level_indent, value)
    }

    fn insert_at(&mut self, _offset: usize, parent_path: &Path, new_key: &str, indent: Indent, value: Value) -> Result<(), Error> {
        let (before, after): (Vec<_>, Vec<_>) = self.paths.keys()
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
        let line_start = self.input[0..next_property_offset]
            .iter()
            .rposition(|&c| c == b'\n')
            .map(|pos| pos + 1)
            .unwrap_or(0);

        let self_indent = indent.self_indent.unwrap_or(0);

        let mut injected_content = vec![];

        // Add indentation
        for _ in 0..self_indent {
            injected_content.push(b' ');
        }

        push_yaml_key(&mut injected_content, new_key);
        injected_content.push(b':');

        let formatted_value = self.format_value(&value, indent.clone());
        if !formatted_value.starts_with('\n') {
            injected_content.push(b' ');
        }
        injected_content.extend_from_slice(formatted_value.as_bytes());
        injected_content.push(b'\n');

        self.replace_range(line_start..line_start, &injected_content)
    }

    fn insert_after_property(&mut self, previous_property_offset: usize, new_key: &str, indent: Indent, value: Value) -> Result<(), Error> {
        let mut scanner = Scanner::new(&self.input, previous_property_offset);

        let self_indent = indent.self_indent.unwrap_or(0);

        // Skip the previous property's key and value
        scanner.skip_key()?;
        scanner.skip_char(b':')?;
        scanner.skip_inline_whitespace();

        // Skip value
        if scanner.peek() == Some(b'\n') {
            // Block value
            scanner.offset += 1;
            let block_indent = indent.child_indent.unwrap_or(self_indent + 2);

            while scanner.offset < self.input.len() {
                let line_indent = scanner.get_line_indent();
                if line_indent < block_indent && !scanner.is_empty_or_comment_line() {
                    break;
                }
                scanner.skip_line();
            }
        } else {
            scanner.skip_to_eol();
            if scanner.peek() == Some(b'\n') {
                scanner.offset += 1;
            }
        }

        let mut injected_content = vec![];

        // Add indentation
        for _ in 0..self_indent {
            injected_content.push(b' ');
        }

        push_yaml_key(&mut injected_content, new_key);
        injected_content.push(b':');

        let formatted_value = self.format_value(&value, indent.clone());
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
            let mut new_content = vec![];

            push_yaml_key(&mut new_content, new_key);
            new_content.push(b':');

            let formatted_value = self.format_value(&value, indent);
            if !formatted_value.starts_with('\n') {
                new_content.push(b' ');
            }
            new_content.extend_from_slice(formatted_value.as_bytes());
            new_content.push(b'\n');

            return self.replace_range(0..self.input.len(), &new_content);
        }

        // Find parent and insert after the colon
        let &parent_offset = self.paths.get(parent_path)
            .expect("Parent path must exist");

        let mut scanner = Scanner::new(&self.input, parent_offset);
        scanner.skip_key()?;
        scanner.skip_char(b':')?;

        let child_indent = indent.child_indent.unwrap_or(indent.self_indent.unwrap_or(0) + 2);

        let mut new_content = vec![];
        new_content.push(b'\n');

        for _ in 0..child_indent {
            new_content.push(b' ');
        }

        push_yaml_key(&mut new_content, new_key);
        new_content.push(b':');

        let child_indent_obj = Indent::with_style(Some(child_indent), Some(child_indent + 2), indent.style);
        let formatted_value = self.format_value(&value, child_indent_obj);
        if !formatted_value.starts_with('\n') {
            new_content.push(b' ');
        }
        new_content.extend_from_slice(formatted_value.as_bytes());

        // Find what comes after the colon
        let insert_offset = scanner.offset;
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
        let mut check_offset = offset;
        let mut indent = 0;

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
        let self_indent_info = self.find_indent_at(offset);

        let (self_indent, style) = match self_indent_info {
            Some((indent, style)) => (Some(indent), style),
            None => (None, IndentStyle::Spaces),
        };

        // Calculate child indent based on parent relationship
        let suggested_child_indent = match self_indent {
            Some(self_indent_val) => {
                let delta = path.parent()
                    .and_then(|p| self.paths.get(&p))
                    .and_then(|&offset| self.find_indent_at(offset))
                    .map(|(parent_indent, _)| self_indent_val.saturating_sub(parent_indent))
                    .unwrap_or(2);

                Some(self_indent_val + delta)
            }
            None => Some(2),
        };

        let child_indent = suggested_child_indent;

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

                let child_indent = indent.child_indent.unwrap_or(indent.self_indent.unwrap_or(0) + 2);
                let mut result = String::new();

                for item in arr {
                    result.push('\n');
                    for _ in 0..child_indent {
                        result.push(' ');
                    }
                    result.push_str("- ");

                    let item_str = self.format_value(item, Indent::with_style(
                        Some(child_indent + 2),
                        Some(child_indent + 4),
                        indent.style,
                    ));
                    result.push_str(&item_str);
                }

                result
            }
            Value::Object(obj) => {
                if obj.is_empty() {
                    return "{}".to_string();
                }

                let child_indent = indent.child_indent.unwrap_or(indent.self_indent.unwrap_or(0) + 2);
                let mut result = String::new();

                for (key, val) in obj {
                    result.push('\n');
                    for _ in 0..child_indent {
                        result.push(' ');
                    }
                    result.push_str(&format_yaml_key(key));
                    result.push(':');

                    let val_str = self.format_value(val, Indent::with_style(
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
        let mut keys_by_position = self.paths.iter()
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
        let mut key_value_pairs: Vec<(&str, Vec<u8>)> = vec![];

        for (key_name, offset) in &keys_by_position {
            let line_start = self.input[0..*offset]
                .iter()
                .rposition(|&c| c == b'\n')
                .map(|pos| pos + 1)
                .unwrap_or(0);

            let mut scanner = Scanner::new(&self.input, *offset);
            scanner.skip_key()?;
            scanner.skip_char(b':')?;
            scanner.skip_inline_whitespace();

            // Find the end of this value
            if scanner.peek() == Some(b'\n') || scanner.peek().is_none() {
                // Block value
                if scanner.peek() == Some(b'\n') {
                    scanner.offset += 1;
                }

                let key_indent = self.find_indent_at(*offset).map(|(i, _)| i).unwrap_or(0);

                while scanner.offset < self.input.len() {
                    let line_indent = scanner.get_line_indent();
                    if line_indent <= key_indent && !scanner.is_empty_or_comment_line() {
                        break;
                    }
                    scanner.skip_line();
                }
            } else {
                scanner.skip_to_eol();
                if scanner.peek() == Some(b'\n') {
                    scanner.offset += 1;
                }
            }

            key_value_pairs.push((key_name, self.input[line_start..scanner.offset].to_vec()));
        }

        // Sort by key name
        key_value_pairs.sort_by_key(|(key_name, _)| *key_name);

        // Calculate the range to replace
        let first_line_start = self.input[0..keys_by_position[0].1]
            .iter()
            .rposition(|&c| c == b'\n')
            .map(|pos| pos + 1)
            .unwrap_or(0);

        let last_offset = keys_by_position.last().unwrap().1;
        let mut scanner = Scanner::new(&self.input, last_offset);
        scanner.skip_key()?;
        scanner.skip_char(b':')?;
        scanner.skip_inline_whitespace();

        if scanner.peek() == Some(b'\n') || scanner.peek().is_none() {
            if scanner.peek() == Some(b'\n') {
                scanner.offset += 1;
            }
            let key_indent = self.find_indent_at(last_offset).map(|(i, _)| i).unwrap_or(0);

            while scanner.offset < self.input.len() {
                let line_indent = scanner.get_line_indent();
                if line_indent <= key_indent && !scanner.is_empty_or_comment_line() {
                    break;
                }
                scanner.skip_line();
            }
        } else {
            scanner.skip_to_eol();
            if scanner.peek() == Some(b'\n') {
                scanner.offset += 1;
            }
        }

        let content_end = scanner.offset;

        // Rebuild content
        let mut sorted_content: Vec<u8> = vec![];
        for (_, entry_bytes) in key_value_pairs {
            sorted_content.extend_from_slice(&entry_bytes);
        }

        self.replace_range(first_line_start..content_end, &sorted_content)?;

        Ok(true)
    }
}

fn format_yaml_string(s: &str) -> String {
    // Check if the string needs quoting
    if s.is_empty() {
        return "\"\"".to_string();
    }

    // Check for special YAML values that would be misinterpreted
    let lower = s.to_lowercase();
    let needs_quotes = lower == "true" || lower == "false" || lower == "null" || lower == "~"
        || s.parse::<f64>().is_ok()
        || s.contains(' ')
        || s.contains(':')
        || s.contains('#')
        || s.contains('\n')
        || s.contains('"')
        || s.contains('\'')
        || s.starts_with(' ')
        || s.ends_with(' ')
        || s.starts_with('-')
        || s.starts_with('[')
        || s.starts_with('{')
        || s.starts_with('!')
        || s.starts_with('&')
        || s.starts_with('*')
        || s.starts_with('|')
        || s.starts_with('>')
        || s.starts_with('%')
        || s.starts_with('@')
        || s.starts_with('`');

    if needs_quotes {
        // Use double quotes and escape special characters
        let escaped = s
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('\n', "\\n")
            .replace('\t', "\\t");
        format!("\"{}\"", escaped)
    } else {
        s.to_string()
    }
}

fn format_yaml_key(key: &str) -> String {
    // Check if the key needs quoting
    let needs_quotes = key.is_empty()
        || key.contains(':')
        || key.contains('#')
        || key.contains('\n')
        || key.starts_with(' ')
        || key.ends_with(' ')
        || key.starts_with('-')
        || key.starts_with('[')
        || key.starts_with('{')
        || key.starts_with('"')
        || key.starts_with('\'');

    if needs_quotes {
        let escaped = key
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('\n', "\\n");
        format!("\"{}\"", escaped)
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
        Self { input, offset, path: None, fields: vec![] }
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

    fn get_line_indent(&self) -> usize {
        let mut offset = self.offset;
        while offset < self.input.len() && self.input[offset] == b' ' {
            offset += 1;
        }
        offset - self.offset
    }

    fn is_empty_or_comment_line(&self) -> bool {
        let mut offset = self.offset;
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

    fn peek_key(&self) -> bool {
        let mut offset = self.offset;

        // Skip leading whitespace
        while offset < self.input.len() && self.input[offset] == b' ' {
            offset += 1;
        }

        // Check for quoted key
        if offset < self.input.len() && (self.input[offset] == b'"' || self.input[offset] == b'\'') {
            let quote = self.input[offset];
            offset += 1;
            let mut escaped = false;

            while offset < self.input.len() {
                if escaped {
                    escaped = false;
                    offset += 1;
                } else if self.input[offset] == b'\\' {
                    escaped = true;
                    offset += 1;
                } else if self.input[offset] == quote {
                    offset += 1;
                    // Skip whitespace after quote
                    while offset < self.input.len() && self.input[offset] == b' ' {
                        offset += 1;
                    }
                    return offset < self.input.len() && self.input[offset] == b':';
                } else if self.input[offset] == b'\n' {
                    return false;
                } else {
                    offset += 1;
                }
            }
            return false;
        }

        // Unquoted key - look for colon
        while offset < self.input.len() && self.input[offset] != b':' && self.input[offset] != b'\n' {
            offset += 1;
        }

        offset < self.input.len() && self.input[offset] == b':'
    }

    fn skip_key(&mut self) -> Result<(), Error> {
        self.skip_inline_whitespace();

        // Check for quoted key
        if self.peek() == Some(b'"') || self.peek() == Some(b'\'') {
            let quote = self.input[self.offset];
            self.offset += 1;
            let mut escaped = false;

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
        let start = self.offset;

        // Check for quoted key
        if self.peek() == Some(b'"') || self.peek() == Some(b'\'') {
            let quote = self.input[self.offset];
            self.offset += 1;
            let mut key = Vec::new();
            let mut escaped = false;

            while self.offset < self.input.len() {
                if escaped {
                    match self.input[self.offset] {
                        b'n' => key.push(b'\n'),
                        b't' => key.push(b'\t'),
                        b'r' => key.push(b'\r'),
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

        let key = std::str::from_utf8(&self.input[start..self.offset])?.trim().to_string();
        Ok(Some(key))
    }

    fn scan_document(&mut self) -> Result<(), Error> {
        self.scan_block(0)?;
        Ok(())
    }

    fn scan_block(&mut self, expected_indent: usize) -> Result<(), Error> {
        while self.offset < self.input.len() {
            // Skip empty lines and comments
            self.skip_empty_lines();

            if self.offset >= self.input.len() {
                break;
            }

            let line_indent = self.get_line_indent();

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

            // Try to parse a key
            let key_start = self.offset;
            let key = self.parse_key()?;

            if let Some(key) = key {
                // Register the field
                if let Some(ref mut path) = self.path {
                    path.push(key.clone());
                    self.fields.push((Path::from_segments(path.clone()), key_start));
                }

                // Skip the colon
                if self.peek() == Some(b':') {
                    self.offset += 1;
                }
                self.skip_inline_whitespace();

                // Check if there's a value on the same line
                if self.peek() != Some(b'\n') && self.peek().is_some() {
                    // Inline value - skip it
                    self.skip_to_eol();
                }

                // Skip newline
                if self.peek() == Some(b'\n') {
                    self.offset += 1;
                }

                // Check for nested block
                self.skip_empty_lines();

                if self.offset < self.input.len() {
                    let next_indent = self.get_line_indent();
                    if next_indent > line_indent {
                        // Nested block
                        self.scan_block(next_indent)?;
                    }
                }

                // Pop the path
                if let Some(ref mut path) = self.path {
                    path.pop();
                }
            } else {
                // Not a valid key line, skip it
                self.skip_line();
            }
        }

        Ok(())
    }

    fn skip_empty_lines(&mut self) {
        while self.offset < self.input.len() {
            let start = self.offset;
            self.skip_inline_whitespace();

            if self.peek() == Some(b'#') {
                // Comment line
                self.skip_line();
            } else if self.peek() == Some(b'\n') {
                // Empty line
                self.offset += 1;
            } else {
                // Not empty, restore position
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

    // Insert nested keys
    #[case(b"parent:\n  existing: value\n", vec!["parent", "new_child"], Value::String("new".to_string()), b"parent:\n  existing: value\n  new_child: new\n")]

    // Delete operations
    #[case(b"keep: this\ndelete: me\n", vec!["delete"], Value::Undefined, b"keep: this\n")]
    #[case(b"first: v1\nsecond: v2\nthird: v3\n", vec!["second"], Value::Undefined, b"first: v1\nthird: v3\n")]

    // Array values
    #[case(b"arr: []\n", vec!["arr"], Value::Array(vec![Value::Number("1".to_string()), Value::Number("2".to_string())]), b"arr:\n  - 1\n  - 2\n")]

    // Object values
    #[case(b"obj: {}\n", vec!["obj"], Value::Object(vec![("inner".to_string(), Value::String("value".to_string()))]), b"obj:\n  inner: value\n")]

    // String quoting
    #[case(b"test: simple\n", vec!["test"], Value::String("has space".to_string()), b"test: \"has space\"\n")]
    #[case(b"test: old\n", vec!["test"], Value::String("true".to_string()), b"test: \"true\"\n")]

    fn test_update_document(#[case] document: &[u8], #[case] path: Vec<&str>, #[case] value: Value, #[case] expected: &[u8]) {
        let mut document = YamlDocument::new(document.to_vec()).unwrap();

        document.set_path(&Path::from_segments(path.into_iter().map(|s| s.to_string()).collect()), value).unwrap();
        assert_eq!(String::from_utf8(document.input).unwrap(), String::from_utf8(expected.to_vec()).unwrap());
    }

    #[test]
    fn test_hydrate_from_str() {
        let yaml = "name: test\nversion: \"1.0\"\n";
        let result: serde_yaml::Value = YamlDocument::hydrate_from_str(yaml).unwrap();

        assert_eq!(result["name"], serde_yaml::Value::String("test".to_string()));
        assert_eq!(result["version"], serde_yaml::Value::String("1.0".to_string()));
    }

    #[test]
    fn test_to_string() {
        use std::collections::HashMap;

        let mut map = HashMap::new();
        map.insert("key", "value");

        let yaml = YamlDocument::to_string(&map).unwrap();
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
        let mut document = YamlDocument::new(document.to_vec()).unwrap();

        let sorted = document.sort_object_keys(&Path::from_segments(path.into_iter().map(|s| s.to_string()).collect())).unwrap();

        assert_eq!(sorted, expected_sorted, "sort_object_keys return value mismatch");
        assert_eq!(String::from_utf8(document.input).unwrap(), String::from_utf8(expected.to_vec()).unwrap());
    }
}
