use crate::error::Error;

#[derive(Debug, PartialEq, Eq)]
pub enum Node {
    Inline {
        offset: usize,
        size: usize,

        indent: usize,
        column: usize,
        lines: usize,
    },

    Block {
        offset: usize,
        size: usize,

        indent: usize,
        column: usize,
        lines: usize,
    },
}

impl Node {
    pub fn offset(&self) -> usize {
        match self {
            Node::Inline { offset, .. } => *offset,
            Node::Block { offset, .. } => *offset,
        }
    }

    pub fn size(&self) -> usize {
        match self {
            Node::Inline { size, .. } => *size,
            Node::Block { size, .. } => *size,
        }
    }

    pub fn indent(&self) -> usize {
        match self {
            Node::Inline { indent, .. } => *indent,
            Node::Block { indent, .. } => *indent,
        }
    }

    pub fn replace_by(&self, input: &mut String, raw: &str) -> () {
        input.replace_range(self.offset()..self.offset() + self.size(), raw);
    }

    pub fn append_object(&self, input: &mut String, suggested_indent: Option<usize>, path: &[String], raw: &str) -> () {
        let mut inserted_str
            = String::new();
        let mut indent
            = String::from(" ".repeat(self.indent()));

        let indent_increment
            = " ".repeat(suggested_indent.unwrap_or(2));

        for parent in path[..path.len() - 1].iter() {
            inserted_str.push_str("\n");
            inserted_str.push_str(&indent);
            inserted_str.push_str(parent);
            inserted_str.push_str(":");
            indent.push_str(&indent_increment);
        }

        inserted_str.push_str("\n");
        inserted_str.push_str(&indent);
        inserted_str.push_str(&path[path.len() - 1]);
        inserted_str.push_str(": ");
        inserted_str.push_str(raw);

        input.insert_str(self.offset() + self.size(), &inserted_str);
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct Field {
    pub path: Vec<String>,
    pub node: Node,
}

pub type Document = Vec<Field>;

#[derive(Debug, PartialEq, Eq)]
pub struct Scope {
    pub path: Vec<String>,
    pub indent: usize,

    pub offset: usize,
    pub column: usize,
    pub lines: usize,
}

pub struct Parser<'a> {
    pub input: &'a [u8],

    pub offset: usize,
    pub column: usize,
    pub lines: usize,

    pub stack: Vec<Scope>,
    pub last_field_end: Option<usize>,
}

impl<'a> Parser<'a> {
    pub fn new(input: &'a [u8]) -> Self {
        Parser {
            input,
            offset: 0,
            column: 0,
            lines: 0,
            stack: vec![
                Scope {
                    path: vec![],
                    indent: 0,
                    offset: 0,
                    column: 0,
                    lines: 0,
                },
            ],
            last_field_end: None,
        }
    }

    fn next_relevant_line(&mut self) -> usize {
        loop {
            let mut offset = self.offset;

            while offset < self.input.len() && self.input[offset] == b' ' {
                offset += 1;
            }

            if offset < self.input.len() && self.input[offset] == b'#' {
                while offset < self.input.len() && self.input[offset] != b'\n' {
                    offset += 1;
                }
            }

            if offset < self.input.len() && self.input[offset] == b'\n' {
                self.offset = offset + 1;
                self.column = 0;
                self.lines += 1;
            } else {
                break;
            }
        }

        self.offset
    }

    fn get_indent(&self) -> usize {
        let mut offset = self.offset;

        while offset < self.input.len() && self.input[offset] == b' ' {
            offset += 1;
        }

        offset - self.offset
    }

    fn skip_whitespace(&mut self) {
        while self.offset < self.input.len() && self.input[self.offset] == b' ' {
            self.offset += 1;
            self.column += 1;
        }
    }

    fn parse_key(&mut self) -> Result<Option<String>, Error> {
        let key_start = self.offset;

        while self.offset < self.input.len() {
            match self.input[self.offset] {
                b':' => {
                    let key
                        = std::str::from_utf8(&self.input[key_start..self.offset])?;

                    self.offset += 1;
                    self.column += 1;

                    return Ok(Some(key.to_string()));
                },

                b'\n' => {
                    self.offset += 1;
                    self.column = 0;
                    self.lines += 1;

                    return Ok(None);
                },

                _ => {
                    self.offset += 1;
                    self.column += 1;
                }
            }
        }

        Ok(None)
    }

