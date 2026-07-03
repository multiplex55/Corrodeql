use crate::schema::model::{DiagnosticSeverity, SchemaDiagnostic};

/// A SQL Server default expression.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DefaultExpression(pub String);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NormalizedDefault {
    pub expression: String,
    pub diagnostics: Vec<SchemaDiagnostic>,
}

/// Normalizes common SQL Server default expressions for SQLite DDL emission.
pub fn normalize_default(expression: impl AsRef<str>) -> NormalizedDefault {
    let stripped = strip_redundant_parens(expression.as_ref().trim());
    let upper = stripped.to_ascii_uppercase();
    let compact_upper = upper.split_whitespace().collect::<String>();
    let (expression, diagnostic) = match compact_upper.as_str() {
        "GETDATE()" => ("CURRENT_TIMESTAMP".to_owned(), None),
        "NEWID()" => (
            "lower(hex(randomblob(4)) || '-' || hex(randomblob(2)) || '-4' || substr(hex(randomblob(2)),2) || '-' || substr('89ab',abs(random()) % 4 + 1,1) || substr(hex(randomblob(2)),2) || '-' || hex(randomblob(6)))".to_owned(),
            Some("NEWID() converted to a SQLite randomblob()-based UUID expression"),
        ),
        _ => (stripped.to_owned(), None),
    };

    NormalizedDefault {
        expression,
        diagnostics: diagnostic
            .map(|message| SchemaDiagnostic {
                severity: DiagnosticSeverity::Warning,
                message: message.to_owned(),
                line: None,
                column: None,
            })
            .into_iter()
            .collect(),
    }
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
    fn normalizes_nested_parentheses_and_literals() {
        assert_eq!(normalize_default("((0))").expression, "0");
        assert_eq!(normalize_default("(('abc'))").expression, "'abc'");
        assert_eq!(normalize_default("(12.50)").expression, "12.50");
    }

    #[test]
    fn normalizes_getdate_case_insensitively() {
        assert_eq!(
            normalize_default("(getdate())").expression,
            "CURRENT_TIMESTAMP"
        );
    }

    #[test]
    fn converts_newid_with_warning() {
        let normalized = normalize_default("NEWID()");
        assert!(normalized.expression.contains("randomblob"));
        assert_eq!(
            normalized.diagnostics[0].severity,
            DiagnosticSeverity::Warning
        );
    }
}
