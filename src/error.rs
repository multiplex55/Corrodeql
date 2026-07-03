//! Error types and result aliases.

use camino::Utf8PathBuf;

/// Crate-wide result type.
pub type Result<T> = std::result::Result<T, Error>;

/// Crate-wide domain errors.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("invalid option {option}: {value}; expected {expected}")]
    InvalidOption {
        option: &'static str,
        value: String,
        expected: &'static str,
    },
    #[error("invalid value for {option}: {value}; expected {expected}")]
    InvalidOptionValue {
        option: &'static str,
        value: String,
        expected: &'static str,
    },
    #[error("invalid path for {kind}: {path} ({reason})")]
    InvalidPath {
        kind: &'static str,
        path: Utf8PathBuf,
        reason: &'static str,
    },
    #[error("parse error: {message}")]
    Parse { message: String },
    #[error("unsupported schema construct: {message}")]
    UnsupportedSchemaConstruct { message: String },
    #[error("CSV discovery error: {message}")]
    CsvDiscovery { message: String },
    #[error("CSV import error for table {table}, column {column}, row {row_number}: {message}")]
    CsvReadImport {
        table: String,
        column: String,
        row_number: u64,
        message: String,
    },
    #[error("SQLite DDL/database error: {message}")]
    SqliteDdlDatabase { message: String },
    #[error("validation error: {message}")]
    Validation { message: String },
    #[error("report writing error: {message}")]
    ReportWriting { message: String },
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("CSV error: {0}")]
    Csv(#[from] csv::Error),
    #[error("SQLite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_strings_are_user_readable() {
        let error = Error::UnsupportedSchemaConstruct {
            message: "filtered indexes are not supported".to_owned(),
        };
        assert_eq!(
            error.to_string(),
            "unsupported schema construct: filtered indexes are not supported"
        );
    }

    #[test]
    fn invalid_table_name_mode_includes_invalid_value() {
        let error = Error::InvalidOptionValue {
            option: "table_name_mode",
            value: "bad-mode".to_owned(),
            expected: "schema-prefix, drop-dbo, or table-only",
        };
        assert!(error.to_string().contains("bad-mode"));
    }

    #[test]
    fn missing_path_error_includes_path() {
        let error = Error::InvalidPath {
            kind: "schema_path",
            path: Utf8PathBuf::from("/tmp/missing-schema.sql"),
            reason: "path does not exist",
        };
        assert!(error.to_string().contains("/tmp/missing-schema.sql"));
    }

    #[test]
    fn csv_row_conversion_error_includes_table_column_and_row() {
        let error = Error::CsvReadImport {
            table: "[dbo].[Widget]".to_owned(),
            column: "Quantity".to_owned(),
            row_number: 42,
            message: "invalid integer".to_owned(),
        };
        let display = error.to_string();
        assert!(display.contains("[dbo].[Widget]"));
        assert!(display.contains("Quantity"));
        assert!(display.contains("42"));
    }
}
