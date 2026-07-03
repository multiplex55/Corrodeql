# Sample MSSQL CSV Fixture

This directory contains one CSV file per table in `sample_mssql_schema.sql`.

## Conventions

- CSV encoding: UTF-8
- Line endings: CRLF
- Header row: yes
- Null token: `\N`
- Text quoting: RFC 4180 style, quote only when needed
- Date/time values: ISO-style strings
- `decimal`, `numeric`, `money`: exact text values, not floats
- `varbinary`, `binary`, `rowversion`: SQL Server-style hex strings such as `0x0000000000000001`
- Computed columns from `[dbo].[Node]` are included as exported values:
  - `ComputedAvailableQty`
  - `CodeAndName`

## Files

- `data/*.csv`: one CSV per table
- `row_counts.csv`: expected row counts per table
- `table_manifest.csv`: compact table/file summary
- `table_manifest.json`: full headers and row counts
- `sample_mssql_schema.sql`: copy of the schema used for this fixture

## Notes

SQL Server does not define one universal CSV export format. SSMS Export Wizard, `bcp`, `sqlcmd`, and custom `SELECT` exports can all emit slightly different CSV-like output. This fixture is intentionally ETL-safe and deterministic: nulls are unambiguous, binary values are textual hex, and decimal values preserve precision.
