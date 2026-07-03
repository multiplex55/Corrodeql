use super::lexer::{lex, Keyword, Token, TokenKind};
use super::model::*;
use super::preprocessor::preprocess;

/// Parses schema input into a schema model. Recoverable unsupported statements
/// are reported in diagnostics and skipped.
pub fn parse(input: impl AsRef<str>) -> Schema {
    Parser::new(input.as_ref()).parse_schema()
}

struct Parser {
    tokens: Vec<Token>,
    pos: usize,
    diagnostics: Vec<SchemaDiagnostic>,
}

impl Parser {
    fn new(input: &str) -> Self {
        Self {
            tokens: lex(&preprocess(input)),
            pos: 0,
            diagnostics: Vec::new(),
        }
    }
    fn parse_schema(mut self) -> Schema {
        let mut tables = Vec::new();
        while !self.is_eof() {
            if self.consume_kw(Keyword::Create) {
                if self.consume_kw(Keyword::Table) {
                    if let Some(t) = self.parse_create_table() {
                        tables.push(t);
                    }
                } else {
                    self.unsupported("unsupported CREATE statement");
                    self.skip_stmt();
                }
            } else if self.consume_sym(';') {
            } else {
                self.unsupported("unsupported statement skipped");
                self.skip_stmt();
            }
        }
        DatabaseSchema {
            tables,
            indexes: Vec::new(),
            diagnostics: self.diagnostics,
        }
    }
    fn parse_create_table(&mut self) -> Option<TableDef> {
        let name = self.parse_table_name()?;
        if !self.expect_sym('(') {
            return None;
        }
        let mut columns = Vec::new();
        let mut pk = None;
        while !self.is_eof() && !self.consume_sym(')') {
            if self.consume_kw(Keyword::Constraint) {
                let cname = self.ident();
                if self.consume_kw(Keyword::Primary) {
                    pk = self.parse_pk(cname);
                } else {
                    self.unsupported("unsupported table constraint");
                    self.skip_to_comma_or_rparen();
                }
            } else if self.consume_kw(Keyword::Primary) {
                pk = self.parse_pk(None);
            } else if let Some(mut col) = self.parse_column() {
                if let Some(inline) = self.inline_pk(&col) {
                    pk = Some(inline);
                    col.check = None;
                }
                columns.push(col);
            } else {
                self.skip_to_comma_or_rparen();
            }
            self.consume_sym(',');
        }
        self.skip_stmt_tail();
        Some(TableDef {
            name,
            columns,
            primary_key: pk,
            unique_constraints: Vec::new(),
            foreign_keys: Vec::new(),
            check_constraints: Vec::new(),
        })
    }
    fn parse_table_name(&mut self) -> Option<TableName> {
        let first = self.ident()?;
        if self.consume_sym('.') {
            let second = self.ident()?;
            Some(TableName::new(Some(first), second))
        } else {
            Some(TableName::new(None, first))
        }
    }
    fn parse_column(&mut self) -> Option<ColumnDef> {
        let name = self.ident()?;
        let data_type = self.parse_type()?;
        let mut nullable = true;
        let mut default = None;
        let mut inline_pk = false;
        while !self.is_eof() && !self.at_sym(',') && !self.at_sym(')') {
            if self.consume_kw(Keyword::Not) {
                self.consume_kw(Keyword::Null);
                nullable = false;
            } else if self.consume_kw(Keyword::Null) {
                nullable = true;
            } else if self.consume_kw(Keyword::Default) {
                default = Some(DefaultConstraintDef {
                    name: None,
                    expression: self.collect_expr(),
                });
            } else if self.consume_kw(Keyword::Primary) {
                self.consume_kw(Keyword::Key);
                inline_pk = true;
                nullable = false;
            } else {
                self.advance();
            }
        }
        Some(ColumnDef {
            name: name.clone(),
            data_type,
            nullable,
            identity: false,
            default,
            check: if inline_pk {
                Some(CheckConstraintDef {
                    name: Some("__INLINE_PK__".into()),
                    expression: String::new(),
                })
            } else {
                None
            },
        })
    }
    fn inline_pk(&self, col: &ColumnDef) -> Option<PrimaryKeyDef> {
        col.check
            .as_ref()
            .filter(|c| c.name.as_deref() == Some("__INLINE_PK__"))
            .map(|_| PrimaryKeyDef {
                name: None,
                columns: vec![col.name.clone()],
                clustered: None,
            })
    }
    fn parse_pk(&mut self, name: Option<String>) -> Option<PrimaryKeyDef> {
        self.consume_kw(Keyword::Key);
        let clustered = if self.consume_kw(Keyword::Clustered) {
            Some(true)
        } else if self.consume_kw(Keyword::NonClustered) {
            Some(false)
        } else {
            None
        };
        if !self.expect_sym('(') {
            return None;
        };
        let mut cols = Vec::new();
        loop {
            if let Some(c) = self.ident() {
                cols.push(c)
            };
            while !self.is_eof() && !self.at_sym(',') && !self.at_sym(')') {
                self.advance();
            }
            if self.consume_sym(',') {
                continue;
            }
            break;
        }
        self.expect_sym(')');
        Some(PrimaryKeyDef {
            name,
            columns: cols,
            clustered,
        })
    }
    fn parse_type(&mut self) -> Option<SqlServerType> {
        use Keyword::*;
        let tok = self.advance().clone();
        let kw = match tok.kind {
            TokenKind::Keyword(k) => k,
            TokenKind::Identifier => {
                return Some(SqlServerType::Other {
                    name: tok.lexeme,
                    arguments: self.type_args(),
                })
            }
            _ => {
                self.error("expected data type");
                return None;
            }
        };
        let args = self.type_args();
        Some(match kw {
            Int => SqlServerType::Int,
            BigInt => SqlServerType::BigInt,
            SmallInt => SqlServerType::SmallInt,
            TinyInt => SqlServerType::TinyInt,
            Bit => SqlServerType::Bit,
            Money => SqlServerType::Money,
            Real => SqlServerType::Real,
            Date => SqlServerType::Date,
            DateTime => SqlServerType::DateTime,
            SmallDateTime => SqlServerType::SmallDateTime,
            UniqueIdentifier => SqlServerType::UniqueIdentifier,
            Text => SqlServerType::Text,
            NText => SqlServerType::NText,
            Xml => SqlServerType::Xml,
            Decimal => SqlServerType::Decimal {
                precision: num8(args.first()),
                scale: num8(args.get(1)),
            },
            Numeric => SqlServerType::Numeric {
                precision: num8(args.first()),
                scale: num8(args.get(1)),
            },
            Float => SqlServerType::Float {
                precision: num8(args.first()),
            },
            Time => SqlServerType::Time {
                scale: num8(args.first()),
            },
            DateTime2 => SqlServerType::DateTime2 {
                scale: num8(args.first()),
            },
            Char => SqlServerType::Char {
                length: num32(args.first()),
            },
            NChar => SqlServerType::NChar {
                length: num32(args.first()),
            },
            VarChar => SqlServerType::VarChar {
                length: num32(args.first()),
                max: is_max(args.first()),
            },
            NVarChar => SqlServerType::NVarChar {
                length: num32(args.first()),
                max: is_max(args.first()),
            },
            Binary => SqlServerType::Binary {
                length: num32(args.first()),
            },
            VarBinary => SqlServerType::VarBinary {
                length: num32(args.first()),
                max: is_max(args.first()),
            },
            _ => SqlServerType::Other {
                name: tok.lexeme,
                arguments: args,
            },
        })
    }
    fn type_args(&mut self) -> Vec<String> {
        let mut a = Vec::new();
        if self.consume_sym('(') {
            while !self.is_eof() && !self.consume_sym(')') {
                if !self.consume_sym(',') {
                    a.push(self.advance().lexeme.clone());
                }
            }
        }
        a
    }
    fn collect_expr(&mut self) -> String {
        let mut s = String::new();
        while !self.is_eof() && !self.at_sym(',') && !self.at_sym(')') {
            if !s.is_empty() {
                s.push(' ')
            };
            s.push_str(&self.advance().lexeme);
        }
        s
    }
    fn ident(&mut self) -> Option<String> {
        match &self.peek().kind {
            TokenKind::Identifier => Some(self.advance().lexeme.clone()),
            _ => None,
        }
    }
    fn skip_stmt(&mut self) {
        while !self.is_eof() && !self.consume_sym(';') {
            self.advance();
        }
    }
    fn skip_stmt_tail(&mut self) {
        while !self.is_eof() && !self.consume_sym(';') {
            if matches!(self.peek().kind, TokenKind::Keyword(Keyword::Create)) {
                break;
            }
            self.advance();
        }
    }
    fn skip_to_comma_or_rparen(&mut self) {
        while !self.is_eof() && !self.at_sym(',') && !self.at_sym(')') {
            self.advance();
        }
    }
    fn unsupported(&mut self, msg: &str) {
        self.diagnostics.push(SchemaDiagnostic {
            severity: DiagnosticSeverity::Unsupported,
            message: msg.into(),
        });
    }
    fn error(&mut self, msg: &str) {
        self.diagnostics.push(SchemaDiagnostic {
            severity: DiagnosticSeverity::Error,
            message: msg.into(),
        });
    }
    fn expect_sym(&mut self, c: char) -> bool {
        if self.consume_sym(c) {
            true
        } else {
            self.error(&format!("expected '{c}'"));
            false
        }
    }
    fn consume_sym(&mut self, c: char) -> bool {
        if self.at_sym(c) {
            self.advance();
            true
        } else {
            false
        }
    }
    fn at_sym(&self, c: char) -> bool {
        self.peek().kind == TokenKind::Symbol(c)
    }
    fn consume_kw(&mut self, k: Keyword) -> bool {
        if self.peek().kind == TokenKind::Keyword(k) {
            self.advance();
            true
        } else {
            false
        }
    }
    fn is_eof(&self) -> bool {
        self.peek().kind == TokenKind::Eof
    }
    fn peek(&self) -> &Token {
        &self.tokens[self.pos]
    }
    fn advance(&mut self) -> &Token {
        let i = self.pos;
        if !self.is_eof() {
            self.pos += 1
        };
        &self.tokens[i]
    }
}
fn num8(v: Option<&String>) -> Option<u8> {
    v.and_then(|s| s.parse().ok())
}
fn num32(v: Option<&String>) -> Option<u32> {
    v.and_then(|s| s.parse().ok())
}
fn is_max(v: Option<&String>) -> bool {
    v.is_some_and(|s| s.eq_ignore_ascii_case("max"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_simple_create_table() {
        let s = parse("CREATE TABLE [dbo].[Customer] ([Id] int NOT NULL);");
        assert_eq!(
            s.tables[0].name,
            TableName::new(Some("dbo".into()), "Customer")
        );
    }
    #[test]
    fn parses_nullable_and_nonnullable() {
        let s = parse("CREATE TABLE T (A int NULL, B varchar(10) NOT NULL);");
        assert!(s.tables[0].columns[0].nullable);
        assert!(!s.tables[0].columns[1].nullable);
    }
    #[test]
    fn parses_inline_primary_key() {
        let s = parse("CREATE TABLE T (Id int PRIMARY KEY);");
        assert_eq!(
            s.tables[0].primary_key.as_ref().unwrap().columns,
            vec!["Id"]
        );
    }
    #[test]
    fn parses_table_level_composite_primary_key() {
        let s = parse("CREATE TABLE T (A int, B int, CONSTRAINT PK_T PRIMARY KEY (A, B));");
        assert_eq!(
            s.tables[0].primary_key.as_ref().unwrap().columns,
            vec!["A", "B"]
        );
    }
    #[test]
    fn unsupported_statements_emit_diagnostics() {
        let s = parse("ALTER TABLE T ADD X int; CREATE TABLE T (Id int);");
        assert_eq!(s.tables.len(), 1);
        assert!(s
            .diagnostics
            .iter()
            .any(|d| d.severity == DiagnosticSeverity::Unsupported));
    }
}
