use super::{range::{OperatorType, Token, TokenType}, version::VersionRc, Version};

pub fn extract_number(str: &mut std::iter::Peekable<std::str::Chars>) -> Option<u32> {
    let mut num: u32 = 0;
    let mut valid = false;

    while let Some(&c) = str.peek() {
        if c.is_digit(10) {
            num = num.saturating_mul(10).saturating_add(c.to_digit(10)?);
            valid = true;

            str.next();
        } else {
            break;
        }
    }

    match valid {
        true => Some(num),
        false => None
    }
}

pub fn extract_alnum_hyphen(str: &mut std::iter::Peekable<std::str::Chars>) -> Option<String> {
    let mut res = String::new();
    let mut valid = false;

    while let Some(&c) = str.peek() {
        if c.is_alphanumeric() || c == '-' {
            res.push(c);
            valid = true;

            str.next();
        } else {
            break;
        }
    }

    match valid {
        true => Some(res),
        false => None
    }
}

pub fn extract_rc_segment(str: &mut std::iter::Peekable<std::str::Chars>) -> Option<VersionRc> {
    let curr = str.clone();

    if let Some(n) = extract_number(str) {
        if let Some('.' | '+') | None = str.peek() {
            return Some(VersionRc::Number(n));
        }
    }

    *str = curr;

    Some(VersionRc::String(extract_alnum_hyphen(str)?))
}

pub fn extract_rc(str: &mut std::iter::Peekable<std::str::Chars>) -> Option<Vec<VersionRc>> {
    let mut segments = vec![];

    segments.push(extract_rc_segment(str)?);

    while let Some(_) = str.next_if_eq(&'.') {
        segments.push(extract_rc_segment(str)?);
    }

    Some(segments)
}

pub fn extract_version(str: &mut std::iter::Peekable<std::str::Chars>) -> Option<(Version, u8)> {
    let mut major = 0;
    let mut minor = 0;
    let mut patch = 0;
    let mut rc = None;
    let mut missing = 3;

    if let Some('*' | 'x' | 'X') = str.peek() {
        str.next();
    } else if let Some(n) = extract_number(str) {
        major = n;
        missing -= 1;
    } else {
        return None;
    }

    if let Some(_) = str.next_if_eq(&'.') {
        if let Some('*' | 'x' | 'X') = str.peek() {
            str.next();
        } else if let Some(n) = extract_number(str) {
            if missing == 2 {
                minor = n;
                missing -= 1;
            }
        } else {
            return None;
        }

        if let Some(_) = str.next_if_eq(&'.') {
            if let Some('*' | 'x' | 'X') = str.peek() {
                str.next();
            } else if let Some(n) = extract_number(str) {
                if missing == 1 {
                    patch = n;
                    missing -= 1;
                }
            } else {
                return None;
            }
        }
    }

    if let Some(_) = str.next_if_eq(&'-') {
        rc = extract_rc(str);
    }

    if let Some(_) = str.next_if_eq(&'+') {
        extract_rc(str)?;
    }

    Some((Version::new_from_components(major, minor, patch, rc), missing))
}

