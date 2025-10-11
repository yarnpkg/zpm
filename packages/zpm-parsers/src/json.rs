use std::{collections::BTreeMap, ops::Range};

use serde::{Deserialize, Serialize};

use crate::{document::Document, value::Indent, Error, Path, Value};

#[cfg(target_pointer_width = "32")]
pub use serde_json as json_provider;

#[cfg(not(target_pointer_width = "32"))]
pub use sonic_rs as json_provider;

pub type RawJsonDeserializer<R> = json_provider::Deserializer<R>;
pub type RawJsonValue = json_provider::Value;

pub struct JsonDocument {
    pub input: Vec<u8>,
    pub paths: BTreeMap<Path, usize>,
    pub changed: bool,
}

impl Document for JsonDocument {
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

impl JsonDocument {
    pub fn hydrate_from_str<'de, T: Deserialize<'de>>(input: &'de str) -> Result<T, Error> {
        Ok(json_provider::from_str(input)?)
    }

    pub fn hydrate_from_slice<'de, T: Deserialize<'de>>(input: &'de [u8]) -> Result<T, Error> {
        Ok(json_provider::from_slice(input)?)
    }

    pub fn to_string<T: Serialize + ?Sized>(input: &T) -> Result<String, Error> {
        Ok(json_provider::to_string(input)?)
    }

    pub fn to_string_pretty<T: Serialize>(input: &T) -> Result<String, Error> {
        Ok(json_provider::to_string_pretty(input)?)
    }

    pub fn new(input: Vec<u8>) -> Result<Self, Error> {
        let mut scanner
            = Scanner::new(&input, 0);

        scanner.path = Some(vec![]);

        scanner.skip_whitespace();
        scanner.skip_object()?;
        scanner.skip_whitespace();
        scanner.skip_eof()?;

        let paths
            = scanner.fields.into_iter()
                .collect();

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

        scanner.skip_whitespace();
        scanner.skip_object()?;
        scanner.skip_whitespace();
        scanner.skip_eof()?;

        self.paths
            = scanner.fields.into_iter()
                .collect();

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
        let previous_stop
            = self.input[0..key_offset]
                .iter()
                .rposition(|&c| c == b'{' || c == b',')
                .expect("A key must be preceded by a '{' or ','");

        let mut scanner
            = Scanner::new(&self.input, key_offset);

        scanner.skip_string()?;
        scanner.skip_whitespace();
        scanner.skip_char(b':')?;
        scanner.skip_whitespace();
        scanner.skip_value()?;

        let post_value_offset
            = scanner.offset;

        scanner.skip_whitespace();

        let is_first_key
            = self.input[previous_stop] == b'{';
        let is_last_key
            = self.input[scanner.offset] == b'}';

        match (is_first_key, is_last_key) {
            (true, true) if previous_stop != 0 => {
                self.set_path(&Path::from_segments(path[0..path.len() - 1].to_vec()), Value::Undefined)
            },

            (true, true) => {
                self.replace_range(previous_stop + 1..scanner.offset, b"")
            },

            (true, false) => {
                scanner.skip_char(b',')?;
                scanner.skip_whitespace();

                self.replace_range(key_offset..scanner.offset, b"")
            },

            (false, _) => {
                self.replace_range(previous_stop..post_value_offset, b"")
            },
        }
    }

    fn update_key_at(&mut self, path: &Path, key_offset: usize, value: Value) -> Result<(), Error> {
        let mut scanner
            = Scanner::new(&self.input, key_offset);

        let indent
            = self.find_property_indent(path, key_offset)?;

        scanner.skip_string()?;
        scanner.skip_whitespace();
        scanner.skip_char(b':')?;
        scanner.skip_whitespace();

        let pre_value_offset
            = scanner.offset;

        scanner.skip_value()?;

        let post_value_offset
            = scanner.offset;

        self.replace_range(pre_value_offset..post_value_offset, value.to_indented_json_string(indent).as_bytes())
    }

