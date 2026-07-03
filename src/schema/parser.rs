use super::classifier::{classify_batches, summarize, ClassifiedStatement, StatementKind};
use super::lexer::{lex, Keyword, Token, TokenKind};
use super::model::*;
use super::preprocessor::{preprocess, SqlBatch};
use crate::config::options::ConvertOptions;
use crate::mssql::{
    defaults::normalize_default,
    identifiers::{object_name_from_identifiers, parse_identifier_token, Identifier},
    types::normalize_type,
};

/// Parses schema input into a schema model. Recoverable unsupported statements
/// are reported in diagnostics and skipped.
pub fn parse(input: impl AsRef<str>) -> Schema {
    parse_with_options(input, &ConvertOptions::default())
}

pub fn parse_with_options(input: impl AsRef<str>, options: &ConvertOptions) -> Schema {
    let batches = match preprocess(input.as_ref()) {
        Ok(batches) => batches,
        Err(diagnostics) => {
            return DatabaseSchema {
                tables: Vec::new(),
                indexes: Vec::new(),
                diagnostics,
                statement_summary: Default::default(),
            }
        }
    };
    parse_batches(&batches, options)
}

fn parse_batches(batches: &[SqlBatch], options: &ConvertOptions) -> Schema {
    let mut tables = Vec::new();
    let mut indexes = Vec::new();
    let mut diagnostics = Vec::new();
    let statements = classify_batches(batches);
    let statement_summary = summarize(&statements);
    for statement in &statements {
        handle_classified_statement(
            statement,
            options,
            &mut tables,
            &mut indexes,
            &mut diagnostics,
        );
    }
    for table in &tables {
        for column in &table.columns {
            diagnostics.extend(normalize_type(&column.data_type).diagnostics);
            if let Some(default) = &column.default {
                diagnostics.extend(normalize_default(&default.expression).diagnostics);
            }
        }
    }
    DatabaseSchema {
        tables,
        indexes,
        diagnostics,
        statement_summary,
    }
}

fn handle_classified_statement(
    statement: &ClassifiedStatement,
    options: &ConvertOptions,
    tables: &mut Vec<TableDef>,
    indexes: &mut Vec<IndexDef>,
    diagnostics: &mut Vec<SchemaDiagnostic>,
) {
    match statement.kind {
        StatementKind::CreateTable
        | StatementKind::AlterTableAddConstraint
        | StatementKind::CreateIndex => {
            let mut parser = Parser::new(&statement.batch);
            parser.parse_schema_into(tables, indexes);
            diagnostics.extend(parser.diagnostics);
        }
        StatementKind::CreateView
        | StatementKind::CreateTrigger
        | StatementKind::CreateProcedure
        | StatementKind::SetOption
        | StatementKind::UseDatabase => {
            diagnostics.push(SchemaDiagnostic {
                severity: if options.strict {
                    DiagnosticSeverity::Error
                } else {
                    DiagnosticSeverity::Unsupported
                },
                message: format!("{} statement ignored", statement.kind.label()),
                line: Some(statement.line_start),
                column: Some(1),
            });
        }
        StatementKind::Unknown => {
            diagnostics.push(SchemaDiagnostic {
                severity: if options.strict {
                    DiagnosticSeverity::Error
                } else {
                    DiagnosticSeverity::Warning
                },
                message: "unknown statement skipped".to_owned(),
                line: Some(statement.line_start),
                column: Some(1),
            });
        }
    }
}