    fn try_start_block(&mut self, key: &str) -> bool {
        let block_offset = self.offset;
        let block_column = self.column;
        let block_lines = self.lines;

        if self.offset >= self.input.len() || self.input[self.offset] != b'\n' {
            return false;
        }

        let new_line_offset = self.offset + 1;
        
        let mut spaces = 0;
        let mut next_offset = new_line_offset;
        
        while next_offset < self.input.len() && self.input[next_offset] == b' ' {
            spaces += 1;
            next_offset += 1;
        }
        
        let current_indent = if let Some(scope) = self.stack.last() {
            scope.indent
        } else {
            0
        };
        
        if spaces > current_indent {
            let mut new_path = if let Some(scope) = self.stack.last() {
                scope.path.clone()
            } else {
                Vec::new()
            };
            
            new_path.push(key.to_string());

            self.stack.push(Scope {
                path: new_path,
                indent: spaces,

                offset: block_offset,
                column: block_column,
                lines: block_lines,
            });
            
            return true;
        }

        false
    }

    fn try_parse_value(&mut self) -> Result<Option<Node>, Error> {
        if self.offset == self.input.len() {
            return Ok(Some(Node::Inline {
                offset: self.offset,
                size: 0,
                indent: self.stack.last().map_or(0, |scope| scope.indent),
                column: self.column,
                lines: self.lines,
            }));
        }

        let value_offset = self.offset;
        let value_column = self.column;
        let value_lines = self.lines;

        while self.offset < self.input.len() {
            match self.input[self.offset] {
                b'\n' => {
                    let node = Node::Inline {
                        offset: value_offset,
                        size: self.offset - value_offset,
                        indent: self.stack.last().map_or(0, |scope| scope.indent),
                        column: value_column,
                        lines: value_lines,
                    };

                    self.offset += 1;
                    self.column = 0;
                    self.lines += 1;

                    return Ok(Some(node));
                }

                _ => {
                    self.offset += 1;
                    self.column += 1;
                }
            }
        }

        Ok(Some(Node::Inline {
            offset: value_offset,
            size: self.offset - value_offset,
            indent: self.stack.last().map_or(0, |scope| scope.indent),
            column: value_column,
            lines: value_lines,
        }))
    }

    fn next_result(&mut self) -> Result<Option<Field>, Error> {
        while self.next_relevant_line() < self.input.len() || !self.stack.is_empty() {
            if let Some(Scope {indent: expected_indent, ..}) = self.stack.last() {
                let indent = self.get_indent();

                if indent < *expected_indent || self.offset >= self.input.len() {
                    let scope = self.stack.pop().unwrap();
    
                    return Ok(Some(Field {
                        path: scope.path,
                        node: Node::Block {
                            offset: scope.offset,
                            size: self.last_field_end.map_or(0, |end| end - scope.offset),
                            indent: scope.indent,
                            column: scope.column,
                            lines: scope.lines,
                        },
                    }));
                }
            }

            self.skip_whitespace();

            let Some(key) = self.parse_key()? else {
                continue;
            };

            self.skip_whitespace();

            if self.try_start_block(&key) {
                continue;
            }

            if let Some(inline_value) = self.try_parse_value()? {
                let mut field_path = self.stack.last()
                    .map(|scope| scope.path.clone())
                    .unwrap_or_default();

                field_path.push(key);

                return Ok(Some(Field {
                    path: field_path,
                    node: inline_value,
                }));
            }
        }

        Ok(None)
    }
}

impl<'a> Iterator for Parser<'a> {
    type Item = Result<Field, Error>;

