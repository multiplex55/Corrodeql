use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use corrodeql::app::run::run_with_args;

static TEMP_COUNTER: AtomicUsize = AtomicUsize::new(0);

fn temp_root(name: &str) -> PathBuf {
    let unique = TEMP_COUNTER.fetch_add(1, Ordering::SeqCst);
    let path = std::env::temp_dir().join(format!(
        "corrodeql-emit-ddl-{name}-{}-{unique}",
        std::process::id()
    ));
    fs::create_dir_all(&path).unwrap();
    path
}

#[test]
fn emit_ddl_writes_sql_file_without_creating_database() {
    let root = temp_root("basic");
    let ddl = root.join("converted_schema.sql");

    run_with_args([
        "corrodeql".into(),
        "emit-ddl".into(),
        "--schema".into(),
        PathBuf::from("examples/basic/schema.sql").into_os_string(),
        "--out".into(),
        ddl.clone().into_os_string(),
    ])
    .unwrap();

    assert!(ddl.exists());
    assert!(fs::read_to_string(&ddl).unwrap().contains("CREATE TABLE"));
    let sqlite_files = fs::read_dir(&root)
        .unwrap()
        .filter_map(Result::ok)
        .filter(|entry| {
            entry
                .path()
                .extension()
                .is_some_and(|extension| extension == "sqlite")
        })
        .collect::<Vec<_>>();
    assert!(
        sqlite_files.is_empty(),
        "emit-ddl should not create SQLite database files"
    );
}

#[test]
fn emit_ddl_parser_error_does_not_write_partial_file() {
    let root = temp_root("parser-error");
    let schema = root.join("broken_schema.sql");
    let ddl = root.join("converted_schema.sql");
    fs::write(&schema, "CREATE TABLE [Broken (Id int);").unwrap();

    let result = run_with_args([
        "corrodeql".into(),
        "emit-ddl".into(),
        "--schema".into(),
        schema.into_os_string(),
        "--out".into(),
        ddl.clone().into_os_string(),
    ]);

    assert!(result.is_err());
    assert!(!ddl.exists(), "failed emit-ddl must not write partial DDL");
}
