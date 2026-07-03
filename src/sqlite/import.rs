//! CSV import into SQLite.

use camino::Utf8PathBuf;
use rusqlite::{params_from_iter, Connection};

use crate::config::options::ConvertOptions;
use crate::data::csv_reader::{CsvReader, CsvReaderOptions};
use crate::data::manifest::{Manifest, ManifestOptions};
use crate::error::{Error, Result};
use crate::report::model::{ImportReport, TableImportReport, TableImportStatus};
use crate::schema::model::{DatabaseSchema, TableDef};
use crate::sqlite::ddl::{self, quote_identifier};
use crate::sqlite::names::{table_names_for_schema, Name};

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
    let generated = ddl::generate(schema, options)?;
    let table_names = table_names_for_schema(schema, options.table_name_mode)?;
    let manifest = Manifest::discover(
        &options.data_dir,
        schema,
        ManifestOptions {
            strict: options.strict,
            allow_missing_csv: options.allow_missing_csv,
        },
    )?;

    let transaction = connection.transaction()?;
    for statement in &generated.statements {
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

        let table_report = import_table(&transaction, table, sqlite_name, path, options)?;
        report.rows_read += table_report.rows_read;
        report.rows_inserted += table_report.rows_inserted;
        report.rows_rejected += table_report.rows_rejected;
        report.tables.push(table_report);
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
) -> Result<TableImportReport> {
    let insert_sql = insert_statement(table, sqlite_name);
    let mut statement = connection.prepare(&insert_sql)?;
    let reader = CsvReader::from_path(
        path,
        table,
        CsvReaderOptions {
            null_token: options.null_token.clone(),
            allow_extra_csv_columns: options.allow_extra_csv_columns,
        },
    )?;

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

    for row in reader {
        report.rows_read += 1;
        match row {
            Ok(row) => match statement.execute(params_from_iter(row.values)) {
                Ok(_) => report.rows_inserted += 1,
                Err(error) => return Err(Error::Sqlite(error)),
            },
            Err(error) if options.strict => return Err(error),
            Err(error) => {
                report.rows_rejected += 1;
                report.status = TableImportStatus::Partial;
                report.diagnostics.push(error.to_string());
            }
        }
    }

    Ok(report)
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
    use crate::schema::model::{ColumnDef, SqlServerType, TableDef, TableName};

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
}
