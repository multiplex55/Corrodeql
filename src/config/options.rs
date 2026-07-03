use std::fmt;
use std::str::FromStr;

use camino::Utf8PathBuf;
use clap::ValueEnum;

use crate::error::Error;

/// Fully resolved options for a conversion run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConvertOptions {
    pub schema_path: Utf8PathBuf,
    pub data_dir: Utf8PathBuf,
    pub output_db_path: Utf8PathBuf,
    pub overwrite: bool,
    pub null_token: String,
    pub table_name_mode: TableNameMode,
    pub emit_ddl_path: Option<Utf8PathBuf>,
    pub report_dir: Option<Utf8PathBuf>,
    pub strict: bool,
    pub allow_missing_csv: bool,
    pub allow_extra_csv_columns: bool,
    pub skip_foreign_key_check: bool,
    pub dry_run: bool,
}

impl ConvertOptions {
    pub const DEFAULT_NULL_TOKEN: &'static str = r"\N";
}

impl Default for ConvertOptions {
    fn default() -> Self {
        Self {
            schema_path: Utf8PathBuf::new(),
            data_dir: Utf8PathBuf::new(),
            output_db_path: Utf8PathBuf::new(),
            overwrite: false,
            null_token: Self::DEFAULT_NULL_TOKEN.to_string(),
            table_name_mode: TableNameMode::default(),
            emit_ddl_path: None,
            report_dir: None,
            strict: false,
            allow_missing_csv: false,
            allow_extra_csv_columns: false,
            skip_foreign_key_check: false,
            dry_run: false,
        }
    }
}

/// Table naming strategies accepted by conversion commands.
#[derive(Debug, Clone, Copy, ValueEnum, PartialEq, Eq, Default)]
#[value(rename_all = "kebab-case")]
pub enum TableNameMode {
    /// Prefix table names with their schema name.
    #[default]
    SchemaPrefix,
    /// Drop the `dbo` schema prefix while preserving non-dbo schema names.
    DropDbo,
    /// Use only the table name, ignoring schema names.
    TableOnly,
}

impl TableNameMode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::SchemaPrefix => "schema-prefix",
            Self::DropDbo => "drop-dbo",
            Self::TableOnly => "table-only",
        }
    }
}

impl fmt::Display for TableNameMode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for TableNameMode {
    type Err = Error;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "schema-prefix" => Ok(Self::SchemaPrefix),
            "drop-dbo" => Ok(Self::DropDbo),
            "table-only" => Ok(Self::TableOnly),
            _ => Err(Error::InvalidOptionValue {
                option: "table_name_mode",
                value: value.to_string(),
                expected: "schema-prefix, drop-dbo, or table-only",
            }),
        }
    }
}

/// Top-level application options.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Options;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_table_name_mode_is_schema_prefix() {
        assert_eq!(TableNameMode::default(), TableNameMode::SchemaPrefix);
        assert_eq!(
            ConvertOptions::default().table_name_mode,
            TableNameMode::SchemaPrefix
        );
    }

    #[test]
    fn parses_valid_table_name_modes() {
        assert_eq!(
            "schema-prefix".parse::<TableNameMode>().unwrap(),
            TableNameMode::SchemaPrefix
        );
        assert_eq!(
            "drop-dbo".parse::<TableNameMode>().unwrap(),
            TableNameMode::DropDbo
        );
        assert_eq!(
            "table-only".parse::<TableNameMode>().unwrap(),
            TableNameMode::TableOnly
        );
    }

    #[test]
    fn rejects_invalid_table_name_mode() {
        assert!("invalid".parse::<TableNameMode>().is_err());
    }

    #[test]
    fn preserves_null_token_default() {
        assert_eq!(ConvertOptions::default().null_token, r"\N");
    }
}