    fn insert_key(&mut self, path: &Path, value: Value) -> Result<(), Error> {
        let parent_path
            = Path::from_segments(path[0..path.len() - 1].to_vec());

        if path.len() > 1 {
            self.insert_nested_key(&parent_path, &path[path.len() - 1], value)
        } else if path.len() == 1 {
            self.insert_top_level_key(&path[path.len() - 1], value)
        } else {
            Ok(())
        }
    }

    fn ensure_object_key(&mut self, path: &Path) -> Result<(), Error> {
        if let Some(_) = self.paths.get(path) {
            return Ok(());
        }

        self.insert_key(&path, Value::Object(vec![]))?;

        Ok(())
    }

    fn insert_nested_key(&mut self, parent_path: &Path, new_key: &str, value: Value) -> Result<(), Error> {
        self.ensure_object_key(&parent_path)?;

        let &parent_key_offset
            = self.paths.get(parent_path)
                .expect("A parent key must exist");

        let mut scanner
            = Scanner::new(&self.input, parent_key_offset);

        scanner.skip_string()?;
        scanner.skip_whitespace();
        scanner.skip_char(b':')?;
        scanner.skip_whitespace();

        let property_indent
            = self.find_property_indent(parent_path, parent_key_offset)?;

        self.insert_at(scanner.offset, parent_path, new_key, property_indent, value)
    }

    fn insert_top_level_key(&mut self, new_key: &str, value: Value) -> Result<(), Error> {
        let mut scanner
            = Scanner::new(&self.input, 0);

        scanner.skip_whitespace();

        let top_level_indent
            = self.find_object_indent(scanner.offset, Some(2))?;

        let property_indent
            = Indent::new(None, top_level_indent);

        self.insert_at(scanner.offset, &Path::new(), new_key, property_indent, value)
    }

    fn insert_before_property(&mut self, next_property_offset: usize, new_key: &str, indent: Indent, value: Value) -> Result<(), Error> {
        let scanner
            = Scanner::new(&self.input, next_property_offset);
        let mut prior_whitespaces
            = scanner.get_prior_whitespaces();

        if prior_whitespaces.len() == 0 {
            if scanner.rpeek() == Some(b'{') {
                prior_whitespaces = b" ";
            }
        }

        let mut injected_content
            = vec![];

        push_string(&mut injected_content, &new_key);
        injected_content.extend_from_slice(b": ");
        injected_content.extend_from_slice(&value.to_indented_json_string(indent).as_bytes());
        injected_content.extend_from_slice(b",");
        injected_content.extend_from_slice(&prior_whitespaces);

        self.replace_range(next_property_offset..next_property_offset, &injected_content)
    }

    fn insert_after_property(&mut self, previous_property_offset: usize, new_key: &str, indent: Indent, value: Value) -> Result<(), Error> {
        let mut scanner
            = Scanner::new(&self.input, previous_property_offset);
        let mut prior_whitespaces
            = scanner.get_prior_whitespaces();

        if prior_whitespaces.len() == 0 {
            let mut tmp_scanner
                = scanner.clone();

            tmp_scanner.rskip_whitespace();

            if tmp_scanner.rpeek() == Some(b'{') {
                prior_whitespaces = b" ";
            }
        }

        scanner.skip_string()?;
        scanner.skip_whitespace();
        scanner.skip_char(b':')?;
        scanner.skip_whitespace();
        scanner.skip_value()?;

        let mut injected_content
            = vec![];

        injected_content.extend_from_slice(b",");
        injected_content.extend_from_slice(&prior_whitespaces);

        push_string(&mut injected_content, &new_key);
        injected_content.extend_from_slice(b": ");
        injected_content.extend_from_slice(&value.to_indented_json_string(indent).as_bytes());

        self.replace_range(scanner.offset..scanner.offset, &injected_content)
    }

