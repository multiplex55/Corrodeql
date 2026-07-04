use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use corrodeql::app::run::run_with_args;

static TEMP_COUNTER: AtomicUsize = AtomicUsize::new(0);

fn temp_root(name: &str) -> PathBuf {
    let unique = TEMP_COUNTER.fetch_add(1, Ordering::SeqCst);
    let path = std::env::temp_dir().join(format!(
        "corrodeql-convert-{name}-{}-{unique}",
        std::process::id()
    ));
    fs::create_dir_all(&path).unwrap();
    path
}

fn write_fixture(root: &Path) -> (PathBuf, PathBuf) {
    let schema = root.join("schema.sql");
    let data_dir = root.join("csv");
    fs::create_dir_all(&data_dir).unwrap();
    fs::write(&schema, include_str!("fixtures/simple_schema.sql")).unwrap();
    fs::write(
        data_dir.join("dbo.Widget.csv"),
        include_str!("fixtures/simple_data/dbo.Widget.csv"),
    )
    .unwrap();
    (schema, data_dir)
}

#[test]
fn minimal_schema_and_csv_produce_sqlite_database_and_reports() {
    let root = temp_root("minimal");
    let (schema, data_dir) = write_fixture(&root);
    let db = root.join("out.sqlite");
    let ddl = root.join("converted_schema.sql");
    let report_dir = root.join("reports");

    run_with_args([
        "corrodeql".into(),
        "convert".into(),
        "--schema".into(),
        schema.into_os_string(),
        "--data-dir".into(),
        data_dir.into_os_string(),
        "--out".into(),
        db.clone().into_os_string(),
        "--emit-ddl".into(),
        ddl.clone().into_os_string(),
        "--report-dir".into(),
        report_dir.clone().into_os_string(),
    ])
    .unwrap();

    assert!(db.exists());
    let connection = rusqlite::Connection::open(&db).unwrap();
    let rows: i64 = connection
        .query_row("SELECT COUNT(*) FROM dbo_Widget", [], |row| row.get(0))
        .unwrap();
    assert_eq!(rows, 1);
    assert!(fs::read_to_string(&ddl)
        .unwrap()
        .contains("CREATE TABLE \"dbo_Widget\""));
    assert!(report_dir.join("converted_schema.sql").exists());
    assert!(report_dir.join("conversion_report.txt").exists());
    assert!(report_dir.join("conversion_report.json").exists());
}

#[test]
fn dry_run_writes_reports_and_ddl_but_not_database() {
    let root = temp_root("dry-run");
    let (schema, data_dir) = write_fixture(&root);
    let db = root.join("out.sqlite");
    let ddl = root.join("ddl.sql");
    let report_dir = root.join("reports");

    run_with_args([
        "corrodeql".into(),
        "convert".into(),
        "--schema".into(),
        schema.into_os_string(),
        "--data-dir".into(),
        data_dir.into_os_string(),
        "--out".into(),
        db.clone().into_os_string(),
        "--emit-ddl".into(),
        ddl.clone().into_os_string(),
        "--report-dir".into(),
        report_dir.clone().into_os_string(),
        "--dry-run".into(),
    ])
    .unwrap();

    assert!(!db.exists());
    assert!(ddl.exists());
    assert!(report_dir.join("converted_schema.sql").exists());
    assert!(report_dir.join("conversion_report.txt").exists());
    assert!(report_dir.join("conversion_report.json").exists());
}

#[test]
fn existing_output_database_is_rejected_without_overwrite() {
    let root = temp_root("existing");
    let (schema, data_dir) = write_fixture(&root);
    let db = root.join("out.sqlite");
    fs::write(&db, b"existing database sentinel").unwrap();

    let result = run_with_args([
        "corrodeql".into(),
        "convert".into(),
        "--schema".into(),
        schema.into_os_string(),
        "--data-dir".into(),
        data_dir.into_os_string(),
        "--out".into(),
        db.clone().into_os_string(),
    ]);

    assert!(result.is_err());
    assert_eq!(fs::read(&db).unwrap(), b"existing database sentinel");
}