#[allow(dead_code)]
pub(crate) fn split_top_level_commas(input: &str) -> Vec<&str> {
    let mut fragments = Vec::new();
    let mut start = 0usize;
    let mut depth = 0i32;
    let mut chars = input.char_indices().peekable();

    while let Some((idx, ch)) = chars.next() {
        match ch {
            '\'' => {
                while let Some((_, string_ch)) = chars.next() {
                    if string_ch == '\'' {
                        if matches!(chars.peek(), Some((_, '\''))) {
                            chars.next();
                        } else {
                            break;
                        }
                    }
                }
            }
            '[' => {
                while let Some((_, ident_ch)) = chars.next() {
                    if ident_ch == ']' {
                        if matches!(chars.peek(), Some((_, ']'))) {
                            chars.next();
                        } else {
                            break;
                        }
                    }
                }
            }
            '-' if matches!(chars.peek(), Some((_, '-'))) => {
                chars.next();
                while let Some((_, comment_ch)) = chars.next() {
                    if comment_ch == '\n' {
                        break;
                    }
                }
            }
            '/' if matches!(chars.peek(), Some((_, '*'))) => {
                chars.next();
                let mut previous = '\0';
                for (_, comment_ch) in chars.by_ref() {
                    if previous == '*' && comment_ch == '/' {
                        break;
                    }
                    previous = comment_ch;
                }
            }
            '(' => depth += 1,
            ')' if depth > 0 => depth -= 1,
            ',' if depth == 0 => {
                fragments.push(input[start..idx].trim());
                start = idx + ch.len_utf8();
            }
            _ => {}
        }
    }

    let tail = input[start..].trim();
    if !tail.is_empty() || input.is_empty() {
        fragments.push(tail);
    }
    fragments
}

struct Parser {
    tokens: Vec<Token>,
    pos: usize,
    diagnostics: Vec<SchemaDiagnostic>,
    line_offset: usize,
}

impl Parser {
    fn new(batch: &SqlBatch) -> Self {
        Self {
            tokens: lex(&batch.original_text),
            pos: 0,
            diagnostics: Vec::new(),
            line_offset: batch.line_start.saturating_sub(1),
        }
    }

    fn from_fragment(mut tokens: Vec<Token>, line_offset: usize) -> Self {
        tokens.push(Token {
            kind: TokenKind::Eof,
            lexeme: String::new(),
            line: tokens.last().map_or(1, |token| token.line),
            column: tokens
                .last()
                .map_or(1, |token| token.column + token.lexeme.len()),
        });
        Self {
            tokens,
            pos: 0,
            diagnostics: Vec::new(),
            line_offset,
        }
    }
    fn parse_schema_into(&mut self, tables: &mut Vec<TableDef>, indexes: &mut Vec<IndexDef>) {
        while !self.is_eof() {
            if self.consume_kw(Keyword::Create) {
                let unique = self.consume_kw(Keyword::Unique);
                let clustered = if self.consume_kw(Keyword::Clustered) {
                    Some(true)
                } else if self.consume_kw(Keyword::NonClustered) {
                    Some(false)
                } else {
                    None
                };
                if self.consume_kw(Keyword::Table) {
                    if let Some(t) = self.parse_create_table() {
                        tables.push(t);
                    }
                } else if self.consume_kw(Keyword::Index) {
                    if let Some(index) = self.parse_create_index(unique, clustered) {
                        indexes.push(index);
                    }
                } else {
                    self.unsupported("unsupported CREATE statement");
                    self.skip_stmt();
                }
            } else if self.consume_kw(Keyword::Alter) {
                if self.consume_kw(Keyword::Table) {
                    self.parse_alter_table(tables);
                } else {
                    self.unsupported("unsupported ALTER statement");
                    self.skip_stmt();
                }
            } else if self.consume_sym(';') {
            } else {
                self.unsupported("unsupported statement skipped");
                self.skip_stmt();
            }
        }
    }
    fn parse_create_index(&mut self, unique: bool, clustered: Option<bool>) -> Option<IndexDef> {
        let name = self.ident()?;
        if !self.consume_kw(Keyword::On) {
            self.error("expected ON in CREATE INDEX");
            return None;
        }
        let table = self.parse_table_name()?;
        let columns = self.parse_column_list().unwrap_or_default();
        self.skip_stmt_tail();
        Some(IndexDef {
            name,
            table,
            columns,
            unique,
            clustered,
            filter: None,
        })
    }
    fn parse_create_table(&mut self) -> Option<TableDef> {
        let name = self.parse_table_name()?;
        if !self.expect_sym('(') {
            return None;
        }
        let fragments = self.collect_table_body_fragments();
        let mut columns = Vec::new();
        let mut pk = None;
        let mut unique_constraints = Vec::new();
        let mut check_constraints = Vec::new();
        for fragment in fragments {
            let mut fragment_parser = Parser::from_fragment(fragment, self.line_offset);
            if fragment_parser.is_eof() {
                continue;
            }
            if fragment_parser.consume_kw(Keyword::Constraint) {
                let cname = fragment_parser.ident();
                if fragment_parser.consume_kw(Keyword::Primary) {
                    pk = fragment_parser.parse_pk(cname);
                } else if fragment_parser.consume_kw(Keyword::Unique) {
                    if let Some(unique) = fragment_parser.parse_unique(cname) {
                        unique_constraints.push(unique);
                    }
                } else if fragment_parser.consume_kw(Keyword::Check) {
                    check_constraints.push(CheckConstraintDef {
                        name: cname,
                        expression: fragment_parser.collect_parenthesized_expr(),
                    });
                } else {
                    fragment_parser.unsupported_table_fragment(&name);
                }
            } else if fragment_parser.consume_kw(Keyword::Primary) {
                pk = fragment_parser.parse_pk(None);
            } else if fragment_parser.peek().kind == TokenKind::Identifier {
                if let Some(mut col) = fragment_parser.parse_column() {
                    if let Some(inline) = fragment_parser.inline_pk(&col) {
                        pk = Some(inline);
                        col.check = None;
                    }
                    columns.push(col);
                } else {
                    fragment_parser.unsupported_table_fragment(&name);
                }
            } else {
                fragment_parser.unsupported_table_fragment(&name);
            }
            self.diagnostics.extend(fragment_parser.diagnostics);
        }
        self.skip_stmt_tail();
        Some(TableDef {
            name,
            columns,
            primary_key: pk,
            unique_constraints,
            foreign_keys: Vec::new(),
            check_constraints,
        })
    }

