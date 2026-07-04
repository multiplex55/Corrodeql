use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use corrodeql::app::run::run_with_args;
use corrodeql::data::csv_reader::{CsvReader, CsvReaderOptions};
use corrodeql::data::row_counts::read_row_count_manifest;
use corrodeql::schema::{model::DiagnosticSeverity, parser};

fn temp_root(name: &str) -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!("corrodeql-example-{name}-{unique}"));
    fs::create_dir_all(&path).unwrap();
    path
}

fn example_schema() -> corrodeql::schema::model::DatabaseSchema {
    let schema_text = fs::read_to_string("examples/basic/schema.sql").unwrap();
    parser::parse(schema_text)
}

#[test]
fn basic_example_schema_parses_successfully() {
    let schema = example_schema();

    assert!(
        schema
            .diagnostics
            .iter()
            .all(|diagnostic| diagnostic.severity != DiagnosticSeverity::Error),
        "unexpected parse diagnostics: {:?}",
        schema.diagnostics
    );
    assert_eq!(schema.tables.len(), 2);
    assert_eq!(schema.indexes.len(), 0);
    assert!(schema.tables.iter().all(|table| table
        .primary_key
        .as_ref()
        .is_some_and(|pk| pk.columns.len() == 1)));
    assert!(schema
        .tables
        .iter()
        .any(|table| table.name.table == "Customer"));
    assert!(schema
        .tables
        .iter()
        .any(|table| table.name.table == "Order"));
    assert!(schema
        .tables
        .iter()
        .any(|table| !table.foreign_keys.is_empty()));
}

#[test]
fn basic_example_csv_headers_match_schema() {
    let schema = example_schema();

    for table in schema.tables() {
        let path = Path::new("examples/basic/data").join(format!(
            "{}.{}.csv",
            table.name.schema.as_deref().unwrap_or("dbo"),
            table.name.table
        ));
        let utf8_path = camino::Utf8PathBuf::from_path_buf(path.clone()).unwrap();
        CsvReader::from_path(&utf8_path, table, CsvReaderOptions::default()).unwrap_or_else(
            |error| panic!("{} should match schema headers: {error}", path.display()),
        );
    }
}

#[test]
fn basic_example_row_counts_manifest_can_be_read() {
    let manifest = read_row_count_manifest("examples/basic")
        .unwrap()
        .expect("examples/basic/row_counts.csv should exist");

    assert_eq!(manifest.counts.len(), 2);
    for table in example_schema().tables() {
        assert!(
            manifest.counts.contains_key(&table.name),
            "missing row count for {}",
            table.name.display_sql_server()
        );
    }
}

#[test]
fn init_example_creates_expected_files_in_temp_directory() {
    let root = temp_root("init");

    run_with_args([
        "corrodeql".into(),
        "init-example".into(),
        "--out".into(),
        root.clone().into_os_string(),
    ])
    .unwrap();

    for relative in [
        "schema.sql",
        "data/dbo.Customer.csv",
        "data/dbo.Order.csv",
        "row_counts.csv",
        "README.md",
    ] {
        assert!(root.join(relative).exists(), "missing {relative}");
    }
}

#[test]
fn init_example_converts_and_validates_successfully() {
    let root = temp_root("init-convert");
    let db_path = root.join("generated.sqlite");

    run_with_args([
        "corrodeql".into(),
        "init-example".into(),
        "--out-dir".into(),
        root.clone().into_os_string(),
    ])
    .unwrap();

    run_with_args([
        "corrodeql".into(),
        "convert".into(),
        "--schema".into(),
        root.join("schema.sql").into_os_string(),
        "--data-dir".into(),
        root.join("data").into_os_string(),
        "--out".into(),
        db_path.clone().into_os_string(),
    ])
    .unwrap();

    assert!(db_path.exists(), "missing generated SQLite database");

    run_with_args([
        "corrodeql".into(),
        "validate".into(),
        "--schema".into(),
        root.join("schema.sql").into_os_string(),
        "--data-dir".into(),
        root.join("data").into_os_string(),
        "--db".into(),
        db_path.into_os_string(),
    ])
    .unwrap();
}

#[test]
fn init_example_refuses_to_overwrite_existing_files() {
    let root = temp_root("overwrite");
    fs::create_dir_all(root.join("data")).unwrap();
    fs::write(root.join("schema.sql"), "sentinel").unwrap();

    let result = run_with_args([
        "corrodeql".into(),
        "init-example".into(),
        "--out-dir".into(),
        root.clone().into_os_string(),
    ]);

    assert!(result.is_err());
    assert_eq!(
        fs::read_to_string(root.join("schema.sql")).unwrap(),
        "sentinel"
    );
}
