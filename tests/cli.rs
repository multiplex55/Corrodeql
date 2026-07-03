use std::path::PathBuf;

use clap::{error::ErrorKind, Parser};
use corrodeql::app::cli::{Cli, Command, TableNameMode};

#[test]
fn root_help_is_available() {
    let error = Cli::try_parse_from(["corrodeql", "--help"]).expect_err("help exits through clap");
    assert_eq!(error.kind(), ErrorKind::DisplayHelp);
}

#[test]
fn convert_help_is_available() {
    let error = Cli::try_parse_from(["corrodeql", "convert", "--help"])
        .expect_err("help exits through clap");
    assert_eq!(error.kind(), ErrorKind::DisplayHelp);
}

#[test]
fn full_convert_arguments_parse() {
    let cli = Cli::try_parse_from([
        "corrodeql",
        "convert",
        "--schema",
        "schema.sql",
        "--data-dir",
        "csv",
        "--out",
        "out.sqlite",
        "--overwrite",
        "--null-token",
        "NULL",
        "--table-name-mode",
        "drop-dbo",
        "--emit-ddl",
        "ddl.sql",
        "--report-dir",
        "reports",
        "--strict",
        "--allow-missing-csv",
        "--allow-extra-csv-columns",
        "--skip-foreign-key-check",
        "--dry-run",
    ])
    .expect("full convert args should parse");

    let Command::Convert(args) = cli.command else {
        panic!("expected convert command");
    };

    assert_eq!(args.schema, Some(PathBuf::from("schema.sql")));
    assert_eq!(args.data_dir, Some(PathBuf::from("csv")));
    assert_eq!(args.out, Some(PathBuf::from("out.sqlite")));
    assert!(args.overwrite);
    assert_eq!(args.null_token.as_deref(), Some("NULL"));
    assert_eq!(args.table_name_mode, Some(TableNameMode::DropDbo));
    assert_eq!(args.emit_ddl, Some(PathBuf::from("ddl.sql")));
    assert_eq!(args.report_dir, Some(PathBuf::from("reports")));
    assert!(args.strict);
    assert!(args.allow_missing_csv);
    assert!(args.allow_extra_csv_columns);
    assert!(args.skip_foreign_key_check);
    assert!(args.dry_run);
}

#[test]
fn invalid_table_name_mode_is_rejected() {
    let error = Cli::try_parse_from(["corrodeql", "convert", "--table-name-mode", "invalid-mode"])
        .expect_err("invalid value should fail");

    assert_eq!(error.kind(), ErrorKind::InvalidValue);
}

#[test]
fn convert_can_parse_without_paths_for_interactive_prompting() {
    let cli =
        Cli::try_parse_from(["corrodeql", "convert"]).expect("paths are optional at parse time");

    let Command::Convert(args) = cli.command else {
        panic!("expected convert command");
    };

    assert!(args.schema.is_none());
    assert!(args.data_dir.is_none());
    assert!(args.out.is_none());
}
