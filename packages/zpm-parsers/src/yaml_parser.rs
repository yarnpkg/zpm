use crate::{node::{Field, Node, Span}, Error, Parser, Path};

#[derive(Debug, PartialEq, Eq)]
pub struct Scope {
    pub path: Path,
    pub indent: usize,
    pub field: usize,
    pub offset: usize,
}

impl Scope {
    pub fn new() -> Self {
        Self {
            path: Path::new(),
            indent: 0,
            field: 0,
            offset: 0,
        }
    }
}

pub struct YamlParser<'a> {
    pub input: &'a [u8],

    pub offset: usize,
    pub column: usize,
    pub lines: usize,

    pub stack: Vec<Scope>,
    pub last_field_end: Option<usize>,
}

impl<'a> YamlParser<'a> {
    pub fn new(input: &'a [u8]) -> Self {
        YamlParser {
            input,
            offset: 0,
            column: 0,
            lines: 0,
            stack: vec![Scope::new()],
            last_field_end: None,
        }
    }

    fn next_relevant_line(&mut self) -> usize {
        loop {
            let mut offset
                = self.offset;

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
        let mut offset
            = self.offset;

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

    fn skip_char(&mut self, c: u8) -> bool {
        if self.offset < self.input.len() && self.input[self.offset] == c {
            self.offset += 1;
            self.column += 1;

            true
        } else {
            false
        }
    }

    fn parse_quoted_key(&mut self, quote_char: u8) -> Result<Option<String>, Error> {
        let mut key
            = vec![];

        let mut escaped
            = false;

        while self.offset < self.input.len() {
            let mut is_escape
                = false;

            match self.input[self.offset] {
                b'\n' => {
                    self.offset += 1;
                    self.column += 1;
                    self.lines += 1;

                    return Ok(None);
                },

                c if escaped => {
                    self.offset += 1;
                    self.column += 1;

                    key.push(c);
                },

                b'\\' => {
                    self.offset += 1;
                    self.column += 1;

                    is_escape = true;
                },

                c if c == quote_char => {
                    self.offset += 1;
                    self.column += 1;

                    self.skip_whitespace();

                    if self.skip_char(b':') {
                        return Ok(Some(String::from_utf8(key)?));
                    } else {
                        return Ok(None);
                    }
                },

                c => {
                    self.offset += 1;
                    self.column += 1;

                    key.push(c);
                },
            }

            escaped = is_escape;
        }

        Ok(None)
    }

    fn parse_key(&mut self) -> Result<Option<String>, Error> {
        if self.offset < self.input.len() {
            let quote_char
                = self.input[self.offset];

            if quote_char == b'"' || quote_char == b'\'' {
                self.offset += 1;
                self.column += 1;

                return self.parse_quoted_key(quote_char);
            }
        }

        let key_start
            = self.offset;

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

    fn try_start_block(&mut self, key: &str, field_start: usize) -> bool {
        let block_offset
            = self.offset;

        // A block must start with a newline
        if self.offset >= self.input.len() || self.input[self.offset] != b'\n' {
            return false;
        }

        let new_line_offset
            = self.offset + 1;

        let mut indent
            = 0;
        let mut next_offset
            = new_line_offset;

        // Count the number of spaces before the key
        while next_offset < self.input.len() && self.input[next_offset] == b' ' {
            indent += 1;
            next_offset += 1;
        }

        let current_indent = self.stack.last()
            .map_or(0, |scope| scope.indent);

        // The new block must have a greater indent than the current scope
        if indent <= current_indent {
            return false;
        }

        let mut new_path
            = self.stack.last()
                .map(|scope| scope.path.clone())
                .unwrap_or_default();

        new_path.push(key.to_string());

        self.stack.push(Scope {
            path: new_path,
            indent,
            field: field_start,
            offset: block_offset,
        });

        return true;
    }

    fn try_parse_value(&mut self, field_start: usize) -> Result<Option<Node>, Error> {
        let value_offset
            = self.offset;

        let indent = self.stack.last()
            .map_or(0, |scope| scope.indent);

        while self.offset < self.input.len() {
            // We only support very simple values for now (no multiline strings, for example)
            match self.input[self.offset] {
                b'\n' => {
                    let node = Node {
                        field_span: Span::new(field_start, self.offset - field_start + 1),
                        value_span: Span::new(value_offset, self.offset - value_offset),
                        indent,
                    };

                    self.offset += 1;

                    return Ok(Some(node));
                }

                _ => {
                    self.offset += 1;
                }
            }
        }

        Ok(Some(Node {
            field_span: Span::new(field_start, self.offset - field_start),
            value_span: Span::new(value_offset, self.offset - value_offset),
            indent,
        }))
    }

    fn next_result(&mut self) -> Result<Option<Field>, Error> {
        while self.next_relevant_line() < self.input.len() || !self.stack.is_empty() {
            // If the indent is less than before, we close the current scope
            if let Some(Scope {indent: expected_indent, ..}) = self.stack.last() {
                let indent
                    = self.get_indent();

                if indent < *expected_indent || self.offset >= self.input.len() {
                    let scope
                        = self.stack.pop().unwrap();
                    let size = self.last_field_end
                        .map_or(0, |end| end - scope.offset);

                    return Ok(Some(Field {
                        path: scope.path,
                        node: Node {
                            field_span: Span::new(scope.field, self.offset - scope.field),
                            value_span: Span::new(scope.offset, size),
                            indent: scope.indent,
                        },
                    }));
                }
            }

            let field_start
                = self.offset;

            // We don't care about whitespaces since the indent was already checked
            // right before (to handle dedents) and in the previous iteration (indents)
            self.skip_whitespace();

            let Some(key) = self.parse_key()? else {
                continue;
            };

            self.skip_whitespace();

            if self.try_start_block(&key, field_start) {
                continue;
            }

            if let Some(inline_value) = self.try_parse_value(field_start)? {
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

impl<'a> Parser for YamlParser<'a> {
    fn parse(input: &str) -> Result<Vec<Field>, Error> {
        YamlParser::new(input.as_bytes()).collect()
    }
}

impl<'a> Iterator for YamlParser<'a> {
    type Item = Result<Field, Error>;

    fn next(&mut self) -> Option<Self::Item> {
        match self.next_result() {
            Err(e) => {
                self.offset = self.input.len();
                Some(Err(e))
            },

            Ok(Some(field)) => {
                self.last_field_end = Some(field.node.value_span.offset + field.node.value_span.size);
                Some(Ok(field))
            },

            Ok(None) => {
                None
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_input() {
        let fields = YamlParser::new("".as_bytes())
            .collect::<Result<Vec<_>, _>>()
            .unwrap();

        assert_eq!(fields, vec![
            Field {
                path: Path::new(),
                node: Node {
                    field_span: Span::new(0, 0),
                    value_span: Span::new(0, 0),
                    indent: 0,
                },
            },
        ]);
    }

    #[test]
    fn test_single_line() {
        let fields = YamlParser::new("test: value\n".as_bytes())
            .collect::<Result<Vec<_>, _>>()
            .unwrap();

        assert_eq!(fields, vec![
            Field {
                path: Path::from_segments(vec!["test".to_string()]),
                node: Node {
                    field_span: Span::new(0, 12),
                    value_span: Span::new(6, 5),
                    indent: 0,
                },
            },
            Field {
                path: Path::new(),
                node: Node {
                    field_span: Span::new(0, 12),
                    value_span: Span::new(0, 11),
                    indent: 0,
                },
            },
        ]);
    }

    #[test]
    fn test_keys_containing_colons() {
        let fields = YamlParser::new("\"foo:bar\": value\n".as_bytes())
            .collect::<Result<Vec<_>, _>>()
            .unwrap();

        assert_eq!(fields, vec![
            Field {
                path: Path::from_segments(vec!["foo:bar".to_string()]),
                node: Node {
                    field_span: Span::new(0, 17),
                    value_span: Span::new(11, 5),
                    indent: 0,
                },
            },
            Field {
                path: Path::new(),
                node: Node {
                    field_span: Span::new(0, 17),
                    value_span: Span::new(0, 16),
                    indent: 0,
                },
            },
        ]);
    }

    #[test]
    fn test_comment() {
        let fields = YamlParser::new("# comment\ntest: value\n".as_bytes())
            .collect::<Result<Vec<_>, _>>()
            .unwrap();

        assert_eq!(fields, vec![
            Field {
                path: Path::from_segments(vec!["test".to_string()]),
                node: Node {
                    field_span: Span::new(10, 12),
                    value_span: Span::new(16, 5),
                    indent: 0,
                },
            },
            Field {
                path: Path::new(),
                node: Node {
                    field_span: Span::new(0, 22),
                    value_span: Span::new(0, 21),
                    indent: 0,
                },
            },
        ]);
    }

    #[test]
    fn test_unterminated_line() {
        let fields = YamlParser::new("test: value".as_bytes())
            .collect::<Result<Vec<_>, _>>()
            .unwrap();

        assert_eq!(fields, vec![
            Field {
                path: Path::from_segments(vec!["test".to_string()]),
                node: Node {
                    field_span: Span::new(0, 11),
                    value_span: Span::new(6, 5),
                    indent: 0,
                },
            },
            Field {
                path: Path::new(),
                node: Node {
                    field_span: Span::new(0, 11),
                    value_span: Span::new(0, 11),
                    indent: 0,
                },
            },
        ]);
    }

    #[test]
    fn test_multiple_fields() {
        let fields = YamlParser::new("foo: hello\nbar: world\n".as_bytes())
            .collect::<Result<Vec<_>, _>>()
            .unwrap();

        assert_eq!(fields, vec![
            Field {
                path: Path::from_segments(vec!["foo".to_string()]),
                node: Node {
                    field_span: Span::new(0, 11),
                    value_span: Span::new(5, 5),
                    indent: 0,
                },
            },
            Field {
                path: Path::from_segments(vec!["bar".to_string()]),
                node: Node {
                    field_span: Span::new(11, 11),
                    value_span: Span::new(16, 5),
                    indent: 0,
                },
            },
            Field {
                path: Path::new(),
                node: Node {
                    field_span: Span::new(0, 22),
                    value_span: Span::new(0, 21),
                    indent: 0,
                },
            },
        ]);
    }

    #[test]
    fn test_multiple_fields_with_empty_lines() {
        let fields = YamlParser::new("foo: hello\n\n\nbar: world\n".as_bytes())
            .collect::<Result<Vec<_>, _>>()
            .unwrap();

        assert_eq!(fields, vec![
            Field {
                path: Path::from_segments(vec!["foo".to_string()]),
                node: Node {
                    field_span: Span::new(0, 11),
                    value_span: Span::new(5, 5),
                    indent: 0,
                },
            },
            Field {
                path: Path::from_segments(vec!["bar".to_string()]),
                node: Node {
                    field_span: Span::new(13, 11),
                    value_span: Span::new(18, 5),
                    indent: 0,
                },
            },
            Field {
                path: Path::new(),
                node: Node {
                    field_span: Span::new(0, 24),
                    value_span: Span::new(0, 23),
                    indent: 0,
                },
            },
        ]);
    }

    #[test]
    fn test_nested_blocks() {
        let fields = YamlParser::new("foo:\n  bar: baz\n".as_bytes())
            .collect::<Result<Vec<_>, _>>()
            .unwrap();

        assert_eq!(fields, vec![
            Field {
                path: Path::from_segments(vec!["foo".to_string(), "bar".to_string()]),
                node: Node {
                    field_span: Span::new(5, 11),
                    value_span: Span::new(12, 3),
                    indent: 2,
                },
            },
            Field {
                path: Path::from_segments(vec!["foo".to_string()]),
                node: Node {
                    field_span: Span::new(0, 16),
                    value_span: Span::new(4, 11),
                    indent: 2,
                },
            },
            Field {
                path: Path::new(),
                node: Node {
                    field_span: Span::new(0, 16),
                    value_span: Span::new(0, 15),
                    indent: 0,
                },
            },
        ]);
    }

    #[test]
    fn test_multiple_nested_blocks() {
        let fields = YamlParser::new("foo:\n  bar: baz\n  baz: qux\n  qux:\n    quux: corge\n  grault: garply\n".as_bytes())
            .collect::<Result<Vec<_>, _>>()
            .unwrap();

        assert_eq!(fields, vec![
            Field {
                path: Path::from_segments(vec!["foo".to_string(), "bar".to_string()]),
                node: Node {
                    field_span: Span::new(5, 11),
                    value_span: Span::new(12, 3),
                    indent: 2,
                },
            },
            Field {
                path: Path::from_segments(vec!["foo".to_string(), "baz".to_string()]),
                node: Node {
                    field_span: Span::new(16, 11),
                    value_span: Span::new(23, 3),
                    indent: 2,
                },
            },
            Field {
                path: Path::from_segments(vec!["foo".to_string(), "qux".to_string(), "quux".to_string()]),
                node: Node {
                    field_span: Span::new(34, 16),
                    value_span: Span::new(44, 5),
                    indent: 4,
                },
            },
            Field {
                path: Path::from_segments(vec!["foo".to_string(), "qux".to_string()]),
                node: Node {
                    field_span: Span::new(27, 23),
                    value_span: Span::new(33, 16),
                    indent: 4,
                },
            },
            Field {
                path: Path::from_segments(vec!["foo".to_string(), "grault".to_string()]),
                node: Node {
                    field_span: Span::new(50, 17),
                    value_span: Span::new(60, 6),
                    indent: 2,
                },
            },
            Field {
                path: Path::from_segments(vec!["foo".to_string()]),
                node: Node {
                    field_span: Span::new(0, 67),
                    value_span: Span::new(4, 62),
                    indent: 2,
                },
            },
            Field {
                path: Path::new(),
                node: Node {
                    field_span: Span::new(0, 67),
                    value_span: Span::new(0, 66),
                    indent: 0,
                },
            },
        ]);
    }
}
