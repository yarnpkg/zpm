use std::{collections::BTreeMap, ops::Range};

use crate::Path;

#[derive(Debug, Clone)]
pub struct Span {
    pub start: usize,
    pub size: usize,
}

pub struct Document {
    pub input: Vec<u8>,
    pub paths: BTreeMap<Path, usize>,
}

impl Document {
    pub fn new(input: Vec<u8>) -> Self {
        let mut scanner
            = Scanner::new(&input, 0);

        scanner.path = Some(vec![]);

        scanner.skip_whitespace();
        scanner.skip_object();
        scanner.skip_whitespace();
        scanner.skip_eof();

        let paths
            = scanner.fields.into_iter()
                .collect();

        Self {
            input,
            paths,
        }
    }

    pub fn rescan(&mut self) {
        let mut scanner
            = Scanner::new(&self.input, 0);

        scanner.path = Some(vec![]);

        scanner.skip_whitespace();
        scanner.skip_object();

        self.paths
            = scanner.fields.into_iter()
                .collect();
    }

    pub fn set_path(&mut self, path: &Path, raw: Option<&[u8]>) {
        let key_span
            = self.paths.get(path);

        let Some(raw) = raw else {
            if let Some(key_span) = key_span {
                return self.remove_key_at(path, key_span.clone());
            } else {
                return;
            }
        };

        if let Some(key_span) = key_span {
            self.update_key_at(key_span.clone(), raw);
        } else {
            if path.len() > 1 {
                let parent_path
                    = Path::from_segments(path[0..path.len() - 1].to_vec());

                self.insert_key_at(&parent_path, &path[path.len() - 1], Vec::from(raw));
            } else if path.len() == 1 {
                self.insert_top_level_key(&path[path.len() - 1], Vec::from(raw));
            }
        }
    }

    fn replace_range(&mut self, range: Range<usize>, data: &[u8]) {
        let (before, after)
            = self.input.split_at(range.start);
        let (_, after)
            = after.split_at(range.end - range.start);

        self.input = [before, data, after].concat();
        self.rescan();
    }

    fn remove_key_at(&mut self, path: &Path, key_offset: usize) {
        let previous_stop
            = self.input[0..key_offset]
                .iter()
                .rposition(|&c| c == b'{' || c == b',')
                .expect("A key must be preceded by a '{' or ','");

        let mut scanner
            = Scanner::new(&self.input, key_offset);

        scanner.skip_string();
        scanner.skip_whitespace();
        scanner.skip_char(b':');
        scanner.skip_whitespace();
        scanner.skip_value();

        scanner.skip_whitespace();

        let this_stop
            = scanner.offset;

        // If we remove the last key in an object, we remove the entire object.
        if self.input[previous_stop] == b'{' && self.input[this_stop] == b'}' {
            let parent_path
                = Path::from_segments(path[0..path.len() - 1].to_vec());

            return self.set_path(&parent_path, None);
        }

        self.replace_range(key_offset..this_stop, b"");
    }

    fn update_key_at(&mut self, key_offset: usize, raw: &[u8]) {
        let mut scanner
            = Scanner::new(&self.input, key_offset);

        scanner.skip_string();
        scanner.skip_whitespace();
        scanner.skip_char(b':');
        scanner.skip_whitespace();

        let pre_value_offset
            = scanner.offset;

        scanner.skip_value();

        let post_value_offset
            = scanner.offset;

        self.replace_range(pre_value_offset..post_value_offset, raw);
    }

    fn ensure_object_key(&mut self, path: &Path) {
        if let Some(_) = self.paths.get(path) {
            return;
        }

        let parent_path
            = Path::from_segments(path[0..path.len() - 1].to_vec());

        self.insert_key_at(&parent_path, &path[path.len() - 1], b"{}".to_vec());
    }

    fn insert_key_at(&mut self, parent_path: &Path, new_key: &str, raw: Vec<u8>) {
        self.ensure_object_key(parent_path);

        let &parent_key_offset
            = self.paths.get(parent_path)
                .expect("A parent key must exist");

        let mut scanner
            = Scanner::new(&self.input, parent_key_offset);

        scanner.skip_string();
        scanner.skip_whitespace();
        scanner.skip_char(b':');
        scanner.skip_whitespace();

        let parent_indent
            = self.find_indent(parent_key_offset);

        self.insert_at(scanner.offset, new_key, parent_indent + 2, raw);
    }

    fn insert_top_level_key(&mut self, new_key: &str, raw: Vec<u8>) {
        let mut scanner
            = Scanner::new(&self.input, 0);

        scanner.skip_whitespace();
        scanner.skip_char(b'{');

        self.insert_at(scanner.offset, new_key, 2, raw);
    }

