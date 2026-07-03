use std::collections::{HashMap, HashSet};
use std::fs;

use camino::{Utf8Path, Utf8PathBuf};

use crate::error::{Error, Result};
use crate::schema::model::{DatabaseSchema, TableName};

/// CSV files discovered for a schema.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Manifest {
    pub tables: HashMap<TableName, Utf8PathBuf>,
    pub diagnostics: Vec<ManifestDiagnostic>,
}

/// Controls how strictly CSV discovery validates files against the schema.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ManifestOptions {
    pub strict: bool,
    pub allow_missing_csv: bool,
}

impl Default for ManifestOptions {
    fn default() -> Self {
        Self {
            strict: true,
            allow_missing_csv: false,
        }
    }
}

/// Non-fatal discovery diagnostics.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ManifestDiagnostic {
    MissingCsv { table: TableName },
    ExtraCsv { path: Utf8PathBuf, table: TableName },
}

impl Manifest {
    /// Discovers schema-qualified CSV files in `data_dir` and maps them to schema tables.
    ///
    /// CSV filenames must be schema-qualified, for example `dbo.Customer.csv`, so they can be
    /// mapped unambiguously to SQL Server table names such as `[dbo].[Customer]`.
    pub fn discover(
        data_dir: impl AsRef<Utf8Path>,
        schema: &DatabaseSchema,
        options: ManifestOptions,
    ) -> Result<Self> {
        let data_dir = data_dir.as_ref();
        let expected: HashSet<TableName> = schema
            .tables()
            .iter()
            .map(|table| table.name.clone())
            .collect();
        let mut discovered = HashMap::new();
        let mut diagnostics = Vec::new();

        for entry in fs::read_dir(data_dir)? {
            let entry = entry?;
            let file_type = entry.file_type()?;
            if !file_type.is_file() {
                continue;
            }

            let path =
                Utf8PathBuf::from_path_buf(entry.path()).map_err(|path| Error::InvalidPath {
                    kind: "CSV file",
                    path: Utf8PathBuf::from(path.to_string_lossy().into_owned()),
                    reason: "path is not valid UTF-8",
                })?;

            if !is_csv_file(&path) {
                continue;
            }

            let table = table_name_from_csv_path(&path)?;
            if let Some(previous) = discovered.insert(table.clone(), path.clone()) {
                return Err(validation_error(format!(
                    "ambiguous CSV files for table {}: {} and {}",
                    table.display_sql_server(),
                    previous,
                    path
                )));
            }

            if !expected.contains(&table) {
                diagnostics.push(ManifestDiagnostic::ExtraCsv { path, table });
            }
        }

        for table in &expected {
            if !discovered.contains_key(table) {
                diagnostics.push(ManifestDiagnostic::MissingCsv {
                    table: table.clone(),
                });
            }
        }

        let fatal: Vec<String> = diagnostics
            .iter()
            .filter_map(|diagnostic| match diagnostic {
                ManifestDiagnostic::MissingCsv { table }
                    if options.strict && !options.allow_missing_csv =>
                {
                    Some(format!("missing CSV for {}", table.display_sql_server()))
                }
                ManifestDiagnostic::ExtraCsv { path, table } if options.strict => Some(format!(
                    "extra CSV file {} maps to {}, which is not present in schema",
                    path,
                    table.display_sql_server()
                )),
                _ => None,
            })
            .collect();

        if !fatal.is_empty() {
            return Err(validation_error(fatal.join("; ")));
        }

        discovered.retain(|table, _| expected.contains(table));

        Ok(Self {
            tables: discovered,
            diagnostics,
        })
    }
}

fn table_name_from_csv_path(path: &Utf8Path) -> Result<TableName> {
    let file_name = path
        .file_name()
        .ok_or_else(|| validation_error(format!("CSV path has no filename component: {}", path)))?;
    let stem = file_name
        .strip_suffix(".csv")
        .or_else(|| file_name.strip_suffix(".CSV"))
        .ok_or_else(|| validation_error(format!("CSV file must end with .csv: {}", path)))?;
    let parts: Vec<&str> = stem.split('.').collect();

    match parts.as_slice() {
        [schema, table] if !schema.is_empty() && !table.is_empty() => Ok(TableName::new(
            Some((*schema).to_owned()),
            (*table).to_owned(),
        )),
        [_table] => Err(validation_error(format!(
            "ambiguous CSV filename {}; use schema-qualified form like dbo.Customer.csv",
            file_name
        ))),
        _ => Err(validation_error(format!(
            "ambiguous CSV filename {}; expected exactly schema.table.csv",
            file_name
        ))),
    }
}

fn is_csv_file(path: &Utf8Path) -> bool {
    path.extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("csv"))
}

fn validation_error(message: String) -> Error {
    Error::Validation { message }
}

