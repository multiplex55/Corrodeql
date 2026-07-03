use crate::schema::{
    lexer::{Token, TokenKind},
    model::TableName,
};

pub const DEFAULT_SCHEMA: &str = "dbo";

/// A SQL Server identifier normalized to its logical value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Identifier(pub String);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdentifierParseError {
    pub message: String,
    pub line: Option<usize>,
    pub column: Option<usize>,
}

impl IdentifierParseError {
    fn new(message: impl Into<String>, token: Option<&Token>) -> Self {
        Self {
            message: message.into(),
            line: token.map(|token| token.line),
            column: token.map(|token| token.column),
        }
    }

    fn raw(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            line: None,
            column: None,
        }
    }
}

pub type IdentifierParseResult<T> = Result<T, IdentifierParseError>;

/// Parses a single SQL Server identifier from an already-lexed token.
///
/// The schema lexer normalizes bracketed identifiers as it lexes them, so this
/// helper is the single parser-facing path for consuming identifier values.
pub fn parse_identifier_token(token: &Token) -> IdentifierParseResult<Identifier> {
    match &token.kind {
        TokenKind::Identifier => Ok(Identifier(token.lexeme.clone())),
        TokenKind::MalformedIdentifier => Err(IdentifierParseError::new(
            format!("unterminated bracketed identifier at line {}", token.line),
            Some(token),
        )),
        _ => Err(IdentifierParseError::new(
            "expected identifier",
            Some(token),
        )),
    }
}

/// Parses a single SQL Server identifier, preserving its case.
pub fn parse_identifier(input: &str) -> Option<Identifier> {
    parse_identifier_result(input).ok()
}

pub fn parse_identifier_result(input: &str) -> IdentifierParseResult<Identifier> {
    let (identifier, remainder) = parse_identifier_part(input.trim())?;
    if remainder.trim().is_empty() {
        Ok(identifier)
    } else {
        Err(IdentifierParseError::raw(
            "unexpected trailing input after identifier",
        ))
    }
}

/// Parses a dot-separated SQL Server multipart name.
pub fn parse_multipart_name(input: &str) -> Option<Vec<Identifier>> {
    parse_multipart_name_result(input).ok()
}

pub fn parse_multipart_name_result(input: &str) -> IdentifierParseResult<Vec<Identifier>> {
    let mut remainder = input.trim();
    let mut parts = Vec::new();

    loop {
        let (identifier, rest) = parse_identifier_part(remainder)?;
        parts.push(identifier);
        remainder = rest.trim_start();

        if remainder.is_empty() {
            return Ok(parts);
        }

        remainder = remainder
            .strip_prefix('.')
            .ok_or_else(|| IdentifierParseError::raw("expected '.' between identifier parts"))?
            .trim_start();
        if remainder.is_empty() {
            return Err(IdentifierParseError::raw("expected identifier after '.'"));
        }
    }
}

/// Parses a one- or two-part SQL Server object name.
///
/// One-part names are normalized to SQL Server's default `dbo` schema.
pub fn parse_object_name(input: &str) -> Option<TableName> {
    parse_object_name_result(input).ok()
}

pub fn parse_object_name_result(input: &str) -> IdentifierParseResult<TableName> {
    let parts = parse_multipart_name_result(input)?;
    object_name_from_identifiers(parts)
}

pub fn object_name_from_identifiers(parts: Vec<Identifier>) -> IdentifierParseResult<TableName> {
    match parts.as_slice() {
        [table] => Ok(TableName::new(
            Some(DEFAULT_SCHEMA.to_owned()),
            table.0.clone(),
        )),
        [schema, table] => Ok(TableName::new(Some(schema.0.clone()), table.0.clone())),
        _ => Err(IdentifierParseError::raw(
            "expected one- or two-part SQL Server object name",
        )),
    }
}

/// Backwards-compatible alias for object name parsing.
pub fn parse_table_name(input: &str) -> Option<TableName> {
    parse_object_name(input)
}

fn parse_identifier_part(input: &str) -> IdentifierParseResult<(Identifier, &str)> {
    if input.starts_with('[') {
        parse_bracketed_identifier(input)
    } else {
        parse_unquoted_identifier(input)
    }
}

fn parse_bracketed_identifier(input: &str) -> IdentifierParseResult<(Identifier, &str)> {
    let mut value = String::new();
    let mut chars = input.char_indices();
    chars.next();

    while let Some((index, ch)) = chars.next() {
        match ch {
            ']' if input[index + ch.len_utf8()..].starts_with(']') => {
                value.push(']');
                chars.next();
            }
            ']' => return Ok((Identifier(value), &input[index + ch.len_utf8()..])),
            _ => value.push(ch),
        }
    }

    Err(IdentifierParseError::raw(
        "unterminated bracketed identifier",
    ))
}

fn parse_unquoted_identifier(input: &str) -> IdentifierParseResult<(Identifier, &str)> {
    let end = input.find('.').unwrap_or(input.len());
    let value = input[..end].trim_end();

    if value.is_empty() || value.chars().any(char::is_whitespace) {
        Err(IdentifierParseError::raw("expected identifier"))
    } else {
        Ok((Identifier(value.to_owned()), &input[end..]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_bracketed_schema_and_table() {
        assert_eq!(
            parse_object_name("[dbo].[Customer]"),
            Some(TableName::new(Some("dbo".to_owned()), "Customer"))
        );
    }

    #[test]
    fn parses_unquoted_schema_and_table() {
        assert_eq!(
            parse_object_name("dbo.Customer"),
            Some(TableName::new(Some("dbo".to_owned()), "Customer"))
        );
    }

    #[test]
    fn parses_bracketed_identifier_with_spaces() {
        assert_eq!(
            parse_object_name("[sales].[Invoice Header]"),
            Some(TableName::new(Some("sales".to_owned()), "Invoice Header"))
        );
    }

    #[test]
    fn defaults_single_part_names_to_dbo() {
        assert_eq!(
            parse_object_name("[Order]"),
            Some(TableName::new(Some("dbo".to_owned()), "Order"))
        );
    }

    #[test]
    fn unescapes_closing_brackets_in_bracketed_identifier() {
        assert_eq!(
            parse_object_name("[Some]]Name]"),
            Some(TableName::new(Some("dbo".to_owned()), "Some]Name"))
        );
    }

    #[test]
    fn malformed_bracketed_identifier_fails_clearly() {
        let error = parse_object_name_result("[Broken").unwrap_err();
        assert!(error.message.contains("unterminated bracketed identifier"));
    }
}
