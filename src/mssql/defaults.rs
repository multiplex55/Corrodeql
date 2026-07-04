use crate::schema::model::{DiagnosticSeverity, SchemaDiagnostic};

/// A SQL Server default expression.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DefaultExpression(pub String);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NormalizedDefault {
    pub expression: String,
    pub diagnostics: Vec<SchemaDiagnostic>,
}

/// Normalizes only SQL Server default expressions that are safe to emit in SQLite DDL.
pub fn normalize_default(expression: impl AsRef<str>) -> NormalizedDefault {
    let stripped = strip_redundant_parens(expression.as_ref().trim());
    let compact_upper = stripped
        .to_ascii_uppercase()
        .split_whitespace()
        .collect::<String>();
    let expression = match compact_upper.as_str() {
        "GETDATE()" | "SYSUTCDATETIME()" => Some("CURRENT_TIMESTAMP".to_owned()),
        _ if is_integer_literal(stripped) => Some(stripped.to_owned()),
        _ => normalize_string_literal(stripped),
    };

    let diagnostics = if expression.is_none() {
        vec![SchemaDiagnostic {
            severity: DiagnosticSeverity::Warning,
            message: format!(
                "default expression {} is not safely portable to SQLite DDL",
                stripped
            ),
            line: None,
            column: None,
        }]
    } else {
        Vec::new()
    };

    NormalizedDefault {
        expression: expression.unwrap_or_default(),
        diagnostics,
    }
}

fn is_integer_literal(s: &str) -> bool {
    let rest = s.strip_prefix('-').unwrap_or(s);
    !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit())
}

fn normalize_string_literal(s: &str) -> Option<String> {
    let literal = s
        .strip_prefix('N')
        .or_else(|| s.strip_prefix('n'))
        .unwrap_or(s);
    if literal.len() < 2 || !literal.starts_with('\'') || !literal.ends_with('\'') {
        return None;
    }
    let inner = &literal[1..literal.len() - 1];
    let mut chars = inner.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\'' && chars.next() != Some('\'') {
            return None;
        }
    }
    Some(format!("'{}'", inner))
}

fn strip_redundant_parens(mut s: &str) -> &str {
    loop {
        let trimmed = s.trim();
        if trimmed.starts_with('(') && trimmed.ends_with(')') && encloses_all(trimmed) {
            s = &trimmed[1..trimmed.len() - 1];
        } else {
            return trimmed;
        }
    }
}

fn encloses_all(s: &str) -> bool {
    let mut depth = 0i32;
    let mut in_string = false;
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        match chars[i] {
            '\'' => {
                in_string = !in_string;
                if in_string && chars.get(i + 1) == Some(&'\'') {
                    i += 1;
                }
            }
            '(' if !in_string => depth += 1,
            ')' if !in_string => {
                depth -= 1;
                if depth == 0 && i != chars.len() - 1 {
                    return false;
                }
            }
            _ => {}
        }
        i += 1;
    }
    depth == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_nested_parentheses_and_safe_literals() {
        assert_eq!(normalize_default("((0))").expression, "0");
        assert_eq!(normalize_default("(('abc'))").expression, "'abc'");
        assert_eq!(normalize_default("(N'a''bc')").expression, "'a''bc'");
        assert_eq!(normalize_default("(12.50)").expression, "");
    }

    #[test]
    fn normalizes_getdate_case_insensitively() {
        assert_eq!(
            normalize_default("(getdate())").expression,
            "CURRENT_TIMESTAMP"
        );
    }

    #[test]
    fn rejects_newid() {
        let normalized = normalize_default("NEWID()");
        assert_eq!(normalized.expression, "");
        assert_eq!(
            normalized.diagnostics[0].severity,
            DiagnosticSeverity::Warning
        );
    }
}
