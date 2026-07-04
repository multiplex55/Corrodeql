use std::fs;

use corrodeql::{config::options::ConvertOptions, schema::parser, sqlite::ddl};

fn assert_schema_snapshot(schema_path: &str, expected_path: &str) {
    let schema_text = fs::read_to_string(schema_path)
        .unwrap_or_else(|error| panic!("failed to read schema fixture {schema_path}: {error}"));
    let expected = fs::read_to_string(expected_path)
        .unwrap_or_else(|error| panic!("failed to read DDL snapshot {expected_path}: {error}"))
        .replace("\r\n", "\n");

    let schema = parser::parse(schema_text);
    let actual = ddl::generate(&schema, &ConvertOptions::default())
        .expect("schema fixture should generate SQLite DDL")
        .to_sql()
        .replace("\r\n", "\n");

    assert_eq!(
        actual, expected,
        "SQLite DDL snapshot mismatch for {schema_path}. Expected files should only be changed when DDL behavior intentionally changes."
    );
}

#[test]
fn basic_schema_sqlite_ddl_matches_snapshot() {
    assert_schema_snapshot(
        "examples/basic/schema.sql",
        "tests/expected/basic_schema.sqlite.sql",
    );
}

#[test]
fn complex_schema_sqlite_ddl_matches_snapshot() {
    assert_schema_snapshot(
        "examples/complex/schema.sql",
        "tests/expected/complex_schema.sqlite.sql",
    );
}
