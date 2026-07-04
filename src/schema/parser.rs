use super::classifier::{classify_batches, summarize, ClassifiedStatement, StatementKind};
use super::lexer::{lex, Keyword, Token, TokenKind};
use super::model::*;
use super::preprocessor::{preprocess, SqlBatch};
use crate::config::options::ConvertOptions;
use crate::mssql::{
    defaults::normalize_default,
    identifiers::{object_name_from_identifiers, parse_identifier_token, Identifier},
    types::normalize_type_with_context,
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
            diagnostics.extend(
                normalize_type_with_context(
                    &column.data_type,
                    Some(&table.name),
                    Some(&column.name),
                )
                .diagnostics,
            );
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
        let filter = self.parse_index_tail(&name);
        Some(IndexDef {
            name,
            table,
            columns,
            unique,
            clustered,
            filter,
        })
    }

    fn parse_index_tail(&mut self, index_name: &str) -> Option<String> {
        let mut filter = None;
        while !self.is_eof() && !self.consume_sym(';') {
            if self.consume_kw(Keyword::Include) {
                self.unsupported(&format!(
                    "unsupported INCLUDE columns on index {index_name} were ignored"
                ));
                self.skip_balanced_parentheses();
            } else if self.consume_kw(Keyword::With) {
                self.unsupported(&format!(
                    "unsupported WITH options on index {index_name} were ignored"
                ));
                self.skip_balanced_parentheses();
            } else if self.consume_kw(Keyword::On) {
                self.unsupported(&format!(
                    "unsupported filegroup option on index {index_name} was ignored"
                ));
                self.skip_filegroup_option();
            } else if self.consume_kw(Keyword::Where) {
                filter = Some(self.collect_index_filter_expr());
            } else if matches!(self.peek().kind, TokenKind::Keyword(Keyword::Create)) {
                break;
            } else {
                self.advance();
            }
        }
        filter.filter(|expression| !expression.trim().is_empty())
    }

    fn collect_index_filter_expr(&mut self) -> String {
        let mut s = String::new();
        let mut depth = 0i32;
        while !self.is_eof() {
            if depth == 0
                && (self.at_sym(';')
                    || self.peek_is_kw(Keyword::Include)
                    || self.peek_is_kw(Keyword::With)
                    || self.peek_is_kw(Keyword::On))
            {
                break;
            }
            if self.at_sym('(') {
                depth += 1;
            } else if self.at_sym(')') && depth > 0 {
                depth -= 1;
            }
            append_expr_token(&mut s, self.advance());
        }
        s.trim().to_owned()
    }

    fn skip_balanced_parentheses(&mut self) {
        if !self.consume_sym('(') {
            return;
        }
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

    fn skip_filegroup_option(&mut self) {
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
        } else if !self.is_eof() {
            self.advance();
        }
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
        let mut foreign_keys = Vec::new();
        let mut check_constraints = Vec::new();
        for fragment in fragments {
            let mut fragment_parser = Parser::from_fragment(fragment, self.line_offset);
            if fragment_parser.is_eof() {
                continue;
            }
            if fragment_parser.consume_kw(Keyword::Constraint) {
                let cname = fragment_parser.ident();
                if fragment_parser.peek_is_kw(Keyword::Primary) {
                    pk = fragment_parser.parse_table_primary_key_constraint(cname);
                } else if fragment_parser.peek_is_kw(Keyword::Unique) {
                    if let Some(unique) = fragment_parser.parse_table_unique_constraint(cname) {
                        unique_constraints.push(unique);
                    }
                } else if fragment_parser.peek_is_kw(Keyword::Check) {
                    if let Some(check) = fragment_parser.parse_table_check_constraint(cname) {
                        check_constraints.push(check);
                    }
                } else if fragment_parser.peek_is_kw(Keyword::Foreign) {
                    if let Some(fk) = fragment_parser.parse_table_foreign_key_constraint(cname) {
                        // Table-level foreign keys declared inline are valid CREATE TABLE fragments.
                        // They are retained with ALTER TABLE foreign keys below.
                        // The vector is attached after parsing the body.
                        foreign_keys.push(fk);
                    }
                } else {
                    fragment_parser.unsupported_table_fragment(&name);
                }
            } else if fragment_parser.peek_is_kw(Keyword::Primary) {
                pk = fragment_parser.parse_table_primary_key_constraint(None);
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
            foreign_keys,
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
        if self.consume_kw(Keyword::With) {
            if self.consume_kw(Keyword::Check) {
                // SQL Server's trusted constraint form. Nothing extra is needed in the model.
            } else if self.consume_kw(Keyword::NoCheck) {
                self.warning("ALTER TABLE WITH NOCHECK constraint was not trusted in SQL Server");
            } else {
                self.unsupported("unsupported ALTER TABLE WITH option");
            }
        }
        if !self.consume_kw(Keyword::Add) {
            self.error("expected ADD in ALTER TABLE");
            self.skip_stmt();
            return;
        }
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
                    self.error(&format!(
                        "ALTER TABLE target table {} was not found for foreign key constraint",
                        table_name.display_sql_server()
                    ));
                }
            }
        } else if self.consume_kw(Keyword::Default) {
            let expression = self.collect_expr_until_kw(Keyword::For);
            if !self.consume_kw(Keyword::For) {
                self.error("expected FOR in ALTER TABLE DEFAULT constraint");
                self.skip_stmt();
                return;
            }
            let Some(column_name) = self.ident() else {
                self.error("expected column name after FOR in ALTER TABLE DEFAULT constraint");
                self.skip_stmt();
                return;
            };
            if let Some(table) = tables.iter_mut().find(|t| t.name == table_name) {
                if let Some(column) = table
                    .columns
                    .iter_mut()
                    .find(|column| column.name.eq_ignore_ascii_case(&column_name))
                {
                    column.default = Some(DefaultConstraintDef {
                        name: constraint_name,
                        expression,
                    });
                } else {
                    self.error(&format!(
                        "ALTER TABLE DEFAULT target column [{}] was not found in table {}",
                        column_name,
                        table_name.display_sql_server()
                    ));
                }
            } else {
                self.error(&format!(
                    "ALTER TABLE target table {} was not found for default constraint",
                    table_name.display_sql_server()
                ));
            }
            self.skip_stmt_tail();
        } else if self.consume_kw(Keyword::Check) {
            let check = CheckConstraintDef {
                name: constraint_name,
                expression: self.collect_parenthesized_expr(),
            };
            if let Some(table) = tables.iter_mut().find(|t| t.name == table_name) {
                table.check_constraints.push(check);
            } else {
                self.error(&format!(
                    "ALTER TABLE target table {} was not found for check constraint",
                    table_name.display_sql_server()
                ));
            }
            self.skip_stmt_tail();
        } else {
            self.unsupported("unsupported ALTER TABLE constraint");
            self.skip_stmt();
        }
    }
    fn parse_table_primary_key_constraint(
        &mut self,
        name: Option<String>,
    ) -> Option<PrimaryKeyDef> {
        self.consume_kw(Keyword::Primary);
        self.consume_kw(Keyword::Key);
        let clustered = if self.consume_kw(Keyword::Clustered) {
            Some(true)
        } else if self.consume_kw(Keyword::NonClustered) {
            Some(false)
        } else {
            None
        };
        let columns = self.parse_column_list()?;
        self.diagnose_unsupported_constraint_tail("PRIMARY KEY");
        Some(PrimaryKeyDef {
            name,
            columns,
            clustered,
        })
    }

    fn parse_table_unique_constraint(
        &mut self,
        name: Option<String>,
    ) -> Option<UniqueConstraintDef> {
        self.consume_kw(Keyword::Unique);
        let _ = self.consume_kw(Keyword::Clustered) || self.consume_kw(Keyword::NonClustered);
        let columns = self.parse_column_list()?;
        self.diagnose_unsupported_constraint_tail("UNIQUE");
        Some(UniqueConstraintDef { name, columns })
    }

    fn parse_table_check_constraint(&mut self, name: Option<String>) -> Option<CheckConstraintDef> {
        self.consume_kw(Keyword::Check);
        let expression = self.collect_parenthesized_expr();
        self.diagnose_unsupported_constraint_tail("CHECK");
        Some(CheckConstraintDef { name, expression })
    }

    fn parse_table_foreign_key_constraint(
        &mut self,
        name: Option<String>,
    ) -> Option<ForeignKeyDef> {
        self.consume_kw(Keyword::Foreign);
        self.consume_kw(Keyword::Key);
        let fk = self.parse_fk(name)?;
        self.diagnose_unsupported_constraint_tail("FOREIGN KEY");
        Some(fk)
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
            SmallMoney => SqlServerType::SmallMoney,
            Real => SqlServerType::Real,
            Date => SqlServerType::Date,
            DateTime => SqlServerType::DateTime,
            SmallDateTime => SqlServerType::SmallDateTime,
            DateTimeOffset => SqlServerType::DateTimeOffset {
                scale: num8(args.first()),
            },
            UniqueIdentifier => SqlServerType::UniqueIdentifier,
            Text => SqlServerType::Text,
            NText => SqlServerType::NText,
            Image => SqlServerType::Image,
            RowVersion => SqlServerType::RowVersion,
            Timestamp => SqlServerType::Timestamp,
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
            "SMALLMONEY" => SqlServerType::SmallMoney,
            "REAL" => SqlServerType::Real,
            "DATE" => SqlServerType::Date,
            "DATETIME" => SqlServerType::DateTime,
            "DATETIME2" => SqlServerType::DateTime2 {
                scale: num8(args.first()),
            },
            "SMALLDATETIME" => SqlServerType::SmallDateTime,
            "DATETIMEOFFSET" => SqlServerType::DateTimeOffset {
                scale: num8(args.first()),
            },
            "UNIQUEIDENTIFIER" => SqlServerType::UniqueIdentifier,
            "TEXT" => SqlServerType::Text,
            "NTEXT" => SqlServerType::NText,
            "IMAGE" => SqlServerType::Image,
            "ROWVERSION" => SqlServerType::RowVersion,
            "TIMESTAMP" => SqlServerType::Timestamp,
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
    fn parse_fk(&mut self, name: Option<String>) -> Option<ForeignKeyDef> {
        let columns = self.parse_column_list()?;
        self.consume_kw(Keyword::References);
        let referenced_table = self.parse_table_name()?;
        let referenced_columns = self.parse_column_list().unwrap_or_default();
        let mut on_delete = None;
        let mut on_update = None;
        while self.consume_kw(Keyword::On) {
            if self.consume_kw(Keyword::Delete) {
                on_delete = self.parse_referential_action();
            } else if self.consume_kw(Keyword::Update) {
                on_update = self.parse_referential_action();
            } else {
                self.unsupported("unsupported FOREIGN KEY ON option");
                break;
            }
        }
        self.skip_stmt_tail();
        Some(ForeignKeyDef {
            name,
            columns,
            referenced_table,
            referenced_columns,
            on_delete,
            on_update,
        })
    }

    fn parse_referential_action(&mut self) -> Option<ReferentialAction> {
        if self.consume_kw(Keyword::Cascade) {
            Some(ReferentialAction::Cascade)
        } else if self.consume_kw(Keyword::Set) {
            if self.consume_kw(Keyword::Null) {
                Some(ReferentialAction::SetNull)
            } else if self.consume_kw(Keyword::Default) {
                Some(ReferentialAction::SetDefault)
            } else {
                self.unsupported("unsupported FOREIGN KEY SET action");
                None
            }
        } else if self.consume_kw(Keyword::No) {
            if self.consume_kw(Keyword::Action) {
                Some(ReferentialAction::NoAction)
            } else {
                self.unsupported("unsupported FOREIGN KEY NO action");
                None
            }
        } else {
            self.unsupported("unsupported FOREIGN KEY referential action");
            None
        }
    }
    fn parse_column_list(&mut self) -> Option<Vec<String>> {
        if !self.expect_sym('(') {
            return None;
        }
        let mut columns = Vec::new();
        loop {
            if self.is_eof() || self.consume_sym(')') {
                break;
            }
            if let Some(column) = self.ident() {
                columns.push(column);
            } else {
                self.advance();
            }
            while !self.is_eof() && !self.at_sym(',') && !self.at_sym(')') {
                // SQL Server allows per-column sort directions in key lists. They do not affect
                // the schema model, so skip them (and any other per-column options) without
                // treating them as additional columns.
                self.advance();
            }
            if self.consume_sym(',') {
                continue;
            }
        }
        Some(columns)
    }

    fn diagnose_unsupported_constraint_tail(&mut self, constraint_kind: &str) {
        if self.is_eof() {
            return;
        }
        let mut trailing = Vec::new();
        while !self.is_eof() {
            trailing.push(self.advance().lexeme.clone());
        }
        if !trailing.is_empty() {
            self.unsupported(&format!(
                "unsupported {constraint_kind} constraint options ignored: {}",
                trailing.join(" ")
            ));
        }
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
    fn collect_expr_until_kw(&mut self, keyword: Keyword) -> String {
        let mut s = String::new();
        let mut depth = 0i32;
        while !self.is_eof() {
            if depth == 0 && self.peek_is_kw(keyword) {
                break;
            }
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
    fn warning(&mut self, msg: &str) {
        self.diagnostics.push(SchemaDiagnostic {
            severity: DiagnosticSeverity::Warning,
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
        if self.peek_is_kw(k) {
            self.advance();
            true
        } else {
            false
        }
    }
    fn peek_is_kw(&self, k: Keyword) -> bool {
        self.peek().kind == TokenKind::Keyword(k)
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
    let no_space_before =
        matches!(lexeme, ")" | "," | "." | ";") || (lexeme == "(" && !output.ends_with(" IN"));
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
    fn parses_named_composite_clustered_primary_key_with_sort_directions() {
        let s = parse(
            "CREATE TABLE OrderLine (OrderId int, LineNumber int, CONSTRAINT [PK_OrderLine] PRIMARY KEY CLUSTERED ([OrderId] ASC, [LineNumber] ASC));",
        );
        let pk = s.tables[0].primary_key.as_ref().unwrap();
        assert_eq!(pk.name.as_deref(), Some("PK_OrderLine"));
        assert_eq!(pk.columns, vec!["OrderId", "LineNumber"]);
        assert_eq!(pk.clustered, Some(true));
        assert!(s.diagnostics.is_empty());
    }

    #[test]
    fn parses_named_unique_nonclustered_constraint_with_sort_direction() {
        let s = parse(
            "CREATE TABLE Customer (Email nvarchar(320), CONSTRAINT [UQ_Customer_Email] UNIQUE NONCLUSTERED ([Email] ASC));",
        );
        let unique = &s.tables[0].unique_constraints[0];
        assert_eq!(unique.name.as_deref(), Some("UQ_Customer_Email"));
        assert_eq!(unique.columns, vec!["Email"]);
        assert!(s.diagnostics.is_empty());
    }

    #[test]
    fn parses_normal_nonunique_nonclustered_index() {
        let s = parse(
            "CREATE NONCLUSTERED INDEX [IX_Order_CustomerId] ON [dbo].[Order] ([CustomerId] ASC);",
        );
        let index = &s.indexes[0];
        assert_eq!(index.name, "IX_Order_CustomerId");
        assert_eq!(index.table, TableName::new(Some("dbo".into()), "Order"));
        assert_eq!(index.columns, vec!["CustomerId"]);
        assert!(!index.unique);
        assert_eq!(index.clustered, Some(false));
        assert_eq!(index.filter, None);
    }

    #[test]
    fn parses_unique_nonclustered_index() {
        let s = parse(
            "CREATE UNIQUE NONCLUSTERED INDEX [UX_Customer_Email] ON [dbo].[Customer] ([Email] ASC);",
        );
        let index = &s.indexes[0];
        assert_eq!(index.name, "UX_Customer_Email");
        assert_eq!(index.table, TableName::new(Some("dbo".into()), "Customer"));
        assert_eq!(index.columns, vec!["Email"]);
        assert!(index.unique);
        assert_eq!(index.clustered, Some(false));
    }

    #[test]
    fn parses_index_columns_and_ignores_sort_directions() {
        let s = parse("CREATE INDEX IX_T_AB ON T (A DESC, B ASC);");
        assert_eq!(s.indexes[0].columns, vec!["A", "B"]);
        assert!(s.diagnostics.is_empty());
    }

    #[test]
    fn parses_filtered_index_expression() {
        let s = parse(
            "CREATE INDEX [IX_Order_Open] ON [dbo].[Order] ([Status]) WHERE [Status] = 'Open';",
        );
        let index = &s.indexes[0];
        assert_eq!(index.name, "IX_Order_Open");
        assert_eq!(index.columns, vec!["Status"]);
        assert_eq!(index.filter.as_deref(), Some("Status = 'Open'"));
    }

    #[test]
    fn diagnoses_unsupported_index_options_and_include_columns() {
        let s =
            parse("CREATE INDEX IX_T_A ON T (A) INCLUDE (B) WITH (FILLFACTOR = 80) ON [PRIMARY];");
        assert_eq!(s.indexes[0].columns, vec!["A"]);
        assert!(s
            .diagnostics
            .iter()
            .any(|d| d.message.contains("unsupported INCLUDE columns")));
        assert!(s
            .diagnostics
            .iter()
            .any(|d| d.message.contains("unsupported WITH options")));
        assert!(s
            .diagnostics
            .iter()
            .any(|d| d.message.contains("unsupported filegroup option")));
    }

    #[test]
    fn parses_named_check_constraint_raw_expression() {
        let s = parse(
            "CREATE TABLE Customer (IsActive bit, CONSTRAINT [CK_Customer_IsActive] CHECK ([IsActive] IN ((0),(1))));",
        );
        let check = &s.tables[0].check_constraints[0];
        assert_eq!(check.name.as_deref(), Some("CK_Customer_IsActive"));
        assert_eq!(check.expression, "IsActive IN ((0),(1))");
    }

    #[test]
    fn ignores_desc_sort_direction_without_corrupting_columns() {
        let s =
            parse("CREATE TABLE T (A int, B int, CONSTRAINT PK_T PRIMARY KEY (A DESC, B ASC));");
        let pk = s.tables[0].primary_key.as_ref().unwrap();
        assert_eq!(pk.columns, vec!["A", "B"]);
    }

    #[test]
    fn stores_nonclustered_primary_key_and_ignores_clustered_unique() {
        let s = parse("CREATE TABLE T (A int, B int, CONSTRAINT PK_T PRIMARY KEY NONCLUSTERED (A), CONSTRAINT UQ_T_B UNIQUE CLUSTERED (B DESC));");
        assert_eq!(
            s.tables[0].primary_key.as_ref().unwrap().clustered,
            Some(false)
        );
        assert_eq!(s.tables[0].primary_key.as_ref().unwrap().columns, vec!["A"]);
        assert_eq!(s.tables[0].unique_constraints[0].columns, vec!["B"]);
    }

    #[test]
    fn diagnoses_unsupported_constraint_options_after_columns() {
        let s = parse("CREATE TABLE T (A int, CONSTRAINT PK_T PRIMARY KEY (A) WITH (IGNORE_DUP_KEY = OFF) ON [PRIMARY]);");
        assert_eq!(s.tables[0].primary_key.as_ref().unwrap().columns, vec!["A"]);
        assert!(s.diagnostics.iter().any(|d| d
            .message
            .contains("unsupported PRIMARY KEY constraint options ignored")));
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
        let s = parse("CREATE TABLE T (A int, B bigint, C smallint, D tinyint, E bit, F decimal(10,2), G numeric(8,3), H money, H2 smallmoney, I float, J real, K char(5), L varchar(20), M varchar(max), N nchar(4), O nvarchar(30), P nvarchar(max), Q text, R ntext, S date, T datetimeoffset, U datetime, V datetime2, W smalldatetime, X time, Y uniqueidentifier, Z binary, AA varbinary, AB varbinary(max), AC image, AD rowversion, AE timestamp, AF xml);");
        let columns = &s.tables[0].columns;
        assert_eq!(columns[0].data_type, SqlServerType::Int);
        assert_eq!(
            columns[5].data_type,
            SqlServerType::Decimal {
                precision: Some(10),
                scale: Some(2)
            }
        );
        assert_eq!(columns[8].data_type, SqlServerType::SmallMoney);
        assert_eq!(
            columns[13].data_type,
            SqlServerType::VarChar {
                length: None,
                max: true
            }
        );
        assert_eq!(
            columns[16].data_type,
            SqlServerType::NVarChar {
                length: None,
                max: true
            }
        );
        assert_eq!(
            columns[26].data_type,
            SqlServerType::Binary { length: None }
        );
        assert_eq!(
            columns[28].data_type,
            SqlServerType::VarBinary {
                length: None,
                max: true
            }
        );
        assert_eq!(
            columns[20].data_type,
            SqlServerType::DateTimeOffset { scale: None }
        );
        assert_eq!(columns[29].data_type, SqlServerType::Image);
        assert_eq!(columns[30].data_type, SqlServerType::RowVersion);
        assert_eq!(columns[31].data_type, SqlServerType::Timestamp);
        assert_eq!(columns[32].data_type, SqlServerType::Xml);
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
    fn alter_table_adds_foreign_key_with_reference_details_and_actions() {
        let s = parse("CREATE TABLE Parent (Id int); CREATE TABLE Child (ParentId int); ALTER TABLE Child ADD CONSTRAINT FK_Child_Parent FOREIGN KEY (ParentId) REFERENCES Parent (Id) ON DELETE CASCADE ON UPDATE NO ACTION;");
        let fk = &s.tables[1].foreign_keys[0];
        assert_eq!(fk.name.as_deref(), Some("FK_Child_Parent"));
        assert_eq!(fk.columns, vec!["ParentId"]);
        assert_eq!(
            fk.referenced_table,
            TableName::new(Some("dbo".into()), "Parent")
        );
        assert_eq!(fk.referenced_columns, vec!["Id"]);
        assert_eq!(fk.on_delete, Some(ReferentialAction::Cascade));
        assert_eq!(fk.on_update, Some(ReferentialAction::NoAction));
    }

    #[test]
    fn alter_table_default_constraint_attaches_to_target_column() {
        let s = parse("CREATE TABLE T (Id int, Created datetime); ALTER TABLE T ADD CONSTRAINT DF_T_Created DEFAULT (GETDATE()) FOR Created;");
        let default = s.tables[0].columns[1].default.as_ref().unwrap();
        assert_eq!(default.name.as_deref(), Some("DF_T_Created"));
        assert_eq!(default.expression, "(GETDATE())");
    }

    #[test]
    fn alter_table_check_constraint_is_stored() {
        let s = parse("CREATE TABLE T (Amount int); ALTER TABLE T ADD CONSTRAINT CK_T_Amount CHECK ([Amount] > 0);");
        let check = &s.tables[0].check_constraints[0];
        assert_eq!(check.name.as_deref(), Some("CK_T_Amount"));
        assert_eq!(check.expression, "Amount > 0");
    }

    #[test]
    fn alter_table_with_check_is_accepted_and_nocheck_warns() {
        let trusted = parse("CREATE TABLE P (Id int); CREATE TABLE C (Pid int); ALTER TABLE C WITH CHECK ADD CONSTRAINT FK_C_P FOREIGN KEY (Pid) REFERENCES P (Id);");
        assert_eq!(trusted.tables[1].foreign_keys.len(), 1);
        assert!(trusted.diagnostics.is_empty());

        let untrusted = parse("CREATE TABLE P (Id int); CREATE TABLE C (Pid int); ALTER TABLE C WITH NOCHECK ADD CONSTRAINT FK_C_P FOREIGN KEY (Pid) REFERENCES P (Id);");
        assert_eq!(untrusted.tables[1].foreign_keys.len(), 1);
        assert!(untrusted.diagnostics.iter().any(
            |d| d.severity == DiagnosticSeverity::Warning && d.message.contains("not trusted")
        ));
    }

    #[test]
    fn alter_table_missing_table_or_column_reports_diagnostic() {
        let missing_table = parse("ALTER TABLE Missing ADD CONSTRAINT CK_Missing CHECK (Id > 0);");
        assert!(missing_table
            .diagnostics
            .iter()
            .any(|d| d.severity == DiagnosticSeverity::Error
                && d.message.contains("[dbo].[Missing]")));

        let missing_column = parse("CREATE TABLE T (Id int); ALTER TABLE T ADD CONSTRAINT DF_T_Missing DEFAULT (0) FOR Missing;");
        assert!(missing_column
            .diagnostics
            .iter()
            .any(|d| d.severity == DiagnosticSeverity::Error
                && d.message.contains("Missing")
                && d.message.contains("[dbo].[T]")));
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
