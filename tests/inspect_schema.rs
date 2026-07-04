use std::ffi::OsString;
use std::process::Command;

use corrodeql::app::run::run_with_args;

#[test]
fn inspect_schema_basic_example_succeeds_with_run_with_args() {
    run_with_args([
        OsString::from("corrodeql"),
        OsString::from("inspect-schema"),
        OsString::from("--schema"),
        OsString::from("examples/basic/schema.sql"),
    ])
    .expect("inspect-schema should parse the basic example schema");
}

#[test]
fn inspect_schema_prints_tables_indexes_and_warnings() {
    let output = Command::new(env!("CARGO_BIN_EXE_corrodeql"))
        .args([
            "inspect-schema",
            "--schema",
            "tests/fixtures/ssms_schema.sql",
        ])
        .output()
        .expect("failed to run corrodeql inspect-schema");

    assert!(
        output.status.success(),
        "inspect-schema failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Tables:"), "stdout was: {stdout}");
    assert!(stdout.contains("  dbo.Customer"), "stdout was: {stdout}");
    assert!(stdout.contains("    columns: 4"), "stdout was: {stdout}");
    assert!(
        stdout.contains("    primary key: CustomerId"),
        "stdout was: {stdout}"
    );
    assert!(
        stdout.contains("    foreign keys: FK_Order_Customer"),
        "stdout was: {stdout}"
    );
    assert!(stdout.contains("Indexes:"), "stdout was: {stdout}");
    assert!(
        stdout.contains("  IX_Order_CustomerId"),
        "stdout was: {stdout}"
    );
    assert!(stdout.contains("Warnings:"), "stdout was: {stdout}");
    assert!(
        stdout.contains("unknown statement skipped"),
        "stdout was: {stdout}"
    );
}

#[test]
fn inspect_schema_requires_schema_path() {
    let error = run_with_args([
        OsString::from("corrodeql"),
        OsString::from("inspect-schema"),
    ])
    .expect_err("missing schema should return an error");

    assert!(
        format!("{error:#}").contains("schema file path is required"),
        "unexpected error: {error:#}"
    );
}
