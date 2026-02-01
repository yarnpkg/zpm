use zpm_ecow::EcoVec;

use super::{range::{OperatorType, Token, TokenType}, version::VersionRc, Version};
use crate::{
    MAX_SAFE_COMPONENT_LENGTH,
    MAX_SAFE_INTEGER,
};

pub fn extract_number(str: &mut std::iter::Peekable<std::str::Chars>) -> Option<u32> {
    let mut num: u64 = 0;
    let mut valid = false;
    let mut digits = 0usize;

    while let Some(&c) = str.peek() {
        if c.is_ascii_digit() {
            digits += 1;
            if digits > MAX_SAFE_COMPONENT_LENGTH {
                return None;
            }

            let digit = c.to_digit(10)? as u64;
            num = num.checked_mul(10)?.checked_add(digit)?;
            if num > MAX_SAFE_INTEGER {
                return None;
            }
            valid = true;

            str.next();
        } else {
            break;
        }
    }

    match valid {
        true => {
            if num > u32::MAX as u64 {
                None
            } else {
                Some(num as u32)
            }
        }
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

    Some(VersionRc::String(extract_alnum_hyphen(str)?.into()))
}

pub fn extract_rc(str: &mut std::iter::Peekable<std::str::Chars>) -> Option<EcoVec<VersionRc>> {
    let mut segments = EcoVec::new();

    segments.push(extract_rc_segment(str)?);

    while str.next_if_eq(&'.').is_some() {
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

    if let Some('v') = str.peek() {
        str.next();
    }

    if let Some('*' | 'x' | 'X') = str.peek() {
        str.next();
    } else if let Some(n) = extract_number(str) {
        major = n;
        missing -= 1;
    } else {
        return None;
    }

    if str.next_if_eq(&'.').is_some() {
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

        if str.next_if_eq(&'.').is_some() {
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

    if str.next_if_eq(&'-').is_some() {
        rc = extract_rc(str);
    }

    if str.next_if_eq(&'+').is_some() {
        extract_rc(str)?;
    }

    Some((Version::new_from_components(major, minor, patch, rc), missing))
}

pub fn extract_predicate(str: &mut std::iter::Peekable<std::str::Chars>) -> Option<EcoVec<Token>> {
    if let Some(c) = str.peek() {
        match c {
            '^' => {
                str.next();

                while str.next_if_eq(&' ').is_some() {
                    // Skip all whitespaces
                }

                if let Some((version, _)) = extract_version(str) {
                    let upper_bound = match (version.major, version.minor) {
                        (0, 0) => version.next_patch_rc(),
                        (0, _) => version.next_minor_rc(),
                        _ => version.next_major_rc(),
                    };

                    Some(EcoVec::from([
                        Token::Operation(
                            OperatorType::GreaterThanOrEqual,
                            version,
                        ),
                        Token::Syntax(TokenType::SAnd),
                        Token::Operation(
                            OperatorType::LessThan,
                            upper_bound,
                        ),
                    ]))
                } else {
                    None
                }
            }

            '~' => {
                str.next();

                while str.next_if_eq(&' ').is_some() {
                    // Skip all whitespaces
                }

                if let Some((version, _)) = extract_version(str) {
                    let next_minor
                        = version.next_minor_rc();

                    Some(EcoVec::from([
                        Token::Operation(
                            OperatorType::GreaterThanOrEqual,
                            version,
                        ),
                        Token::Syntax(TokenType::SAnd),
                        Token::Operation(
                            OperatorType::LessThan,
                            next_minor,
                        ),
                    ]))
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

                while str.next_if_eq(&' ').is_some() {
                    // Skip all whitespaces
                }

                if let Some((version, _)) = extract_version(str) {
                    Some(EcoVec::from([Token::Operation(
                        operator,
                        version,
                    )]))
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

                while str.next_if_eq(&' ').is_some() {
                    // Skip all whitespaces
                }

                if let Some((version, _)) = extract_version(str) {
                    Some(EcoVec::from([Token::Operation(
                        operator,
                        version,
                    )]))
                } else {
                    None
                }
            }

            '=' => {
                str.next();
                str.next_if_eq(&'=');

                while str.next_if_eq(&' ').is_some() {
                    // Skip all whitespaces
                }

                if let Some((version, _)) = extract_version(str) {
                    Some(EcoVec::from([Token::Operation(
                        OperatorType::Equal,
                        version,
                    )]))
                } else {
                    None
                }
            }

            _ => {
                if let Some((version, missing)) = extract_version(str) {
                    if str.next_if_eq(&' ').is_some() {
                        while str.next_if_eq(&' ').is_some() {
                            // Skip all whitespaces
                        }

                        if str.next_if_eq(&'-').is_some() {
                            while str.next_if_eq(&' ').is_some() {
                                // Skip all whitespaces
                            }

                            return extract_version(str).map(|(other_version, _)| {
                                EcoVec::from([
                                    Token::Operation(
                                        OperatorType::GreaterThanOrEqual,
                                        version,
                                    ),
                                    Token::Syntax(TokenType::SAnd),
                                    Token::Operation(
                                        OperatorType::LessThan,
                                        other_version,
                                    ),
                                ])
                            })
                        }
                    }

                    match missing {
                        3 => {
                            Some(EcoVec::from([
                                Token::Operation(
                                    OperatorType::GreaterThanOrEqual,
                                    version,
                                ),
                            ]))
                        }

                        2 => {
                            let next_major = version.next_major();

                            Some(EcoVec::from([
                                Token::Operation(
                                    OperatorType::GreaterThanOrEqual,
                                    version,
                                ),
                                Token::Syntax(TokenType::SAnd),
                                Token::Operation(
                                    OperatorType::LessThan,
                                    next_major,
                                ),
                            ]))
                        }

                        1 => {
                            let next_minor = version.next_minor();

                            Some(EcoVec::from([
                                Token::Operation(
                                    OperatorType::GreaterThanOrEqual,
                                    version,
                                ),
                                Token::Syntax(TokenType::SAnd),
                                Token::Operation(
                                    OperatorType::LessThan,
                                    next_minor,
                                ),
                            ]))
                        }

                        0 => {
                            Some(EcoVec::from([Token::Operation(
                                OperatorType::Equal,
                                version,
                            )]))
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

pub fn extract_tokens(str: &mut std::iter::Peekable<std::str::Chars>) -> Option<EcoVec<Token>> {
    let mut tokens = EcoVec::new();

    while let Some(c) = str.peek() {
        match c {
            ' ' => {
                str.next();
            }

            '|' => {
                str.next();

                if str.next_if_eq(&'|').is_some() {
                    tokens.push(Token::Syntax(TokenType::Or));
                } else {
                    return None;
                }
            }

            '&' => {
                str.next();

                if str.next_if_eq(&'&').is_some() {
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
                    if let Some(Token::Operation(_, _)) = tokens.last() {
                        tokens.push(Token::Syntax(TokenType::SAnd));
                    }

                    tokens.extend(predicate.into_iter());
                } else {
                    return None;
                }
            }
        }
    }

    Some(tokens)
}

pub fn infix_to_prefix(input: &[Token]) -> Option<EcoVec<Token>> {
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
                while !stack.is_empty() && stack.last() != Some(&Token::Syntax(TokenType::RParen)) {
                    prefix.push(stack.pop().unwrap());
                }

                if stack.is_empty() {
                    return None;
                }

                stack.pop();
            }

            _ => {
                while stack.last() == Some(&Token::Syntax(TokenType::SAnd)) {
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

    Some(EcoVec::from(prefix))
}
