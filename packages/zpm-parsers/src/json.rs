use zpm_utils::{impl_serialization_traits, FromFileString, ToFileString, ToHumanString};

use crate::{error::Error, JsonPath};

#[derive(Debug, Clone, PartialEq)]
pub enum JsonValue {
    Null,
    Bool(bool),
    Number(String), // Store as string to preserve exact formatting
    String(String),
    Array(Vec<JsonValue>),
    Object(Vec<(String, JsonValue)>), // Preserves insertion order
    Undefined, // Used to remove values
}

impl From<&sonic_rs::Value> for JsonValue {
    fn from(value: &sonic_rs::Value) -> Self {
        match value.as_ref() {
            sonic_rs::ValueRef::Null => {
                JsonValue::Null
            },

            sonic_rs::ValueRef::Bool(b) => {
                JsonValue::Bool(b)
            },

            sonic_rs::ValueRef::Number(n) => {
                JsonValue::Number(n.to_string())
            },

            sonic_rs::ValueRef::String(s) => {
                JsonValue::String(s.to_string())
            },

            sonic_rs::ValueRef::Array(arr) => {
                JsonValue::Array(arr.iter().map(From::from).collect())
            },

            sonic_rs::ValueRef::Object(obj) => {
                JsonValue::Object(obj.iter().map(|(k, v)| (k.to_string(), From::from(v))).collect())
            },
        }
    }
}

