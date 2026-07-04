use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use corrodeql::app::run::run_with_args;
use rusqlite::Connection;

static TEMP_COUNTER: AtomicUsize = AtomicUsize::new(0);

fn temp_root(name: &str) -> PathBuf {
    let unique = TEMP_COUNTER.fetch_add(1, Ordering::SeqCst);
    let path = std::env::temp_dir().join(format!(
        "corrodeql-integration-basic-{name}-{}-{unique}",
        std::process::id()
    ));
    fs::create_dir_all(&path).unwrap();
    path
}

fn convert_basic(root: &Path) -> PathBuf {
    let db = root.join("output.sqlite");
    run_with_args([
        "corrodeql".into(),
        "convert".into(),
        "--schema".into(),
        "examples/basic/schema.sql".into(),
        "--data-dir".into(),
        "examples/basic/data".into(),
        "--out".into(),
        db.clone().into_os_string(),
        "--report-dir".into(),
        root.join("reports").into_os_string(),
    ])
    .unwrap();
    db
}

fn expected_row_counts(path: &str) -> Vec<(String, i64)> {
    let mut reader = csv::Reader::from_path(path).unwrap();
    reader
        .records()
        .map(|record| {
            let record = record.unwrap();
            (
                format!("{}_{}", &record[0], &record[1]),
                record[2].parse::<i64>().unwrap(),
            )
        })
        .collect()
}

#[test]
fn basic_example_full_conversion_flow_succeeds() {
    let root = temp_root("full-flow");
    let db = convert_basic(&root);

    assert!(db.exists(), "expected output database at {}", db.display());
    let connection = Connection::open(&db).unwrap();

    for table in ["dbo_Customer", "dbo_Order"] {
        let exists: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
                [table],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(exists, 1, "missing table {table}");
    }

    for (table, expected) in expected_row_counts("examples/basic/row_counts.csv") {
        let actual: i64 = connection
            .query_row(&format!("SELECT COUNT(*) FROM \"{table}\""), [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(actual, expected, "row count mismatch for {table}");
    }

    let fk_violations: i64 = connection
        .query_row("SELECT COUNT(*) FROM pragma_foreign_key_check", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(fk_violations, 0);

    let integrity: String = connection
        .query_row("PRAGMA integrity_check", [], |row| row.get(0))
        .unwrap();
    assert_eq!(integrity, "ok");

    let (name, email, credit_limit, credit_limit_type): (String, String, String, String) =
        connection
            .query_row(
                "SELECT CustomerName, Email, CreditLimit, typeof(CreditLimit) FROM dbo_Customer WHERE CustomerId = 2",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .unwrap();
    assert_eq!(name, "Grace Hopper");
    assert_eq!(email, "");
    assert_eq!(credit_limit, "2500.50");
    assert_eq!(credit_limit_type, "text");

    let (notes, total, total_type): (String, String, String) = connection
        .query_row(
            "SELECT Notes, OrderTotal, typeof(OrderTotal) FROM dbo_Order WHERE OrderId = 1001",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!(notes, "First order");
    assert_eq!(total, "89.99");
    assert_eq!(total_type, "text");
}