    fn collect_table_body_fragments(&mut self) -> Vec<Vec<Token>> {
        let mut fragments = Vec::new();
        let mut current = Vec::new();
        let mut depth = 0i32;
        while !self.is_eof() {
            if self.at_sym('(') {
                depth += 1;
                current.push(self.advance().clone());
            } else if self.at_sym(')') {
                if depth == 0 {
                    self.advance();
                    if !current.is_empty() {
                        fragments.push(current);
                    }
                    break;
                }
                depth -= 1;
                current.push(self.advance().clone());
            } else if depth == 0 && self.at_sym(',') {
                self.advance();
                fragments.push(std::mem::take(&mut current));
            } else {
                current.push(self.advance().clone());
            }
        }
        fragments
    }
    fn parse_table_name(&mut self) -> Option<TableName> {
        let mut parts = Vec::new();
        parts.push(Identifier(self.ident()?));
        if self.consume_sym('.') {
            parts.push(Identifier(self.ident()?));
        }
        match object_name_from_identifiers(parts) {
            Ok(name) => Some(name),
            Err(error) => {
                self.identifier_error(error.message.as_str());
                None
            }
        }
    }
    fn parse_column(&mut self) -> Option<ColumnDef> {
        let name = self.ident()?;
        let data_type = self.parse_type()?;
        let mut nullable = true;
        let mut default = None;
        let mut inline_pk = false;
        let mut inline_unique = false;
        let mut identity = false;
        while !self.is_eof() && !self.at_sym(',') && !self.at_sym(')') {
            if self.consume_kw(Keyword::Not) {
                self.consume_kw(Keyword::Null);
                nullable = false;
            } else if self.consume_kw(Keyword::Null) {
                nullable = true;
            } else if self.consume_kw(Keyword::Identity) {
                identity = true;
                if self.consume_sym('(') {
                    let mut depth = 1i32;
                    while !self.is_eof() && depth > 0 {
                        if self.at_sym('(') {
                            depth += 1;
                        } else if self.at_sym(')') {
                            depth -= 1;
                        }
                        self.advance();
                    }
                }
            } else if self.consume_kw(Keyword::Constraint) {
                let constraint_name = self.ident();
                if self.consume_kw(Keyword::Default) {
                    default = Some(DefaultConstraintDef {
                        name: constraint_name,
                        expression: self.collect_expr(),
                    });
                } else if self.consume_kw(Keyword::Check) {
                    self.collect_parenthesized_expr();
                }
            } else if self.consume_kw(Keyword::Default) {
                default = Some(DefaultConstraintDef {
                    name: None,
                    expression: self.collect_expr(),
                });
            } else if self.consume_kw(Keyword::Primary) {
                self.consume_kw(Keyword::Key);
                inline_pk = true;
                nullable = false;
            } else if self.consume_kw(Keyword::Unique) {
                let _ =
                    self.consume_kw(Keyword::Clustered) || self.consume_kw(Keyword::NonClustered);
                inline_unique = true;
            } else {
                self.advance();
            }
        }
        Some(ColumnDef {
            name: name.clone(),
            data_type,
            nullable,
            identity,
            primary_key: inline_pk,
            unique: inline_unique,
            default,
            check: None,
        })
    }
    fn inline_pk(&self, col: &ColumnDef) -> Option<PrimaryKeyDef> {
        col.primary_key.then(|| PrimaryKeyDef {
            name: None,
            columns: vec![col.name.clone()],
            clustered: None,
        })
    }
    fn parse_alter_table(&mut self, tables: &mut [TableDef]) {
        let Some(table_name) = self.parse_table_name() else {
            return;
        };
        self.consume_kw(Keyword::Add);
        let constraint_name = if self.consume_kw(Keyword::Constraint) {
            self.ident()
        } else {
            None
        };
        if self.consume_kw(Keyword::Foreign) {
            self.consume_kw(Keyword::Key);
            if let Some(fk) = self.parse_fk(constraint_name) {
                if let Some(table) = tables.iter_mut().find(|t| t.name == table_name) {
                    table.foreign_keys.push(fk);
                } else {
                    self.unsupported("foreign key references table not parsed yet");
                }
            }
        } else {
            self.unsupported("unsupported ALTER TABLE constraint");
            self.skip_stmt();
        }
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
            TokenKind::Identifier => return Some(self.type_from_name(tok.lexeme)),
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
    fn type_from_name(&mut self, name: String) -> SqlServerType {
        let args = self.type_args();
        match name.to_ascii_uppercase().as_str() {
            "INT" => SqlServerType::Int,
            "BIGINT" => SqlServerType::BigInt,
            "SMALLINT" => SqlServerType::SmallInt,
            "TINYINT" => SqlServerType::TinyInt,
            "BIT" => SqlServerType::Bit,
            "MONEY" => SqlServerType::Money,
            "REAL" => SqlServerType::Real,
            "DATE" => SqlServerType::Date,
            "DATETIME" => SqlServerType::DateTime,
            "DATETIME2" => SqlServerType::DateTime2 {
                scale: num8(args.first()),
            },
            "SMALLDATETIME" => SqlServerType::SmallDateTime,
            "UNIQUEIDENTIFIER" => SqlServerType::UniqueIdentifier,
            "TEXT" => SqlServerType::Text,
            "NTEXT" => SqlServerType::NText,
            "XML" => SqlServerType::Xml,
            "DECIMAL" => SqlServerType::Decimal {
                precision: num8(args.first()),
                scale: num8(args.get(1)),
            },
            "NUMERIC" => SqlServerType::Numeric {
                precision: num8(args.first()),
                scale: num8(args.get(1)),
            },
            "FLOAT" => SqlServerType::Float {
                precision: num8(args.first()),
            },
            "TIME" => SqlServerType::Time {
                scale: num8(args.first()),
            },
            "CHAR" => SqlServerType::Char {
                length: num32(args.first()),
            },
            "NCHAR" => SqlServerType::NChar {
                length: num32(args.first()),
            },
            "VARCHAR" => SqlServerType::VarChar {
                length: num32(args.first()),
                max: is_max(args.first()),
            },
            "NVARCHAR" => SqlServerType::NVarChar {
                length: num32(args.first()),
                max: is_max(args.first()),
            },
            "BINARY" => SqlServerType::Binary {
                length: num32(args.first()),
            },
            "VARBINARY" => SqlServerType::VarBinary {
                length: num32(args.first()),
                max: is_max(args.first()),
            },
            _ => SqlServerType::Other {
                name,
                arguments: args,
            },
        }
    }
    fn parse_unique(&mut self, name: Option<String>) -> Option<UniqueConstraintDef> {
        let _ = self.consume_kw(Keyword::Clustered) || self.consume_kw(Keyword::NonClustered);
        let columns = self.parse_column_list()?;
        Some(UniqueConstraintDef { name, columns })
    }
    fn parse_fk(&mut self, name: Option<String>) -> Option<ForeignKeyDef> {
        let columns = self.parse_column_list()?;
        self.consume_kw(Keyword::References);
        let referenced_table = self.parse_table_name()?;
        let referenced_columns = self.parse_column_list().unwrap_or_default();
        self.skip_stmt_tail();
        Some(ForeignKeyDef {
            name,
            columns,
            referenced_table,
            referenced_columns,
            on_delete: None,
            on_update: None,
        })
    }
    fn parse_column_list(&mut self) -> Option<Vec<String>> {
        if !self.expect_sym('(') {
            return None;
        }
        let mut columns = Vec::new();
        while !self.is_eof() && !self.consume_sym(')') {
            if let Some(column) = self.ident() {
                columns.push(column);
            } else {
                self.advance();
            }
            self.consume_sym(',');
        }
        Some(columns)
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
        let mut depth = 0i32;
        while !self.is_eof() {
            if depth == 0 && (self.at_sym(',') || self.at_sym(')')) {
                break;
            }
            if self.at_sym('(') {
                depth += 1;
            }
            if self.at_sym(')') {
                depth -= 1;
            }
            append_expr_token(&mut s, self.advance());
        }
        s
    }
    fn collect_parenthesized_expr(&mut self) -> String {
        if !self.consume_sym('(') {
            return self.collect_expr();
        }
        let mut s = String::new();
        let mut depth = 1i32;
        while !self.is_eof() && depth > 0 {
            if self.at_sym('(') {
                depth += 1;
            }
            if self.at_sym(')') {
                depth -= 1;
                if depth == 0 {
                    self.advance();
                    break;
                }
            }
            append_expr_token(&mut s, self.advance());
        }
        s
    }
    fn ident(&mut self) -> Option<String> {
        match parse_identifier_token(self.peek()) {
            Ok(identifier) => {
                self.advance();
                Some(identifier.0)
            }
            Err(error) if matches!(self.peek().kind, TokenKind::MalformedIdentifier) => {
                self.identifier_error(error.message.as_str());
                self.advance();
                None
            }
            Err(_) => None,
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

    fn unsupported_table_fragment(&mut self, table: &TableName) {
        self.diagnostics.push(SchemaDiagnostic {
            severity: DiagnosticSeverity::Unsupported,
            message: format!(
                "unsupported CREATE TABLE fragment in table {}",
                table.display_sql_server()
            ),
            line: Some(self.peek().line + self.line_offset),
            column: Some(self.peek().column),
        });
    }
    fn unsupported(&mut self, msg: &str) {
        self.diagnostics.push(SchemaDiagnostic {
            severity: DiagnosticSeverity::Unsupported,
            message: msg.into(),
            line: Some(self.peek().line + self.line_offset),
            column: Some(self.peek().column),
        });
    }
    fn error(&mut self, msg: &str) {
        self.diagnostics.push(SchemaDiagnostic {
            severity: DiagnosticSeverity::Error,
            message: msg.into(),
            line: Some(self.peek().line + self.line_offset),
            column: Some(self.peek().column),
        });
    }
    fn identifier_error(&mut self, msg: &str) {
        self.diagnostics.push(SchemaDiagnostic {
            severity: DiagnosticSeverity::Error,
            message: msg.into(),
            line: Some(self.peek().line + self.line_offset),
            column: Some(self.peek().column),
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
fn append_expr_token(output: &mut String, token: &Token) {
    let lexeme = token.lexeme.as_str();
    let no_space_before = matches!(lexeme, "(" | ")" | "," | "." | ";");
    let no_space_after_previous =
        output.ends_with('(') || output.ends_with('.') || output.is_empty();
    if !no_space_before && !no_space_after_previous {
        output.push(' ');
    }
    output.push_str(lexeme);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_top_level_commas_keeps_nested_decimal_arguments() {
        assert_eq!(
            split_top_level_commas("Amount decimal(18,2), Name int"),
            vec!["Amount decimal(18,2)", "Name int"]
        );
    }

    #[test]
    fn split_top_level_commas_keeps_string_default_with_comma() {
        assert_eq!(
            split_top_level_commas("Name varchar(50) DEFAULT ('hello, world'), Id int"),
            vec!["Name varchar(50) DEFAULT ('hello, world')", "Id int"]
        );
    }

    #[test]
    fn split_top_level_commas_keeps_check_in_list() {
        assert_eq!(
            split_top_level_commas("CHECK ([Amount] IN (1,2,3)), Id int"),
            vec!["CHECK ([Amount] IN (1,2,3))", "Id int"]
        );
    }

    #[test]
    fn split_top_level_commas_keeps_bracketed_names_and_strings_with_commas() {
        assert_eq!(
            split_top_level_commas("[Last, First] nvarchar(100), [Note] varchar(100) DEFAULT 'a,b', [Esc]]aped,Name] int"),
            vec!["[Last, First] nvarchar(100)", "[Note] varchar(100) DEFAULT 'a,b'", "[Esc]]aped,Name] int"]
        );
    }

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
    fn parses_required_inline_column_metadata() {
        let s = parse(
            "CREATE TABLE T (
                [CustomerId] [int] IDENTITY(1,1) NOT NULL,
                [Name] [nvarchar](200) NOT NULL,
                [CreatedAt] [datetime2](7) NOT NULL DEFAULT (sysutcdatetime()),
                [ExternalId] [uniqueidentifier] NOT NULL DEFAULT (newid()),
                [Amount] [decimal](18,2) NULL,
                [Code] [nvarchar](50) UNIQUE,
                [Id] [int] PRIMARY KEY
            );",
        );
        let columns = &s.tables[0].columns;

        assert_eq!(columns[0].name, "CustomerId");
        assert_eq!(columns[0].data_type, SqlServerType::Int);
        assert!(columns[0].identity);
        assert!(!columns[0].nullable);

        assert_eq!(
            columns[1].data_type,
            SqlServerType::NVarChar {
                length: Some(200),
                max: false
            }
        );
        assert!(!columns[1].nullable);

        assert_eq!(
            columns[2].data_type,
            SqlServerType::DateTime2 { scale: Some(7) }
        );
        assert_eq!(
            columns[2].default.as_ref().unwrap().expression,
            "(sysutcdatetime())"
        );

        assert_eq!(columns[3].data_type, SqlServerType::UniqueIdentifier);
        assert_eq!(columns[3].default.as_ref().unwrap().expression, "(newid())");

        assert_eq!(
            columns[4].data_type,
            SqlServerType::Decimal {
                precision: Some(18),
                scale: Some(2)
            }
        );
        assert!(columns[4].nullable);

        assert!(columns[5].unique);
        assert!(columns[6].primary_key);
        assert_eq!(
            s.tables[0].primary_key.as_ref().unwrap().columns,
            vec!["Id"]
        );
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
    fn parses_create_table_body_as_top_level_fragments() {
        let s = parse("CREATE TABLE [dbo].[Invoice] ([Id] int NOT NULL, [Amount] decimal(18,2) NOT NULL DEFAULT (0), CONSTRAINT [PK_Invoice] PRIMARY KEY ([Id]), CONSTRAINT [CK_Invoice_Amount] CHECK ([Amount] IN (1,2,3)));");
        let table = &s.tables[0];
        assert_eq!(table.name, TableName::new(Some("dbo".into()), "Invoice"));
        assert_eq!(
            table
                .columns
                .iter()
                .map(|c| c.name.as_str())
                .collect::<Vec<_>>(),
            vec!["Id", "Amount"]
        );
        assert_eq!(table.primary_key.as_ref().unwrap().columns, vec!["Id"]);
        assert_eq!(table.check_constraints.len(), 1);
    }

    #[test]
    fn parses_decimal_type_preserving_precision_and_scale() {
        let s = parse("CREATE TABLE T (Amount decimal(18,2));");
        assert_eq!(
            s.tables[0].columns[0].data_type,
            SqlServerType::Decimal {
                precision: Some(18),
                scale: Some(2)
            }
        );
    }

    #[test]
    fn parses_string_default_containing_comma() {
        let s = parse("CREATE TABLE T (Greeting varchar(50) DEFAULT ('hello, world'), Id int);");
        assert_eq!(
            s.tables[0].columns[0].default.as_ref().unwrap().expression,
            "('hello, world')"
        );
        assert_eq!(s.tables[0].columns[1].name, "Id");
    }

    #[test]
    fn parses_table_level_primary_key() {
        let s = parse("CREATE TABLE T (Id int, PRIMARY KEY (Id));");
        assert_eq!(
            s.tables[0].primary_key.as_ref().unwrap().columns,
            vec!["Id"]
        );
    }

    #[test]
    fn parses_reserved_table_name_order() {
        let s = parse("CREATE TABLE [Order] ([Id] int PRIMARY KEY);");
        assert_eq!(
            s.tables[0].name,
            TableName::new(Some("dbo".into()), "Order")
        );
        assert_eq!(
            s.tables[0].primary_key.as_ref().unwrap().columns,
            vec!["Id"]
        );
    }

    #[test]
    fn unsupported_table_fragment_produces_diagnostic() {
        let s = parse("CREATE TABLE T (Id int, INDEX IX_T_Id (Id));");
        assert_eq!(s.tables[0].columns.len(), 1);
        assert!(s
            .diagnostics
            .iter()
            .any(|d| d.severity == DiagnosticSeverity::Unsupported
                && d.message.contains("[dbo].[T]")));
    }

    #[test]
    fn parses_major_sql_server_type_families() {
        let s = parse("CREATE TABLE T (A int, B bigint, C smallint, D tinyint, E bit, F decimal(10,2), G numeric(8,3), H money, I float, J real, K char(5), L varchar(20), M varchar(max), N nchar(4), O nvarchar(30), P nvarchar(max), Q text, R ntext, S date, U datetime, V datetime2, W smalldatetime, X time, Y uniqueidentifier, Z binary, AA varbinary, AB varbinary(max));");
        let columns = &s.tables[0].columns;
        assert_eq!(columns[0].data_type, SqlServerType::Int);
        assert_eq!(
            columns[5].data_type,
            SqlServerType::Decimal {
                precision: Some(10),
                scale: Some(2)
            }
        );
        assert_eq!(
            columns[12].data_type,
            SqlServerType::VarChar {
                length: None,
                max: true
            }
        );
        assert_eq!(
            columns[15].data_type,
            SqlServerType::NVarChar {
                length: None,
                max: true
            }
        );
        assert_eq!(
            columns[24].data_type,
            SqlServerType::Binary { length: None }
        );
        assert_eq!(
            columns[26].data_type,
            SqlServerType::VarBinary {
                length: None,
                max: true
            }
        );
        assert!(s.diagnostics.iter().any(|d| d.message.contains("affinity")));
    }

    #[test]
    fn parses_named_default_constraints_and_default_diagnostics() {
        let s = parse("CREATE TABLE T (Created datetime CONSTRAINT DF_T_Created DEFAULT (GETDATE()), RowGuid uniqueidentifier DEFAULT (NEWID()), Flag int CONSTRAINT DF_T_Flag DEFAULT ((0)));");
        assert_eq!(
            s.tables[0].columns[0]
                .default
                .as_ref()
                .unwrap()
                .name
                .as_deref(),
            Some("DF_T_Created")
        );
        assert_eq!(
            normalize_default(&s.tables[0].columns[2].default.as_ref().unwrap().expression)
                .expression,
            "0"
        );
        assert_eq!(
            normalize_default(&s.tables[0].columns[0].default.as_ref().unwrap().expression)
                .expression,
            "CURRENT_TIMESTAMP"
        );
        assert!(s.diagnostics.iter().any(|d| d.message.contains("NEWID()")));
    }

    #[test]
    fn parses_named_unique_check_and_alter_foreign_key_constraints() {
        let s = parse("CREATE TABLE Parent (Id int, Code varchar(10), CONSTRAINT PK_Parent PRIMARY KEY (Id), CONSTRAINT UQ_Parent_Code UNIQUE (Code), CONSTRAINT CK_Parent_Id CHECK (Id > 0)); CREATE TABLE Child (Id int, ParentId int); ALTER TABLE Child ADD CONSTRAINT FK_Child_Parent FOREIGN KEY (ParentId) REFERENCES Parent (Id);");
        let parent = &s.tables[0];
        assert_eq!(
            parent.primary_key.as_ref().unwrap().name.as_deref(),
            Some("PK_Parent")
        );
        assert_eq!(
            parent.unique_constraints[0].name.as_deref(),
            Some("UQ_Parent_Code")
        );
        assert_eq!(
            parent.check_constraints[0].name.as_deref(),
            Some("CK_Parent_Id")
        );
        let child = &s.tables[1];
        assert_eq!(
            child.foreign_keys[0].name.as_deref(),
            Some("FK_Child_Parent")
        );
        assert_eq!(child.foreign_keys[0].columns, vec!["ParentId"]);
        assert_eq!(child.foreign_keys[0].referenced_columns, vec!["Id"]);
    }

    #[test]
    fn normalizes_escaped_identifiers_for_tables_constraints_foreign_keys_and_indexes() {
        let s = parse(
            "CREATE TABLE [dbo].[Parent]]Table] ([Id]]Col] int NOT NULL, CONSTRAINT [PK]]Parent] PRIMARY KEY ([Id]]Col]));
             CREATE TABLE [dbo].[Child]]Table] ([Id]]Col] int NOT NULL, [Parent]]Id] int NOT NULL);
             ALTER TABLE [dbo].[Child]]Table] ADD CONSTRAINT [FK]]Child]]Parent] FOREIGN KEY ([Parent]]Id]) REFERENCES [dbo].[Parent]]Table] ([Id]]Col]);
             CREATE INDEX [IX]]Child]]Parent] ON [dbo].[Child]]Table] ([Parent]]Id]);",
        );

        let parent = &s.tables[0];
        assert_eq!(parent.name.table, "Parent]Table");
        assert_eq!(parent.columns[0].name, "Id]Col");
        assert_eq!(
            parent.primary_key.as_ref().unwrap().name.as_deref(),
            Some("PK]Parent")
        );
        assert_eq!(parent.primary_key.as_ref().unwrap().columns, vec!["Id]Col"]);

        let child = &s.tables[1];
        assert_eq!(child.name.table, "Child]Table");
        assert_eq!(child.columns[1].name, "Parent]Id");
        assert_eq!(
            child.foreign_keys[0].name.as_deref(),
            Some("FK]Child]Parent")
        );
        assert_eq!(child.foreign_keys[0].columns, vec!["Parent]Id"]);
        assert_eq!(child.foreign_keys[0].referenced_table.table, "Parent]Table");
        assert_eq!(child.foreign_keys[0].referenced_columns, vec!["Id]Col"]);

        assert_eq!(s.indexes[0].name, "IX]Child]Parent");
        assert_eq!(s.indexes[0].table.table, "Child]Table");
        assert_eq!(s.indexes[0].columns, vec!["Parent]Id"]);
    }

    #[test]
    fn reports_unterminated_bracketed_identifier() {
        let s = parse("CREATE TABLE [Broken (Id int);");
        assert!(s.diagnostics.iter().any(|diagnostic| diagnostic
            .message
            .contains("unterminated bracketed identifier at line 1")));
    }

    #[test]
    fn unsupported_object_warns_in_non_strict_mode() {
        let s = parse("CREATE VIEW V AS SELECT 1; CREATE TABLE T (Id int);");
        assert_eq!(s.tables.len(), 1);
        assert!(s
            .diagnostics
            .iter()
            .any(|d| d.severity == DiagnosticSeverity::Unsupported));
    }

    #[test]
    fn unknown_statement_fails_in_strict_mode() {
        let options = ConvertOptions {
            strict: true,
            ..ConvertOptions::default()
        };
        let s = parse_with_options("SELECT 1; CREATE TABLE T (Id int);", &options);
        assert_eq!(s.tables.len(), 1);
        assert!(s
            .diagnostics
            .iter()
            .any(|d| d.severity == DiagnosticSeverity::Error && d.message.contains("unknown")));
    }

    #[test]
    fn mixed_schema_file_produces_classification_summary_counts() {
        let s = parse("CREATE TABLE T (Id int); CREATE INDEX IX_T_Id ON T (Id); CREATE PROCEDURE P AS SELECT 1; SELECT 1;");
        assert_eq!(s.statement_summary.detected_count, 2);
        assert_eq!(s.statement_summary.ignored_count, 2);
        assert_eq!(s.statement_summary.warning_count, 2);
        assert_eq!(
            s.statement_summary
                .detected
                .get(&StatementKind::CreateTable),
            Some(&1)
        );
        assert_eq!(
            s.statement_summary
                .ignored
                .get(&StatementKind::CreateProcedure),
            Some(&1)
        );
        assert_eq!(
            s.statement_summary.warnings.get(&StatementKind::Unknown),
            Some(&1)
        );
    }
}
