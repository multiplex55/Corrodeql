# CorrodeQL

CorrodeQL is a Rust CLI for converting SQL Server-style schema and CSV exports into SQLite databases with conversion reports.

## Conversion defaults

`corrodeql convert` is intentionally strict about data loss and validation:

- Missing CSV files for schema tables are errors by default.
- Extra columns in a table CSV are errors by default.
- Unknown schema statements are reported as warnings by default; `--strict` promotes them to errors.
- Unsupported constraints are reported as warnings by default; `--strict` promotes them to errors.
- Unsupported index features are errors by default because they can change query behavior or index coverage.
- Post-import validation failures, including row-count mismatches, missing tables/constraints, SQLite integrity failures, and foreign-key violations, are errors.

Warnings, skipped work, unsupported SQL Server features, CSV issues, and validation diagnostics are still written to the text and JSON conversion reports so that permissive runs remain auditable.

## Permissive conversion options

Use these options when you want conversion to continue while still recording diagnostics:

- `--allow-missing-csv`: turns missing CSV errors into warnings and skips importing those tables, leaving created SQLite tables empty.
- `--allow-extra-csv-columns`: ignores CSV columns that do not exist in the schema while still reporting them.
- `--skip-foreign-key-check`: skips SQLite `PRAGMA foreign_key_check` after import and records that validation was skipped.
- `--ignore-unsupported-indexes`: downgrades unsupported index constructs to warnings while still reporting the affected index and unsupported feature.
- `--strict`: promotes supported schema warnings such as unknown statements and unsupported constraints to errors. This does not make the permissive options silent; reports still include their diagnostics.

## Repository conventions

This repository uses modern Rust module layout conventions:

- No `mod.rs` files are allowed anywhere in the repository.
- Every module with child modules must use the sibling-file-plus-directory style.
- For example, a `schema` module with children should be organized as:
  - `src/schema.rs`
  - `src/schema/parser.rs`

## Development

Run the standard checks before submitting changes:

```sh
cargo check
cargo test
find . -name mod.rs
```
