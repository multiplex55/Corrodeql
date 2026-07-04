use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::{fs, io};

use anyhow::{bail, Context, Result};
use clap::Parser;

use super::cli::{Cli, Command, EmitDdlArgs, InitExampleArgs, InspectSchemaArgs, ValidateArgs};
use super::interactive::{complete_convert_options, ConvertOptions};
use crate::config::options as core_options;
use crate::report::{
    json,
    model::{
        ConversionReport, CsvIssueReport, Diagnostic, DiagnosticSeverity,
        ForeignKeyValidationReport, ForeignKeyViolationReport, ImportReport, SchemaSummary,
        StatementKindReport, StatementReport, TableImportReport, TableImportStatus, TableReport,
        ValidationReport,
    },
    text,
};
use crate::schema::{model as schema_model, parser};
use crate::sqlite::{database, ddl, import as sqlite_import, validate as sqlite_validate};

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
    crate::logging::init(cli.verbose);

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
    validate_convert_options(&options)?;

    let schema_text = fs::read_to_string(&options.schema)
        .with_context(|| format!("failed to read schema file {}", options.schema.display()))?;
    let core_options = core_convert_options(&options);
    let parsed_schema = parser::parse_with_options(&schema_text, &core_options);
    if parsed_schema
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.severity == schema_model::DiagnosticSeverity::Error)
    {
        let generated = ddl::generate(&parsed_schema, &core_options).unwrap_or_default();
        write_convert_artifacts(
            &options,
            &parsed_schema,
            &generated,
            &core_options,
            None,
            report_validation_not_attempted("parse errors prevented database creation"),
        )?;
        bail!("parse errors prevented database creation");
    }

    let schema = crate::mssql::normalize(parsed_schema);
    let generated = ddl::generate(&schema, &core_options)?;

    if options.dry_run {
        write_convert_artifacts(
            &options,
            &schema,
            &generated,
            &core_options,
            None,
            report_validation_not_attempted("dry run did not create or validate SQLite output"),
        )?;
        println!("convert dry run: schema parsed and outputs generated without touching SQLite");
        return Ok(());
    }

    let output_path = camino::Utf8Path::from_path(options.out.as_path())
        .ok_or_else(|| anyhow::anyhow!("output SQLite path is not valid UTF-8"))?;
    let mut connection = database::create_output_connection(output_path, options.overwrite)
        .with_context(|| format!("failed to create SQLite database {}", options.out.display()))?;
    database::apply_import_pragmas(&connection).context("failed to apply SQLite import PRAGMAs")?;
    let import_report =
        match sqlite_import::import_database(&mut connection, &schema, &core_options) {
            Ok(report) => report,
            Err(error) => {
                write_convert_artifacts(
                    &options,
                    &schema,
                    &generated,
                    &core_options,
                    None,
                    report_validation_not_attempted(&format!("import failed: {error}")),
                )?;
                return Err(error.into());
            }
        };
    sqlite_validate::enable_foreign_keys_for_validation(&connection)
        .context("failed to enable SQLite foreign-key enforcement before validation")?;
    let validation = sqlite_validate::validate_database(&connection, &schema, &core_options)
        .context("failed to validate SQLite database after import")?;
    let report_validation = report_validation_from_sqlite(&validation);
    write_convert_artifacts(
        &options,
        &schema,
        &generated,
        &core_options,
        Some(import_report),
        report_validation,
    )?;

    if !validation.is_success() {
        bail!("validation failed");
    }

    println!("created SQLite database: {}", options.out.display());
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
    let schema_text = fs::read_to_string(schema_path)
        .with_context(|| format!("failed to read schema file {}", schema_path.display()))?;
    let schema = parser::parse(&schema_text);
    let sql = ddl::schema_sql(&schema, &core_options::ConvertOptions::default())?;
    if let Some(out) = args.out {
        fs::write(&out, sql)
            .with_context(|| format!("failed to write SQLite DDL to {}", out.display()))?;
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

    let schema_text = fs::read_to_string(schema_path)
        .with_context(|| format!("failed to read schema file {}", schema_path.display()))?;
    let schema = parser::parse(&schema_text);
    let connection = rusqlite::Connection::open(db_path)
        .with_context(|| format!("failed to open SQLite database {}", db_path.display()))?;
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

    sqlite_validate::enable_foreign_keys_for_validation(&connection)
        .context("failed to enable SQLite foreign-key enforcement before validation")?;
    let report = sqlite_validate::validate_database(&connection, &schema, &options)?;
    print_validation_report(&report);
    if !report.is_success() {
        bail!("validation failed");
    }

    Ok(())
}

