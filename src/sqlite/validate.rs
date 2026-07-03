//! Validation of an existing SQLite database against schema and CSV inputs.

use std::collections::{HashMap, HashSet};

use rusqlite::Connection;

use crate::config::options::ConvertOptions;
use crate::data::manifest::{Manifest, ManifestOptions};
use crate::error::{Error, Result};
use crate::schema::model::{DatabaseSchema, TableDef};
use crate::sqlite::ddl::quote_identifier;
use crate::sqlite::names::table_names_for_schema;

/// Backwards-compatible no-op marker for module-tree smoke tests.
pub fn validate() {}

/// Aggregate validation results for an existing SQLite database.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ValidationReport {
    pub tables: Vec<TableValidationReport>,
    pub foreign_key_violations: Vec<ForeignKeyViolation>,
    pub missing_indexes_or_constraints: Vec<String>,
}

impl ValidationReport {
    /// Returns true when all requested validation checks passed.
    pub fn is_success(&self) -> bool {
        self.tables
            .iter()
            .all(|table| table.status == TableValidationStatus::Valid)
            && self.foreign_key_violations.is_empty()
            && self.missing_indexes_or_constraints.is_empty()
    }
}

/// Per-table validation status and row-count details.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableValidationReport {
    pub source_table: String,
    pub sqlite_table: String,
    pub status: TableValidationStatus,
    pub expected_row_count: Option<u64>,
    pub actual_row_count: Option<u64>,
    pub missing_not_null_columns: Vec<String>,
}

/// Per-table validation outcome.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TableValidationStatus {
    Valid,
    MissingTable,
    RowCountMismatch,
    MissingConstraints,
}

/// A row returned by `PRAGMA foreign_key_check`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForeignKeyViolation {
    pub table: String,
    pub rowid: Option<i64>,
    pub parent: String,
    pub fkid: i64,
}

/// Validates an existing SQLite database against the expected schema and CSV row counts.
pub fn validate_database(
    connection: &Connection,
    schema: &DatabaseSchema,
    options: &ConvertOptions,
) -> Result<ValidationReport> {
    let table_names = table_names_for_schema(schema, options.table_name_mode)?;
    let manifest = Manifest::discover(
        &options.data_dir,
        schema,
        ManifestOptions {
            strict: false,
            allow_missing_csv: true,
        },
    )?;
    let existing_tables = existing_tables(connection)?;
    let existing_indexes = existing_indexes(connection)?;

    let mut report = ValidationReport::default();

    for table in schema.tables() {
        let sqlite_table = table_names.get(&table.name).ok_or_else(|| {
            validation_error(format!(
                "missing generated SQLite table name for {}",
                table.name.display_sql_server()
            ))
        })?;
        let expected_row_count = manifest
            .tables
            .get(&table.name)
            .map(count_csv_records)
            .transpose()?;

        if !existing_tables.contains(&sqlite_table.0) {
            report.tables.push(TableValidationReport {
                source_table: table.name.display_sql_server(),
                sqlite_table: sqlite_table.0.clone(),
                status: TableValidationStatus::MissingTable,
                expected_row_count,
                actual_row_count: None,
                missing_not_null_columns: required_not_null_columns(table),
            });
            continue;
        }

        let actual_row_count = Some(table_row_count(connection, &sqlite_table.0)?);
        let missing_not_null_columns =
            missing_required_not_null_columns(connection, table, &sqlite_table.0)?;
        let status = if !missing_not_null_columns.is_empty() {
            TableValidationStatus::MissingConstraints
        } else if expected_row_count.is_some() && expected_row_count != actual_row_count {
            TableValidationStatus::RowCountMismatch
        } else {
            TableValidationStatus::Valid
        };

        report.tables.push(TableValidationReport {
            source_table: table.name.display_sql_server(),
            sqlite_table: sqlite_table.0.clone(),
            status,
            expected_row_count,
            actual_row_count,
            missing_not_null_columns,
        });
    }

    for index in &schema.indexes {
        if index.columns.is_empty()
            || index.filter.is_some()
            || !table_names.contains_key(&index.table)
        {
            continue;
        }
        if !existing_indexes.contains(&index.name) {
            report
                .missing_indexes_or_constraints
                .push(format!("missing index {}", index.name));
        }
    }

    if !options.skip_foreign_key_check {
        report.foreign_key_violations = foreign_key_violations(connection)?;
    }

    Ok(report)
}

fn existing_tables(connection: &Connection) -> Result<HashSet<String>> {
    let mut statement = connection.prepare(
        "SELECT name FROM sqlite_master WHERE type IN ('table', 'view') AND name NOT LIKE 'sqlite_%'",
    )?;
    let rows = statement.query_map([], |row| row.get::<_, String>(0))?;
    rows.collect::<std::result::Result<HashSet<_>, _>>()
        .map_err(Into::into)
}