    fn insert_at(&mut self, offset: usize, new_key: &str, indent: usize, raw: Vec<u8>) {
        let mut scanner
            = Scanner::new(&self.input, offset);

        scanner.skip_char(b'{');

        let post_open_brace_offset
            = scanner.offset;

        scanner.skip_whitespace();

        let mut new_content
            = vec![];

        push_string(&mut new_content, &new_key);
        new_content.extend_from_slice(b": ");
        new_content.extend_from_slice(&raw);

        if scanner.peek() == Some(b'}') {
            let mut final_replacement
                = vec![];

            if indent > 0 {
                final_replacement.push(b'\n');
                for _ in 0..indent {
                    final_replacement.push(b' ');
                }
            }

            final_replacement.extend_from_slice(&new_content);

            if indent > 0 {
                final_replacement.push(b'\n');
                for _ in 0..indent.saturating_sub(2) {
                    final_replacement.push(b' ');
                }
            }

            self.replace_range(post_open_brace_offset..scanner.offset, &final_replacement);
        } else {
            scanner.skip_char(b',');

            let mut final_replacement
                = vec![];

            final_replacement.extend_from_slice(&self.input[post_open_brace_offset..scanner.offset]);
            final_replacement.extend_from_slice(&new_content);

            self.replace_range(scanner.offset..scanner.offset, &final_replacement);
        }
    }

    fn find_indent(&self, mut offset: usize) -> usize {
        let mut indent
            = 0;

        while offset > 0 && self.input[offset - 1] == b' ' {
            indent += 1;
            offset -= 1;
        }

        indent
    }
}

fn push_string(content: &mut Vec<u8>, string: &str) {
    content.push(b'"');
    content.extend_from_slice(string.as_bytes());
    content.push(b'"');
}

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

    fn skip_whitespace(&mut self) {
        while self.offset < self.input.len() && (self.input[self.offset] == b' ' || self.input[self.offset] == b'\n') {
            self.offset += 1;
        }
    }

    fn skip_eof(&mut self) {
        if self.offset < self.input.len() {
            self.syntax_error(vec![None]);
        }
    }

    fn syntax_error(&self, expected: Vec<Option<u8>>) {
        let mut message
            = String::new();

        message.push_str("Expected ");

        let Some((tail, head)) = expected.split_last() else {
            panic!("Expected at least one character");
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

        panic!("{}", message);
    }

    fn skip_char(&mut self, c: u8) {
        if self.input[self.offset] == c {
            self.offset += 1;
        } else {
            self.syntax_error(vec![Some(c)]);
        }
    }

    fn skip_value(&mut self) {
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

    fn skip_keyword(&mut self, keyword: &str) {
        for c in keyword.as_bytes() {
            self.skip_char(*c);
        }
    }

    fn skip_string(&mut self) {
        self.skip_char(b'"');

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
                    return;
                },

                _ => {
                    self.offset += 1;
                },
            }
        }

        self.syntax_error(vec![Some(b'"'),]);
    }

    fn skip_number(&mut self) {
        while self.offset < self.input.len() && self.input[self.offset].is_ascii_digit() {
            self.offset += 1;
        }
    }

    fn skip_array(&mut self) {
        self.skip_char(b'[');
        self.skip_whitespace();

        if self.peek() == Some(b']') {
            self.skip_char(b']');
            return;
        }

        let path
            = std::mem::take(&mut self.path);

        while self.peek().is_none() {
            self.skip_value();
            self.skip_whitespace();

            match self.peek() {
                Some(b',') => {
                    self.skip_char(b',');
                    self.skip_whitespace();
                },

                Some(b']') => {
                    self.skip_char(b']');

                    self.path = path;
                    return;
                },

                _ => {
                    self.syntax_error(vec![Some(b','), Some(b']'),]);
                },
            }
        }

        self.syntax_error(vec![Some(b','), Some(b']'),]);
    }

    fn skip_key(&mut self) {
        let before_key_offset
            = self.offset;

        self.skip_string();

        let slice
            = &self.input[before_key_offset..self.offset];

        if let Some(path) = &mut self.path {
            path.push(sonic_rs::from_slice(slice).unwrap());
            self.fields.push((Path::from_segments(path.clone()), before_key_offset));
        }
    }

    fn skip_object(&mut self) {
        self.skip_char(b'{');
        self.skip_whitespace();

        if self.peek() == Some(b'}') {
            self.skip_char(b'}');
            return;
        }

        while !self.peek().is_none() {
            self.skip_key();
            self.skip_whitespace();
            self.skip_char(b':');
            self.skip_whitespace();
            self.skip_value();
            self.skip_whitespace();

            if let Some(path) = &mut self.path {
                path.pop();
            }

            match self.peek() {
                Some(b',') => {
                    self.skip_char(b',');
                    self.skip_whitespace();
                },

                Some(b'}') => {
                    self.skip_char(b'}');
                    return;
                },

                _ => {
                    self.syntax_error(vec![Some(b','), Some(b'}'),]);
                },
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
    #[case(b"{\"test\": \"value\"}", vec!["test"], Some(b"\"foo\"".as_slice()), b"{\"test\": \"foo\"}")]
    #[case(b"{\n  \"test\": {}\n}", vec!["test", "nested"], Some(b"\"foo\"".as_slice()), b"{\n  \"test\": {\n    \"nested\": \"foo\"\n  }\n}")]
    fn test_update_document(#[case] document: &[u8], #[case] path: Vec<&str>, #[case] raw: Option<&[u8]>, #[case] expected: &[u8]) {
        let mut document = Document::new(document.to_vec());
        document.set_path(&Path::from_segments(path.into_iter().map(|s| s.to_string()).collect()), raw);
        assert_eq!(String::from_utf8(document.input).unwrap(), String::from_utf8(expected.to_vec()).unwrap());
    }
}