    fn next(&mut self) -> Option<Self::Item> {
        match self.next_result() {
            Err(e) => {
                self.offset = self.input.len();
                Some(Err(e))
            },

            Ok(Some(field)) => {
                self.last_field_end = Some(field.node.offset() + field.node.size());
                Some(Ok(field))
            },

            Ok(None) => {
                None
            },
        }
    }
}

/// Updates a field in a YAML document, or adds it if it doesn't exist.
/// 
/// # Arguments
/// 
/// * `document` - The original YAML document as a string
/// * `key` - The key to update, can be a dot-separated path for nested fields
/// * `value` - The new value for the field
/// 
/// # Returns
/// 
/// A new document string with the field updated or added
pub fn update_document_field(document: &str, key: &str, value: &str) -> Result<String, Error> {
    let mut output = document.to_string();

    let field_path = key.split('.')
        .map(|s| s.to_string())
        .collect::<Vec<_>>();

    let mut suggested_indent = None;

    for field in Parser::new(document.as_bytes()) {
        if let Ok(field) = &field {
            suggested_indent = suggested_indent.or(Some(field.node.indent()));
        }

        match field {
            Ok(field) if field.path == field_path => {
                field.node.replace_by(&mut output, value);
                return Ok(output);
            },

            Ok(field) if field_path.starts_with(&field.path) => {
                field.node.append_object(&mut output, suggested_indent, &field_path[field.path.len()..], value);
                return Ok(output);
            },

            Err(e) => {
                return Err(e);
            },

            _ => {
                continue;
            },
        }
    }

    Ok("FOO".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_input() {
        let fields = Parser::new("".as_bytes())
            .collect::<Result<Vec<_>, _>>()
            .unwrap();

        assert_eq!(fields, vec![
            Field {
                path: vec![],
                node: Node::Block {
                    offset: 0,
                    size: 0,
                    indent: 0,
                    column: 0,
                    lines: 0,
                },
            },
        ]);
    }

    #[test]
    fn test_single_line() {
        let fields = Parser::new("test: value\n".as_bytes())
            .collect::<Result<Vec<_>, _>>()
            .unwrap();

        assert_eq!(fields, vec![
            Field {
                path: vec!["test".to_string()],
                node: Node::Inline {
                    offset: 6,
                    size: 5,
                    indent: 0,
                    column: 6,
                    lines: 0,
                },
            },
            Field {
                path: vec![],
                node: Node::Block {
                    offset: 0,
                    size: 11,
                    indent: 0,
                    column: 0,
                    lines: 0,
                },
            },
        ]);
    }

    #[test]
    fn test_comment() {
        let fields = Parser::new("# comment\ntest: value\n".as_bytes())
            .collect::<Result<Vec<_>, _>>()
            .unwrap();

        assert_eq!(fields, vec![
            Field {
                path: vec!["test".to_string()],
                node: Node::Inline {
                    offset: 16,
                    size: 5,
                    indent: 0,
                    column: 6,
                    lines: 1,
                },
            },
            Field {
                path: vec![],
                node: Node::Block {
                    offset: 0,
                    size: 21,
                    indent: 0,
                    column: 0,
                    lines: 0,
                },
            },
        ]);
    }

    #[test]
    fn test_unterminated_line() {
        let fields = Parser::new("test: value".as_bytes())
            .collect::<Result<Vec<_>, _>>()
            .unwrap();

        assert_eq!(fields, vec![
            Field {
                path: vec!["test".to_string()],
                node: Node::Inline {
                    offset: 6,
                    size: 5,
                    indent: 0,
                    column: 6,
                    lines: 0,
                },
            },
            Field {
                path: vec![],
                node: Node::Block {
                    offset: 0,
                    size: 11,
                    indent: 0,
                    column: 0,
                    lines: 0,
                },
            },
        ]);
    }

    #[test]
    fn test_multiple_fields() {
        let fields = Parser::new("foo: hello\nbar: world\n".as_bytes())
            .collect::<Result<Vec<_>, _>>()
            .unwrap();

        assert_eq!(fields, vec![
            Field {
                path: vec!["foo".to_string()],
                node: Node::Inline {
                    offset: 5,
                    size: 5,
                    indent: 0,
                    column: 5,
                    lines: 0,
                },
            },
            Field {
                path: vec!["bar".to_string()],
                node: Node::Inline {
                    offset: 16,
                    size: 5,
                    indent: 0,
                    column: 5,
                    lines: 1,
                },
            },
            Field {
                path: vec![],
                node: Node::Block {
                    offset: 0,
                    size: 21,
                    indent: 0,
                    column: 0,
                    lines: 0,
                },
            },
        ]);
    }

    #[test]
    fn test_multiple_fields_with_empty_lines() {
        let fields = Parser::new("foo: hello\n\n\nbar: world\n".as_bytes())
            .collect::<Result<Vec<_>, _>>()
            .unwrap();

        assert_eq!(fields, vec![
            Field {
                path: vec!["foo".to_string()],
                node: Node::Inline {
                    offset: 5,
                    size: 5,
                    indent: 0,
                    column: 5,
                    lines: 0,
                },
            },
            Field {
                path: vec!["bar".to_string()],
                node: Node::Inline {
                    offset: 18,
                    size: 5,
                    indent: 0,
                    column: 5,
                    lines: 3,
                },
            },
            Field {
                path: vec![],
                node: Node::Block {
                    offset: 0,
                    size: 23,
                    indent: 0,
                    column: 0,
                    lines: 0,
                },
            },
        ]);
    }

    #[test]
    fn test_nested_blocks() {
        let fields = Parser::new("foo:\n  bar: baz\n".as_bytes())
            .collect::<Result<Vec<_>, _>>()
            .unwrap();

        assert_eq!(fields, vec![
            Field {
                path: vec!["foo".to_string(), "bar".to_string()],
                node: Node::Inline {
                    offset: 12,
                    size: 3,
                    indent: 2,
                    column: 7,
                    lines: 1,
                },
            },
            Field {
                path: vec!["foo".to_string()],
                node: Node::Block {
                    offset: 4,
                    size: 11,
                    indent: 2,
                    column: 4,
                    lines: 0,
                },
            },
            Field {
                path: vec![],
                node: Node::Block {
                    offset: 0,
                    size: 15,
                    indent: 0,
                    column: 0,
                    lines: 0,
                },
            },
        ]);
    }

    #[test]
    fn test_multiple_nested_blocks() {
        let fields = Parser::new("foo:\n  bar: baz\n  baz: qux\n  qux:\n    quux: corge\n  grault: garply\n".as_bytes())
            .collect::<Result<Vec<_>, _>>()
            .unwrap();

        assert_eq!(fields, vec![
            Field {
                path: vec!["foo".to_string(), "bar".to_string()],
                node: Node::Inline {
                    offset: 12,
                    size: 3,
                    indent: 2,
                    column: 7,
                    lines: 1,
                },
            },
            Field {
                path: vec!["foo".to_string(), "baz".to_string()],
                node: Node::Inline {
                    offset: 23,
                    size: 3,
                    indent: 2,
                    column: 7,
                    lines: 2,
                },
            },
            Field {
                path: vec!["foo".to_string(), "qux".to_string(), "quux".to_string()],
                node: Node::Inline {
                    offset: 44,
                    size: 5,
                    indent: 4,
                    column: 10,
                    lines: 4,
                },
            },
            Field {
                path: vec!["foo".to_string(), "qux".to_string()],
                node: Node::Block {
                    offset: 33,
                    size: 16,
                    indent: 4,
                    column: 6,
                    lines: 3,
                },
            },
            Field {
                path: vec!["foo".to_string(), "grault".to_string()],
                node: Node::Inline {
                    offset: 60,
                    size: 6,
                    indent: 2,
                    column: 10,
                    lines: 5,
                },
            },
            Field {
                path: vec!["foo".to_string()],
                node: Node::Block {
                    offset: 4,
                    size: 62,
                    indent: 2,
                    column: 4,
                    lines: 0,
                },
            },
            Field {
                path: vec![],
                node: Node::Block {
                    offset: 0,
                    size: 66,
                    indent: 0,
                    column: 0,
                    lines: 0,
                },
            },
        ]);
    }

    #[test]
    fn test_update_document_field() {
        // Test updating an existing field
        let document = "foo: bar\nbaz: qux\n";
        let updated = update_document_field(document, "foo", "updated").unwrap();
        assert_eq!(updated, "foo: updated\nbaz: qux\n");

        // Test adding a new field
        let document = "foo: bar\nbaz: qux\n";
        let updated = update_document_field(document, "quux", "corge").unwrap();
        assert_eq!(updated, "foo: bar\nbaz: qux\nquux: corge\n");

        // Test updating a nested field
        let document = "foo:\n  bar: baz\n  baz: qux\n";
        let updated = update_document_field(document, "foo.bar", "updated").unwrap();
        assert_eq!(updated, "foo:\n  bar: updated\n  baz: qux\n");

        // Test adding a new nested field
        let document = "foo:\n  bar: baz\n";
        let updated = update_document_field(document, "foo.qux", "corge").unwrap();
        assert_eq!(updated, "foo:\n  bar: baz\n  qux: corge\n");

        // Test adding a new nested field with a four-space indent
        let document = "foo:\n    bar: baz\n";
        let updated = update_document_field(document, "foo.qux", "corge").unwrap();
        assert_eq!(updated, "foo:\n    bar: baz\n    qux: corge\n");

        // Test adding a new nested field with a four-space indent
        let document = "foo:\n    bar: baz\n";
        let updated = update_document_field(document, "foo.qux.quux", "corge").unwrap();
        assert_eq!(updated, "foo:\n    bar: baz\n    qux:\n        quux: corge\n");

        // Test updating a document with a comment (before)
        let document = "foo: bar\n# comment\nbaz: qux\n";
        let updated = update_document_field(document, "foo", "updated").unwrap();
        assert_eq!(updated, "foo: updated\n# comment\nbaz: qux\n");

        // Test updating a document with a comment (after)
        let document = "foo: bar\n# comment\nbaz: qux\n";
        let updated = update_document_field(document, "baz", "updated").unwrap();
        assert_eq!(updated, "foo: bar\n# comment\nbaz: updated\n");
    }
}