pub fn extract_predicate(str: &mut std::iter::Peekable<std::str::Chars>) -> Option<Vec<Token>> {
    if let Some(c) = str.peek() {
        match c {
            '^' => {
                str.next();

                while let Some(_) = str.next_if_eq(&' ') {
                    // Skip all whitespaces
                }

                if let Some((version, _)) = extract_version(str) {
                    let next_major = version.next_major_rc();

                    Some(vec![
                        Token::Operation(
                            OperatorType::GreaterThanOrEqual,
                            version,
                        ),
                        Token::Syntax(TokenType::SAnd),
                        Token::Operation(
                            OperatorType::LessThan,
                            next_major,
                        ),
                    ])
                } else {
                    None
                }
            }

            '~' => {
                str.next();

                while let Some(_) = str.next_if_eq(&' ') {
                    // Skip all whitespaces
                }

                if let Some((version, _)) = extract_version(str) {
                    let next_minor = version.next_minor_rc();

                    Some(vec![
                        Token::Operation(
                            OperatorType::GreaterThanOrEqual,
                            version,
                        ),
                        Token::Syntax(TokenType::SAnd),
                        Token::Operation(
                            OperatorType::LessThan,
                            next_minor,
                        ),
                    ])
                } else {
                    None
                }
            }

            '>' => {
                str.next();

                let operator = match str.next_if_eq(&'=') {
                    Some(_) => OperatorType::GreaterThanOrEqual,
                    None => OperatorType::GreaterThan,
                };

                while let Some(_) = str.next_if_eq(&' ') {
                    // Skip all whitespaces
                }

                if let Some((version, _)) = extract_version(str) {
                    Some(vec![Token::Operation(
                        operator,
                        version,
                    )])
                } else {
                    None
                }
            }

            '<' => {
                str.next();

                let operator = match str.next_if_eq(&'=') {
                    Some(_) => OperatorType::LessThanOrEqual,
                    None => OperatorType::LessThan,
                };

                while let Some(_) = str.next_if_eq(&' ') {
                    // Skip all whitespaces
                }

                if let Some((version, _)) = extract_version(str) {
                    Some(vec![Token::Operation(
                        operator,
                        version,
                    )])
                } else {
                    None
                }
            }

            '=' => {
                str.next();
                str.next_if_eq(&'=');

                while let Some(_) = str.next_if_eq(&' ') {
                    // Skip all whitespaces
                }

                if let Some((version, _)) = extract_version(str) {
                    Some(vec![Token::Operation(
                        OperatorType::Equal,
                        version,
                    )])
                } else {
                    None
                }
            }

            _ => {
                if let Some((version, missing)) = extract_version(str) {
                    if let Some(_) = str.next_if_eq(&' ') {
                        while let Some(_) = str.next_if_eq(&' ') {
                            // Skip all whitespaces
                        }

                        if let Some(_) = str.next_if_eq(&'-') {
                            while let Some(_) = str.next_if_eq(&' ') {
                                // Skip all whitespaces
                            }

                            return extract_version(str).map(|(other_version, _)| {
                                vec![
                                    Token::Operation(
                                        OperatorType::GreaterThanOrEqual,
                                        version,
                                    ),
                                    Token::Syntax(TokenType::SAnd),
                                    Token::Operation(
                                        OperatorType::LessThan,
                                        other_version,
                                    ),
                                ]
                            })
                        }
                    }

                    match missing {
                        3 => {
                            Some(vec![
                                Token::Operation(
                                    OperatorType::GreaterThanOrEqual,
                                    version,
                                ),
                            ])
                        }

                        2 => {
                            let next_major = version.next_major();

                            Some(vec![
                                Token::Operation(
                                    OperatorType::GreaterThanOrEqual,
                                    version,
                                ),
                                Token::Syntax(TokenType::SAnd),
                                Token::Operation(
                                    OperatorType::LessThan,
                                    next_major,
                                ),
                            ])
                        }

                        1 => {
                            let next_minor = version.next_minor();

                            Some(vec![
                                Token::Operation(
                                    OperatorType::GreaterThanOrEqual,
                                    version,
                                ),
                                Token::Syntax(TokenType::SAnd),
                                Token::Operation(
                                    OperatorType::LessThan,
                                    next_minor,
                                ),
                            ])
                        }

                        0 => {
                            Some(vec![Token::Operation(
                                OperatorType::Equal,
                                version,
                            )])
                        }

                        _ => {
                            unreachable!();
                        }
                    }
                } else {
                    None
                }
            }
        }
    } else {
        None
    }
}

pub fn extract_tokens(str: &mut std::iter::Peekable<std::str::Chars>) -> Option<Vec<Token>> {
    let mut tokens = Vec::new();

    while let Some(c) = str.peek() {
        match c {
            ' ' => {
                str.next();
            }

            '|' => {
                str.next();

                if let Some(_) = str.next_if_eq(&'|') {
                    tokens.push(Token::Syntax(TokenType::Or));
                } else {
                    return None;
                }
            }

            '&' => {
                str.next();

                if let Some(_) = str.next_if_eq(&'&') {
                    tokens.push(Token::Syntax(TokenType::And));
                } else {
                    return None;
                }
            }

            '(' => {
                str.next();

                tokens.push(Token::Syntax(TokenType::LParen));
            }

            ')' => {
                str.next();

                tokens.push(Token::Syntax(TokenType::RParen));
            }

            _ => {
                if let Some(predicate) = extract_predicate(str) {
                    if let Some(last) = tokens.last() {
                        match last {
                            Token::Operation(_, _) => {
                                tokens.push(Token::Syntax(TokenType::SAnd));
                            }

                            _ => {}
                        }
                    }

                    tokens.extend(predicate);
                } else {
                    return None;
                }
            }
        }
    }

    Some(tokens)
}

pub fn infix_to_prefix(input: &Vec<Token>) -> Option<Vec<Token>> {
    let mut prefix = vec![];
    let mut stack = vec![];

    for token in input.iter().rev() {
        match token {
            Token::Operation(_, _) => {
                prefix.push(token.clone());
            }

            Token::Syntax(TokenType::RParen) => {
                stack.push(token.clone())
            }

            Token::Syntax(TokenType::LParen) => {
                while !stack.is_empty() && stack.last() != Some(&&Token::Syntax(TokenType::RParen)) {
                    prefix.push(stack.pop().unwrap());
                }

                if stack.is_empty() {
                    return None;
                }

                stack.pop();
            }

            _ => {
                while stack.last() == Some(&&Token::Syntax(TokenType::SAnd)) {
                    prefix.push(stack.pop().unwrap());
                }

                stack.push(token.clone());
            }
        }
    }

    while let Some(token) = stack.pop() {
        prefix.push(token);
    }

    prefix.reverse();

    Some(prefix)
}
