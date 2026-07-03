use std::path::Path;
use std::process::ExitCode;
use std::{fs, io};

use anyhow::{bail, Result};
use clap::Parser;

use super::cli::{Cli, Command, EmitDdlArgs, InitExampleArgs, InspectSchemaArgs, ValidateArgs};
use super::interactive::{complete_convert_options, ConvertOptions};
use crate::config::options as core_options;
use crate::report::{json, model::Report, text};
use crate::schema::parser;
use crate::sqlite::{database, ddl, validate as sqlite_validate};

/// Runs the CorrodeQL command-line application.
pub fn run() -> ExitCode {
    match run_with_args(std::env::args_os()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error:#}");
            ExitCode::FAILURE
        }
    }
}

/// Parses command-line arguments and routes the selected command.
pub fn run_with_args<I, T>(args: I) -> Result<()>
where
    I: IntoIterator<Item = T>,
    T: Into<std::ffi::OsString> + Clone,
{
    let cli = Cli::parse_from(args);

    match cli.command {
        Some(Command::Convert(args)) => run_convert(args),
        Some(Command::InspectSchema(args)) => run_inspect_schema(args),
        Some(Command::EmitDdl(args)) => run_emit_ddl(args),
        Some(Command::Validate(args)) => run_validate(args),
        Some(Command::InitExample(args)) => run_init_example(args),
        None => run_convert(Default::default()),
    }
}

/// Placeholder implementation for `corrodeql convert`.
pub fn run_convert(args: super::cli::ConvertArgs) -> Result<()> {
    let options = complete_convert_options(args)?;
    run_convert_with_options(options)
}

fn run_convert_with_options(options: ConvertOptions) -> Result<()> {
    validate_schema_path(Some(options.schema.as_path()))?;
    validate_data_dir(Some(options.data_dir.as_path()))?;
    validate_output_parent(Some(options.out.as_path()))?;
    validate_output_parent(options.emit_ddl.as_deref())?;
    if let Some(report_dir) = &options.report_dir {
        validate_output_parent(Some(report_dir.as_path()))?;
    }

    let schema_text = fs::read_to_string(&options.schema)?;
    let schema = parser::parse(&schema_text);
    let core_options = core_convert_options(&options);
    let generated = ddl::generate(&schema, &core_options)?;
    write_convert_artifacts(&options, &generated)?;

    if options.dry_run {
        println!("convert dry run: schema parsed and outputs generated without touching SQLite");
    } else {
        database::open_output_database(
            camino::Utf8Path::from_path(options.out.as_path())
                .ok_or_else(|| anyhow::anyhow!("output SQLite path is not valid UTF-8"))?,
            options.overwrite,
            &generated,
        )?;
        println!("created SQLite database: {}", options.out.display());
    }

    Ok(())
}

/// Placeholder implementation for `corrodeql inspect-schema`.
pub fn run_inspect_schema(args: InspectSchemaArgs) -> Result<()> {
    validate_schema_path(args.schema.as_deref())?;
    println!("inspect-schema is not yet implemented; no schema inspection was attempted");
    Ok(())
}

/// Placeholder implementation for `corrodeql emit-ddl`.
pub fn run_emit_ddl(args: EmitDdlArgs) -> Result<()> {
    validate_schema_path(args.schema.as_deref())?;
    validate_output_parent(args.out.as_deref())?;
    let schema_path = args
        .schema
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("schema file path is required"))?;
    let schema_text = fs::read_to_string(schema_path)?;
    let schema = parser::parse(&schema_text);
    let sql = ddl::schema_sql(&schema, &core_options::ConvertOptions::default())?;
    if let Some(out) = args.out {
        fs::write(out, sql)?;
    } else {
        print!("{sql}");
    }
    Ok(())
}

/// Validates an existing SQLite database against schema and CSV inputs.
pub fn run_validate(args: ValidateArgs) -> Result<()> {
    validate_schema_path(args.schema.as_deref())?;
    validate_data_dir(args.data_dir.as_deref())?;
    validate_sqlite_db_path(args.db.as_deref())?;

    let schema_path = args
        .schema
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("schema file path is required"))?;
    let data_dir = args
        .data_dir
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("data directory path is required"))?;
    let db_path = args
        .db
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("SQLite database path is required; pass --db"))?;

    let schema_text = fs::read_to_string(schema_path)?;
    let schema = parser::parse(&schema_text);
    let connection = rusqlite::Connection::open(db_path)?;
    let options = core_options::ConvertOptions {
        schema_path: camino::Utf8PathBuf::from_path_buf(schema_path.to_path_buf())
            .unwrap_or_default(),
        data_dir: camino::Utf8PathBuf::from_path_buf(data_dir.to_path_buf()).unwrap_or_default(),
        output_db_path: camino::Utf8PathBuf::from_path_buf(db_path.to_path_buf())
            .unwrap_or_default(),
        table_name_mode: args.table_name_mode.unwrap_or_default(),
        skip_foreign_key_check: args.skip_foreign_key_check,
        ..core_options::ConvertOptions::default()
    };

    let report = sqlite_validate::validate_database(&connection, &schema, &options)?;
    print_validation_report(&report);
    if !report.is_success() {
        bail!("validation failed");
    }

    Ok(())
}

/// Placeholder implementation for `corrodeql init-example`.
pub fn run_init_example(_args: InitExampleArgs) -> Result<()> {
    println!("init-example is not yet implemented; no files were written");
    Ok(())
}

