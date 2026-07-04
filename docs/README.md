# CorrodeQL Documentation

## Strict/default conversion behavior

CorrodeQL defaults to fail-fast behavior for inputs or validation results that could hide data loss:

| Condition | Default behavior | Permissive option |
| --- | --- | --- |
| Missing CSV for a schema table | Error | `--allow-missing-csv` records a warning and skips import for that table. |
| Extra column in a table CSV | Error | `--allow-extra-csv-columns` ignores the values and records a diagnostic. |
| Unknown schema statement | Warning | `--strict` promotes the warning to an error. |
| Unsupported constraint option or constraint fragment | Warning | `--strict` promotes the warning to an error. |
| Unsupported index construct, such as unsupported `INCLUDE` or index options | Error | `--ignore-unsupported-indexes` downgrades the issue to a warning. |
| Failed post-import validation | Error | `--skip-foreign-key-check` only skips SQLite foreign-key validation; other validation failures still fail conversion. |

## Permissive options and reporting

Permissive options let conversion continue, but they do not silently discard information:

- `--allow-missing-csv` changes missing CSV files from fatal errors into skipped table imports. Reports include the skipped table and diagnostic context.
- `--allow-extra-csv-columns` allows CSV headers that are not present in the schema. Reports still include the extra columns that were ignored.
- `--skip-foreign-key-check` does not run SQLite `PRAGMA foreign_key_check`. The validation report marks the check as skipped.
- `--ignore-unsupported-indexes` allows conversion when index-only features cannot be represented safely in SQLite. Reports still include the unsupported index diagnostic.
- `--strict` promotes unknown schema statements and unsupported constraints from warnings to errors for stricter schema compatibility checks.

Text and JSON conversion reports are the audit trail for warnings, skipped imports, unsupported SQL Server features, CSV issues, and skipped validation checks.
