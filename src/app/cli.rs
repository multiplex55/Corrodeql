use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};

pub use crate::config::options::TableNameMode;

/// CorrodeQL command-line parser.
#[derive(Debug, Clone, Parser, PartialEq, Eq)]
#[command(name = "corrodeql")]
#[command(version, about = "CLI tooling for CorrodeQL.")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,
}

/// Commands supported by the CorrodeQL CLI.
#[derive(Debug, Clone, Subcommand, PartialEq, Eq)]
pub enum Command {
    /// Convert schema and CSV data into a SQLite database.
    Convert(ConvertArgs),
    /// Inspect an input schema and print schema metadata.
    InspectSchema(InspectSchemaArgs),
    /// Emit SQLite DDL from an input schema.
    EmitDdl(EmitDdlArgs),
    /// Validate schema and data inputs without converting them.
    Validate(ValidateArgs),
    /// Write a small example project to disk.
    InitExample(InitExampleArgs),
}

/// Arguments for `corrodeql convert`.
#[derive(Debug, Clone, Args, Default, PartialEq, Eq)]
pub struct ConvertArgs {
    /// MSSQL schema file to convert. If omitted, interactive prompting may ask for it.
    #[arg(long, value_name = "FILE")]
    pub schema: Option<PathBuf>,

    /// Directory containing CSV files. If omitted, interactive prompting may ask for it.
    #[arg(long, value_name = "DIR")]
    pub data_dir: Option<PathBuf>,

    /// SQLite database file to create. If omitted, interactive prompting may ask for it.
    #[arg(long, value_name = "FILE")]
    pub out: Option<PathBuf>,

    /// Allow replacing an existing output file.
    #[arg(long)]
    pub overwrite: bool,

    /// Token to treat as a SQL NULL value while reading CSV files.
    #[arg(long, value_name = "TOKEN")]
    pub null_token: Option<String>,

    /// Strategy for converting SQL Server schema/table names to SQLite table names.
    #[arg(long, value_enum, value_name = "schema-prefix|drop-dbo|table-only")]
    pub table_name_mode: Option<TableNameMode>,

    /// Optional file where generated SQLite DDL should be written.
    #[arg(long, value_name = "FILE")]
    pub emit_ddl: Option<PathBuf>,

    /// Optional directory where conversion reports should be written.
    #[arg(long, value_name = "DIR")]
    pub report_dir: Option<PathBuf>,

    /// Treat warnings as errors where supported.
    #[arg(long)]
    pub strict: bool,

    /// Continue when an expected CSV file is missing.
    #[arg(long)]
    pub allow_missing_csv: bool,

    /// Continue when CSV files contain columns not present in the schema.
    #[arg(long)]
    pub allow_extra_csv_columns: bool,

    /// Skip foreign-key validation checks.
    #[arg(long)]
    pub skip_foreign_key_check: bool,

    /// Validate command routing and inputs without writing output.
    #[arg(long)]
    pub dry_run: bool,
}

/// Arguments for `corrodeql inspect-schema`.
#[derive(Debug, Clone, Args, Default, PartialEq, Eq)]
pub struct InspectSchemaArgs {
    /// MSSQL schema file to inspect. If omitted, interactive prompting may ask for it.
    #[arg(long, value_name = "FILE")]
    pub schema: Option<PathBuf>,
}

/// Arguments for `corrodeql emit-ddl`.
#[derive(Debug, Clone, Args, Default, PartialEq, Eq)]
pub struct EmitDdlArgs {
    /// MSSQL schema file to read. If omitted, interactive prompting may ask for it.
    #[arg(long, value_name = "FILE")]
    pub schema: Option<PathBuf>,

    /// File to write generated SQLite DDL to. If omitted, DDL may be printed to stdout.
    #[arg(long, value_name = "FILE")]
    pub out: Option<PathBuf>,
}

/// Arguments for `corrodeql validate`.
#[derive(Debug, Clone, Args, Default, PartialEq, Eq)]
pub struct ValidateArgs {
    /// MSSQL schema file to validate. If omitted, interactive prompting may ask for it.
    #[arg(long, value_name = "FILE")]
    pub schema: Option<PathBuf>,

    /// Directory containing CSV files to validate. If omitted, interactive prompting may ask for it.
    #[arg(long, value_name = "DIR")]
    pub data_dir: Option<PathBuf>,
}

/// Arguments for `corrodeql init-example`.
#[derive(Debug, Clone, Args, Default, PartialEq, Eq)]
pub struct InitExampleArgs {
    /// Directory where the example project should be created.
    #[arg(long, value_name = "DIR")]
    pub out_dir: Option<PathBuf>,

    /// Allow replacing existing example files.
    #[arg(long)]
    pub overwrite: bool,
}