fn validate_schema_path(path: Option<&Path>) -> Result<()> {
    if let Some(path) = path {
        if !path.exists() {
            bail!("schema path does not exist: {}", path.display());
        }
        if !path.is_file() {
            bail!("schema path must be a file: {}", path.display());
        }
    }

    Ok(())
}

fn validate_data_dir(path: Option<&Path>) -> Result<()> {
    if let Some(path) = path {
        if !path.exists() {
            bail!("data directory does not exist: {}", path.display());
        }
        if !path.is_dir() {
            bail!("data directory must be a directory: {}", path.display());
        }
    }

    Ok(())
}

fn validate_sqlite_db_path(path: Option<&Path>) -> Result<()> {
    if let Some(path) = path {
        if !path.exists() {
            bail!("SQLite database path does not exist: {}", path.display());
        }
        if !path.is_file() {
            bail!("SQLite database path must be a file: {}", path.display());
        }
    }

    Ok(())
}

fn print_validation_report(report: &sqlite_validate::ValidationReport) {
    for table in &report.tables {
        println!(
            "table {} ({}): {:?}; expected rows: {}; actual rows: {}",
            table.source_table,
            table.sqlite_table,
            table.status,
            table
                .expected_row_count
                .map(|value| value.to_string())
                .unwrap_or_else(|| "unknown".to_owned()),
            table
                .actual_row_count
                .map(|value| value.to_string())
                .unwrap_or_else(|| "missing".to_owned())
        );
        if !table.missing_not_null_columns.is_empty() {
            println!(
                "  missing NOT NULL metadata for columns: {}",
                table.missing_not_null_columns.join(", ")
            );
        }
    }
    for violation in &report.foreign_key_violations {
        println!(
            "foreign key violation: table={}, rowid={:?}, parent={}, fkid={}",
            violation.table, violation.rowid, violation.parent, violation.fkid
        );
    }
    for missing in &report.missing_indexes_or_constraints {
        println!("missing index or constraint: {missing}");
    }
}

fn validate_output_parent(path: Option<&Path>) -> Result<()> {
    if let Some(path) = path {
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            if !parent.exists() {
                bail!(
                    "output parent directory does not exist: {}",
                    parent.display()
                );
            }
            if !parent.is_dir() {
                bail!(
                    "output parent path must be a directory: {}",
                    parent.display()
                );
            }
        }
    }

    Ok(())
}

fn core_convert_options(options: &ConvertOptions) -> core_options::ConvertOptions {
    core_options::ConvertOptions {
        schema_path: camino::Utf8PathBuf::from_path_buf(options.schema.clone()).unwrap_or_default(),
        data_dir: camino::Utf8PathBuf::from_path_buf(options.data_dir.clone()).unwrap_or_default(),
        output_db_path: camino::Utf8PathBuf::from_path_buf(options.out.clone()).unwrap_or_default(),
        overwrite: options.overwrite,
        null_token: options.null_token.clone(),
        table_name_mode: options.table_name_mode,
        emit_ddl_path: options
            .emit_ddl
            .clone()
            .and_then(|path| camino::Utf8PathBuf::from_path_buf(path).ok()),
        report_dir: options
            .report_dir
            .clone()
            .and_then(|path| camino::Utf8PathBuf::from_path_buf(path).ok()),
        strict: options.strict,
        allow_missing_csv: options.allow_missing_csv,
        allow_extra_csv_columns: options.allow_extra_csv_columns,
        skip_foreign_key_check: options.skip_foreign_key_check,
        dry_run: options.dry_run,
    }
}

fn write_convert_artifacts(options: &ConvertOptions, generated: &ddl::GeneratedDdl) -> Result<()> {
    let schema_sql = generated.to_sql();
    if let Some(path) = &options.emit_ddl {
        fs::write(path, &schema_sql)?;
    }

    if let Some(report_dir) = &options.report_dir {
        fs::create_dir_all(report_dir)?;
        fs::write(report_dir.join("converted_schema.sql"), &schema_sql)?;
        let report = Report::default();
        fs::write(report_dir.join("report.txt"), text::render(&report))?;
        fs::write(report_dir.join("report.json"), json::render(&report))?;
    }

    io::Write::flush(&mut io::stdout())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;

    fn temp_root(name: &str) -> std::path::PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("corrodeql-run-{name}-{unique}"));
        fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn dry_run_writes_schema_and_report_outputs_without_creating_db() {
        let root = temp_root("dry-run");
        let schema = root.join("schema.sql");
        let data_dir = root.join("csv");
        let db = root.join("out.sqlite");
        let ddl_out = root.join("ddl.sql");
        let report_dir = root.join("reports");
        fs::write(&schema, "CREATE TABLE T (Id int NOT NULL);").unwrap();
        fs::create_dir_all(&data_dir).unwrap();

        run_convert_with_options(ConvertOptions {
            schema,
            data_dir,
            out: db.clone(),
            overwrite: false,
            null_token: r"\N".to_string(),
            table_name_mode: crate::app::cli::TableNameMode::SchemaPrefix,
            emit_ddl: Some(ddl_out.clone()),
            report_dir: Some(report_dir.clone()),
            strict: false,
            allow_missing_csv: false,
            allow_extra_csv_columns: false,
            skip_foreign_key_check: false,
            dry_run: true,
        })
        .unwrap();

        assert!(!db.exists());
        assert!(fs::read_to_string(ddl_out)
            .unwrap()
            .contains("CREATE TABLE \"T\""));
        assert!(report_dir.join("converted_schema.sql").exists());
        assert!(report_dir.join("report.txt").exists());
        assert!(report_dir.join("report.json").exists());
    }
}
