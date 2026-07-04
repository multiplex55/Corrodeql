use corrodeql::config::options::ConvertOptions;
use corrodeql::schema::model::{DiagnosticSeverity, SqlServerType, TableName};
use corrodeql::schema::{parser, preprocessor};
use corrodeql::sqlite::ddl;

const SSMS_SCHEMA_BYTES: &[u8] = include_bytes!("fixtures/ssms_schema.sql");

fn parse_fixture() -> corrodeql::schema::model::DatabaseSchema {
    let batches = preprocessor::preprocess_bytes(SSMS_SCHEMA_BYTES).unwrap();
    assert!(
        batches.len() > 1,
        "the fixture should exercise GO batch splitting"
    );
    let text = std::str::from_utf8(SSMS_SCHEMA_BYTES).unwrap();
    parser::parse(text)
}

#[test]
fn complete_v1_schema_pipeline_parses_ssms_style_fixture() {
    let schema = parse_fixture();

    assert_eq!(schema.tables.len(), 2);

    let customer = schema
        .find_table(&TableName::new(Some("dbo".to_owned()), "Customer"))
        .unwrap();
    assert_eq!(
        customer
            .columns
            .iter()
            .map(|column| column.name.as_str())
            .collect::<Vec<_>>(),
        vec!["CustomerId", "Email", "CreditLimit", "Status"]
    );
    assert_eq!(
        customer.primary_key.as_ref().unwrap().name.as_deref(),
        Some("PK_Customer")
    );
    assert_eq!(
        customer.primary_key.as_ref().unwrap().columns,
        vec!["CustomerId"]
    );
    assert_eq!(customer.unique_constraints.len(), 1);
    assert_eq!(
        customer.unique_constraints[0].name.as_deref(),
        Some("UQ_Customer_Email")
    );
    assert_eq!(customer.unique_constraints[0].columns, vec!["Email"]);
    assert_eq!(
        customer
            .columns
            .iter()
            .find(|column| column.name == "Status")
            .unwrap()
            .default
            .as_ref()
            .unwrap()
            .name
            .as_deref(),
        Some("DF_Customer_Status")
    );
    assert!(matches!(
        customer
            .columns
            .iter()
            .find(|column| column.name == "CreditLimit")
            .unwrap()
            .data_type,
        SqlServerType::Decimal {
            precision: Some(19),
            scale: Some(4)
        }
    ));

    let order = schema
        .find_table(&TableName::new(Some("dbo".to_owned()), "Order"))
        .unwrap();
    assert_eq!(
        order
            .columns
            .iter()
            .map(|column| column.name.as_str())
            .collect::<Vec<_>>(),
        vec!["OrderId", "CustomerId", "OrderTotal", "CreatedAt"]
    );
    assert_eq!(order.primary_key.as_ref().unwrap().columns, vec!["OrderId"]);
    assert_eq!(order.foreign_keys.len(), 1);
    assert_eq!(
        order.foreign_keys[0].name.as_deref(),
        Some("FK_Order_Customer")
    );
    assert_eq!(order.foreign_keys[0].columns, vec!["CustomerId"]);
    assert_eq!(order.foreign_keys[0].referenced_table, customer.name);
    assert_eq!(order.foreign_keys[0].referenced_columns, vec!["CustomerId"]);
    assert_eq!(order.check_constraints.len(), 1);
    assert_eq!(
        order.check_constraints[0].name.as_deref(),
        Some("CK_Order_Total")
    );
    assert!(order.check_constraints[0].expression.contains("OrderTotal"));
    assert!(matches!(
        order
            .columns
            .iter()
            .find(|column| column.name == "OrderTotal")
            .unwrap()
            .data_type,
        SqlServerType::Numeric {
            precision: Some(12),
            scale: Some(2)
        }
    ));

    assert_eq!(schema.indexes.len(), 2);
    assert_eq!(schema.indexes[0].name, "IX_Order_CustomerId");
    assert_eq!(schema.indexes[0].columns, vec!["CustomerId"]);
    assert!(!schema.indexes[0].unique);
    assert_eq!(schema.indexes[1].name, "UX_Order_Customer_Total");
    assert_eq!(schema.indexes[1].columns, vec!["CustomerId", "OrderTotal"]);
    assert!(schema.indexes[1].unique);

    assert!(schema.diagnostics.iter().any(|diagnostic| {
        diagnostic.severity == DiagnosticSeverity::Unsupported
            && diagnostic.message.contains("CREATE VIEW")
            && diagnostic.line.is_some()
            && diagnostic.column.is_some()
    }));
    assert!(schema.diagnostics.iter().any(|diagnostic| {
        diagnostic.severity == DiagnosticSeverity::Warning
            && diagnostic.message.contains("unknown statement")
            && diagnostic.line.is_some()
            && diagnostic.column.is_some()
    }));
}

#[test]
fn strict_mode_reports_unknown_statement_with_line_context() {
    let options = ConvertOptions {
        strict: true,
        ..ConvertOptions::default()
    };
    let schema = parser::parse_with_options(
        "CREATE TABLE [dbo].[T] ([Id] int);\nGO\nMYSTERY TOKEN;",
        &options,
    );

    let diagnostic = schema
        .diagnostics
        .iter()
        .find(|diagnostic| {
            diagnostic.severity == DiagnosticSeverity::Error
                && diagnostic.message.contains("unknown statement")
        })
        .expect("strict mode should promote unknown statements to errors");
    assert_eq!(diagnostic.line, Some(3));
    assert_eq!(diagnostic.column, Some(1));
}

#[test]
fn sqlite_ddl_quotes_names_maps_decimal_to_text_and_splits_indexes() {
    let schema = parse_fixture();
    let options = ConvertOptions::default();

    let table_sql = ddl::generate_tables(&schema, &options).unwrap().to_sql();
    assert!(table_sql.contains("CREATE TABLE \"dbo_Customer\""));
    assert!(table_sql.contains("\"CreditLimit\" TEXT"));
    assert!(table_sql.contains("CREATE TABLE \"dbo_Order\""));
    assert!(table_sql.contains("\"OrderTotal\" TEXT NOT NULL"));
    assert!(!table_sql.contains("CREATE INDEX"));

    let index_sql = ddl::generate_indexes(&schema, &options).unwrap().to_sql();
    assert!(index_sql
        .contains("CREATE INDEX \"IX_Order_CustomerId\" ON \"dbo_Order\" (\"CustomerId\");"));
    assert!(index_sql.contains("CREATE UNIQUE INDEX \"UX_Order_Customer_Total\" ON \"dbo_Order\" (\"CustomerId\", \"OrderTotal\");"));

    let full_sql = ddl::schema_sql(&schema, &options).unwrap();
    assert!(
        full_sql.find("CREATE TABLE \"dbo_Order\"").unwrap()
            < full_sql
                .find("CREATE INDEX \"IX_Order_CustomerId\"")
                .unwrap()
    );
}
