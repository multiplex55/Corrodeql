use crate::schema::model::TableName;

/// A SQL Server identifier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Identifier(pub String);

/// Parses a single SQL Server identifier, preserving its case.
pub fn parse_identifier(input: &str) -> Option<Identifier> {
    let (identifier, remainder) = parse_identifier_part(input.trim())?;
    if remainder.trim().is_empty() {
        Some(identifier)
    } else {
        None
    }
}

/// Parses a dot-separated SQL Server multipart name.
pub fn parse_multipart_name(input: &str) -> Option<Vec<Identifier>> {
    let mut remainder = input.trim();
    let mut parts = Vec::new();

    loop {
        let (identifier, rest) = parse_identifier_part(remainder)?;
        parts.push(identifier);
        remainder = rest.trim_start();

        if remainder.is_empty() {
            return Some(parts);
        }

        remainder = remainder.strip_prefix('.')?.trim_start();
        if remainder.is_empty() {
            return None;
        }
    }
}

/// Parses a one- or two-part SQL Server table name.
pub fn parse_table_name(input: &str) -> Option<TableName> {
    let parts = parse_multipart_name(input)?;
    match parts.as_slice() {
        [table] => Some(TableName::new(None, table.0.clone())),
        [schema, table] => Some(TableName::new(Some(schema.0.clone()), table.0.clone())),
        _ => None,
    }
}

fn parse_identifier_part(input: &str) -> Option<(Identifier, &str)> {
    if input.starts_with('[') {
        parse_bracketed_identifier(input)
    } else {
        parse_unquoted_identifier(input)
    }
}

fn parse_bracketed_identifier(input: &str) -> Option<(Identifier, &str)> {
    let mut value = String::new();
    let mut chars = input.char_indices();
    chars.next()?;

    while let Some((index, ch)) = chars.next() {
        match ch {
            ']' if input[index + ch.len_utf8()..].starts_with(']') => {
                value.push(']');
                chars.next();
            }
            ']' => return Some((Identifier(value), &input[index + ch.len_utf8()..])),
            _ => value.push(ch),
        }
    }

    None
}

fn parse_unquoted_identifier(input: &str) -> Option<(Identifier, &str)> {
    let end = input.find('.').unwrap_or(input.len());
    let value = input[..end].trim_end();

    if value.is_empty() || value.chars().any(char::is_whitespace) {
        None
    } else {
        Some((Identifier(value.to_owned()), &input[end..]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_bracketed_identifier() {
        assert_eq!(
            parse_identifier("[dbo]"),
            Some(Identifier("dbo".to_owned()))
        );
    }

    #[test]
    fn parses_bracketed_multipart_name() {
        assert_eq!(
            parse_table_name("[dbo].[Customer]"),
            Some(TableName::new(Some("dbo".to_owned()), "Customer"))
        );
    }

    #[test]
    fn parses_unquoted_multipart_name() {
        assert_eq!(
            parse_table_name("dbo.Customer"),
            Some(TableName::new(Some("dbo".to_owned()), "Customer"))
        );
    }

    #[test]
    fn unescapes_closing_brackets_in_bracketed_identifier() {
        assert_eq!(
            parse_identifier("[Cost]]Center]"),
            Some(Identifier("Cost]Center".to_owned()))
        );
    }

    #[test]
    fn preserves_identifier_case() {
        assert_eq!(
            parse_table_name("[Sales].[CustomerOrder]"),
            Some(TableName::new(Some("Sales".to_owned()), "CustomerOrder"))
        );
    }
}
