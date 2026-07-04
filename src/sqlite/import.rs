//! CSV import into SQLite.

use camino::Utf8PathBuf;
use rusqlite::{params_from_iter, Connection};

use crate::config::options::ConvertOptions;
use crate::data::csv_reader::{CsvReader, CsvReaderOptions};
use crate::data::manifest::{Manifest, ManifestOptions};
use crate::error::{Error, Result};
use crate::report::model::{ImportReport, TableImportReport, TableImportStatus};
use crate::schema::model::{DatabaseSchema, TableDef};
use crate::sqlite::ddl;
use crate::sqlite::names::{quote_identifier, table_names_for_schema, Name};

/// Backwards-compatible no-op marker for module-tree smoke tests.
pub fn import() {}

/// Creates the SQLite schema and imports matching schema-qualified CSV files.
///
/// Structural table errors (missing headers, unexpected headers, statement preparation failures,
/// or SQLite constraint/insert failures) are fatal. In strict mode, row conversion errors abort the
/// import immediately. Outside strict mode, row conversion errors are accumulated as rejected rows
/// and import continues with subsequent CSV records.
pub fn import_database(
    connection: &mut Connection,
    schema: &DatabaseSchema,
    options: &ConvertOptions,
) -> Result<ImportReport> {
    let table_ddl = ddl::generate_tables(schema, options)?;
    let index_ddl = ddl::generate_indexes(schema, options)?;
    let table_names = table_names_for_schema(schema, options.table_name_mode)?;
    let manifest = Manifest::discover(
        &options.data_dir,
        schema,
        ManifestOptions {
            strict: true,
            allow_missing_csv: options.allow_missing_csv,
        },
    )?;

    let transaction = connection.transaction()?;
    for statement in &table_ddl.statements {
        transaction.execute_batch(&statement.0)?;
    }

    let mut report = ImportReport::default();
    for table in schema.tables() {
        let sqlite_name = table_names.get(&table.name).ok_or_else(|| {
            validation_error(format!(
                "missing generated SQLite table name for {}",
                table.name.display_sql_server()
            ))
        })?;
        let Some(path) = manifest.tables.get(&table.name) else {
            report.tables.push(TableImportReport {
                source_table: table.name.display_sql_server(),
                sqlite_table: sqlite_name.0.clone(),
                csv_path: None,
                status: TableImportStatus::Skipped,
                rows_read: 0,
                rows_inserted: 0,
                rows_rejected: 0,
                diagnostics: vec!["CSV file was not provided".to_owned()],
            });
            continue;
        };

        let table_report = match import_table(&transaction, table, sqlite_name, path, options) {
            Ok(table_report) => table_report,
            Err(failure) => {
                report.rows_read += failure.report.rows_read;
                report.rows_inserted += failure.report.rows_inserted;
                report.rows_rejected += failure.report.rows_rejected;
                report.tables.push(failure.report);
                return Err(Error::ImportFailure {
                    report,
                    source: Box::new(failure.source),
                });
            }
        };
        report.rows_read += table_report.rows_read;
        report.rows_inserted += table_report.rows_inserted;
        report.rows_rejected += table_report.rows_rejected;
        report.tables.push(table_report);
    }

    for statement in &index_ddl.statements {
        transaction.execute_batch(&statement.0)?;
    }

    transaction.commit()?;
    Ok(report)
}

