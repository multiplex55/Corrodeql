use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use corrodeql::app::run::run_with_args;
use rusqlite::Connection;

static TEMP_COUNTER: AtomicUsize = AtomicUsize::new(0);

fn temp_root(name: &str) -> PathBuf {
    let unique = TEMP_COUNTER.fetch_add(1, Ordering::SeqCst);
    let path = std::env::temp_dir().join(format!(
        "corrodeql-integration-complex-{name}-{}-{unique}",
        std::process::id()
    ));
    fs::create_dir_all(&path).unwrap();
    path
}

fn convert_complex(root: &Path) -> PathBuf {
    let db = root.join("output.sqlite");
    run_with_args([
        "corrodeql".into(),
        "convert".into(),
        "--schema".into(),
        "examples/complex/schema.sql".into(),
        "--data-dir".into(),
        "examples/complex/data".into(),
        "--out".into(),
        db.clone().into_os_string(),
        "--report-dir".into(),
        root.join("reports").into_os_string(),
    ])
    .unwrap();
    db
}

#[test]
fn complex_example_full_conversion_flow_succeeds() {
    let root = temp_root("full-flow");
    let db = convert_complex(&root);

    assert!(db.exists(), "expected output database at {}", db.display());
    let connection = Connection::open(&db).unwrap();

    for table in [
        "dbo_Customer",
        "dbo_Order",
        "dbo_OrderLine",
        "sales_Invoice",
    ] {
        let exists: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
                [table],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(exists, 1, "missing table {table}");
    }

    let order_rows: i64 = connection
        .query_row("SELECT COUNT(*) FROM \"dbo_Order\"", [], |row| row.get(0))
        .unwrap();
    let invoice_rows: i64 = connection
        .query_row("SELECT COUNT(*) FROM \"sales_Invoice\"", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(order_rows, 2);
    assert_eq!(invoice_rows, 2);

    let pk_columns = {
        let mut statement = connection
            .prepare(
                "SELECT name, pk FROM pragma_table_info('dbo_OrderLine') WHERE pk > 0 ORDER BY pk",
            )
            .unwrap();
        statement
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
            })
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap()
    };
    assert_eq!(
        pk_columns,
        vec![("OrderId".to_owned(), 1), ("LineNumber".to_owned(), 2)]
    );

    let duplicate = connection.execute(
        "INSERT INTO dbo_Customer (CustomerId, CustomerGuid, Email, FullName, CreditLimit, CreatedAt, IsActive) VALUES (3, '33333333-3333-3333-3333-333333333333', 'ada@example.com', 'Duplicate Ada', '1.00', '2024-04-01T00:00:00', 1)",
        [],
    );
    assert!(duplicate.is_err(), "duplicate unique email should fail");

    let fk_violations: i64 = connection
        .query_row("SELECT COUNT(*) FROM pragma_foreign_key_check", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(fk_violations, 0);

    let (order_total, order_total_type): (String, String) = connection
        .query_row(
            "SELECT OrderTotal, typeof(OrderTotal) FROM dbo_Order WHERE OrderId = 1001",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(order_total, "114.99");
    assert_eq!(order_total_type, "text");

    let (unit_price, unit_price_type): (String, String) = connection
        .query_row(
            "SELECT UnitPrice, typeof(UnitPrice) FROM dbo_OrderLine WHERE OrderId = 1001 AND LineNumber = 2",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(unit_price, "12.50");
    assert_eq!(unit_price_type, "text");

    let guid: String = connection
        .query_row(
            "SELECT CustomerGuid FROM dbo_Customer WHERE CustomerId = 1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(guid, "11111111-1111-1111-1111-111111111111");

    assert!(root.join("reports/converted_schema.sql").exists());
    assert!(root.join("reports/conversion_report.txt").exists());
    assert!(root.join("reports/conversion_report.json").exists());
}