const BASIC_EXAMPLE_FILES: &[(&str, &str)] = &[
    (
        "schema.sql",
        include_str!("../../examples/basic/schema.sql"),
    ),
    (
        "data/dbo.Customer.csv",
        include_str!("../../examples/basic/data/dbo.Customer.csv"),
    ),
    (
        "data/dbo.Order.csv",
        include_str!("../../examples/basic/data/dbo.Order.csv"),
    ),
    (
        "data/dbo.OrderLine.csv",
        include_str!("../../examples/basic/data/dbo.OrderLine.csv"),
    ),
];

/// Writes the bundled `examples/basic` project to disk.
pub fn run_init_example(args: InitExampleArgs) -> Result<()> {
    let out_dir = args
        .out_dir
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("output directory is required; pass --out-dir"))?;

    let existing = BASIC_EXAMPLE_FILES
        .iter()
        .map(|(relative, _)| out_dir.join(relative))
        .find(|path| path.exists());
    if let Some(path) = existing {
        if !args.overwrite {
            bail!(
                "example file already exists (use --overwrite to replace it): {}",
                path.display()
            );
        }
    }

    for (relative, contents) in BASIC_EXAMPLE_FILES {
        let path = out_dir.join(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).with_context(|| {
                format!("failed to create example directory {}", parent.display())
            })?;
        }
        fs::write(&path, contents)
            .with_context(|| format!("failed to write example file {}", path.display()))?;
    }

    println!("created example project: {}", out_dir.display());
    Ok(())
}