    fn insert_into_empty(&mut self, object_offset: usize, new_key: &str, indent: Indent, value: Value) -> Result<(), Error> {
        let mut scanner
            = Scanner::new(&self.input, object_offset);

        scanner.skip_char(b'{')?;

        let pre_whitespace_offset
            = scanner.offset;

        scanner.skip_whitespace();

        let post_whitespace_offset
            = scanner.offset;

        scanner.skip_char(b'}')?;

        let mut new_content
            = vec![];

        if let Some(child_indent) = indent.child_indent {
            new_content.push(b'\n');
            for _ in 0..child_indent {
                new_content.push(b' ');
            }
        }

        push_string(&mut new_content, &new_key);
        new_content.extend_from_slice(b": ");
        new_content.extend_from_slice(&value.to_indented_json_string(indent.clone()).as_bytes());

        if indent.child_indent.is_some() {
            new_content.push(b'\n');
            if let Some(self_indent) = indent.self_indent {
                for _ in 0..self_indent {
                    new_content.push(b' ');
                }
            }
        }

        self.replace_range(pre_whitespace_offset..post_whitespace_offset, &new_content)
    }

    fn insert_at(&mut self, offset: usize, parent_path: &Path, new_key: &str, indent: Indent, value: Value) -> Result<(), Error> {
        let (before, after): (Vec<_>, Vec<_>)
            = self.paths.keys()
                .filter(|p| p.is_direct_child_of(parent_path))
                .partition(|p| p.last() < Some(new_key));

        if let Some(insert_offset) = after.first() {
            return self.insert_before_property(self.paths[insert_offset], new_key, indent, value);
        }

        if let Some(insert_offset) = before.last() {
            return self.insert_after_property(self.paths[insert_offset], new_key, indent, value);
        }

        self.insert_into_empty(offset, new_key, indent, value)
    }

    /**
     * Return the indent level at the given offset. Return None if the given
     * offset is inline (i.e. not at the beginning of a line).
     */
    fn find_indent_at(&self, mut offset: usize) -> Option<usize> {
        let mut indent
            = 0;

        while offset > 0 && self.input[offset - 1] == b' ' {
            indent += 1;
            offset -= 1;
        }

        if offset == 0 || self.input[offset - 1] == b'\n' {
            Some(indent)
        } else {
            None
        }
    }

    fn find_object_indent(&self, offset: usize, default_if_empty: Option<usize>) -> Result<Option<usize>, Error> {
        let mut scanner
            = Scanner::new(&self.input, offset);

        match self.input[offset] {
            b'{' => {
                scanner.skip_char(b'{')?;
                scanner.skip_whitespace();

                if scanner.peek() == Some(b'}') {
                    Ok(default_if_empty)
                } else {
                    Ok(self.find_indent_at(scanner.offset))
                }
            },

            b'[' => {
                scanner.skip_char(b'[')?;
                scanner.skip_whitespace();

                if scanner.peek() == Some(b']') {
                    Ok(default_if_empty)
                } else {
                    Ok(self.find_indent_at(scanner.offset))
                }
            },

            _ => Ok(None),
        }
    }

    fn find_property_indent(&self, path: &Path, offset: usize) -> Result<Indent, Error> {
        let self_indent
            = self.find_indent_at(offset);

        let suggested_child_indent = match self_indent {
            Some(self_indent) => {
                let delta_between_parent_and_self
                    = path.parent()
                        .map(|p| self.paths.get(&p).unwrap_or(&0))
                        .and_then(|&offset| self.find_indent_at(offset))
                        .map(|parent_indent| self_indent.saturating_sub(parent_indent))
                        .unwrap_or(2);

                Some(self_indent + delta_between_parent_and_self)
            },

            None => {
                if offset == 0 {
                    Some(2)
                } else {
                    None
                }
            },
        };

        let mut scanner
            = Scanner::new(&self.input, offset);

        scanner.skip_string()?;
        scanner.skip_whitespace();
        scanner.skip_char(b':')?;
        scanner.skip_whitespace();

        let child_indent
            = self.find_object_indent(scanner.offset, suggested_child_indent)?;

        Ok(Indent {
            self_indent,
            child_indent,
        })
    }
}

fn push_string(content: &mut Vec<u8>, string: &str) {
    content.push(b'"');
    content.extend_from_slice(string.as_bytes());
    content.push(b'"');
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

    fn rpeek(&self) -> Option<u8> {
        if self.offset == 0 {
            return None;
        }

        self.input.get(self.offset - 1).copied()
    }

    fn get_prior_whitespaces(&self) -> &'a [u8] {
        let mut clone
            = Scanner::new(self.input, self.offset);

        clone.rskip_whitespace();

        &self.input[clone.offset..self.offset]
    }

