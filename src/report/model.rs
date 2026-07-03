//! Report model types.

use serde::{Deserialize, Serialize};

/// Conversion and import report model.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Report {
    pub import: Option<ImportReport>,
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