fn validate_convert_options(options: &ConvertOptions) -> Result<()> {
    validate_schema_path(Some(options.schema.as_path()))?;
    validate_data_dir(Some(options.data_dir.as_path()))?;
    validate_output_parent(Some(options.out.as_path()))?;
    validate_output_parent(options.emit_ddl.as_deref())?;
    if let Some(report_dir) = &options.report_dir {
        validate_output_parent(Some(report_dir.as_path()))?;
    }
    if options.out.exists() && !options.overwrite && !options.dry_run {
        bail!(
            "output SQLite database already exists (use --overwrite to replace it): {}",
            options.out.display()
        );
    }
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
    println!(
        "row-count validation: {:?}",
        report.row_count_validation.status
    );
    for diagnostic in &report.row_count_validation.diagnostics {
        println!("row-count diagnostic: {:?}", diagnostic);
    }
    println!(
        "integrity check: success={}, results={}",
        report.integrity_check.success,
        report.integrity_check.results.join("; ")
    );
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

fn write_convert_artifacts(
    options: &ConvertOptions,
    schema: &schema_model::DatabaseSchema,
    generated: &ddl::GeneratedDdl,
    core_options: &core_options::ConvertOptions,
    import_report: Option<ImportReport>,
    validation: ValidationReport,
) -> Result<()> {
    let schema_sql = generated.to_sql();
    if let Some(path) = &options.emit_ddl {
        fs::write(path, &schema_sql)
            .with_context(|| format!("failed to write SQLite DDL to {}", path.display()))?;
    }

    let report_dir = resolved_report_dir(options);
    fs::create_dir_all(&report_dir)
        .with_context(|| format!("failed to create report directory {}", report_dir.display()))?;
    fs::write(report_dir.join("converted_schema.sql"), &schema_sql).with_context(|| {
        format!(
            "failed to write converted schema report in {}",
            report_dir.display()
        )
    })?;
    let report = build_conversion_report(
        options,
        schema,
        generated,
        core_options,
        import_report,
        validation,
    )?;
    fs::write(
        report_dir.join("conversion_report.txt"),
        text::render(&report),
    )
    .with_context(|| {
        format!(
            "failed to write text conversion report in {}",
            report_dir.display()
        )
    })?;
    fs::write(
        report_dir.join("conversion_report.json"),
        json::render(&report),
    )
    .with_context(|| {
        format!(
            "failed to write JSON conversion report in {}",
            report_dir.display()
        )
    })?;

    io::Write::flush(&mut io::stdout())?;
    Ok(())
}

pub(crate) fn resolved_report_dir(options: &ConvertOptions) -> PathBuf {
    options.report_dir.clone().unwrap_or_else(|| {
        let base = options
            .out
            .file_stem()
            .and_then(|stem| stem.to_str())
            .filter(|stem| !stem.is_empty())
            .unwrap_or("conversion");
        options
            .out
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."))
            .join(format!("{base}_reports"))
    })
}

fn build_conversion_report(
    options: &ConvertOptions,
    schema: &schema_model::DatabaseSchema,
    generated: &ddl::GeneratedDdl,
    core_options: &core_options::ConvertOptions,
    import_report: Option<ImportReport>,
    validation: ValidationReport,
) -> Result<ConversionReport> {
    let table_names =
        crate::sqlite::names::table_names_for_schema(schema, core_options.table_name_mode)?;
    let mut indexes_by_table: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for index in &schema.indexes {
        indexes_by_table
            .entry(index.table.display_sql_server())
            .or_default()
            .push(index.name.clone());
    }
    for indexes in indexes_by_table.values_mut() {
        indexes.sort();
    }

    let mut tables = schema
        .tables()
        .iter()
        .map(|table| {
            let source_table = table.name.display_sql_server();
            let sqlite_table = table_names
                .get(&table.name)
                .map(|name| name.0.clone())
                .unwrap_or_else(|| source_table.clone());
            let mut constraints = table_constraints(table);
            constraints.sort();
            let indexes = indexes_by_table.remove(&source_table).unwrap_or_default();
            TableReport {
                source_table,
                sqlite_table,
                columns_detected: table.columns.len(),
                constraints_detected: constraints.len(),
                indexes_detected: indexes.len(),
                columns: table
                    .columns
                    .iter()
                    .map(|column| column.name.clone())
                    .collect(),
                constraints,
                indexes,
            }
        })
        .collect::<Vec<_>>();
    tables.sort_by(|left, right| left.source_table.cmp(&right.source_table));

    let mut diagnostics = schema
        .diagnostics
        .iter()
        .map(|diagnostic| Diagnostic {
            severity: report_severity(&diagnostic.severity),
            message: diagnostic.message.clone(),
        })
        .chain(generated.diagnostics.iter().map(|diagnostic| Diagnostic {
            severity: report_severity(&diagnostic.severity),
            message: diagnostic.message.clone(),
        }))
        .collect::<Vec<_>>();
    diagnostics.sort_by(|left, right| {
        left.severity
            .cmp(&right.severity)
            .then_with(|| left.message.cmp(&right.message))
    });

    let type_mapping_warnings = diagnostics
        .iter()
        .filter(|diagnostic| is_type_mapping_warning(&diagnostic.message))
        .cloned()
        .collect::<Vec<_>>();
    let default_mapping_warnings = diagnostics
        .iter()
        .filter(|diagnostic| is_default_mapping_warning(&diagnostic.message))
        .cloned()
        .collect::<Vec<_>>();
    let unsupported_sql_server_features = diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.severity == DiagnosticSeverity::Unsupported)
        .map(|diagnostic| diagnostic.message.clone())
        .collect::<Vec<_>>();
    let skipped_objects = schema
        .statement_summary
        .ignored
        .iter()
        .map(|(kind, count)| {
            format!(
                "{} {} statement{}",
                count,
                kind.label(),
                if *count == 1 { "" } else { "s" }
            )
        })
        .collect::<Vec<_>>();

    let import = sorted_import_report(
        import_report.unwrap_or_else(|| skipped_import_report(schema, &table_names)),
    );
    let csv_issues = csv_issues_from_import_report(&import);
    let validation = sorted_validation_report(validation);

    Ok(ConversionReport {
        input_schema_path: options.schema.display().to_string(),
        data_directory: options.data_dir.display().to_string(),
        output_database_path: options.out.display().to_string(),
        table_name_mode: core_options.table_name_mode.to_string(),
        null_token: core_options.null_token.clone(),
        statements: statement_report(&schema.statement_summary),
        schema: SchemaSummary {
            tables_detected: tables.len(),
            columns_detected: tables.iter().map(|table| table.columns_detected).sum(),
            constraints_detected: tables.iter().map(|table| table.constraints_detected).sum(),
            indexes_detected: schema.indexes.len(),
            tables,
        },
        import,
        row_count_validation: validation.row_count_validation.clone(),
        foreign_key_validation: ForeignKeyValidationReport {
            attempted: validation.foreign_key_check_attempted,
            skipped: validation.foreign_key_check_skipped,
            violations: validation.foreign_key_violations.clone(),
        },
        integrity_check: validation.integrity_check.clone(),
        validation,
        diagnostics,
        type_mapping_warnings,
        default_mapping_warnings,
        skipped_objects,
        unsupported_sql_server_features,
        csv_issues,
    })
}

