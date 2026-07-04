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
        schema.clone().into_os_string(),
        "--data-dir".into(),
        data_dir.clone().into_os_string(),
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
    let text_report = fs::read_to_string(report_dir.join("conversion_report.txt")).unwrap();
    assert!(text_report.contains(&format!("Input schema: {}", schema.display())));
    assert!(text_report.contains(&format!("Data directory: {}", data_dir.display())));
    assert!(text_report.contains(&format!("Output database: {}", db.display())));
    assert!(text_report.contains("Table naming mode: schema-prefix"));
    assert!(text_report.contains(r"Null token: \N"));
    assert!(text_report.contains("Rows imported per table"));
    assert!(text_report.contains("[dbo].[Widget]: 1 inserted"));
    assert!(text_report.contains("Row-count validation"));
    assert!(text_report.contains("Foreign-key validation"));
    assert!(text_report.contains("Integrity check"));
    assert!(report_dir.join("conversion_report.txt").exists());
    let json_report: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(report_dir.join("conversion_report.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(
        json_report["input_schema_path"],
        schema.display().to_string()
    );
    assert_eq!(
        json_report["data_directory"],
        data_dir.display().to_string()
    );
    assert_eq!(
        json_report["output_database_path"],
        db.display().to_string()
    );
    assert_eq!(json_report["table_name_mode"], "schema-prefix");
    assert_eq!(json_report["null_token"], r"\N");
    assert_eq!(json_report["import"]["tables"][0]["rows_inserted"], 1);
    assert!(json_report.get("row_count_validation").is_some());
    assert!(json_report.get("foreign_key_validation").is_some());
    assert!(json_report.get("integrity_check").is_some());
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
    fs::write(
        data_dir.join("row_counts.csv"),
        "schema_name,table_name,row_count\ndbo,Widget,1\n",
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
fn import_failure_after_ddl_generation_still_writes_reports() {
    let root = temp_root("partial-failure-report");
    let schema = root.join("schema.sql");
    let data_dir = root.join("csv");
    let db = root.join("out.sqlite");
    let report_dir = root.join("reports");
    fs::create_dir_all(&data_dir).unwrap();
    fs::write(&schema, include_str!("fixtures/simple_schema.sql")).unwrap();
    fs::write(
        data_dir.join("dbo.Widget.csv"),
        "Id,Name\nnot-an-int,Gear\n",
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
        "--report-dir".into(),
        report_dir.clone().into_os_string(),
        "--strict".into(),
    ]);

    assert!(result.is_err());
    assert!(report_dir.join("converted_schema.sql").exists());
    assert!(report_dir.join("conversion_report.txt").exists());
    let report: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(report_dir.join("conversion_report.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(report["validation"]["attempted"], false);
    assert!(report["validation"]["diagnostics"][0]["message"]
        .as_str()
        .unwrap()
        .contains("import failed"));
}

#[test]
fn import_failure_reports_partial_progress_for_completed_and_failed_tables() {
    let root = temp_root("partial-import-progress");
    let schema = root.join("schema.sql");
    let data_dir = root.join("csv");
    let db = root.join("out.sqlite");
    let report_dir = root.join("reports");
    fs::create_dir_all(&data_dir).unwrap();
    fs::write(
        &schema,
        "CREATE TABLE [dbo].[Alpha] (\n  [Id] int NOT NULL\n);\nCREATE TABLE [dbo].[Zebra] (\n  [Id] int NOT NULL\n);\n",
    )
    .unwrap();
    fs::write(data_dir.join("dbo.Alpha.csv"), "Id\n1\n2\n").unwrap();
    fs::write(data_dir.join("dbo.Zebra.csv"), "Id\n10\nnot-an-int\n30\n").unwrap();

    let result = run_with_args([
        "corrodeql".into(),
        "convert".into(),
        "--schema".into(),
        schema.into_os_string(),
        "--data-dir".into(),
        data_dir.clone().into_os_string(),
        "--out".into(),
        db.into_os_string(),
        "--report-dir".into(),
        report_dir.clone().into_os_string(),
        "--strict".into(),
    ]);

    assert!(result.is_err());
    let text_report = fs::read_to_string(report_dir.join("conversion_report.txt")).unwrap();
    assert!(text_report
        .contains("[dbo].[Alpha] -> \"dbo_Alpha\": Imported (read=2, inserted=2, rejected=0)"));
    assert!(text_report
        .contains("[dbo].[Zebra] -> \"dbo_Zebra\": Failed (read=2, inserted=1, rejected=0)"));
    assert!(text_report.contains("invalid value"));

    let report: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(report_dir.join("conversion_report.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(report["import"]["rows_read"], 4);
    assert_eq!(report["import"]["rows_inserted"], 3);
    assert_eq!(
        report["import"]["tables"][0]["source_table"],
        "[dbo].[Alpha]"
    );
    assert_eq!(report["import"]["tables"][0]["status"], "imported");
    assert_eq!(report["import"]["tables"][0]["rows_read"], 2);
    assert_eq!(report["import"]["tables"][0]["rows_inserted"], 2);
    assert_eq!(
        report["import"]["tables"][1]["source_table"],
        "[dbo].[Zebra]"
    );
    assert_eq!(report["import"]["tables"][1]["sqlite_table"], "dbo_Zebra");
    assert_eq!(
        report["import"]["tables"][1]["csv_path"],
        data_dir.join("dbo.Zebra.csv").display().to_string()
    );
    assert_eq!(report["import"]["tables"][1]["status"], "failed");
    assert_eq!(report["import"]["tables"][1]["rows_read"], 2);
    assert_eq!(report["import"]["tables"][1]["rows_inserted"], 1);
    assert!(report["import"]["tables"][1]["diagnostics"][0]
        .as_str()
        .unwrap()
        .contains("[dbo].[Zebra]"));
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

#[test]
fn invalid_foreign_key_data_fails_conversion_by_default() {
    let root = temp_root("invalid-fk-default");
    let schema = root.join("schema.sql");
    let data_dir = root.join("csv");
    let db = root.join("out.sqlite");
    let report_dir = root.join("reports");
    fs::create_dir_all(&data_dir).unwrap();
    fs::write(
        &schema,
        "CREATE TABLE [dbo].[Parent] (\n  [Id] int NOT NULL,\n  CONSTRAINT [PK_Parent] PRIMARY KEY ([Id])\n);\nCREATE TABLE [dbo].[Child] (\n  [Id] int NOT NULL,\n  [ParentId] int NOT NULL,\n  CONSTRAINT [PK_Child] PRIMARY KEY ([Id]),\n  CONSTRAINT [FK_Child_Parent] FOREIGN KEY ([ParentId]) REFERENCES [dbo].[Parent] ([Id])\n);\n",
    )
    .unwrap();
    fs::write(data_dir.join("dbo.Parent.csv"), "Id\n1\n").unwrap();
    fs::write(data_dir.join("dbo.Child.csv"), "Id,ParentId\n10,999\n").unwrap();

    let result = run_with_args([
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
    ]);

    assert!(result.is_err());
    let report: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(report_dir.join("conversion_report.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(report["validation"]["foreign_key_check_attempted"], true);
    assert_eq!(report["validation"]["foreign_key_check_skipped"], false);
    let violations = report["validation"]["foreign_key_violations"]
        .as_array()
        .unwrap();
    assert_eq!(violations.len(), 1);
    assert_eq!(violations[0]["child_table"], "dbo_Child");
    assert_eq!(violations[0]["parent_table"], "dbo_Parent");
    assert_eq!(violations[0]["foreign_key_id"], 0);
}

#[test]
fn invalid_foreign_key_data_succeeds_when_check_skipped_and_report_says_skipped() {
    let root = temp_root("invalid-fk-skipped");
    let schema = root.join("schema.sql");
    let data_dir = root.join("csv");
    let db = root.join("out.sqlite");
    let report_dir = root.join("reports");
    fs::create_dir_all(&data_dir).unwrap();
    fs::write(
        &schema,
        "CREATE TABLE [dbo].[Parent] (\n  [Id] int NOT NULL,\n  CONSTRAINT [PK_Parent] PRIMARY KEY ([Id])\n);\nCREATE TABLE [dbo].[Child] (\n  [Id] int NOT NULL,\n  [ParentId] int NOT NULL,\n  CONSTRAINT [PK_Child] PRIMARY KEY ([Id]),\n  CONSTRAINT [FK_Child_Parent] FOREIGN KEY ([ParentId]) REFERENCES [dbo].[Parent] ([Id])\n);\n",
    )
    .unwrap();
    fs::write(data_dir.join("dbo.Parent.csv"), "Id\n1\n").unwrap();
    fs::write(data_dir.join("dbo.Child.csv"), "Id,ParentId\n10,999\n").unwrap();

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
        "--skip-foreign-key-check".into(),
    ])
    .unwrap();

    let report: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(report_dir.join("conversion_report.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(report["validation"]["success"], true);
    assert_eq!(report["validation"]["foreign_key_check_attempted"], false);
    assert_eq!(report["validation"]["foreign_key_check_skipped"], true);
    assert!(report["validation"]["foreign_key_violations"]
        .as_array()
        .unwrap()
        .is_empty());
}

#[test]
fn row_count_manifest_matching_counts_allows_conversion() {
    let root = temp_root("row-counts-ok");
    let (schema, data_dir) = write_fixture(&root);
    fs::write(
        data_dir.join("row_counts.csv"),
        "schema_name,table_name,row_count\ndbo,Widget,1\n",
    )
    .unwrap();
    let db = root.join("out.sqlite");

    run_with_args([
        "corrodeql".into(),
        "convert".into(),
        "--schema".into(),
        schema.into_os_string(),
        "--data-dir".into(),
        data_dir.into_os_string(),
        "--out".into(),
        db.into_os_string(),
    ])
    .unwrap();
}

#[test]
fn row_count_manifest_mismatch_fails_conversion() {
    let root = temp_root("row-counts-bad");
    let (schema, data_dir) = write_fixture(&root);
    fs::write(
        data_dir.join("row_counts.csv"),
        "schema_name,table_name,row_count\ndbo,Widget,2\n",
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
        root.join("out.sqlite").into_os_string(),
    ]);

    assert!(result.is_err());
}

#[test]
fn extra_csv_column_fails_by_default() {
    let root = temp_root("extra-columns-default");
    let schema = root.join("schema.sql");
    let data_dir = root.join("csv");
    fs::create_dir_all(&data_dir).unwrap();
    fs::write(&schema, include_str!("fixtures/simple_schema.sql")).unwrap();
    fs::write(data_dir.join("dbo.Widget.csv"), "Id,Name,Extra\n1,Gear,x\n").unwrap();

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

    assert!(result.is_err());
}

#[test]
fn unsupported_index_fails_by_default_and_is_reported_when_ignored() {
    let root = temp_root("unsupported-index");
    let schema = root.join("schema.sql");
    let data_dir = root.join("csv");
    let report_dir = root.join("reports");
    fs::create_dir_all(&data_dir).unwrap();
    fs::write(
        &schema,
        "CREATE TABLE [dbo].[Widget] (\n  [Id] int NOT NULL,\n  [Name] nvarchar(50) NULL\n);\nCREATE INDEX [IX_Widget_Name] ON [dbo].[Widget] ([Name]) INCLUDE ([Id]);\n",
    )
    .unwrap();
    fs::write(data_dir.join("dbo.Widget.csv"), "Id,Name\n1,Gear\n").unwrap();

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

    run_with_args([
        "corrodeql".into(),
        "convert".into(),
        "--schema".into(),
        schema.into_os_string(),
        "--data-dir".into(),
        data_dir.into_os_string(),
        "--out".into(),
        root.join("ok.sqlite").into_os_string(),
        "--report-dir".into(),
        report_dir.clone().into_os_string(),
        "--ignore-unsupported-indexes".into(),
    ])
    .unwrap();

    let report: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(report_dir.join("conversion_report.json")).unwrap(),
    )
    .unwrap();
    let diagnostics = report["diagnostics"].as_array().unwrap();
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic["message"]
            .as_str()
            .unwrap()
            .contains("unsupported INCLUDE columns on index IX_Widget_Name")
            && diagnostic["severity"] == "warning"
    }));
}