fn existing_indexes(connection: &Connection) -> Result<HashSet<String>> {
    let mut statement = connection.prepare(
        "SELECT name FROM sqlite_master WHERE type = 'index' AND name NOT LIKE 'sqlite_autoindex_%'",
    )?;
    let rows = statement.query_map([], |row| row.get::<_, String>(0))?;
    rows.collect::<std::result::Result<HashSet<_>, _>>()
        .map_err(Into::into)
}

fn table_row_count(connection: &Connection, table: &str) -> Result<u64> {
    let sql = format!("SELECT COUNT(*) FROM {}", quote_identifier(table));
    Ok(connection.query_row(&sql, [], |row| row.get::<_, i64>(0))? as u64)
}

fn count_csv_records(path: &camino::Utf8PathBuf) -> Result<u64> {
    let mut reader = csv::Reader::from_path(path)?;
    let mut count = 0;
    for record in reader.records() {
        record?;
        count += 1;
    }
    Ok(count)
}

fn required_not_null_columns(table: &TableDef) -> Vec<String> {
    table
        .columns
        .iter()
        .filter(|column| !column.nullable)
        .map(|column| column.name.clone())
        .collect()
}

fn missing_required_not_null_columns(
    connection: &Connection,
    table: &TableDef,
    sqlite_table: &str,
) -> Result<Vec<String>> {
    let sql = format!("PRAGMA table_info({})", quote_identifier(sqlite_table));
    let mut statement = connection.prepare(&sql)?;
    let rows = statement.query_map([], |row| {
        Ok((row.get::<_, String>(1)?, row.get::<_, i64>(3)? != 0))
    })?;
    let metadata = rows.collect::<std::result::Result<HashMap<_, _>, _>>()?;

    Ok(table
        .columns
        .iter()
        .filter(|column| !column.nullable)
        .filter(|column| !metadata.get(&column.name).copied().unwrap_or(false))
        .map(|column| column.name.clone())
        .collect())
}

