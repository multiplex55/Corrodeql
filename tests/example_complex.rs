use std::fs;
use std::path::Path;

use corrodeql::data::csv_reader::{CsvReader, CsvReaderOptions};
use corrodeql::data::row_counts::read_row_count_manifest;
use corrodeql::schema::{model::DiagnosticSeverity, parser};

fn example_schema() -> corrodeql::schema::model::DatabaseSchema {
    let schema_text = fs::read_to_string("examples/complex/schema.sql").unwrap();
    parser::parse(schema_text)
}

#[test]
fn complex_example_schema_parses_successfully() {
    let schema = example_schema();

    assert!(
        schema
            .diagnostics
            .iter()
            .all(|diagnostic| diagnostic.severity != DiagnosticSeverity::Error),
        "unexpected parse diagnostics: {:?}",
        schema.diagnostics
    );
    assert_eq!(schema.tables.len(), 4);
    assert_eq!(schema.indexes.len(), 3);
    assert!(schema
        .tables
        .iter()
        .any(|table| table.name.schema.as_deref() == Some("sales")));
    assert!(schema
        .tables
        .iter()
        .any(|table| table.name.table == "Order"));
    assert!(schema.tables.iter().any(|table| table
        .primary_key
        .as_ref()
        .is_some_and(|pk| pk.columns.len() == 2)));
    assert!(schema
        .tables
        .iter()
        .any(|table| !table.unique_constraints.is_empty()));
    assert!(schema
        .tables
        .iter()
        .any(|table| !table.check_constraints.is_empty()));
    assert!(
        schema
            .tables
            .iter()
            .filter(|table| !table.foreign_keys.is_empty())
            .count()
            >= 3
    );
}

#[test]
fn complex_example_csv_headers_match_schema() {
    let schema = example_schema();

    for table in schema.tables() {
        let path = Path::new("examples/complex/data").join(format!(
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
fn complex_example_row_counts_manifest_can_be_read() {
    let manifest = read_row_count_manifest("examples/complex")
        .unwrap()
        .expect("examples/complex/row_counts.csv should exist");

    assert_eq!(manifest.counts.len(), 4);
    for table in example_schema().tables() {
        assert!(
            manifest.counts.contains_key(&table.name),
            "missing row count for {}",
            table.name.display_sql_server()
        );
    }
}