fn import_table(
    connection: &rusqlite::Transaction<'_>,
    table: &TableDef,
    sqlite_name: &Name,
    path: &Utf8PathBuf,
    options: &ConvertOptions,
) -> std::result::Result<TableImportReport, ImportTableFailure> {
    let mut report = TableImportReport {
        source_table: table.name.display_sql_server(),
        sqlite_table: sqlite_name.0.clone(),
        csv_path: Some(path.to_string()),
        status: TableImportStatus::Imported,
        rows_read: 0,
        rows_inserted: 0,
        rows_rejected: 0,
        diagnostics: Vec::new(),
    };
    let insert_sql = insert_statement(table, sqlite_name);
    // Prepare the INSERT once and reuse it for each streamed CSV row; this keeps import memory
    // usage independent of CSV length and avoids per-row statement compilation.
    let mut statement = connection
        .prepare(&insert_sql)
        .map_err(|error| fail_table_report(&mut report, error.into()))?;
    let reader = CsvReader::from_path(
        path,
        table,
        CsvReaderOptions {
            null_token: options.null_token.clone(),
            allow_extra_csv_columns: options.allow_extra_csv_columns,
        },
    )
    .map_err(|error| fail_table_report(&mut report, error))?;

    for row in reader {
        report.rows_read += 1;
        match row {
            Ok(row) => match statement.execute(params_from_iter(row.values)) {
                Ok(_) => report.rows_inserted += 1,
                Err(error) => {
                    let source = import_table_error(
                        table,
                        sqlite_name,
                        path,
                        row.row_number,
                        error.to_string(),
                    );
                    return Err(fail_table_report(&mut report, source));
                }
            },
            Err(error) if options.strict => {
                let source = import_table_error(
                    table,
                    sqlite_name,
                    path,
                    row_number_from_error(&error).unwrap_or(report.rows_read),
                    error.to_string(),
                );
                return Err(fail_table_report(&mut report, source));
            }
            Err(error) => {
                report.rows_rejected += 1;
                report.status = TableImportStatus::Partial;
                report.diagnostics.push(error.to_string());
            }
        }
    }

    Ok(report)
}

struct ImportTableFailure {
    report: TableImportReport,
    source: Error,
}

fn fail_table_report(report: &mut TableImportReport, source: Error) -> ImportTableFailure {
    report.status = TableImportStatus::Failed;
    report.diagnostics.push(source.to_string());
    ImportTableFailure {
        report: report.clone(),
        source,
    }
}

fn insert_statement(table: &TableDef, sqlite_name: &Name) -> String {
    let columns = table
        .columns
        .iter()
        .map(|column| quote_identifier(&column.name))
        .collect::<Vec<_>>()
        .join(", ");
    let placeholders = (1..=table.columns.len())
        .map(|index| format!("?{index}"))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "INSERT INTO {} ({columns}) VALUES ({placeholders})",
        quote_identifier(&sqlite_name.0)
    )
}

fn import_table_error(
    table: &TableDef,
    sqlite_name: &Name,
    path: &Utf8PathBuf,
    row_number: u64,
    message: String,
) -> Error {
    Error::ImportTable {
        source_table: table.name.display_sql_server(),
        sqlite_table: sqlite_name.0.clone(),
        csv_path: path.to_string(),
        row_number,
        message,
    }
}

fn row_number_from_error(error: &Error) -> Option<u64> {
    match error {
        Error::CsvReadImport { row_number, .. } => Some(*row_number),
        _ => None,
    }
}

