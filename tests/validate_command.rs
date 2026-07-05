use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use corrodeql::app::run::run_with_args;
use rusqlite::Connection;

fn temp_root(name: &str) -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!("corrodeql-validate-command-{name}-{unique}"));
    fs::create_dir_all(&path).unwrap();
    path
}

#[test]
fn validate_database_only_succeeds_for_existing_sqlite_database() {
    let root = temp_root("db-only");
    let db_path = root.join("output.sqlite");
    let connection = Connection::open(&db_path).unwrap();
    connection
        .execute(
            "CREATE TABLE item (id INTEGER PRIMARY KEY, name TEXT NOT NULL)",
            [],
        )
        .unwrap();
    drop(connection);

    run_with_args([
        "corrodeql".into(),
        "validate".into(),
        "--db".into(),
        db_path.into_os_string(),
    ])
    .unwrap();
}

#[test]
fn validate_database_only_rejects_missing_sqlite_database_path() {
    let root = temp_root("missing-db");
    let db_path = root.join("missing.sqlite");

    let error = run_with_args([
        "corrodeql".into(),
        "validate".into(),
        "--db".into(),
        db_path.into_os_string(),
    ])
    .expect_err("missing database should fail validation");

    assert!(
        format!("{error:#}").contains("SQLite database path does not exist"),
        "unexpected error: {error:#}"
    );
}

#[test]
fn validate_full_schema_data_and_database_still_succeeds() {
    let root = temp_root("full");
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