fn is_type_mapping_warning(message: &str) -> bool {
    message.contains("unrecognized SQL Server type") || message.contains("SQLite TEXT affinity")
}

fn is_default_mapping_warning(message: &str) -> bool {
    message.starts_with("default on ") || message.contains("default") && message.contains("SQLite")
}

fn csv_issues_from_import_report(report: &ImportReport) -> Vec<CsvIssueReport> {
    let mut issues = report
        .tables
        .iter()
        .flat_map(|table| {
            table.diagnostics.iter().map(|message| CsvIssueReport {
                source_table: table.source_table.clone(),
                sqlite_table: table.sqlite_table.clone(),
                csv_path: table.csv_path.clone(),
                message: message.clone(),
            })
        })
        .collect::<Vec<_>>();
    issues.sort_by(|left, right| {
        left.source_table
            .cmp(&right.source_table)
            .then_with(|| left.sqlite_table.cmp(&right.sqlite_table))
            .then_with(|| left.csv_path.cmp(&right.csv_path))
            .then_with(|| left.message.cmp(&right.message))
    });
    issues
}

fn statement_report(summary: &crate::schema::classifier::ClassificationSummary) -> StatementReport {
    fn entries(
        map: &std::collections::BTreeMap<crate::schema::classifier::StatementKind, usize>,
    ) -> Vec<StatementKindReport> {
        map.iter()
            .map(|(kind, count)| StatementKindReport {
                kind: kind.label().to_owned(),
                count: *count,
            })
            .collect()
    }
    StatementReport {
        detected_count: summary.detected_count,
        ignored_count: summary.ignored_count,
        warning_count: summary.warning_count,
        detected: entries(&summary.detected),
        ignored: entries(&summary.ignored),
        warnings: entries(&summary.warnings),
    }
}

fn sorted_import_report(mut report: ImportReport) -> ImportReport {
    report.tables.sort_by(|left, right| {
        left.source_table
            .cmp(&right.source_table)
            .then_with(|| left.sqlite_table.cmp(&right.sqlite_table))
    });
    for table in &mut report.tables {
        table.diagnostics.sort();
    }
    report
}