fn validation_error(message: String) -> Error {
    Error::Validation { message }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    use camino::Utf8PathBuf;
    use rusqlite::Connection;

    use super::*;
    use crate::schema::model::{
        ColumnDef, IndexDef, PrimaryKeyDef, SqlServerType, TableDef, TableName,
    };

    fn temp_dir(name: &str) -> Utf8PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = Utf8PathBuf::from_path_buf(
            std::env::temp_dir().join(format!("corrodeql-import-{name}-{unique}")),
        )
        .unwrap();
        fs::create_dir_all(&path).unwrap();
        path
    }

    fn column(name: &str, data_type: SqlServerType, nullable: bool) -> ColumnDef {
        ColumnDef {
            name: name.to_owned(),
            data_type,
            nullable,
            identity: false,
            primary_key: false,
            unique: false,
            default: None,
            check: None,
        }
    }

    fn table(name: &str, columns: Vec<ColumnDef>) -> TableDef {
        TableDef {
            name: TableName::new(Some("dbo".to_owned()), name),
            columns,
            primary_key: None,
            unique_constraints: Vec::new(),
            foreign_keys: Vec::new(),
            check_constraints: Vec::new(),
        }
    }

    fn schema(tables: Vec<TableDef>) -> DatabaseSchema {
        DatabaseSchema {
            tables,
            indexes: Vec::new(),
            diagnostics: Vec::new(),
            statement_summary: Default::default(),
        }
    }

    fn schema_with_indexes(tables: Vec<TableDef>, indexes: Vec<IndexDef>) -> DatabaseSchema {
        DatabaseSchema {
            tables,
            indexes,
            diagnostics: Vec::new(),
            statement_summary: Default::default(),
        }
    }

    fn options(dir: Utf8PathBuf) -> ConvertOptions {
        ConvertOptions {
            data_dir: dir,
            overwrite: true,
            ..ConvertOptions::default()
        }
    }

    fn write_csv(dir: &Utf8PathBuf, name: &str, contents: &str) {
        fs::write(dir.join(name), contents).unwrap();
    }

    #[test]
    fn imports_simple_table() {
        let dir = temp_dir("simple");
        write_csv(&dir, "dbo.Widget.csv", "Id,Name\n1,Gear\n2,Bolt\n");
        let schema = schema(vec![table(
            "Widget",
            vec![
                column("Id", SqlServerType::Int, false),
                column(
                    "Name",
                    SqlServerType::NVarChar {
                        length: Some(50),
                        max: false,
                    },
                    true,
                ),
            ],
        )]);
        let mut conn = Connection::open_in_memory().unwrap();

        let report = import_database(&mut conn, &schema, &options(dir)).unwrap();

        assert_eq!(report.rows_read, 2);
        assert_eq!(report.rows_inserted, 2);
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM dbo_Widget", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 2);
    }

    #[test]
    fn successful_import_tracks_rows_read_and_inserted() {
        let dir = temp_dir("row-counts");
        write_csv(&dir, "dbo.Widget.csv", "Id,Name\n1,Gear\n2,Bolt\n3,Nut\n");
        let schema = schema(vec![table(
            "Widget",
            vec![
                column("Id", SqlServerType::Int, false),
                column("Name", SqlServerType::Text, true),
            ],
        )]);
        let mut conn = Connection::open_in_memory().unwrap();

        let report = import_database(&mut conn, &schema, &options(dir)).unwrap();

        assert_eq!(report.rows_read, 3);
        assert_eq!(report.rows_inserted, 3);
        assert_eq!(report.rows_rejected, 0);
        assert_eq!(report.tables[0].rows_read, 3);
        assert_eq!(report.tables[0].rows_inserted, 3);
    }

    #[test]
    fn strict_conversion_error_includes_table_and_csv_path() {
        let dir = temp_dir("strict-context");
        let csv_path = dir.join("dbo.Widget.csv");
        write_csv(&dir, "dbo.Widget.csv", "Id\n1\nnot-int\n");
        let schema = schema(vec![table(
            "Widget",
            vec![column("Id", SqlServerType::Int, false)],
        )]);
        let mut conn = Connection::open_in_memory().unwrap();
        let mut opts = options(dir);
        opts.strict = true;

        let error = import_database(&mut conn, &schema, &opts).unwrap_err();
        let display = error.to_string();

        assert!(display.contains("[dbo].[Widget]"), "{display}");
        assert!(display.contains("dbo_Widget"), "{display}");
        assert!(display.contains(csv_path.as_str()), "{display}");
        assert!(display.contains("row 3"), "{display}");
        assert!(display.contains("invalid value"), "{display}");
    }

    #[test]
    fn sqlite_constraint_failure_includes_table_and_row_number() {
        let dir = temp_dir("constraint-context");
        write_csv(&dir, "dbo.Widget.csv", "Id\n1\n1\n");
        let mut widget = table("Widget", vec![column("Id", SqlServerType::Int, false)]);
        widget.primary_key = Some(PrimaryKeyDef {
            name: Some("PK_Widget".to_owned()),
            columns: vec!["Id".to_owned()],
            clustered: None,
        });
        let schema = schema(vec![widget]);
        let mut conn = Connection::open_in_memory().unwrap();

        let error = import_database(&mut conn, &schema, &options(dir)).unwrap_err();
        let display = error.to_string();

        assert!(display.contains("[dbo].[Widget]"), "{display}");
        assert!(display.contains("dbo_Widget"), "{display}");
        assert!(display.contains("row 3"), "{display}");
        assert!(display.contains("UNIQUE constraint failed"), "{display}");
    }

    #[test]
    fn imports_streaming_multi_row_csv_without_collecting_records() {
        let dir = temp_dir("streaming-many");
        let mut csv = String::from("Id,Name\n");
        for id in 1..=4096 {
            csv.push_str(&format!("{id},Widget {id}\n"));
        }
        write_csv(&dir, "dbo.Widget.csv", &csv);
        let schema = schema(vec![table(
            "Widget",
            vec![
                column("Id", SqlServerType::Int, false),
                column("Name", SqlServerType::Text, true),
            ],
        )]);
        let mut conn = Connection::open_in_memory().unwrap();

        let report = import_database(&mut conn, &schema, &options(dir)).unwrap();

        assert_eq!(report.rows_read, 4096);
        assert_eq!(report.rows_inserted, 4096);
        let max_id: i64 = conn
            .query_row("SELECT MAX(Id) FROM dbo_Widget", [], |row| row.get(0))
            .unwrap();
        assert_eq!(max_id, 4096);
    }

    #[test]
    fn creates_indexes_after_importing_rows() {
        let dir = temp_dir("post-index");
        write_csv(&dir, "dbo.Widget.csv", "Id,Name\n1,Gear\n2,Bolt\n");
        let widget = table(
            "Widget",
            vec![
                column("Id", SqlServerType::Int, false),
                column("Name", SqlServerType::Text, true),
            ],
        );
        let schema = schema_with_indexes(
            vec![widget.clone()],
            vec![IndexDef {
                name: "IX_Widget_Name".into(),
                table: widget.name.clone(),
                columns: vec!["Name".into()],
                unique: false,
                clustered: None,
                filter: None,
            }],
        );
        let mut conn = Connection::open_in_memory().unwrap();

        import_database(&mut conn, &schema, &options(dir)).unwrap();

        let index_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'index' AND name = 'IX_Widget_Name'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(index_count, 1);
        let row_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM dbo_Widget", [], |row| row.get(0))
            .unwrap();
        assert_eq!(row_count, 2);
    }

    #[test]
    fn imports_columns_in_csv_order_different_from_schema_order() {
        let dir = temp_dir("order");
        write_csv(&dir, "dbo.Widget.csv", "Name,Id\nGear,7\n");
        let schema = schema(vec![table(
            "Widget",
            vec![
                column("Id", SqlServerType::Int, false),
                column(
                    "Name",
                    SqlServerType::NVarChar {
                        length: Some(50),
                        max: false,
                    },
                    true,
                ),
            ],
        )]);
        let mut conn = Connection::open_in_memory().unwrap();

        import_database(&mut conn, &schema, &options(dir)).unwrap();

        let row: (i64, String) = conn
            .query_row("SELECT Id, Name FROM dbo_Widget", [], |row| {
                Ok((row.get(0)?, row.get(1)?))
            })
            .unwrap();
        assert_eq!(row, (7, "Gear".to_owned()));
    }

    #[test]
    fn imports_null_values() {
        let dir = temp_dir("null");
        write_csv(&dir, "dbo.Widget.csv", "Id,Name\n1,\\N\n");
        let schema = schema(vec![table(
            "Widget",
            vec![
                column("Id", SqlServerType::Int, false),
                column(
                    "Name",
                    SqlServerType::NVarChar {
                        length: Some(50),
                        max: false,
                    },
                    true,
                ),
            ],
        )]);
        let mut conn = Connection::open_in_memory().unwrap();

        import_database(&mut conn, &schema, &options(dir)).unwrap();

        let is_null: i64 = conn
            .query_row("SELECT Name IS NULL FROM dbo_Widget", [], |row| row.get(0))
            .unwrap();
        assert_eq!(is_null, 1);
    }

    #[test]
    fn imports_multiple_tables() {
        let dir = temp_dir("multiple");
        write_csv(&dir, "dbo.Widget.csv", "Id\n1\n");
        write_csv(&dir, "dbo.Gadget.csv", "Id\n2\n");
        let schema = schema(vec![
            table("Widget", vec![column("Id", SqlServerType::Int, false)]),
            table("Gadget", vec![column("Id", SqlServerType::Int, false)]),
        ]);
        let mut conn = Connection::open_in_memory().unwrap();

        let report = import_database(&mut conn, &schema, &options(dir)).unwrap();

        assert_eq!(report.tables.len(), 2);
        assert_eq!(report.rows_inserted, 2);
        let total: i64 = conn
            .query_row(
                "SELECT (SELECT COUNT(*) FROM dbo_Widget) + (SELECT COUNT(*) FROM dbo_Gadget)",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(total, 2);
    }

    #[test]
    fn rejects_invalid_rows_in_non_strict_mode_and_documents_accumulation() {
        let dir = temp_dir("invalid");
        write_csv(&dir, "dbo.Widget.csv", "Id\n1\nnot-int\n3\n");
        let schema = schema(vec![table(
            "Widget",
            vec![column("Id", SqlServerType::Int, false)],
        )]);
        let mut conn = Connection::open_in_memory().unwrap();
        let mut opts = options(dir);
        opts.strict = false;

        let report = import_database(&mut conn, &schema, &opts).unwrap();

        assert_eq!(report.rows_read, 3);
        assert_eq!(report.rows_inserted, 2);
        assert_eq!(report.rows_rejected, 1);
        assert_eq!(report.tables[0].status, TableImportStatus::Partial);
    }

    #[test]
    fn rolls_back_schema_and_rows_on_fatal_import_error() {
        let dir = temp_dir("rollback");
        write_csv(&dir, "dbo.Widget.csv", "Id\n1\n");
        write_csv(&dir, "dbo.Gadget.csv", "Wrong\n2\n");
        let schema = schema(vec![
            table("Widget", vec![column("Id", SqlServerType::Int, false)]),
            table("Gadget", vec![column("Id", SqlServerType::Int, false)]),
        ]);
        let mut conn = Connection::open_in_memory().unwrap();

        let error = import_database(&mut conn, &schema, &options(dir)).unwrap_err();

        assert!(error.to_string().contains("missing required CSV column Id"));
        let tables: i64 = conn.query_row("SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name IN ('dbo_Widget', 'dbo_Gadget')", [], |row| row.get(0)).unwrap();
        assert_eq!(tables, 0);
    }

    #[test]
    fn fatal_import_error_returns_partial_report_for_completed_and_failed_tables() {
        let dir = temp_dir("partial-report");
        write_csv(&dir, "dbo.Widget.csv", "Id\n1\n2\n");
        write_csv(&dir, "dbo.Gadget.csv", "Id\n10\nnot-int\n30\n");
        let schema = schema(vec![
            table("Widget", vec![column("Id", SqlServerType::Int, false)]),
            table("Gadget", vec![column("Id", SqlServerType::Int, false)]),
        ]);
        let mut conn = Connection::open_in_memory().unwrap();
        let mut opts = options(dir.clone());
        opts.strict = true;

        let error = import_database(&mut conn, &schema, &opts).unwrap_err();
        let Error::ImportFailure { report, source } = error else {
            panic!("expected ImportFailure");
        };

        assert!(source.to_string().contains("[dbo].[Gadget]"));
        assert_eq!(report.tables.len(), 2);
        assert_eq!(report.rows_read, 4);
        assert_eq!(report.rows_inserted, 3);
        assert_eq!(report.tables[0].source_table, "[dbo].[Widget]");
        assert_eq!(report.tables[0].sqlite_table, "dbo_Widget");
        assert_eq!(report.tables[0].status, TableImportStatus::Imported);
        assert_eq!(report.tables[0].rows_read, 2);
        assert_eq!(report.tables[0].rows_inserted, 2);

        let failed = &report.tables[1];
        assert_eq!(failed.source_table, "[dbo].[Gadget]");
        assert_eq!(failed.sqlite_table, "dbo_Gadget");
        assert_eq!(
            failed.csv_path.as_deref(),
            Some(dir.join("dbo.Gadget.csv").as_str())
        );
        assert_eq!(failed.status, TableImportStatus::Failed);
        assert_eq!(failed.rows_read, 2);
        assert_eq!(failed.rows_inserted, 1);
        assert!(failed.diagnostics[0].contains("[dbo].[Gadget]"));
        assert!(failed.diagnostics[0].contains("row 3"));
        assert!(failed.diagnostics[0].contains("invalid value"));
    }
}
