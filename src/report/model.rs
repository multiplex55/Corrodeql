//! Report model types.

use serde::{Deserialize, Serialize};

/// Complete conversion report written in text and JSON formats.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConversionReport {
    pub input_schema_path: String,
    pub data_directory: String,
    pub output_database_path: String,
    pub schema: SchemaSummary,
    pub statements: StatementReport,
    pub import: ImportReport,
    pub validation: ValidationReport,
    pub diagnostics: Vec<Diagnostic>,
    pub unsupported_sql_server_features: Vec<String>,
}

/// Backwards-compatible report type name.
pub type Report = ConversionReport;

/// Summary of classified SQL statements before deep parsing.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct StatementReport {
    pub detected_count: usize,
    pub ignored_count: usize,
    pub warning_count: usize,
    pub detected: Vec<StatementKindReport>,
    pub ignored: Vec<StatementKindReport>,
    pub warnings: Vec<StatementKindReport>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StatementKindReport {
    pub kind: String,
    pub count: usize,
}

/// Summary of parsed schema objects.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SchemaSummary {
    pub tables_detected: usize,
    pub columns_detected: usize,
    pub constraints_detected: usize,
    pub indexes_detected: usize,
    pub tables: Vec<TableReport>,
}

/// Per-table schema summary.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TableReport {
    pub source_table: String,
    pub sqlite_table: String,
    pub columns_detected: usize,
    pub constraints_detected: usize,
    pub indexes_detected: usize,
    pub columns: Vec<String>,
    pub constraints: Vec<String>,
    pub indexes: Vec<String>,
}

/// Aggregate import diagnostics and counters.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportReport {
    pub rows_read: u64,
    pub rows_inserted: u64,
    pub rows_rejected: u64,
    pub tables: Vec<TableImportReport>,
}

/// Per-table import diagnostics and counters.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TableImportReport {
    pub source_table: String,
    pub sqlite_table: String,
    pub csv_path: Option<String>,
    pub status: TableImportStatus,
    pub rows_read: u64,
    pub rows_inserted: u64,
    pub rows_rejected: u64,
    pub diagnostics: Vec<String>,
}

/// Per-table import outcome.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TableImportStatus {
    Imported,
    Partial,
    Skipped,
}

/// Validation status included in conversion reports.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ValidationReport {
    pub attempted: bool,
    pub success: bool,
    pub tables_validated: usize,
    pub row_count_validation: RowCountValidationReport,
    pub integrity_check: IntegrityCheckReport,
    pub diagnostics: Vec<Diagnostic>,
}

/// SQLite `PRAGMA integrity_check` results included in conversion reports.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct IntegrityCheckReport {
    pub success: bool,
    pub results: Vec<String>,
}

/// Optional row-count manifest validation status included in conversion reports.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RowCountValidationReport {
    pub status: RowCountValidationStatus,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RowCountValidationStatus {
    #[default]
    Skipped,
    Validated,
    Failed,
}

/// A warning, error, or unsupported-feature note emitted during conversion.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Diagnostic {
    pub severity: DiagnosticSeverity,
    pub message: String,
}

/// Severity for report diagnostics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticSeverity {
    Warning,
    Error,
    Unsupported,
}