fn sorted_validation_report(mut report: ValidationReport) -> ValidationReport {
    report.diagnostics.sort_by(|left, right| {
        left.severity
            .cmp(&right.severity)
            .then_with(|| left.message.cmp(&right.message))
    });
    report
}

fn skipped_import_report(
    schema: &schema_model::DatabaseSchema,
    table_names: &HashMap<schema_model::TableName, crate::sqlite::names::Name>,
) -> ImportReport {
    ImportReport {
        tables: schema
            .tables()
            .iter()
            .map(|table| {
                let sqlite_table = table_names
                    .get(&table.name)
                    .map(|name| name.0.clone())
                    .unwrap_or_else(|| table.name.display_sql_server());
                TableImportReport {
                    source_table: table.name.display_sql_server(),
                    sqlite_table,
                    csv_path: None,
                    status: TableImportStatus::Skipped,
                    rows_read: 0,
                    rows_inserted: 0,
                    rows_rejected: 0,
                    diagnostics: vec!["CSV import was not run".to_owned()],
                }
            })
            .collect(),
        ..ImportReport::default()
    }
}

fn report_validation_not_attempted(message: &str) -> ValidationReport {
    ValidationReport {
        attempted: false,
        success: false,
        tables_validated: 0,
        foreign_key_check_attempted: false,
        foreign_key_check_skipped: false,
        foreign_key_violations: vec![],
        row_count_validation: Default::default(),
        integrity_check: Default::default(),
        diagnostics: vec![Diagnostic {
            severity: DiagnosticSeverity::Warning,
            message: message.to_owned(),
        }],
    }
}

fn report_validation_from_sqlite(report: &sqlite_validate::ValidationReport) -> ValidationReport {
    let mut diagnostics = Vec::new();
    for table in &report.tables {
        if table.status != sqlite_validate::TableValidationStatus::Valid {
            diagnostics.push(Diagnostic {
                severity: DiagnosticSeverity::Error,
                message: format!(
                    "validation failed for {}: {:?}",
                    table.source_table, table.status
                ),
            });
        }
    }
    let foreign_key_violations = report
        .foreign_key_violations
        .iter()
        .map(|violation| ForeignKeyViolationReport {
            child_table: violation.table.clone(),
            rowid: violation.rowid,
            parent_table: violation.parent.clone(),
            foreign_key_id: violation.fkid,
        })
        .collect::<Vec<_>>();
    for violation in &foreign_key_violations {
        diagnostics.push(Diagnostic {
            severity: DiagnosticSeverity::Error,
            message: format!(
                "foreign key violation in {} rowid {:?} referencing {} (foreign key id {})",
                violation.child_table,
                violation.rowid,
                violation.parent_table,
                violation.foreign_key_id
            ),
        });
    }
    if report.foreign_key_check_skipped {
        diagnostics.push(Diagnostic {
            severity: DiagnosticSeverity::Warning,
            message: "foreign-key validation skipped by option".to_owned(),
        });
    }
    for missing in &report.missing_indexes_or_constraints {
        diagnostics.push(Diagnostic {
            severity: DiagnosticSeverity::Warning,
            message: missing.clone(),
        });
    }
    let row_count_validation =
        report_row_count_validation_from_sqlite(&report.row_count_validation);
    diagnostics.extend(row_count_validation.diagnostics.clone());
    let integrity_check = report_integrity_check_from_sqlite(&report.integrity_check);
    if !integrity_check.success {
        diagnostics.push(Diagnostic {
            severity: DiagnosticSeverity::Error,
            message: format!(
                "SQLite integrity_check failed: {}",
                if integrity_check.results.is_empty() {
                    "<no rows>".to_owned()
                } else {
                    integrity_check.results.join("; ")
                }
            ),
        });
    }
    ValidationReport {
        attempted: true,
        success: report.is_success(),
        tables_validated: report.tables.len(),
        foreign_key_check_attempted: report.foreign_key_check_attempted,
        foreign_key_check_skipped: report.foreign_key_check_skipped,
        foreign_key_violations,
        row_count_validation,
        integrity_check,
        diagnostics,
    }
}