#[test]
fn conversion_report_json_has_deterministic_table_and_diagnostic_order() {
    let root = temp_root("report-order");
    let schema = root.join("schema.sql");
    let data_dir = root.join("csv");
    let db = root.join("out.sqlite");
    let report_dir = root.join("reports");
    fs::create_dir_all(&data_dir).unwrap();
    fs::write(&schema, include_str!("fixtures/constraints_schema.sql")).unwrap();
    fs::write(
        data_dir.join("dbo.Customer.csv"),
        "Id,Email\n1,a@example.test\n",
    )
    .unwrap();
    fs::write(data_dir.join("dbo.Order.csv"), "Id,CustomerId\n10,1\n").unwrap();

    run_with_args([
        "corrodeql".into(),
        "convert".into(),
        "--schema".into(),
        schema.into_os_string(),
        "--data-dir".into(),
        data_dir.into_os_string(),
        "--out".into(),
        db.into_os_string(),
        "--report-dir".into(),
        report_dir.clone().into_os_string(),
    ])
    .unwrap();

    let report: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(report_dir.join("conversion_report.json")).unwrap(),
    )
    .unwrap();
    let tables = report["schema"]["tables"].as_array().unwrap();
    let names = tables
        .iter()
        .map(|table| table["source_table"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(names, vec!["[dbo].[Customer]", "[dbo].[Order]"]);
}

#[test]
fn invalid_csv_causes_validation_import_failure_without_sql_server() {
    let root = temp_root("bad-csv");
    let schema = root.join("schema.sql");
    let data_dir = root.join("csv");
    let db = root.join("out.sqlite");
    fs::create_dir_all(&data_dir).unwrap();
    fs::write(&schema, include_str!("fixtures/simple_schema.sql")).unwrap();
    fs::write(
        data_dir.join("dbo.Widget.csv"),
        include_str!("fixtures/bad_csv_data/dbo.Widget.csv"),
    )
    .unwrap();

    let result = run_with_args([
        "corrodeql".into(),
        "convert".into(),
        "--schema".into(),
        schema.into_os_string(),
        "--data-dir".into(),
        data_dir.into_os_string(),
        "--out".into(),
        db.into_os_string(),
    ]);

    assert!(result.is_err());
}

#[test]
fn missing_csv_fails_by_default_and_allow_missing_creates_empty_table() {
    let root = temp_root("missing-csv");
    let schema = root.join("schema.sql");
    let data_dir = root.join("csv");
    fs::create_dir_all(&data_dir).unwrap();
    fs::write(&schema, include_str!("fixtures/simple_schema.sql")).unwrap();

    let fail = run_with_args([
        "corrodeql".into(),
        "convert".into(),
        "--schema".into(),
        schema.clone().into_os_string(),
        "--data-dir".into(),
        data_dir.clone().into_os_string(),
        "--out".into(),
        root.join("fail.sqlite").into_os_string(),
    ]);
    assert!(fail.is_err());

    let db = root.join("ok.sqlite");
    run_with_args([
        "corrodeql".into(),
        "convert".into(),
        "--schema".into(),
        schema.into_os_string(),
        "--data-dir".into(),
        data_dir.into_os_string(),
        "--out".into(),
        db.clone().into_os_string(),
        "--allow-missing-csv".into(),
    ])
    .unwrap();
    let connection = rusqlite::Connection::open(&db).unwrap();
    let rows: i64 = connection
        .query_row("SELECT COUNT(*) FROM dbo_Widget", [], |row| row.get(0))
        .unwrap();
    assert_eq!(rows, 0);
}

#[test]
fn extra_csv_is_reported_as_strict_error() {
    let root = temp_root("extra-csv");
    let (schema, data_dir) = write_fixture(&root);
    fs::write(data_dir.join("dbo.Extra.csv"), "Id\n1\n").unwrap();
    let result = run_with_args([
        "corrodeql".into(),
        "convert".into(),
        "--schema".into(),
        schema.into_os_string(),
        "--data-dir".into(),
        data_dir.into_os_string(),
        "--out".into(),
        root.join("out.sqlite").into_os_string(),
    ]);
    assert!(result.unwrap_err().to_string().contains("extra CSV file"));
}

#[test]
fn allow_extra_csv_columns_ignores_extra_values() {
    let root = temp_root("extra-columns");
    let schema = root.join("schema.sql");
    let data_dir = root.join("csv");
    let db = root.join("out.sqlite");
    fs::create_dir_all(&data_dir).unwrap();
    fs::write(&schema, include_str!("fixtures/simple_schema.sql")).unwrap();
    fs::write(
        data_dir.join("dbo.Widget.csv"),
        "Id,Name,Ignored\n1,Gear,x\n",
    )
    .unwrap();
    run_with_args([
        "corrodeql".into(),
        "convert".into(),
        "--schema".into(),
        schema.into_os_string(),
        "--data-dir".into(),
        data_dir.into_os_string(),
        "--out".into(),
        db.clone().into_os_string(),
        "--allow-extra-csv-columns".into(),
    ])
    .unwrap();
    let connection = rusqlite::Connection::open(&db).unwrap();
    let name: String = connection
        .query_row("SELECT Name FROM dbo_Widget", [], |row| row.get(0))
        .unwrap();
    assert_eq!(name, "Gear");
}

#[test]
fn related_child_rows_import_before_parent_and_validate_after_import() {
    let root = temp_root("deferred-fk-validation");
    let schema = root.join("schema.sql");
    let data_dir = root.join("csv");
    let db = root.join("out.sqlite");
    fs::create_dir_all(&data_dir).unwrap();
    fs::write(
        &schema,
        "CREATE TABLE [dbo].[Child] (\n  [Id] int NOT NULL,\n  [ParentId] int NOT NULL,\n  CONSTRAINT [PK_Child] PRIMARY KEY ([Id]),\n  CONSTRAINT [FK_Child_Parent] FOREIGN KEY ([ParentId]) REFERENCES [dbo].[Parent] ([Id])\n);\nCREATE TABLE [dbo].[Parent] (\n  [Id] int NOT NULL,\n  CONSTRAINT [PK_Parent] PRIMARY KEY ([Id])\n);\n",
    )
    .unwrap();
    fs::write(data_dir.join("dbo.Child.csv"), "Id,ParentId\n10,1\n").unwrap();
    fs::write(data_dir.join("dbo.Parent.csv"), "Id\n1\n").unwrap();

    run_with_args([
        "corrodeql".into(),
        "convert".into(),
        "--schema".into(),
        schema.into_os_string(),
        "--data-dir".into(),
        data_dir.into_os_string(),
        "--out".into(),
        db.clone().into_os_string(),
    ])
    .unwrap();

    let connection = rusqlite::Connection::open(&db).unwrap();
    let child_rows: i64 = connection
        .query_row("SELECT COUNT(*) FROM dbo_Child", [], |row| row.get(0))
        .unwrap();
    let parent_rows: i64 = connection
        .query_row("SELECT COUNT(*) FROM dbo_Parent", [], |row| row.get(0))
        .unwrap();
    let fk_violations: i64 = connection
        .query_row("SELECT COUNT(*) FROM pragma_foreign_key_check", [], |row| {
            row.get(0)
        })
        .unwrap();

    assert_eq!(child_rows, 1);
    assert_eq!(parent_rows, 1);
    assert_eq!(fk_violations, 0);
}