    fn skip_whitespace(&mut self) {
        while self.offset < self.input.len() && (self.input[self.offset] == b' ' || self.input[self.offset] == b'\n') {
            self.offset += 1;
        }
    }

    fn rskip_whitespace(&mut self) {
        while self.offset > 0 && (self.input[self.offset - 1] == b' ' || self.input[self.offset - 1] == b'\n') {
            self.offset -= 1;
        }
    }

    fn skip_eof(&mut self) -> Result<(), Error> {
        if self.offset < self.input.len() {
            self.syntax_error(vec![None])?;
        }

        Ok(())
    }

    fn syntax_error(&self, expected: Vec<Option<u8>>) -> Result<(), Error> {
        let mut message
            = String::new();

        message.push_str("Expected ");

        let Some((tail, head)) = expected.split_last() else {
            return Err(Error::InvalidSyntax("Expected at least one character".to_string()));
        };

        for c in head {
            if let Some(c) = c {
                message.push_str("'");
                message.push(*c as char);
                message.push_str("'");
            } else {
                message.push_str("EOF");
            }

            if head.len() > 1 {
                message.push_str(", ");
            }
        }

        if head.len() > 0 {
            message.push_str(" or ");
        }

        if let Some(c) = tail {
            message.push_str("'");
            message.push(*c as char);
            message.push_str("'");
        } else {
            message.push_str("EOF");
        }

        message.push_str(" at offset ");
        message.push_str(&self.offset.to_string());
        message.push_str(", got ");

        if let Some(c) = self.peek() {
            message.push_str("'");
            message.push(c as char);
            message.push_str("'");
        } else {
            message.push_str("EOF");
        }

        message.push_str(" instead");

        Err(Error::InvalidSyntax(message))
    }

    fn skip_char(&mut self, c: u8) -> Result<(), Error> {
        if self.input[self.offset] == c {
            self.offset += 1;
            Ok(())
        } else {
            self.syntax_error(vec![Some(c)])
        }
    }

    fn skip_value(&mut self) -> Result<(), Error> {
        match self.peek() {
            Some(b'"') => self.skip_string(),
            Some(b'{') => self.skip_object(),
            Some(b'[') => self.skip_array(),
            Some(b't') => self.skip_keyword("true"),
            Some(b'f') => self.skip_keyword("false"),
            Some(b'n') => self.skip_keyword("null"),
            Some(b'0'..=b'9') => self.skip_number(),
            _ => self.syntax_error(vec![Some(b'"'), Some(b'{'), Some(b'['), Some(b't'), Some(b'f'), Some(b'n'), Some(b'0'), Some(b'1'), Some(b'2'), Some(b'3'), Some(b'4'), Some(b'5'), Some(b'6'), Some(b'7'), Some(b'8'), Some(b'9'),]),
        }
    }

    fn skip_keyword(&mut self, keyword: &str) -> Result<(), Error> {
        for c in keyword.as_bytes() {
            self.skip_char(*c)?;
        }

        Ok(())
    }

    fn skip_string(&mut self) -> Result<(), Error> {
        self.skip_char(b'"')?;

        let mut escaped
            = false;

        while self.offset < self.input.len() {
            match self.input[self.offset] {
                _ if escaped => {
                    escaped = false;
                    self.offset += 1;
                },

                b'\\' => {
                    escaped = true;
                    self.offset += 1;
                },

                b'"' => {
                    self.offset += 1;
                    return Ok(());
                },

                _ => {
                    self.offset += 1;
                },
            }
        }

        self.syntax_error(vec![Some(b'"'),])
    }

    fn skip_number(&mut self) -> Result<(), Error> {
        while self.offset < self.input.len() && self.input[self.offset].is_ascii_digit() {
            self.offset += 1;
        }

        if self.peek() == Some(b'.') {
            self.offset += 1;

            while self.offset < self.input.len() && self.input[self.offset].is_ascii_digit() {
                self.offset += 1;
            }
        }

        Ok(())
    }