fn report_integrity_check_from_sqlite(
    report: &sqlite_validate::IntegrityCheckReport,
) -> crate::report::model::IntegrityCheckReport {
    crate::report::model::IntegrityCheckReport {
        success: report.success,
        results: report.results.clone(),
    }
}

fn report_row_count_validation_from_sqlite(
    report: &sqlite_validate::RowCountValidationReport,
) -> crate::report::model::RowCountValidationReport {
    let diagnostics = report
        .diagnostics
        .iter()
        .map(|diagnostic| match diagnostic {
            sqlite_validate::RowCountDiagnostic::ManifestMissing { path } => Diagnostic {
                severity: DiagnosticSeverity::Warning,
                message: format!(
                    "row-count validation skipped because manifest is missing: {path}"
                ),
            },
            sqlite_validate::RowCountDiagnostic::MissingTable { table } => Diagnostic {
                severity: DiagnosticSeverity::Error,
                message: format!("row-count manifest is missing expected table {table}"),
            },
            sqlite_validate::RowCountDiagnostic::UnknownTable { table } => Diagnostic {
                severity: DiagnosticSeverity::Warning,
                message: format!("row-count manifest contains unknown table {table}"),
            },
            sqlite_validate::RowCountDiagnostic::Mismatch {
                table,
                expected,
                actual,
            } => Diagnostic {
                severity: DiagnosticSeverity::Error,
                message: format!(
                    "row-count mismatch for {table}: expected {expected}, actual {actual}"
                ),
            },
        })
        .collect();
    crate::report::model::RowCountValidationReport {
        status: match report.status {
            sqlite_validate::RowCountValidationStatus::Skipped => {
                crate::report::model::RowCountValidationStatus::Skipped
            }
            sqlite_validate::RowCountValidationStatus::Validated => {
                crate::report::model::RowCountValidationStatus::Validated
            }
            sqlite_validate::RowCountValidationStatus::Failed => {
                crate::report::model::RowCountValidationStatus::Failed
            }
        },
        diagnostics,
    }
}

fn table_constraints(table: &schema_model::TableDef) -> Vec<String> {
    let mut constraints = Vec::new();
    if table.primary_key.is_some() {
        constraints.push("primary_key".to_owned());
    }
    constraints.extend(table.unique_constraints.iter().map(|c| {
        format!(
            "unique:{}",
            c.name.clone().unwrap_or_else(|| c.columns.join("+"))
        )
    }));
    constraints.extend(table.foreign_keys.iter().map(|c| {
        format!(
            "foreign_key:{}",
            c.name.clone().unwrap_or_else(|| c.columns.join("+"))
        )
    }));
    constraints.extend(table.check_constraints.iter().map(|c| {
        format!(
            "check:{}",
            c.name.clone().unwrap_or_else(|| c.expression.clone())
        )
    }));
    constraints.extend(table.columns.iter().filter_map(|column| {
        column.default.as_ref().map(|c| {
            format!(
                "default:{}",
                c.name.clone().unwrap_or_else(|| column.name.clone())
            )
        })
    }));
    constraints.extend(table.columns.iter().filter_map(|column| {
        column.check.as_ref().map(|c| {
            format!(
                "check:{}",
                c.name.clone().unwrap_or_else(|| column.name.clone())
            )
        })
    }));
    constraints
}