#[cfg(test)]
mod tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use crate::schema::model::TableDef;

    use super::*;

    fn temp_dir(name: &str) -> Utf8PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("corrodeql-manifest-{name}-{unique}"));
        fs::create_dir_all(&path).unwrap();
        Utf8PathBuf::from_path_buf(path).unwrap()
    }

    fn schema() -> DatabaseSchema {
        DatabaseSchema {
            tables: vec![
                table("dbo", "Customer"),
                table("dbo", "Order"),
                table("dbo", "OrderLine"),
            ],
            ..DatabaseSchema::default()
        }
    }

    fn table(schema: &str, table: &str) -> TableDef {
        TableDef {
            name: TableName::new(Some(schema.to_owned()), table.to_owned()),
            columns: Vec::new(),
            primary_key: None,
            unique_constraints: Vec::new(),
            foreign_keys: Vec::new(),
            check_constraints: Vec::new(),
        }
    }

    fn write_csv(dir: &Utf8Path, file_name: &str) {
        fs::write(dir.join(file_name), "Id\n1\n").unwrap();
    }

    #[test]
    fn discovers_expected_csv_files() {
        let dir = temp_dir("expected");
        write_csv(&dir, "dbo.Customer.csv");
        write_csv(&dir, "dbo.Order.csv");
        write_csv(&dir, "dbo.OrderLine.csv");

        let manifest = Manifest::discover(&dir, &schema(), ManifestOptions::default()).unwrap();

        assert_eq!(manifest.tables.len(), 3);
        assert!(manifest
            .tables
            .contains_key(&TableName::new(Some("dbo".to_owned()), "Customer")));
        assert!(manifest
            .tables
            .contains_key(&TableName::new(Some("dbo".to_owned()), "Order")));
        assert!(manifest
            .tables
            .contains_key(&TableName::new(Some("dbo".to_owned()), "OrderLine")));
        assert!(manifest.diagnostics.is_empty());
    }

    #[test]
    fn maps_schema_qualified_csv_to_sql_server_table_name() {
        let path = Utf8Path::new("dbo.Customer.csv");

        let table = table_name_from_csv_path(path).unwrap();

        assert_eq!(table, TableName::new(Some("dbo".to_owned()), "Customer"));
        assert_eq!(table.display_sql_server(), "[dbo].[Customer]");
    }

    #[test]
    fn missing_csv_errors_unless_allowed() {
        let dir = temp_dir("missing-error");
        write_csv(&dir, "dbo.Customer.csv");
        write_csv(&dir, "dbo.Order.csv");

        let error = Manifest::discover(&dir, &schema(), ManifestOptions::default()).unwrap_err();
        assert!(error
            .to_string()
            .contains("missing CSV for [dbo].[OrderLine]"));

        let manifest = Manifest::discover(
            &dir,
            &schema(),
            ManifestOptions {
                strict: true,
                allow_missing_csv: true,
            },
        )
        .unwrap();
        assert!(manifest
            .diagnostics
            .contains(&ManifestDiagnostic::MissingCsv {
                table: TableName::new(Some("dbo".to_owned()), "OrderLine")
            }));
    }

    #[test]
    fn extra_csv_errors_in_strict_mode_and_warns_otherwise() {
        let dir = temp_dir("extra");
        write_csv(&dir, "dbo.Customer.csv");
        write_csv(&dir, "dbo.Order.csv");
        write_csv(&dir, "dbo.OrderLine.csv");
        write_csv(&dir, "dbo.Invoice.csv");

        let error = Manifest::discover(&dir, &schema(), ManifestOptions::default()).unwrap_err();
        assert!(error.to_string().contains("extra CSV file"));
        assert!(error.to_string().contains("[dbo].[Invoice]"));

        let manifest = Manifest::discover(
            &dir,
            &schema(),
            ManifestOptions {
                strict: false,
                allow_missing_csv: false,
            },
        )
        .unwrap();
        assert!(manifest.diagnostics.iter().any(|diagnostic| matches!(
            diagnostic,
            ManifestDiagnostic::ExtraCsv { table, .. }
                if table == &TableName::new(Some("dbo".to_owned()), "Invoice")
        )));
    }

    #[test]
    fn duplicate_or_ambiguous_csv_errors() {
        let ambiguous = table_name_from_csv_path(Utf8Path::new("Customer.csv")).unwrap_err();
        assert!(ambiguous.to_string().contains("ambiguous CSV filename"));

        let too_many_parts =
            table_name_from_csv_path(Utf8Path::new("server.dbo.Customer.csv")).unwrap_err();
        assert!(too_many_parts
            .to_string()
            .contains("expected exactly schema.table.csv"));

        let dir = temp_dir("duplicate");
        write_csv(&dir, "dbo.Customer.csv");
        write_csv(&dir, "dbo.Customer.CSV");

        let error = Manifest::discover(
            &dir,
            &DatabaseSchema {
                tables: vec![table("dbo", "Customer")],
                ..DatabaseSchema::default()
            },
            ManifestOptions::default(),
        )
        .unwrap_err();
        assert!(error.to_string().contains("ambiguous CSV files"));
    }
}