    fn skip_array(&mut self) -> Result<(), Error> {
        self.skip_char(b'[')?;
        self.skip_whitespace();

        if self.peek() == Some(b']') {
            self.skip_char(b']')?;
            return Ok(());
        }

        let path
            = std::mem::take(&mut self.path);

        while self.peek().is_some() {
            self.skip_value()?;
            self.skip_whitespace();

            match self.peek() {
                Some(b',') => {
                    self.skip_char(b',')?;
                    self.skip_whitespace();
                },

                Some(b']') => {
                    self.skip_char(b']')?;

                    self.path = path;
                    return Ok(());
                },

                _ => {
                    self.syntax_error(vec![Some(b','), Some(b']'),])?;
                },
            }
        }

        self.syntax_error(vec![Some(b','), Some(b']'),])
    }

    fn skip_key(&mut self) -> Result<(), Error> {
        let before_key_offset
            = self.offset;

        self.skip_string()?;

        let slice
            = &self.input[before_key_offset..self.offset];

        if let Some(path) = &mut self.path {
            path.push(JsonDocument::hydrate_from_slice(slice)?);
            self.fields.push((Path::from_segments(path.clone()), before_key_offset));
        }

        Ok(())
    }

    fn skip_object(&mut self) -> Result<(), Error> {
        self.skip_char(b'{')?;
        self.skip_whitespace();

        if self.peek() == Some(b'}') {
            self.skip_char(b'}')?;
            return Ok(());
        }

        while !self.peek().is_none() {
            self.skip_key()?;
            self.skip_whitespace();
            self.skip_char(b':')?;
            self.skip_whitespace();
            self.skip_value()?;
            self.skip_whitespace();

            if let Some(path) = &mut self.path {
                path.pop();
            }

            match self.peek() {
                Some(b',') => {
                    self.skip_char(b',')?;
                    self.skip_whitespace();
                },

                Some(b'}') => {
                    self.skip_char(b'}')?;
                    return Ok(());
                },

                _ => {
                    self.syntax_error(vec![Some(b','), Some(b'}'),])?;
                },
            }
        }

        self.syntax_error(vec![Some(b','), Some(b'}'),])
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use rstest::rstest;

    use super::*;

    #[rstest]
    // Basic value updates
    #[case(b"{\"test\": \"value\"}", vec!["test"], Value::String("foo".to_string()), b"{\"test\": \"foo\"}")]
    #[case(b"{\"test\": 42}", vec!["test"], Value::Number("100".to_string()), b"{\"test\": 100}")]
    #[case(b"{\"test\": true}", vec!["test"], Value::Bool(false), b"{\"test\": false}")]
    #[case(b"{\"test\": null}", vec!["test"], Value::String("not null".to_string()), b"{\"test\": \"not null\"}")]

    // Insert new top-level keys
    #[case(b"{}", vec!["new_key"], Value::String("value".to_string()), b"{\n  \"new_key\": \"value\"\n}")]
    #[case(b"{\n}", vec!["new_key"], Value::String("value".to_string()), b"{\n  \"new_key\": \"value\"\n}")]
    #[case(b"{\"existing\": \"value\"}", vec!["new_key"], Value::String("another".to_string()), b"{\"existing\": \"value\", \"new_key\": \"another\"}")]
    #[case(b"{\n  \"existing\": \"value\"\n}", vec!["new_key"], Value::String("another".to_string()), b"{\n  \"existing\": \"value\",\n  \"new_key\": \"another\"\n}")]

    // Insert nested keys
    #[case(b"{\n  \"test\": {}\n}", vec!["test", "nested"], Value::String("foo".to_string()), b"{\n  \"test\": {\n    \"nested\": \"foo\"\n  }\n}")]
    #[case(b"{\n  \"test\": {\n  }\n}", vec!["test", "nested"], Value::String("foo".to_string()), b"{\n  \"test\": {\n    \"nested\": \"foo\"\n  }\n}")]
    #[case(b"{\"parent\": {}}", vec!["parent", "child"], Value::Number("42".to_string()), b"{\"parent\": {\"child\": 42}}")]
    #[case(b"{\n  \"parent\": {\n    \"existing\": \"value\"\n  }\n}", vec!["parent", "new_child"], Value::String("new".to_string()), b"{\n  \"parent\": {\n    \"existing\": \"value\",\n    \"new_child\": \"new\"\n  }\n}")]

    // Delete operations
    #[case(b"{\"test\": \"value\"}", vec!["test"], Value::Undefined, b"{}")]
    #[case(b"{\"keep\": \"this\", \"delete\": \"me\"}", vec!["delete"], Value::Undefined, b"{\"keep\": \"this\"}")]
    #[case(b"{\n  \"keep\": \"this\",\n  \"delete\": \"me\"\n}", vec!["delete"], Value::Undefined, b"{\n  \"keep\": \"this\"\n}")]
    #[case(b"{\"parent\": {\"child\": \"value\"}}", vec!["parent", "child"], Value::Undefined, b"{}")]
    #[case(b"{\"parent\": {\"keep\": \"this\", \"delete\": \"me\"}}", vec!["parent", "delete"], Value::Undefined, b"{\"parent\": {\"keep\": \"this\"}}")]

    #[case(b"{\"first\": \"value1\", \"second\": \"value2\", \"third\": \"value3\"}", vec!["first"], Value::Undefined, b"{\"second\": \"value2\", \"third\": \"value3\"}")]
    #[case(b"{\"first\": \"value1\", \"second\": \"value2\", \"third\": \"value3\"}", vec!["second"], Value::Undefined, b"{\"first\": \"value1\", \"third\": \"value3\"}")]
    #[case(b"{\"first\": \"value1\", \"second\": \"value2\", \"third\": \"value3\"}", vec!["third"], Value::Undefined, b"{\"first\": \"value1\", \"second\": \"value2\"}")]
    #[case(b"{\n  \"first\": \"value1\",\n  \"second\": \"value2\",\n  \"third\": \"value3\"\n}", vec!["first"], Value::Undefined, b"{\n  \"second\": \"value2\",\n  \"third\": \"value3\"\n}")]
    #[case(b"{\n  \"first\": \"value1\",\n  \"second\": \"value2\",\n  \"third\": \"value3\"\n}", vec!["second"], Value::Undefined, b"{\n  \"first\": \"value1\",\n  \"third\": \"value3\"\n}")]
    #[case(b"{\n  \"first\": \"value1\",\n  \"second\": \"value2\",\n  \"third\": \"value3\"\n}", vec!["third"], Value::Undefined, b"{\n  \"first\": \"value1\",\n  \"second\": \"value2\"\n}")]

    #[case(b"{\"nested\": {\"first\": \"value1\", \"second\": \"value2\", \"third\": \"value3\"}}", vec!["nested", "first"], Value::Undefined, b"{\"nested\": {\"second\": \"value2\", \"third\": \"value3\"}}")]
    #[case(b"{\"nested\": {\"first\": \"value1\", \"second\": \"value2\", \"third\": \"value3\"}}", vec!["nested", "second"], Value::Undefined, b"{\"nested\": {\"first\": \"value1\", \"third\": \"value3\"}}")]
    #[case(b"{\"nested\": {\"first\": \"value1\", \"second\": \"value2\", \"third\": \"value3\"}}", vec!["nested", "third"], Value::Undefined, b"{\"nested\": {\"first\": \"value1\", \"second\": \"value2\"}}")]
    #[case(b"{\n  \"nested\": {\n    \"first\": \"value1\",\n    \"second\": \"value2\",\n    \"third\": \"value3\"\n  }\n}", vec!["nested", "first"], Value::Undefined, b"{\n  \"nested\": {\n    \"second\": \"value2\",\n    \"third\": \"value3\"\n  }\n}")]
    #[case(b"{\n  \"nested\": {\n    \"first\": \"value1\",\n    \"second\": \"value2\",\n    \"third\": \"value3\"\n  }\n}", vec!["nested", "second"], Value::Undefined, b"{\n  \"nested\": {\n    \"first\": \"value1\",\n    \"third\": \"value3\"\n  }\n}")]
    #[case(b"{\n  \"nested\": {\n    \"first\": \"value1\",\n    \"second\": \"value2\",\n    \"third\": \"value3\"\n  }\n}", vec!["nested", "third"], Value::Undefined, b"{\n  \"nested\": {\n    \"first\": \"value1\",\n    \"second\": \"value2\"\n  }\n}")]

    // Complex nested structures
    #[case(b"{\"a\": {\"b\": {\"c\": \"value\"}}}", vec!["a", "b", "c"], Value::String("updated".to_string()), b"{\"a\": {\"b\": {\"c\": \"updated\"}}}")]
    #[case(b"{\n  \"a\": {\n    \"b\": {}\n  }\n}", vec!["a", "b", "c"], Value::String("deep".to_string()), b"{\n  \"a\": {\n    \"b\": {\n      \"c\": \"deep\"\n    }\n  }\n}")]
    #[case(b"{\"level1\": {}}", vec!["level1", "level2", "level3"], Value::String("very_deep".to_string()), b"{\"level1\": {\"level2\": {\"level3\": \"very_deep\"}}}")]

    // Array values
    #[case(b"{\"arr\": []}", vec!["arr"], Value::Array(vec![Value::Number("1".to_string()), Value::Number("2".to_string()), Value::Number("3".to_string())]), b"{\"arr\": [1, 2, 3]}")]
    #[case(b"{\"test\": [1, 2, 3]}", vec!["test"], Value::Array(vec![Value::Number("4".to_string()), Value::Number("5".to_string()), Value::Number("6".to_string())]), b"{\"test\": [4, 5, 6]}")]
    #[case(b"{\n  \"items\": []\n}", vec!["items"], Value::Array(vec![Value::String("item1".to_string()), Value::String("item2".to_string())]), b"{\n  \"items\": [\n    \"item1\",\n    \"item2\"\n  ]\n}")]

    // Object values
    #[case(b"{\"obj\": {}}", vec!["obj"], Value::Object(vec![("inner".to_string(), Value::String("value".to_string()))]), b"{\"obj\": {\"inner\": \"value\"}}")]
    #[case(b"{\n  \"config\": {}\n}", vec!["config"], Value::Object(vec![("enabled".to_string(), Value::Bool(true)), ("timeout".to_string(), Value::Number("30".to_string()))]), b"{\n  \"config\": {\n    \"enabled\": true,\n    \"timeout\": 30\n  }\n}")]

    // Multiple keys with different indentation
    #[case(b"{\n\"key1\": \"value1\"\n}", vec!["key2"], Value::String("value2".to_string()), b"{\n\"key1\": \"value1\",\n\"key2\": \"value2\"\n}")]
    #[case(b"{\n    \"deep_indent\": \"value\"\n}", vec!["another"], Value::String("test".to_string()), b"{\n    \"another\": \"test\",\n    \"deep_indent\": \"value\"\n}")]
    #[case(b"{\n    \"test\": {}\n}", vec!["test", "nested"], Value::String("foo".to_string()), b"{\n    \"test\": {\n        \"nested\": \"foo\"\n    }\n}")]

    // Edge cases with whitespace
    #[case(b"{ \"spaced\": \"value\" }", vec!["spaced"], Value::String("updated".to_string()), b"{ \"spaced\": \"updated\" }")]
    #[case(b"{\n\n  \"key\": \"value\"\n\n}", vec!["key"], Value::String("new_value".to_string()), b"{\n\n  \"key\": \"new_value\"\n\n}")]
    #[case(b"{\"key\":\"no_spaces\"}", vec!["key"], Value::String("with_spaces".to_string()), b"{\"key\":\"with_spaces\"}")]

    // Escaped characters
    #[case(b"{\"test\": \"value with \\\"quotes\\\"\"}", vec!["test"], Value::String("no quotes".to_string()), b"{\"test\": \"no quotes\"}")]
    #[case(b"{\"key\\nwith\\nnewlines\": \"value\"}", vec!["key\nwith\nnewlines"], Value::String("updated".to_string()), b"{\"key\\nwith\\nnewlines\": \"updated\"}")]

    // Numbers and booleans
    #[case(b"{\"int\": 42}", vec!["int"], Value::Number("3.14".to_string()), b"{\"int\": 3.14}")]
    #[case(b"{\"float\": 3.14}", vec!["float"], Value::Number("42".to_string()), b"{\"float\": 42}")]
    #[case(b"{\"bool\": true}", vec!["bool"], Value::Null, b"{\"bool\": null}")]
    #[case(b"{\"null_val\": null}", vec!["null_val"], Value::Bool(true), b"{\"null_val\": true}")]

    // Complex mixed operations
    #[case(b"{\n  \"keep\": \"this\",\n  \"update\": \"old\",\n  \"nested\": {\n    \"inner\": \"value\"\n  }\n}", vec!["update"], Value::String("new".to_string()), b"{\n  \"keep\": \"this\",\n  \"update\": \"new\",\n  \"nested\": {\n    \"inner\": \"value\"\n  }\n}")]
    #[case(b"{\n  \"config\": {\n    \"database\": {\n      \"host\": \"localhost\"\n    }\n  }\n}", vec!["config", "database", "port"], Value::Number("5432".to_string()), b"{\n  \"config\": {\n    \"database\": {\n      \"host\": \"localhost\",\n      \"port\": 5432\n    }\n  }\n}")]

    // Delete nested leaving parent structure
    #[case(b"{\n  \"parent\": {\n    \"child1\": \"keep\",\n    \"child2\": \"delete\"\n  }\n}", vec!["parent", "child2"], Value::Undefined, b"{\n  \"parent\": {\n    \"child1\": \"keep\"\n  }\n}")]

    // Insert into deeply nested structure
    #[case(b"{\n  \"a\": {\n    \"b\": {\n      \"c\": {\n        \"existing\": \"value\"\n      }\n    }\n  }\n}", vec!["a", "b", "c", "new_key"], Value::String("deep_value".to_string()), b"{\n  \"a\": {\n    \"b\": {\n      \"c\": {\n        \"existing\": \"value\",\n        \"new_key\": \"deep_value\"\n      }\n    }\n  }\n}")]

    // Replace entire nested object
    #[case(b"{\n  \"config\": {\n    \"old\": \"structure\"\n  }\n}", vec!["config"], Value::Object(vec![("new".to_string(), Value::String("structure".to_string())), ("with".to_string(), Value::String("multiple".to_string())), ("keys".to_string(), Value::Bool(true))]), b"{\n  \"config\": {\n    \"new\": \"structure\",\n    \"with\": \"multiple\",\n    \"keys\": true\n  }\n}")]

    // Edge case: single key object deletion results in empty object
    #[case(b"{\"only_key\": \"value\"}", vec!["only_key"], Value::Undefined, b"{}")]
    #[case(b"{\n  \"only_key\": \"value\"\n}", vec!["only_key"], Value::Undefined, b"{}")]

    // Edge case: creating nested structure from empty object
    #[case(b"{}", vec!["level1", "level2", "level3"], Value::String("created".to_string()), b"{\n  \"level1\": {\n    \"level2\": {\n      \"level3\": \"created\"\n    }\n  }\n}")]

    // Preserve formatting in complex structures
    #[case(b"{\n  \"section1\": {\n    \"key1\": \"value1\",\n    \"key2\": \"value2\"\n  },\n  \"section2\": {\n    \"key3\": \"value3\"\n  }\n}", vec!["section1", "key2"], Value::String("updated_value2".to_string()), b"{\n  \"section1\": {\n    \"key1\": \"value1\",\n    \"key2\": \"updated_value2\"\n  },\n  \"section2\": {\n    \"key3\": \"value3\"\n  }\n}")]

    fn test_update_document(#[case] document: &[u8], #[case] path: Vec<&str>, #[case] value: Value, #[case] expected: &[u8]) {
        let mut document
            = JsonDocument::new(document.to_vec()).unwrap();

        document.set_path(&Path::from_segments(path.into_iter().map(|s| s.to_string()).collect()), value).unwrap();
        assert_eq!(String::from_utf8(document.input).unwrap(), String::from_utf8(expected.to_vec()).unwrap());
    }
}