fn report_severity(severity: &schema_model::DiagnosticSeverity) -> DiagnosticSeverity {
    match severity {
        schema_model::DiagnosticSeverity::Warning => DiagnosticSeverity::Warning,
        schema_model::DiagnosticSeverity::Error => DiagnosticSeverity::Error,
        schema_model::DiagnosticSeverity::Unsupported => DiagnosticSeverity::Unsupported,
    }
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
            .contains("CREATE TABLE \"dbo_T\""));
        assert!(report_dir.join("converted_schema.sql").exists());
        assert!(report_dir.join("conversion_report.txt").exists());
        assert!(report_dir.join("conversion_report.json").exists());
    }

    #[test]
    fn resolves_explicit_and_default_report_paths() {
        let explicit = PathBuf::from("custom-reports");
        let options = ConvertOptions {
            schema: PathBuf::from("schema.sql"),
            data_dir: PathBuf::from("data"),
            out: PathBuf::from("target/out/app.sqlite"),
            overwrite: false,
            null_token: r"\N".to_string(),
            table_name_mode: crate::app::cli::TableNameMode::SchemaPrefix,
            emit_ddl: None,
            report_dir: Some(explicit.clone()),
            strict: false,
            allow_missing_csv: false,
            allow_extra_csv_columns: false,
            skip_foreign_key_check: false,
            dry_run: true,
        };
        assert_eq!(resolved_report_dir(&options), explicit);

        let mut defaulted = options.clone();
        defaulted.report_dir = None;
        assert_eq!(
            resolved_report_dir(&defaulted),
            PathBuf::from("target/out/app_reports")
        );
    }

    #[test]
    fn conversion_report_orders_tables_and_diagnostics_deterministically() {
        let schema = parser::parse(
            "CREATE TABLE [dbo].[B] (Id int IDENTITY NOT NULL);\nCREATE TABLE [dbo].[A] (Id madeup NOT NULL);",
        );
        let options = ConvertOptions {
            schema: PathBuf::from("schema.sql"),
            data_dir: PathBuf::from("data"),
            out: PathBuf::from("out.sqlite"),
            overwrite: false,
            null_token: r"\N".to_string(),
            table_name_mode: crate::app::cli::TableNameMode::SchemaPrefix,
            emit_ddl: None,
            report_dir: None,
            strict: false,
            allow_missing_csv: false,
            allow_extra_csv_columns: false,
            skip_foreign_key_check: false,
            dry_run: true,
        };
        let core_options = core_convert_options(&options);
        let generated = ddl::generate(&schema, &core_options).unwrap();
        let report = build_conversion_report(
            &options,
            &schema,
            &generated,
            &core_options,
            None,
            report_validation_not_attempted("test"),
        )
        .unwrap();

        let table_names = report
            .schema
            .tables
            .iter()
            .map(|table| table.source_table.as_str())
            .collect::<Vec<_>>();
        assert_eq!(table_names, vec!["[dbo].[A]", "[dbo].[B]"]);

        let diagnostics = report
            .diagnostics
            .iter()
            .map(|diagnostic| (diagnostic.severity, diagnostic.message.as_str()))
            .collect::<Vec<_>>();
        assert!(diagnostics.windows(2).all(|pair| pair[0] <= pair[1]));
    }

    #[test]
    fn sqlite_integrity_failure_becomes_validation_diagnostic() {
        let sqlite_report = sqlite_validate::ValidationReport {
            integrity_check: sqlite_validate::IntegrityCheckReport {
                success: false,
                results: vec!["row 1 missing from index".to_owned()],
            },
            ..sqlite_validate::ValidationReport::default()
        };

        let report = report_validation_from_sqlite(&sqlite_report);

        assert!(!report.success);
        assert_eq!(
            report.integrity_check.results,
            vec!["row 1 missing from index"]
        );
        assert!(report.diagnostics.iter().any(|diagnostic| {
            diagnostic.severity == DiagnosticSeverity::Error
                && diagnostic
                    .message
                    .contains("SQLite integrity_check failed: row 1 missing from index")
        }));
    }
}