impl From<serde_json::Value> for JsonValue {
    fn from(value: serde_json::Value) -> Self {
        match value {
            serde_json::Value::Null => {
                JsonValue::Null
            },

            serde_json::Value::Bool(b) => {
                JsonValue::Bool(b)
            },

            serde_json::Value::Number(n) => {
                JsonValue::Number(n.to_string())
            },

            serde_json::Value::String(s) => {
                JsonValue::String(s.clone())
            },

            serde_json::Value::Array(arr) => {
                JsonValue::Array(arr.into_iter().map(From::from).collect())
            },

            serde_json::Value::Object(obj) => {
                JsonValue::Object(obj.into_iter().map(|(k, v)| {
                    (k, From::from(v))
                }).collect())
            },
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Span {
    start: usize,
    end: usize,
}

#[derive(Debug, Clone)]
struct FormattedNode {
    value: JsonValue,
    span: Span,                    // Position in original text
    leading_ws: String,            // Whitespace before the value
    trailing_ws: String,           // Whitespace after the value (before comma/closing bracket)
    separator_ws: Option<String>,  // Whitespace after colon (for object entries)
}

#[derive(Debug, Clone)]
struct FormatPreferences {
    indent: String,          // Detected indent (e.g., "  " or "    ")
    line_ending: String,     // "\n" or "\r\n"
    object_spacing: bool,    // Whether to use spaces around colons
    array_spacing: bool,     // Whether to use spaces after commas
    trailing_newline: bool,  // Whether to add a trailing newline at the end
}

impl Default for FormatPreferences {
    fn default() -> Self {
        Self {
            indent: "  ".to_string(),
            line_ending: "\n".to_string(),
            object_spacing: true,
            array_spacing: true,
            trailing_newline: false,
        }
    }
}

pub struct JsonFormatter {
    original: String,
    root: Option<FormattedNode>,
    format_prefs: FormatPreferences,
}

// Token types for the tokenizer
#[derive(Debug, Clone, PartialEq, Eq)]
enum Token {
    LeftBrace,      // {
    RightBrace,     // }
    LeftBracket,    // [
    RightBracket,   // ]
    Colon,          // :
    Comma,          // ,
    String(String), // String value (without quotes)
    Number(String), // Number as string
    Bool(bool),     // true/false
    Null,           // null
    Whitespace(String), // Any whitespace
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TokenWithSpan {
    token: Token,
    span: Span,
}

// Tokenizer implementation
struct Tokenizer<'a> {
    input: &'a str,
    position: usize,
}

impl<'a> Tokenizer<'a> {
    fn new(input: &'a str) -> Self {
        Self { input, position: 0 }
    }

    fn current_char(&self) -> Option<char> {
        self.input.chars().nth(self.position)
    }

    fn peek_char(&self, offset: usize) -> Option<char> {
        self.input.chars().nth(self.position + offset)
    }

    fn advance(&mut self) -> Option<char> {
        let ch = self.current_char();
        if ch.is_some() {
            self.position += ch.unwrap().len_utf8();
        }
        ch
    }

    fn skip_whitespace(&mut self) -> Option<String> {
        let start = self.position;
        while let Some(ch) = self.current_char() {
            if ch.is_whitespace() {
                self.advance();
            } else {
                break;
            }
        }
        
        if self.position > start {
            Some(self.input[start..self.position].to_string())
        } else {
            None
        }
    }

    fn read_string(&mut self) -> Result<String, Error> {
        // Assumes we're at the opening quote
        self.advance(); // Skip opening quote
        
        let mut result = String::new();
        let mut escaped = false;
        
        while let Some(ch) = self.current_char() {
            if escaped {
                match ch {
                    '"' | '\\' | '/' => result.push(ch),
                    'b' => result.push('\u{0008}'),
                    'f' => result.push('\u{000C}'),
                    'n' => result.push('\n'),
                    'r' => result.push('\r'),
                    't' => result.push('\t'),
                    'u' => {
                        // Handle Unicode escape
                        self.advance(); // Skip 'u'
                        let mut code_point = String::new();
                        for _ in 0..4 {
                            if let Some(hex_char) = self.current_char() {
                                code_point.push(hex_char);
                                self.advance();
                            } else {
                                return Err(Error::InvalidSyntax("Incomplete Unicode escape".to_string()));
                            }
                        }
                        
                        if let Ok(code) = u32::from_str_radix(&code_point, 16) {
                            if let Some(unicode_char) = char::from_u32(code) {
                                result.push(unicode_char);
                            } else {
                                return Err(Error::InvalidSyntax("Invalid Unicode code point".to_string()));
                            }
                        } else {
                            return Err(Error::InvalidSyntax("Invalid Unicode escape".to_string()));
                        }
                        escaped = false;
                        continue;
                    }
                    _ => return Err(Error::InvalidSyntax(format!("Invalid escape sequence: \\{}", ch))),
                }
                escaped = false;
                self.advance();
            } else if ch == '\\' {
                escaped = true;
                self.advance();
            } else if ch == '"' {
                self.advance(); // Skip closing quote
                return Ok(result);
            } else {
                result.push(ch);
                self.advance();
            }
        }
        
        Err(Error::InvalidSyntax("Unterminated string".to_string()))
    }

    fn read_number(&mut self) -> String {
        let start = self.position;
        
        // Optional minus
        if self.current_char() == Some('-') {
            self.advance();
        }
        
        // Integer part
        if self.current_char() == Some('0') {
            self.advance();
        } else {
            // Read digits
            while let Some(ch) = self.current_char() {
                if ch.is_ascii_digit() {
                    self.advance();
                } else {
                    break;
                }
            }
        }
        
        // Fractional part
        if self.current_char() == Some('.') {
            self.advance();
            while let Some(ch) = self.current_char() {
                if ch.is_ascii_digit() {
                    self.advance();
                } else {
                    break;
                }
            }
        }
        
        // Exponent part
        if let Some(ch) = self.current_char() {
            if ch == 'e' || ch == 'E' {
                self.advance();
                if let Some(sign) = self.current_char() {
                    if sign == '+' || sign == '-' {
                        self.advance();
                    }
                }
                while let Some(ch) = self.current_char() {
                    if ch.is_ascii_digit() {
                        self.advance();
                    } else {
                        break;
                    }
                }
            }
        }
        
        self.input[start..self.position].to_string()
    }

    fn read_literal(&mut self, literal: &str) -> bool {
        let chars: Vec<char> = literal.chars().collect();
        
        for (i, expected_ch) in chars.iter().enumerate() {
            if self.peek_char(i) != Some(*expected_ch) {
                return false;
            }
        }
        
        // Advance past the literal
        for _ in 0..chars.len() {
            self.advance();
        }
        
        true
    }

    fn next_token(&mut self) -> Result<Option<TokenWithSpan>, Error> {
        let start = self.position;
        
        match self.current_char() {
            None => Ok(None),
            Some(ch) => {
                let token = match ch {
                    '{' => {
                        self.advance();
                        Token::LeftBrace
                    },

                    '}' => {
                        self.advance();
                        Token::RightBrace
                    },

                    '[' => {
                        self.advance();
                        Token::LeftBracket
                    },

                    ']' => {
                        self.advance();
                        Token::RightBracket
                    },

                    ':' => {
                        self.advance();
                        Token::Colon
                    },

                    ',' => {
                        self.advance();
                        Token::Comma
                    },

                    '"' => {
                        let string_val = self.read_string()?;
                        Token::String(string_val)
                    },

                    't' if self.read_literal("true") => {
                        Token::Bool(true)
                    },

                    'f' if self.read_literal("false") => {
                        Token::Bool(false)
                    },

                    'n' if self.read_literal("null") => {
                        Token::Null
                    },

                    ch if ch.is_whitespace() => {
                        if let Some(ws) = self.skip_whitespace() {
                            Token::Whitespace(ws)
                        } else {
                            return Err(Error::InvalidSyntax("Failed to read whitespace".to_string()));
                        }
                    },

                    '-' | '0'..='9' => {
                        let number_str = self.read_number();
                        Token::Number(number_str)
                    },

                    _ => {
                        return Err(Error::InvalidSyntax(format!("Unexpected character: {}", ch)))
                    },
                };
                
                Ok(Some(TokenWithSpan {
                    token,
                    span: Span { start, end: self.position },
                }))
            }
        }
    }

    fn tokenize(mut self) -> Result<Vec<TokenWithSpan>, Error> {
        let mut tokens = Vec::new();
        
        while let Some(token) = self.next_token()? {
            tokens.push(token);
        }
        
        Ok(tokens)
    }
}

// Parser implementation
struct Parser {
    tokens: Vec<TokenWithSpan>,
    position: usize,
}

impl Parser {
    fn new(tokens: Vec<TokenWithSpan>) -> Self {
        Self {
            tokens,
            position: 0,
        }
    }

    fn current_token(&self) -> Option<&TokenWithSpan> {
        self.tokens.get(self.position)
    }

    fn advance(&mut self) -> Option<&TokenWithSpan> {
        let token = self.tokens.get(self.position);
        if token.is_some() {
            self.position += 1;
        }
        token
    }

    fn consume_whitespace(&mut self) -> String {
        let mut ws = String::new();
        while let Some(token) = self.current_token() {
            if let Token::Whitespace(s) = &token.token {
                ws.push_str(s);
                self.advance();
            } else {
                break;
            }
        }
        ws
    }

    fn expect_token(&mut self, expected: &Token) -> Result<TokenWithSpan, Error> {
        self.consume_whitespace();
        
        if let Some(token) = self.current_token() {
            if std::mem::discriminant(&token.token) == std::mem::discriminant(expected) {
                let token = token.clone();
                self.advance();
                Ok(token)
            } else {
                Err(Error::InvalidSyntax(format!("Expected {:?}, found {:?}", expected, token.token)))
            }
        } else {
            Err(Error::InvalidSyntax("Unexpected end of input".to_string()))
        }
    }

    fn parse_value(&mut self) -> Result<FormattedNode, Error> {
        let leading_ws = self.consume_whitespace();
        
        if let Some(token) = self.current_token() {
            let start_span = token.span;
            
            let (value, end_span) = match &token.token {
                Token::String(s) => {
                    let val = JsonValue::String(s.clone());
                    let span = token.span;
                    self.advance();
                    (val, span)
                },

                Token::Number(n) => {
                    let val = JsonValue::Number(n.clone());
                    let span = token.span;
                    self.advance();
                    (val, span)
                },

                Token::Bool(b) => {
                    let val = JsonValue::Bool(*b);
                    let span = token.span;
                    self.advance();
                    (val, span)
                },

                Token::Null => {
                    let val = JsonValue::Null;
                    let span = token.span;
                    self.advance();
                    (val, span)
                },

                Token::LeftBrace => {
                    self.advance(); // consume '{'
                    let (object_val, end_span) = self.parse_object()?;
                    (object_val, end_span)
                },

                Token::LeftBracket => {
                    self.advance(); // consume '['
                    let (array_val, end_span) = self.parse_array()?;
                    (array_val, end_span)
                },

                _ => {
                    return Err(Error::InvalidSyntax(format!("Unexpected token: {:?}", token.token)))
                },
            };
            
            let trailing_ws = if let Some(next_token) = self.current_token() {
                if matches!(next_token.token, Token::Comma | Token::RightBrace | Token::RightBracket) {
                    self.consume_whitespace()
                } else {
                    String::new()
                }
            } else {
                String::new()
            };

            Ok(FormattedNode {
                value,
                span: Span {
                    start: start_span.start,
                    end: end_span.end,
                },
                leading_ws,
                trailing_ws,
                separator_ws: None,
            })
        } else {
            Err(Error::InvalidSyntax("Expected value".to_string()))
        }
    }

    fn parse_object(&mut self) -> Result<(JsonValue, Span), Error> {
        let mut entries = Vec::new();
        
        self.consume_whitespace();
        
        // Check for empty object
        if let Some(token) = self.current_token() {
            if matches!(token.token, Token::RightBrace) {
                let end_span = token.span;
                self.advance();
                return Ok((JsonValue::Object(entries), end_span));
            }
        }
        
        loop {
            // Parse key
            let _ = self.consume_whitespace();
            
            if let Some(token) = self.current_token().cloned() {
                if let Token::String(key) = &token.token {
                    self.advance();
                    
                    // Parse colon
                    let _ = self.consume_whitespace();
                    self.expect_token(&Token::Colon)?;
                    let post_colon_ws = self.consume_whitespace();
                    
                    // Parse value
                    let mut value_node = self.parse_value()?;
                    value_node.separator_ws = Some(post_colon_ws);
                    
                    // Convert FormattedNode to JsonValue for now
                    // In a full implementation, we'd preserve the FormattedNode
                    entries.push((key.clone(), value_node.value));
                    
                    // Check for comma or closing brace
                    self.consume_whitespace();
                    
                    if let Some(token) = self.current_token() {
                        match &token.token {
                            Token::Comma => {
                                self.advance();
                                // Continue to next entry
                            },

                            Token::RightBrace => {
                                let end_span = token.span;
                                self.advance();
                                return Ok((JsonValue::Object(entries), end_span));
                            },

                            _ => {
                                return Err(Error::InvalidSyntax("Expected ',' or '}'".to_string()))
                            },
                        }
                    } else {
                        return Err(Error::InvalidSyntax("Unexpected end of input".to_string()));
                    }
                } else {
                    return Err(Error::InvalidSyntax("Expected string key".to_string()));
                }
            } else {
                return Err(Error::InvalidSyntax("Unexpected end of input in object".to_string()));
            }
        }
    }

    fn parse_array(&mut self) -> Result<(JsonValue, Span), Error> {
        let mut elements = Vec::new();
        
        self.consume_whitespace();
        
        // Check for empty array
        if let Some(token) = self.current_token() {
            if matches!(token.token, Token::RightBracket) {
                let end_span = token.span;
                self.advance();
                return Ok((JsonValue::Array(elements), end_span));
            }
        }
        
        loop {
            // Parse value
            let value_node = self.parse_value()?;
            elements.push(value_node.value);
            
            // Check for comma or closing bracket
            self.consume_whitespace();
            
            if let Some(token) = self.current_token() {
                match &token.token {
                    Token::Comma => {
                        self.advance();
                        // Continue to next element
                    },

                    Token::RightBracket => {
                        let end_span = token.span;
                        self.advance();
                        return Ok((JsonValue::Array(elements), end_span));
                    },

                                                _ => {
                                return Err(Error::InvalidSyntax("Expected ',' or ']'".to_string()))
                            },
                        }
                    } else {
                        return Err(Error::InvalidSyntax("Unexpected end of input".to_string()));
                    }
        }
    }

    fn parse(&mut self) -> Result<FormattedNode, Error> {
        self.parse_value()
    }
}

// Format detection implementation
fn detect_format_preferences(tokens: &[TokenWithSpan], original: &str) -> FormatPreferences {
    let mut prefs = FormatPreferences::default();
    
    // Detect indentation
    let mut indent_samples = Vec::new();
    let mut current_depth: usize = 0;
    
    for (i, token) in tokens.iter().enumerate() {
        match &token.token {
            Token::LeftBrace | Token::LeftBracket => {
                current_depth += 1;
            }
            Token::RightBrace | Token::RightBracket => {
                current_depth = current_depth.saturating_sub(1);
            }
            Token::Whitespace(ws) => {
                if ws.contains('\n') {
                    // Look at the next non-whitespace token
                    if let Some(next_token) = tokens.get(i + 1) {
                        if !matches!(next_token.token, Token::RightBrace | Token::RightBracket) {
                            // Extract the indentation after the newline
                            if let Some(last_newline) = ws.rfind('\n') {
                                let indent = &ws[last_newline + 1..];
                                if !indent.is_empty() && current_depth > 0 {
                                    indent_samples.push((indent.to_string(), current_depth));
                                }
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }
    
    // Determine the most common indent per level
    if !indent_samples.is_empty() {
        // Group by depth and find most common indent
        let mut indent_by_depth: std::collections::HashMap<usize, Vec<String>> = std::collections::HashMap::new();
        for (indent, depth) in indent_samples {
            indent_by_depth.entry(depth).or_default().push(indent);
        }
        
        // Find the indent unit (difference between level 1 and level 0)
        if let Some(level1_indents) = indent_by_depth.get(&1) {
            // Find most common indent at level 1
            let mut indent_counts: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
            for indent in level1_indents {
                *indent_counts.entry(indent).or_default() += 1;
            }
            
            if let Some((most_common, _)) = indent_counts.iter().max_by_key(|(_, count)| *count) {
                prefs.indent = most_common.to_string();
            }
        }
    }
    
    // Detect object spacing (spaces around colons)
    let mut colon_spacing_samples = Vec::new();
    for i in 0..tokens.len() {
        if matches!(tokens[i].token, Token::Colon) {
            let has_space_before = i > 0 && matches!(tokens[i-1].token, Token::Whitespace(_));
            let has_space_after = i + 1 < tokens.len() && matches!(tokens[i+1].token, Token::Whitespace(_));
            colon_spacing_samples.push(has_space_before || has_space_after);
        }
    }
    
    if !colon_spacing_samples.is_empty() {
        let spaces_count = colon_spacing_samples.iter().filter(|&&x| x).count();
        prefs.object_spacing = spaces_count > colon_spacing_samples.len() / 2;
    }
    
    // Detect array spacing (spaces after commas)
    let mut comma_spacing_samples = Vec::new();
    for i in 0..tokens.len() {
        if matches!(tokens[i].token, Token::Comma) {
            let has_space_after = i + 1 < tokens.len() && matches!(tokens[i+1].token, Token::Whitespace(_));
            comma_spacing_samples.push(has_space_after);
        }
    }
    
    if !comma_spacing_samples.is_empty() {
        let spaces_count = comma_spacing_samples.iter().filter(|&&x| x).count();
        prefs.array_spacing = spaces_count > comma_spacing_samples.len() / 2;
    }
    
    // Detect line ending style
    if original.contains("\r\n") {
        prefs.line_ending = "\r\n".to_string();
    }
    
    // Detect trailing newline
    if original.ends_with('\n') || original.ends_with("\r\n") {
        prefs.trailing_newline = true;
    }
    
    prefs
}

impl JsonFormatter {
    pub fn from(input: &str) -> Result<Self, Error> {
        let tokenizer
            = Tokenizer::new(input);
        let tokens
            = tokenizer.tokenize()?;

        let format_prefs
            = detect_format_preferences(&tokens, input);

        let mut parser
            = Parser::new(tokens);
        let root
            = parser.parse()?;

        Ok(Self {
            original: input.to_string(),
            root: Some(root),
            format_prefs,
        })
    }

    pub fn set<P: Into<JsonPath>>(&mut self, path: P, value: JsonValue) -> Result<(), Error> {
        self.set_path(&path.into(), value)
    }

    pub fn set_path(&mut self, path: &JsonPath, value: JsonValue) -> Result<(), Error> {
        if let Some(root) = &mut self.root {
            if path.is_empty() {
                root.value = value;
            } else {
                set_at_path(&mut root.value, path.segments(), value, true)?;
            }
        }
        
        Ok(())
    }

    pub fn update<P: Into<JsonPath>>(&mut self, path: P, value: JsonValue) -> Result<(), Error> {
        self.update_path(&path.into(), value)
    }

    pub fn update_path(&mut self, path: &JsonPath, value: JsonValue) -> Result<(), Error> {
        if let Some(root) = &mut self.root {
            if path.is_empty() {
                // Replace the entire root
                root.value = value;
            } else {
                set_at_path(&mut root.value, path.segments(), value, false)?;
            }
        }

        Ok(())
    }

    pub fn remove<P: Into<JsonPath>>(&mut self, path: P) -> Result<(), Error> {
        self.remove_path(&path.into())
    }

    pub fn remove_path(&mut self, path: &JsonPath) -> Result<(), Error> {
        self.set_path(path, JsonValue::Undefined)
    }

    pub fn to_string(&self) -> String {
        if let Some(root) = &self.root {
            let mut result = self.serialize_node(root, 0);
            
            // Add trailing newline if the original had one
            if self.format_prefs.trailing_newline {
                result.push_str(&self.format_prefs.line_ending);
            }
            
            result
        } else {
            String::new()
        }
    }

    fn serialize_node(&self, node: &FormattedNode, depth: usize) -> String {
        // For unchanged nodes, we could use the original text from spans
        // For now, we'll generate fresh text with formatting
        self.serialize_value(&node.value, depth)
    }

    fn serialize_value(&self, value: &JsonValue, depth: usize) -> String {
        match value {
            JsonValue::Null => {
                "null".to_string()
            },

            JsonValue::Bool(b) => {
                b.to_string()
            },

            JsonValue::Number(n) => {
                n.clone()
            },

            JsonValue::String(s) => {
                escape_string(s)
            },

            JsonValue::Array(elements) => {
                // Filter out Undefined values
                let valid_elements: Vec<_> = elements.iter()
                    .filter(|v| !matches!(v, JsonValue::Undefined))
                    .collect();
                    
                if valid_elements.is_empty() {
                    "[]".to_string()
                } else {
                    let mut result = "[".to_string();
                    result.push_str(&self.format_prefs.line_ending);
                    
                    for (i, elem) in valid_elements.iter().enumerate() {
                        result.push_str(&self.format_prefs.indent.repeat(depth + 1));
                        result.push_str(&self.serialize_value(elem, depth + 1));
                        
                        if i < valid_elements.len() - 1 {
                            result.push(',');
                        }
                        
                        result.push_str(&self.format_prefs.line_ending);
                    }
                    
                    result.push_str(&self.format_prefs.indent.repeat(depth));
                    result.push(']');
                    result
                }
            },

            JsonValue::Object(entries) => {
                // Filter out entries with Undefined values
                let valid_entries: Vec<_> = entries.iter()
                    .filter(|(_, v)| !matches!(v, JsonValue::Undefined))
                    .collect();
                    
                if valid_entries.is_empty() {
                    "{}".to_string()
                } else {
                    let mut result = "{".to_string();
                    result.push_str(&self.format_prefs.line_ending);
                    
                    for (i, (key, value)) in valid_entries.iter().enumerate() {
                        result.push_str(&self.format_prefs.indent.repeat(depth + 1));
                        result.push_str(&escape_string(key));
                        result.push(':');
                        
                        if self.format_prefs.object_spacing {
                            result.push(' ');
                        }
                        
                        result.push_str(&self.serialize_value(value, depth + 1));
                        
                        if i < valid_entries.len() - 1 {
                            result.push(',');
                        }
                        
                        result.push_str(&self.format_prefs.line_ending);
                    }
                    
                    result.push_str(&self.format_prefs.indent.repeat(depth));
                    result.push('}');
                    result
                }
            },

            JsonValue::Undefined => {
                unreachable!()
            },
        }
    }
}

pub fn escape_string(s: &str) -> String {
    let mut result
        = String::with_capacity(s.len() + 2);

    result.push('"');

    for ch in s.chars() {
        match ch {
            '"' => {
                result.push_str("\\\"")
            },

            '\\' => {
                result.push_str("\\\\")
            },

            '\u{0008}' => {
                result.push_str("\\b")
            },

            '\u{000C}' => {
                result.push_str("\\f")
            },

            '\n' => {
                result.push_str("\\n")
            },

            '\r' => {
                result.push_str("\\r")
            },

            '\t' => {
                result.push_str("\\t")
            },

            ch if ch.is_control() => {
                result.push_str(&format!("\\u{:04x}", ch as u32))
            },

            ch => {
                result.push(ch)
            },
        }
    }

    result.push('"');

    result
}

// Move set_at_path outside of impl block as a standalone function
fn set_at_path(current: &mut JsonValue, path: &[String], value: JsonValue, create_if_missing: bool) -> Result<bool, Error> {
    if path.is_empty() {
        *current = value;
        return Ok(false);
    }
    
    let key = &path[0];
    let remaining_path = &path[1..];
    
    match current {
        JsonValue::Object(entries) => {
            // Try to find existing key
            for i in 0..entries.len() {
                if entries[i].0 == *key {
                    if remaining_path.len() > 0 || value != JsonValue::Undefined {
                        if !set_at_path(&mut entries[i].1, remaining_path, value, create_if_missing)? {
                            return Ok(false);
                        }
                    }

                    // Remove the entry
                    entries.remove(i);
                    return Ok(entries.is_empty());
                }
            }

            // Key not found
            if value == JsonValue::Undefined || !create_if_missing {
                return Ok(false);
            }

            // Add new entry
            if remaining_path.is_empty() {
                entries.push((key.clone(), value));
            } else {
                entries.push((key.clone(), if remaining_path[0].parse::<usize>().is_ok() {
                    JsonValue::Array(vec![])
                } else {
                    JsonValue::Object(vec![])
                }));

                let last_idx
                    = entries.len() - 1;

                set_at_path(&mut entries[last_idx].1, remaining_path, value, create_if_missing)?;
            }

            Ok(false)
        },

        JsonValue::Array(elements) => {
            if let Ok(index) = key.parse::<usize>() {
                if remaining_path.is_empty() && value == JsonValue::Undefined {
                    // Remove the element if it exists
                    if index < elements.len() {
                        elements.remove(index);
                    }

                    return Ok(elements.is_empty());
                }

                if !create_if_missing && index >= elements.len() {
                    return Ok(false);
                }
                
                // Extend array if needed
                while elements.len() <= index {
                    elements.push(JsonValue::Null);
                }

                if remaining_path.is_empty() {
                    elements[index] = value;
                } else {
                    if set_at_path(&mut elements[index], remaining_path, value, create_if_missing)? {
                        elements.remove(index);
                        return Ok(elements.is_empty());
                    }
                }

                Ok(false)
            } else {
                return Err(Error::InvalidArrayAccess(key.clone()));
            }
        },

        _ => {
            if remaining_path.is_empty() {
                *current = value;
            } else {
                return Err(Error::InvalidPrimitiveNavigation);
            }

            Ok(false)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use indoc::indoc;

    #[test]
    fn test_tokenizer_simple_object() {
        let input = r#"{"key": "value"}"#;
        let tokenizer = Tokenizer::new(input);
        let tokens = tokenizer.tokenize().unwrap();
        
        // Debug print to see what tokens we're getting
        for (i, token) in tokens.iter().enumerate() {
            println!("Token {}: {:?}", i, token.token);
        }
        
        assert_eq!(tokens, vec![
            TokenWithSpan { token: Token::LeftBrace, span: Span { start: 0, end: 1 } },
            TokenWithSpan { token: Token::String("key".to_string()), span: Span { start: 1, end: 6 } },
            TokenWithSpan { token: Token::Colon, span: Span { start: 6, end: 7 } },
            TokenWithSpan { token: Token::Whitespace(" ".to_string()), span: Span { start: 7, end: 8 } },
            TokenWithSpan { token: Token::String("value".to_string()), span: Span { start: 8, end: 15 } },
            TokenWithSpan { token: Token::RightBrace, span: Span { start: 15, end: 16 } },
        ]);
    }

    #[test]
    fn test_tokenizer_with_whitespace() {
        let input = r#"{ "key" : "value" }"#;
        let tokenizer = Tokenizer::new(input);
        let tokens = tokenizer.tokenize().unwrap();
        
        assert_eq!(tokens, vec![
            TokenWithSpan { token: Token::LeftBrace, span: Span { start: 0, end: 1 } },
            TokenWithSpan { token: Token::Whitespace(" ".to_string()), span: Span { start: 1, end: 2 } },
            TokenWithSpan { token: Token::String("key".to_string()), span: Span { start: 2, end: 7 } },
            TokenWithSpan { token: Token::Whitespace(" ".to_string()), span: Span { start: 7, end: 8 } },
            TokenWithSpan { token: Token::Colon, span: Span { start: 8, end: 9 } },
            TokenWithSpan { token: Token::Whitespace(" ".to_string()), span: Span { start: 9, end: 10 } },
            TokenWithSpan { token: Token::String("value".to_string()), span: Span { start: 10, end: 17 } },
            TokenWithSpan { token: Token::Whitespace(" ".to_string()), span: Span { start: 17, end: 18 } },
            TokenWithSpan { token: Token::RightBrace, span: Span { start: 18, end: 19 } },
        ]);
    }

    #[test]
    fn test_tokenizer_array() {
        let input = r#"[1, 2, 3]"#;
        let tokenizer = Tokenizer::new(input);
        let tokens = tokenizer.tokenize().unwrap();
        
        assert_eq!(tokens, vec![
            TokenWithSpan { token: Token::LeftBracket, span: Span { start: 0, end: 1 } },
            TokenWithSpan { token: Token::Number("1".to_string()), span: Span { start: 1, end: 2 } },
            TokenWithSpan { token: Token::Comma, span: Span { start: 2, end: 3 } },
            TokenWithSpan { token: Token::Whitespace(" ".to_string()), span: Span { start: 3, end: 4 } },
            TokenWithSpan { token: Token::Number("2".to_string()), span: Span { start: 4, end: 5 } },
            TokenWithSpan { token: Token::Comma, span: Span { start: 5, end: 6 } },
            TokenWithSpan { token: Token::Whitespace(" ".to_string()), span: Span { start: 6, end: 7 } },
            TokenWithSpan { token: Token::Number("3".to_string()), span: Span { start: 7, end: 8 } },
            TokenWithSpan { token: Token::RightBracket, span: Span { start: 8, end: 9 } },
        ]);
    }

    #[test]
    fn test_tokenizer_literals() {
        let input = indoc! {r#"{
          "bool": true,
          "null": null,
          "false": false
        }"#};
        let tokenizer = Tokenizer::new(input);
        let tokens = tokenizer.tokenize().unwrap();
        
        // Let's check some key tokens to verify literals are parsed correctly
        let mut found_true = false;
        let mut found_false = false;
        let mut found_null = false;
        
        for token in &tokens {
            match &token.token {
                Token::Bool(true) => found_true = true,
                Token::Bool(false) => found_false = true,
                Token::Null => found_null = true,
                _ => {}
            }
        }
        
        assert!(found_true, "Should find true literal");
        assert!(found_false, "Should find false literal");
        assert!(found_null, "Should find null literal");
    }

    #[test]
    fn test_tokenizer_numbers() {
        let input = indoc! {r#"[
          0,
          -1,
          3.14,
          1e10,
          -2.5e-3
        ]"#};
        let tokenizer = Tokenizer::new(input);
        let tokens = tokenizer.tokenize().unwrap();
        
        // Extract all number tokens
        let numbers: Vec<_> = tokens.iter()
            .filter_map(|t| match &t.token {
                Token::Number(n) => Some(n.as_str()),
                _ => None,
            })
            .collect();
        
        assert_eq!(numbers, vec!["0", "-1", "3.14", "1e10", "-2.5e-3"]);
    }

    #[test]
    fn test_tokenizer_string_escapes() {
        let input = indoc! {r#"[
          "hello\nworld",
          "quote\"test",
          "backslash\\test",
          "unicode\u0041"
        ]"#};
        let tokenizer = Tokenizer::new(input);
        let tokens = tokenizer.tokenize().unwrap();

        // Extract all string tokens
        let strings: Vec<_> = tokens.iter()
            .filter_map(|t| match &t.token {
                Token::String(s) => Some(s.as_str()),
                _ => None,
            })
            .collect();
        
        assert_eq!(strings[0], "hello\nworld");
        assert_eq!(strings[1], "quote\"test");
        assert_eq!(strings[2], "backslash\\test");
        assert_eq!(strings[3], "unicodeA");
    }

    #[test]
    fn test_json_formatter_basic() {
        let input = r#"{"name": "test", "value": 42}"#;
        let mut formatter = JsonFormatter::from(input).unwrap();
        
        // Test setting a value
        formatter.set(
            ["name"],
            JsonValue::String("updated".to_string())
        ).unwrap();
        
        // The output should preserve the format
        let output = formatter.to_string();

        assert_eq!(output, indoc! {r#"{
          "name": "updated",
          "value": 42
        }"#});
    }

    #[test]
    fn test_json_formatter_nested() {
        let input = indoc! {r#"
            {
              "user": {
                "name": "John",
                "age": 30
              }
            }"#};
        let mut formatter = JsonFormatter::from(input).unwrap();
        
        // Test setting a nested value
        formatter.set(
            ["user", "name"],
            JsonValue::String("Jane".to_string())
        ).unwrap();
        
        let output = formatter.to_string();

        assert_eq!(output, indoc! {r#"{
          "user": {
            "name": "Jane",
            "age": 30
          }
        }"#});
    }

    #[test]
    fn test_json_formatter_array() {
        let input = r#"{"items": [1, 2, 3]}"#;
        let mut formatter = JsonFormatter::from(input).unwrap();
        
        // Test setting an array element
        formatter.set(
            ["items", "1"],
            JsonValue::Number("42".to_string())
        ).unwrap();
        
        let output = formatter.to_string();

        assert_eq!(output, indoc! {r#"{
          "items": [
            1,
            42,
            3
          ]
        }"#});
    }

    #[test]
    fn test_format_detection() {
        let formatter = JsonFormatter::from(indoc!{r#"{
          "key": "value"
        }"#}).unwrap();

        assert_eq!(formatter.format_prefs.indent, "  ");
        
        let formatter = JsonFormatter::from(indoc!{r#"{
            "key": "value"
        }"#}).unwrap();

        assert_eq!(formatter.format_prefs.indent, "    ");
    }

    #[test]
    fn test_adding_new_fields() {
        let input = r#"{"existing": "value"}"#;
        let mut formatter = JsonFormatter::from(input).unwrap();
        
        // Add a new field
        formatter.set(
            ["new_field"],
            JsonValue::String("new value".to_string())
        ).unwrap();
        
        let output = formatter.to_string();

        assert_eq!(output, indoc! {r#"{
          "existing": "value",
          "new_field": "new value"
        }"#});
    }

    #[test]
    fn test_adding_new_object_at_root() {
        let input = r#"{"name": "test"}"#;
        let mut formatter = JsonFormatter::from(input).unwrap();
        
        // Add a new object at root level
        formatter.set(
            ["address"],
            JsonValue::Object(vec![
                ("street".to_string(), JsonValue::String("123 Main St".to_string())),
                ("city".to_string(), JsonValue::String("New York".to_string())),
                ("zip".to_string(), JsonValue::String("10001".to_string())),
            ])
        ).unwrap();
        
        let output = formatter.to_string();

        assert_eq!(output, indoc! {r#"{
          "name": "test",
          "address": {
            "street": "123 Main St",
            "city": "New York",
            "zip": "10001"
          }
        }"#});
    }

    #[test]
    fn test_adding_new_array_at_root() {
        let input = r#"{"id": 1}"#;
        let mut formatter = JsonFormatter::from(input).unwrap();
        
        // Add a new array at root level
        formatter.set(
            ["tags"],
            JsonValue::Array(vec![
                JsonValue::String("rust".to_string()),
                JsonValue::String("json".to_string()),
                JsonValue::String("parser".to_string()),
            ])
        ).unwrap();
        
        let output = formatter.to_string();

        assert_eq!(output, indoc! {r#"{
          "id": 1,
          "tags": [
            "rust",
            "json",
            "parser"
          ]
        }"#});
    }

    #[test]
    fn test_adding_nested_object() {
        let input = r#"{"user": {"name": "John"}}"#;
        let mut formatter = JsonFormatter::from(input).unwrap();
        
        // Add a nested object
        formatter.set(
            ["user", "preferences"],
            JsonValue::Object(vec![
                ("theme".to_string(), JsonValue::String("dark".to_string())),
                ("notifications".to_string(), JsonValue::Bool(true)),
            ])
        ).unwrap();
        
        let output = formatter.to_string();

        assert_eq!(output, indoc! {r#"{
          "user": {
            "name": "John",
            "preferences": {
              "theme": "dark",
              "notifications": true
            }
          }
        }"#});
    }

    #[test]
    fn test_adding_nested_array() {
        let input = r#"{"data": {"values": [1, 2]}}"#;
        let mut formatter = JsonFormatter::from(input).unwrap();
        
        // Add a nested array
        formatter.set(
            ["data", "labels"],
            JsonValue::Array(vec![
                JsonValue::String("first".to_string()),
                JsonValue::String("second".to_string()),
            ])
        ).unwrap();
        
        let output = formatter.to_string();

        assert_eq!(output, indoc! {r#"{
          "data": {
            "values": [
              1,
              2
            ],
            "labels": [
              "first",
              "second"
            ]
          }
        }"#});
    }

    #[test]
    fn test_adding_multiple_types() {
        let input = r#"{"base": "value"}"#;
        let mut formatter = JsonFormatter::from(input).unwrap();
        
        // Add multiple fields of different types using both approaches
        formatter.set(
            ["string_field"],
            JsonValue::String("hello".to_string())
        ).unwrap();
        
        formatter.set(
            ["number_field"],
            JsonValue::Number("42".to_string())
        ).unwrap();
        
        formatter.set(
            ["bool_field"],
            JsonValue::Bool(true)
        ).unwrap();
        
        formatter.set(
            ["null_field"],
            JsonValue::Null
        ).unwrap();
        
        formatter.set(
            ["array_field"],
            JsonValue::Array(vec![
                JsonValue::Number("1".to_string()),
                JsonValue::Number("2".to_string()),
                JsonValue::Number("3".to_string()),
            ])
        ).unwrap();
        
        formatter.set(
            ["object_field"],
            JsonValue::Object(vec![
                ("nested".to_string(), JsonValue::String("value".to_string())),
            ])
        ).unwrap();
        
        let output = formatter.to_string();

        assert_eq!(output, indoc! {r#"{
          "base": "value",
          "string_field": "hello",
          "number_field": 42,
          "bool_field": true,
          "null_field": null,
          "array_field": [
            1,
            2,
            3
          ],
          "object_field": {
            "nested": "value"
          }
        }"#});
    }

    #[test]
    fn test_deeply_nested_creation() {
        let input = r#"{}"#;
        let mut formatter = JsonFormatter::from(input).unwrap();
        
        // Create a deeply nested structure using vector
        formatter.set(
            ["level1", "level2", "level3", "level4"],
            JsonValue::String("deep value".to_string())
        ).unwrap();
        
        let output = formatter.to_string();

        assert_eq!(output, indoc! {r#"{
          "level1": {
            "level2": {
              "level3": {
                "level4": "deep value"
              }
            }
          }
        }"#});
    }

    #[test]
    fn test_mixed_array_object_creation() {
        let input = r#"{"root": {}}"#;
        let mut formatter = JsonFormatter::from(input).unwrap();
        
        // Create mixed structure with arrays and objects
        formatter.set(
            ["root", "users"],
            JsonValue::Array(vec![
                JsonValue::Object(vec![
                    ("name".to_string(), JsonValue::String("Alice".to_string())),
                    ("age".to_string(), JsonValue::Number("30".to_string())),
                ]),
                JsonValue::Object(vec![
                    ("name".to_string(), JsonValue::String("Bob".to_string())),
                    ("age".to_string(), JsonValue::Number("25".to_string())),
                ]),
            ])
        ).unwrap();
        
        formatter.set(
            ["root", "config"],
            JsonValue::Object(vec![
                ("enabled".to_string(), JsonValue::Bool(true)),
                ("options".to_string(), JsonValue::Array(vec![
                    JsonValue::String("opt1".to_string()),
                    JsonValue::String("opt2".to_string()),
                ])),
            ])
        ).unwrap();
        
        let output = formatter.to_string();

        assert_eq!(output, indoc! {r#"{
          "root": {
            "users": [
              {
                "name": "Alice",
                "age": 30
              },
              {
                "name": "Bob",
                "age": 25
              }
            ],
            "config": {
              "enabled": true,
              "options": [
                "opt1",
                "opt2"
              ]
            }
          }
        }"#});
    }

    #[test]
    fn test_empty_containers() {
        let input = r#"{"existing": 1}"#;
        let mut formatter = JsonFormatter::from(input).unwrap();
        
        // Add empty object and array
        formatter.set(["empty_object"], JsonValue::Object(vec![])).unwrap();
        formatter.set(["empty_array"], JsonValue::Array(vec![])).unwrap();
        
        let output = formatter.to_string();

        assert_eq!(output, indoc! {r#"{
          "existing": 1,
          "empty_object": {},
          "empty_array": []
        }"#});
    }

    #[test]
    fn test_array_index_creation() {
        let input = r#"{"data": []}"#;
        let mut formatter = JsonFormatter::from(input).unwrap();
        
        // Add items at specific array indices
        formatter.set(["data", "0"], JsonValue::String("first".to_string())).unwrap();
        formatter.set(["data", "2"], JsonValue::String("third".to_string())).unwrap();
        formatter.set(["data", "1"], JsonValue::String("second".to_string())).unwrap();
        
        let output = formatter.to_string();

        assert_eq!(output, indoc! {r#"{
          "data": [
            "first",
            "second",
            "third"
          ]
        }"#});
    }

    #[test]
    fn test_json_path_basic_dot_notation() {
        let path = JsonPath::from_file_string("foo.bar.baz").unwrap();
        assert_eq!(path.segments(), &["foo", "bar", "baz"]);
        assert_eq!(path.to_file_string(), "foo.bar.baz");
    }

    #[test]
    fn test_json_path_array_notation() {
        let path = JsonPath::from_file_string("foo[0]").unwrap();
        assert_eq!(path.segments(), &["foo", "0"]);
        assert_eq!(path.to_file_string(), "foo[0]");
        
        let path = JsonPath::from_file_string("foo[123]").unwrap();
        assert_eq!(path.segments(), &["foo", "123"]);
        assert_eq!(path.to_file_string(), "foo[123]");
    }

    #[test]
    fn test_json_path_string_bracket_notation() {
        let path = JsonPath::from_file_string(r#"foo["bar"]"#).unwrap();
        assert_eq!(path.segments(), &["foo", "bar"]);
        assert_eq!(path.to_file_string(), "foo.bar");
        
        let path = JsonPath::from_file_string(r#"foo['bar']"#).unwrap();
        assert_eq!(path.segments(), &["foo", "bar"]);
        assert_eq!(path.to_file_string(), "foo.bar");
    }

    #[test]
    fn test_json_path_complex() {
        let path = JsonPath::from_file_string(r#"users[0].name"#).unwrap();
        assert_eq!(path.segments(), &["users", "0", "name"]);
        assert_eq!(path.to_file_string(), "users[0].name");
        
        let path = JsonPath::from_file_string(r#"data["key-with-dash"][0].value"#).unwrap();
        assert_eq!(path.segments(), &["data", "key-with-dash", "0", "value"]);
        assert_eq!(path.to_file_string(), r#"data["key-with-dash"][0].value"#);
    }

    #[test]
    fn test_json_path_special_chars() {
        // Test keys that require bracket notation
        let path = JsonPath::from_file_string(r#"["key with spaces"]"#).unwrap();
        assert_eq!(path.segments(), &["key with spaces"]);
        assert_eq!(path.to_file_string(), r#"["key with spaces"]"#);
        
        let path = JsonPath::from_file_string(r#"["key.with.dots"]"#).unwrap();
        assert_eq!(path.segments(), &["key.with.dots"]);
        assert_eq!(path.to_file_string(), r#"["key.with.dots"]"#);
        
        // "123" is parsed as a string but serialized as a number when it's all digits
        let path = JsonPath::from_file_string(r#"["123"]"#).unwrap();
        assert_eq!(path.segments(), &["123"]);
        assert_eq!(path.to_file_string(), "[123]"); // Numeric strings are serialized as array indices
    }

    #[test]
    fn test_json_path_escapes() {
        let path = JsonPath::from_file_string(r#"["key\"with\"quotes"]"#).unwrap();
        assert_eq!(path.segments(), &[r#"key"with"quotes"#]);
        assert_eq!(path.to_file_string(), r#"["key\"with\"quotes"]"#);
        
        let path = JsonPath::from_file_string(r#"["key\\with\\backslashes"]"#).unwrap();
        assert_eq!(path.segments(), &[r#"key\with\backslashes"#]);
        assert_eq!(path.to_file_string(), r#"["key\\with\\backslashes"]"#);
    }

    #[test]
    fn test_json_path_empty() {
        let path = JsonPath::from_file_string("").unwrap();
        assert!(path.is_empty());
        assert_eq!(path.segments(), &[] as &[String]);
        assert_eq!(path.to_file_string(), "");
    }

    #[test]
    fn test_json_path_errors() {
        // Missing closing bracket
        assert!(JsonPath::from_file_string("foo[").is_err());
        assert!(JsonPath::from_file_string("foo[0").is_err());
        assert!(JsonPath::from_file_string(r#"foo["bar"#).is_err());
        
        // Invalid bracket content
        assert!(JsonPath::from_file_string("foo[]").is_err());
        assert!(JsonPath::from_file_string("foo[bar]").is_err()); // Unquoted string
    }

    #[test]
    fn test_json_formatter_with_json_path() {
        let input = r#"{"users": [{"name": "John"}, {"name": "Jane"}], "config": {"debug": false}}"#;
        let mut formatter = JsonFormatter::from(input).unwrap();
        
        // Test array access with bracket notation - keeping from_file_string to test parsing
        let path = JsonPath::from_file_string("users[0].name").unwrap();
        formatter.set_path(&path, JsonValue::String("Bob".to_string())).unwrap();
        let path = JsonPath::from_file_string("users[1].age").unwrap();
        formatter.set_path(&path, JsonValue::Number("25".to_string())).unwrap();
        
        // Test nested object access using simpler vector approach  
        formatter.set(
            ["config", "debug"],
            JsonValue::Bool(true)
        ).unwrap();
        formatter.set(
            ["config", "timeout"],
            JsonValue::Number("30".to_string())
        ).unwrap();
        
        let output = formatter.to_string();
        assert!(output.contains(r#""name": "Bob""#));
        assert!(output.contains(r#""age": 25"#));
        assert!(output.contains(r#""debug": true"#));
        assert!(output.contains(r#""timeout": 30"#));
    }

    #[test]
    fn test_json_formatter_bracket_notation_keys() {
        let input = r#"{}"#;
        let mut formatter = JsonFormatter::from(input).unwrap();
        
        // Test keys that require bracket notation in from_file_string
        let path = JsonPath::from_file_string(r#"["key with spaces"]"#).unwrap();
        formatter.set_path(&path, JsonValue::String("value1".to_string())).unwrap();
        
        // But can also be created directly with vector
        formatter.set(
            ["key-with-dash"],
            JsonValue::String("value2".to_string())
        ).unwrap();
        
        // Numeric string key
        formatter.set(
            ["123"],
            JsonValue::String("value3".to_string())
        ).unwrap();
        
        let output = formatter.to_string();
        assert!(output.contains(r#""key with spaces": "value1""#));
        assert!(output.contains(r#""key-with-dash": "value2""#));
        assert!(output.contains(r#""123": "value3""#));
    }

    #[test]
    fn test_json_path_roundtrip() {
        let test_cases = vec![
            "foo",
            "foo.bar",
            "foo[0]",
            "foo.bar[0]",
            r#"foo["bar"]"#,
            r#"["special-key"][0].value"#,
            r#"data["key with spaces"]["nested"]"#,
        ];
        
        for test_case in test_cases {
            let path = JsonPath::from_file_string(test_case).unwrap();
            let segments = path.segments().to_vec();
            let reconstructed = JsonPath::from_segments(segments.clone());
            
            // Check that segments match
            assert_eq!(path.segments(), reconstructed.segments(), 
                "Segments mismatch for path: {}", test_case);
        }
    }

    #[test]
    fn test_remove_object_property() {
        let input = r#"{"name": "test", "value": 42, "active": true}"#;
        let mut formatter = JsonFormatter::from(input).unwrap();
        
        // Remove a property
        formatter.set(["value"], JsonValue::Undefined).unwrap();
        
        let output = formatter.to_string();
        assert_eq!(output, indoc! {r#"{
          "name": "test",
          "active": true
        }"#});
    }

    #[test]
    fn test_remove_nested_property() {
        let input = indoc! {r#"{
          "user": {
            "name": "John",
            "age": 30,
            "email": "john@example.com"
          }
        }"#};
        let mut formatter = JsonFormatter::from(input).unwrap();
        
        // Remove nested property
        formatter.set(["user", "email"], JsonValue::Undefined).unwrap();
        
        let output = formatter.to_string();
        assert_eq!(output, indoc! {r#"{
          "user": {
            "name": "John",
            "age": 30
          }
        }"#});
    }

    #[test]
    fn test_remove_array_element() {
        let input = r#"{"items": [1, 2, 3, 4, 5]}"#;
        let mut formatter = JsonFormatter::from(input).unwrap();
        
        // Remove element at index 2 (value 3)
        formatter.set(["items", "2"], JsonValue::Undefined).unwrap();
        
        let output = formatter.to_string();
        assert_eq!(output, indoc! {r#"{
          "items": [
            1,
            2,
            4,
            5
          ]
        }"#});
    }

    #[test]
    fn test_remove_multiple_elements() {
        let input = r#"{"a": 1, "b": 2, "c": 3, "d": 4}"#;
        let mut formatter = JsonFormatter::from(input).unwrap();
        
        // Remove multiple properties
        formatter.set(["b"], JsonValue::Undefined).unwrap();
        formatter.set(["d"], JsonValue::Undefined).unwrap();
        
        let output = formatter.to_string();
        assert_eq!(output, indoc! {r#"{
          "a": 1,
          "c": 3
        }"#});
    }

    #[test]
    fn test_remove_last_element() {
        let input = r#"{"only": "value"}"#;
        let mut formatter = JsonFormatter::from(input).unwrap();
        
        // Remove the only property
        formatter.set(["only"], JsonValue::Undefined).unwrap();
        
        let output = formatter.to_string();
        assert_eq!(output, "{}");
    }

    #[test]
    fn test_remove_non_existent() {
        let input = r#"{"existing": "value"}"#;
        let mut formatter = JsonFormatter::from(input).unwrap();
        
        // Try to remove non-existent property (should not error)
        formatter.set(
            ["non_existent"],
            JsonValue::Undefined
        ).unwrap();
        
        let output = formatter.to_string();
        assert_eq!(output, indoc! {r#"{
          "existing": "value"
        }"#});
    }

    #[test]
    fn test_remove_from_complex_structure() {
        let input = indoc! {r#"{
          "users": [
            {"name": "Alice", "role": "admin"},
            {"name": "Bob", "role": "user"},
            {"name": "Charlie", "role": "user"}
          ],
          "settings": {
            "debug": true,
            "timeout": 30,
            "retries": 3
          }
        }"#};
        let mut formatter = JsonFormatter::from(input).unwrap();
        
        // Remove middle user - keeping from_file_string to test bracket notation
        let path = JsonPath::from_file_string("users[1]").unwrap();
        formatter.set_path(&path, JsonValue::Undefined).unwrap();
        // Remove a setting
        formatter.set(["settings", "timeout"], JsonValue::Undefined).unwrap();
        
        let output = formatter.to_string();
        // After removing users[1], Charlie becomes users[1]
        assert!(output.contains(r#""name": "Alice""#));
        assert!(!output.contains(r#""name": "Bob""#));
        assert!(output.contains(r#""name": "Charlie""#));
        assert!(!output.contains("timeout"));
        assert!(output.contains(r#""debug": true"#));
        assert!(output.contains(r#""retries": 3"#));
    }

    #[test]
    fn test_undefined_in_arrays() {
        // Test that Undefined values in arrays are filtered out during serialization
        let input = r#"{"items": []}"#;
        let mut formatter = JsonFormatter::from(input).unwrap();
        
        // Create an array with some undefined values
        formatter.set(
            ["items"],
            JsonValue::Array(vec![
                JsonValue::Number("1".to_string()),
                JsonValue::Undefined,
                JsonValue::Number("3".to_string()),
                JsonValue::Undefined,
                JsonValue::Number("5".to_string()),
            ])
        ).unwrap();
        
        let output = formatter.to_string();
        assert_eq!(output, indoc! {r#"{
          "items": [
            1,
            3,
            5
          ]
        }"#});
    }

    #[test]
    fn test_undefined_in_objects() {
        // Test that Undefined values in objects are filtered out during serialization
        let input = r#"{}"#;
        let mut formatter = JsonFormatter::from(input).unwrap();
        
        // Create an object with some undefined values
        formatter.set(
            ["data"],
            JsonValue::Object(vec![
                ("a".to_string(), JsonValue::Number("1".to_string())),
                ("b".to_string(), JsonValue::Undefined),
                ("c".to_string(), JsonValue::Number("3".to_string())),
                ("d".to_string(), JsonValue::Undefined),
            ])
        ).unwrap();
        
        let output = formatter.to_string();
        assert_eq!(output, indoc! {r#"{
          "data": {
            "a": 1,
            "c": 3
          }
        }"#});
    }

    #[test]
    fn test_remove_convenience_method() {
        let input = indoc! {r#"{
          "keep": "this",
          "remove": "that",
          "nested": {
            "keep": "value",
            "remove": "another"
          }
        }"#};
        let mut formatter = JsonFormatter::from(input).unwrap();
        
        // Use the convenience remove method
        formatter.remove(["remove"]).unwrap();
        formatter.remove(["nested", "remove"]).unwrap();
        
        let output = formatter.to_string();
        assert_eq!(output, indoc! {r#"{
          "keep": "this",
          "nested": {
            "keep": "value"
          }
        }"#});
    }

    #[test]
    fn test_json_path_from_vec() {
        // Test From<Vec<String>>
        let path: JsonPath = ["foo", "bar", "baz"].into();
        assert_eq!(path.segments(), &["foo", "bar", "baz"]);
        
        // Test From<Vec<&str>>
        let path: JsonPath = ["users", "0", "name"].into();
        assert_eq!(path.segments(), &["users", "0", "name"]);
        
        // Test empty vec
        let path: JsonPath = Vec::<String>::new().into();
        assert!(path.is_empty());
        
        // Test mixed usage
        let segments = ["api", "v1", "endpoints"];
        let path: JsonPath = segments.into();
        assert_eq!(path.to_file_string(), "api.v1.endpoints");
    }

    #[test]
    fn test_trailing_newline_detection() {
        // Test with trailing newline
        let input_with_newline = r#"{"key": "value"}
"#;
        let formatter = JsonFormatter::from(input_with_newline).unwrap();
        assert!(formatter.format_prefs.trailing_newline);
        
        // Test without trailing newline
        let input_without_newline = r#"{"key": "value"}"#;
        let formatter = JsonFormatter::from(input_without_newline).unwrap();
        assert!(!formatter.format_prefs.trailing_newline);
        
        // Test with Windows-style line ending
        let input_with_crlf = "{\"key\": \"value\"}\r\n";
        let formatter = JsonFormatter::from(input_with_crlf).unwrap();
        assert!(formatter.format_prefs.trailing_newline);
        assert_eq!(formatter.format_prefs.line_ending, "\r\n");
    }

    #[test]
    fn test_trailing_newline_preservation() {
        // Test that trailing newline is preserved after modifications
        let input = r#"{"name": "test"}
"#;
        let mut formatter = JsonFormatter::from(input).unwrap();
        
        // Make a modification
        formatter.set(
            ["version"],
            JsonValue::String("1.0.0".to_string())
        ).unwrap();
        
        let output = formatter.to_string();
        assert!(output.ends_with('\n'), "Output should preserve trailing newline");
        assert_eq!(output, indoc! {r#"{
          "name": "test",
          "version": "1.0.0"
        }
        "#});
    }

    #[test]
    fn test_no_trailing_newline_preservation() {
        // Test that absence of trailing newline is preserved
        let input = r#"{"name": "test"}"#;
        let mut formatter = JsonFormatter::from(input).unwrap();
        
        // Make a modification
        formatter.set(
            ["version"],
            JsonValue::String("1.0.0".to_string())
        ).unwrap();
        
        let output = formatter.to_string();
        assert!(!output.ends_with('\n'), "Output should not have trailing newline");
        assert_eq!(output, indoc! {r#"{
          "name": "test",
          "version": "1.0.0"
        }"#});
    }

    #[test]
    fn test_multiple_trailing_newlines() {
        // Test that multiple trailing newlines are normalized to one
        let input = r#"{"name": "test"}


"#;
        let formatter = JsonFormatter::from(input).unwrap();
        assert!(formatter.format_prefs.trailing_newline);
        
        let output = formatter.to_string();
        // Should have exactly one trailing newline, not multiple
        assert!(output.ends_with('\n'));
        assert!(!output.ends_with("\n\n"));
    }
}
