use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use corrodeql::app::run::run_with_args;

fn temp_root(name: &str) -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!("corrodeql-convert-{name}-{unique}"));
    fs::create_dir_all(&path).unwrap();
    path
}

fn write_fixture(root: &Path) -> (PathBuf, PathBuf) {
    let schema = root.join("schema.sql");
    let data_dir = root.join("csv");
    fs::create_dir_all(&data_dir).unwrap();
    fs::write(
        &schema,
        "CREATE TABLE [dbo].[Widget] (Id int NOT NULL, Name nvarchar(50) NULL);",
    )
    .unwrap();
    fs::write(data_dir.join("dbo.Widget.csv"), "Id,Name\n1,Gear\n").unwrap();
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