fn foreign_key_violations(connection: &Connection) -> Result<Vec<ForeignKeyViolation>> {
    let mut statement = connection.prepare("PRAGMA foreign_key_check")?;
    let rows = statement.query_map([], |row| {
        Ok(ForeignKeyViolation {
            table: row.get(0)?,
            rowid: row.get(1)?,
            parent: row.get(2)?,
            fkid: row.get(3)?,
        })
    })?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map_err(Into::into)
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
        ColumnDef, ForeignKeyDef, IndexDef, PrimaryKeyDef, SqlServerType, TableDef, TableName,
    };
    use crate::sqlite::ddl;

    fn temp_dir(name: &str) -> Utf8PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("corrodeql-validate-{name}-{unique}"));
        fs::create_dir_all(&path).unwrap();
        Utf8PathBuf::from_path_buf(path).unwrap()
    }

    fn col(name: &str, nullable: bool) -> ColumnDef {
        ColumnDef {
            name: name.to_owned(),
            data_type: SqlServerType::Int,
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
            unique_constraints: vec![],
            foreign_keys: vec![],
            check_constraints: vec![],
        }
    }

    fn schema(tables: Vec<TableDef>, indexes: Vec<IndexDef>) -> DatabaseSchema {
        DatabaseSchema {
            tables,
            indexes,
            diagnostics: vec![],
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

    fn parent_table() -> TableDef {
        let mut parent = table("Parent", vec![col("Id", false)]);
        parent.primary_key = Some(PrimaryKeyDef {
            name: None,
            columns: vec!["Id".to_owned()],
            clustered: None,
        });
        parent
    }

    fn create_schema(connection: &Connection, schema: &DatabaseSchema) {
        let generated = ddl::generate(schema, &ConvertOptions::default()).unwrap();
        connection.execute_batch(&generated.to_sql()).unwrap();
    }

    #[test]
    fn successful_validation() {
        let dir = temp_dir("success");
        write_csv(&dir, "dbo.Widget.csv", "Id,ParentId\n1,1\n");
        let schema = schema(
            vec![table(
                "Widget",
                vec![col("Id", false), col("ParentId", true)],
            )],
            vec![IndexDef {
                name: "IX_Widget_ParentId".to_owned(),
                table: TableName::new(Some("dbo".to_owned()), "Widget"),
                columns: vec!["ParentId".to_owned()],
                unique: false,
                clustered: None,
                filter: None,
            }],
        );
        let connection = Connection::open_in_memory().unwrap();
        create_schema(&connection, &schema);
        connection
            .execute("INSERT INTO dbo_Widget (Id, ParentId) VALUES (1, 1)", [])
            .unwrap();

        let report = validate_database(&connection, &schema, &options(dir)).unwrap();
        assert!(report.is_success());
        assert_eq!(report.tables[0].expected_row_count, Some(1));
        assert_eq!(report.tables[0].actual_row_count, Some(1));
    }

    #[test]
    fn row_count_mismatch() {
        let dir = temp_dir("rows");
        write_csv(&dir, "dbo.Widget.csv", "Id\n1\n2\n");
        let schema = schema(vec![table("Widget", vec![col("Id", false)])], vec![]);
        let connection = Connection::open_in_memory().unwrap();
        create_schema(&connection, &schema);
        connection
            .execute("INSERT INTO dbo_Widget (Id) VALUES (1)", [])
            .unwrap();

        let report = validate_database(&connection, &schema, &options(dir)).unwrap();
        assert_eq!(
            report.tables[0].status,
            TableValidationStatus::RowCountMismatch
        );
        assert!(!report.is_success());
    }

    #[test]
    fn foreign_key_violation_detection() {
        let dir = temp_dir("fk");
        write_csv(&dir, "dbo.Parent.csv", "Id\n");
        write_csv(&dir, "dbo.Child.csv", "Id,ParentId\n1,99\n");
        let mut child = table("Child", vec![col("Id", false), col("ParentId", false)]);
        child.foreign_keys.push(ForeignKeyDef {
            name: None,
            columns: vec!["ParentId".to_owned()],
            referenced_table: TableName::new(Some("dbo".to_owned()), "Parent"),
            referenced_columns: vec!["Id".to_owned()],
            on_delete: None,
            on_update: None,
        });
        let schema = schema(vec![parent_table(), child], vec![]);
        let connection = Connection::open_in_memory().unwrap();
        connection
            .execute_batch("PRAGMA foreign_keys = OFF;")
            .unwrap();
        create_schema(&connection, &schema);
        connection
            .execute("INSERT INTO dbo_Child (Id, ParentId) VALUES (1, 99)", [])
            .unwrap();

        let report = validate_database(&connection, &schema, &options(dir)).unwrap();
        assert_eq!(report.foreign_key_violations.len(), 1);
        assert!(!report.is_success());
    }

    #[test]
    fn skipping_foreign_key_check_ignores_violations() {
        let dir = temp_dir("skip-fk");
        write_csv(&dir, "dbo.Parent.csv", "Id\n");
        write_csv(&dir, "dbo.Child.csv", "Id,ParentId\n1,99\n");
        let mut child = table("Child", vec![col("Id", false), col("ParentId", false)]);
        child.foreign_keys.push(ForeignKeyDef {
            name: None,
            columns: vec!["ParentId".to_owned()],
            referenced_table: TableName::new(Some("dbo".to_owned()), "Parent"),
            referenced_columns: vec!["Id".to_owned()],
            on_delete: None,
            on_update: None,
        });
        let schema = schema(vec![parent_table(), child], vec![]);
        let connection = Connection::open_in_memory().unwrap();
        connection
            .execute_batch("PRAGMA foreign_keys = OFF;")
            .unwrap();
        create_schema(&connection, &schema);
        connection
            .execute("INSERT INTO dbo_Child (Id, ParentId) VALUES (1, 99)", [])
            .unwrap();
        let mut opts = options(dir);
        opts.skip_foreign_key_check = true;

        let report = validate_database(&connection, &schema, &opts).unwrap();
        assert!(report.foreign_key_violations.is_empty());
    }

    #[test]
    fn missing_expected_table() {
        let dir = temp_dir("missing-table");
        write_csv(&dir, "dbo.Widget.csv", "Id\n1\n");
        let schema = schema(vec![table("Widget", vec![col("Id", false)])], vec![]);
        let connection = Connection::open_in_memory().unwrap();

        let report = validate_database(&connection, &schema, &options(dir)).unwrap();
        assert_eq!(report.tables[0].status, TableValidationStatus::MissingTable);
        assert!(!report.is_success());
    }

    #[test]
    fn missing_expected_index() {
        let dir = temp_dir("missing-index");
        write_csv(&dir, "dbo.Widget.csv", "Id\n");
        let schema = schema(
            vec![table("Widget", vec![col("Id", false)])],
            vec![IndexDef {
                name: "IX_Widget_Id".to_owned(),
                table: TableName::new(Some("dbo".to_owned()), "Widget"),
                columns: vec!["Id".to_owned()],
                unique: false,
                clustered: None,
                filter: None,
            }],
        );
        let connection = Connection::open_in_memory().unwrap();
        connection
            .execute("CREATE TABLE dbo_Widget (Id INTEGER NOT NULL)", [])
            .unwrap();

        let report = validate_database(&connection, &schema, &options(dir)).unwrap();
        assert_eq!(
            report.missing_indexes_or_constraints,
            vec!["missing index IX_Widget_Id"]
        );
        assert!(!report.is_success());
    }
}
