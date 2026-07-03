use super::lexer::{lex, Keyword, TokenKind};
use super::preprocessor::SqlBatch;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum StatementKind {
    CreateTable,
    AlterTableAddConstraint,
    CreateIndex,
    CreateView,
    CreateTrigger,
    CreateProcedure,
    SetOption,
    UseDatabase,
    Unknown,
}

impl StatementKind {
    pub const fn label(self) -> &'static str {
        match self {
            Self::CreateTable => "CREATE TABLE",
            Self::AlterTableAddConstraint => "ALTER TABLE ADD CONSTRAINT",
            Self::CreateIndex => "CREATE INDEX",
            Self::CreateView => "CREATE VIEW",
            Self::CreateTrigger => "CREATE TRIGGER",
            Self::CreateProcedure => "CREATE PROCEDURE",
            Self::SetOption => "SET",
            Self::UseDatabase => "USE",
            Self::Unknown => "unknown",
        }
    }

    pub const fn is_supported_v1(self) -> bool {
        matches!(
            self,
            Self::CreateTable | Self::AlterTableAddConstraint | Self::CreateIndex
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClassifiedStatement {
    pub kind: StatementKind,
    pub batch: SqlBatch,
    pub line_start: usize,
    pub line_end: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ClassificationSummary {
    pub detected_count: usize,
    pub ignored_count: usize,
    pub warning_count: usize,
    pub detected: BTreeMap<StatementKind, usize>,
    pub ignored: BTreeMap<StatementKind, usize>,
    pub warnings: BTreeMap<StatementKind, usize>,
}

impl ClassificationSummary {
    pub fn record_detected(&mut self, kind: StatementKind) {
        self.detected_count += 1;
        *self.detected.entry(kind).or_default() += 1;
    }
    pub fn record_ignored(&mut self, kind: StatementKind) {
        self.ignored_count += 1;
        *self.ignored.entry(kind).or_default() += 1;
    }
    pub fn record_warning(&mut self, kind: StatementKind) {
        self.warning_count += 1;
        *self.warnings.entry(kind).or_default() += 1;
    }
}

pub fn classify_batches(batches: &[SqlBatch]) -> Vec<ClassifiedStatement> {
    batches.iter().flat_map(classify_batch).collect()
}

pub fn summarize(statements: &[ClassifiedStatement]) -> ClassificationSummary {
    let mut summary = ClassificationSummary::default();
    for statement in statements {
        if statement.kind.is_supported_v1() {
            summary.record_detected(statement.kind);
        } else {
            summary.record_ignored(statement.kind);
            summary.record_warning(statement.kind);
        }
    }
    summary
}

fn classify_batch(batch: &SqlBatch) -> Vec<ClassifiedStatement> {
    split_statements(batch)
        .into_iter()
        .map(|stmt| {
            let kind = classify_text(&stmt.original_text);
            ClassifiedStatement {
                kind,
                line_start: stmt.line_start,
                line_end: stmt.line_end,
                batch: stmt,
            }
        })
        .collect()
}

pub fn classify_text(text: &str) -> StatementKind {
    let meaningful = lex(text)
        .into_iter()
        .filter(|t| !matches!(t.kind, TokenKind::Eof | TokenKind::Symbol(';')))
        .collect::<Vec<_>>();
    let kw = |i: usize, k: Keyword| {
        meaningful
            .get(i)
            .is_some_and(|t| t.kind == TokenKind::Keyword(k))
    };
    if kw(0, Keyword::Create) {
        let mut i = 1;
        if kw(i, Keyword::Unique) {
            i += 1;
        }
        if kw(i, Keyword::Clustered) || kw(i, Keyword::NonClustered) {
            i += 1;
        }
        if kw(i, Keyword::Table) {
            return StatementKind::CreateTable;
        }
        if kw(i, Keyword::Index) {
            return StatementKind::CreateIndex;
        }
        if kw(i, Keyword::View) {
            return StatementKind::CreateView;
        }
        if kw(i, Keyword::Trigger) {
            return StatementKind::CreateTrigger;
        }
        if kw(i, Keyword::Procedure)
            || meaningful
                .get(i)
                .is_some_and(|t| t.lexeme.eq_ignore_ascii_case("proc"))
        {
            return StatementKind::CreateProcedure;
        }
    }
    if kw(0, Keyword::Alter) && kw(1, Keyword::Table) {
        let has_add_constraint = meaningful.windows(2).any(|w| {
            w[0].kind == TokenKind::Keyword(Keyword::Add)
                && w[1].kind == TokenKind::Keyword(Keyword::Constraint)
        });
        if has_add_constraint {
            return StatementKind::AlterTableAddConstraint;
        }
    }
    if kw(0, Keyword::Set) {
        return StatementKind::SetOption;
    }
    if kw(0, Keyword::Use) {
        return StatementKind::UseDatabase;
    }
    StatementKind::Unknown
}

fn split_statements(batch: &SqlBatch) -> Vec<SqlBatch> {
    let mut out = Vec::new();
    let mut start = 0usize;
    let mut line = batch.line_start;
    let mut start_line = batch.line_start;
    let mut state = 0u8; // 0 normal, 1 string, 2 bracket
    for (idx, ch) in batch.original_text.char_indices() {
        match state {
            0 if ch == '\'' => state = 1,
            0 if ch == '[' => state = 2,
            0 if ch == ';' => {
                push_piece(
                    &mut out,
                    batch,
                    start,
                    idx + ch.len_utf8(),
                    start_line,
                    line,
                );
                start = idx + ch.len_utf8();
                start_line = line;
            }
            1 if ch == '\'' => state = 0,
            2 if ch == ']' => state = 0,
            _ => {}
        }
        if ch == '\n' {
            line += 1;
            if batch.original_text[start..=idx].trim().is_empty() {
                start_line = line;
            }
        }
    }
    push_piece(
        &mut out,
        batch,
        start,
        batch.original_text.len(),
        start_line,
        batch.line_end,
    );
    out
}

fn push_piece(
    out: &mut Vec<SqlBatch>,
    batch: &SqlBatch,
    start: usize,
    end: usize,
    line_start: usize,
    line_end: usize,
) {
    let text = batch.original_text[start..end].to_owned();
    if text.trim().is_empty() {
        return;
    }
    out.push(SqlBatch {
        original_text: text,
        line_start,
        line_end: line_end.max(line_start),
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn classifies_required_kinds() {
        let cases = [
            ("CREATE TABLE T (Id int);", StatementKind::CreateTable),
            (
                "ALTER TABLE T ADD CONSTRAINT C CHECK (Id > 0);",
                StatementKind::AlterTableAddConstraint,
            ),
            (
                "CREATE UNIQUE NONCLUSTERED INDEX IX ON T (Id);",
                StatementKind::CreateIndex,
            ),
            ("CREATE VIEW V AS SELECT 1;", StatementKind::CreateView),
            (
                "CREATE TRIGGER Tr ON T AFTER INSERT AS SELECT 1;",
                StatementKind::CreateTrigger,
            ),
            (
                "CREATE PROCEDURE P AS SELECT 1;",
                StatementKind::CreateProcedure,
            ),
            ("SET ANSI_NULLS ON;", StatementKind::SetOption),
            ("USE Db;", StatementKind::UseDatabase),
            ("SELECT 1;", StatementKind::Unknown),
        ];
        for (sql, kind) in cases {
            assert_eq!(classify_text(sql), kind);
        }
    }
}
