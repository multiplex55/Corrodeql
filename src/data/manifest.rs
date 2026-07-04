use std::collections::{HashMap, HashSet};
use std::fs;

use camino::{Utf8Path, Utf8PathBuf};

use super::row_counts::ROW_COUNTS_FILE;

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
        let mut csv_paths = Vec::new();

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

            if !is_csv_file(&path) || path.file_name() == Some(ROW_COUNTS_FILE) {
                continue;
            }

            csv_paths.push(path);
        }

        manifest_from_csv_paths(csv_paths, expected, options)
    }
}

fn manifest_from_csv_paths(
    csv_paths: Vec<Utf8PathBuf>,
    expected: HashSet<TableName>,
    options: ManifestOptions,
) -> Result<Manifest> {
    #[derive(Debug, Clone)]
    struct Candidate {
        path: Utf8PathBuf,
        table: TableName,
        rank: u8,
    }

    let mut diagnostics = Vec::new();
    let mut candidates: Vec<Candidate> = Vec::new();
    let mut table_only: HashMap<String, Vec<TableName>> = HashMap::new();
    for table in &expected {
        table_only
            .entry(table.table.clone())
            .or_default()
            .push(table.clone());
    }

    for path in csv_paths {
        let file_name = path.file_name().ok_or_else(|| {
            validation_error(format!("CSV path has no filename component: {}", path))
        })?;
        let stem = csv_stem(file_name, &path)?;
        let exact = table_name_from_exact_stem(stem);
        if let Some(table) = exact {
            candidates.push(Candidate {
                path,
                table,
                rank: 0,
            });
            continue;
        }

        let mut matched = Vec::new();
        for table in &expected {
            if stem == format!("{}_{}", table.schema.as_deref().unwrap_or(""), table.table) {
                matched.push((table.clone(), 1));
            }
        }
        if matched.is_empty() {
            if let Some(tables) = table_only.get(stem) {
                if tables.len() > 1 {
                    let names = tables
                        .iter()
                        .map(TableName::display_sql_server)
                        .collect::<Vec<_>>()
                        .join(", ");
                    return Err(validation_error(format!(
                        "ambiguous CSV filename {file_name}: table-only fallback matches multiple schema tables: {names}"
                    )));
                }
                matched.push((tables[0].clone(), 2));
            }
        }
        if matched.len() > 1 {
            let names = matched
                .iter()
                .map(|(t, _)| t.display_sql_server())
                .collect::<Vec<_>>()
                .join(", ");
            return Err(validation_error(format!(
                "ambiguous CSV fallback filename {file_name}: matches multiple schema tables: {names}"
            )));
        }
        if let Some((table, rank)) = matched.pop() {
            candidates.push(Candidate { path, table, rank });
        } else {
            let table = table_name_from_csv_path_lossy(&path)?;
            diagnostics.push(ManifestDiagnostic::ExtraCsv { path, table });
        }
    }

    let mut by_table: HashMap<TableName, Vec<Candidate>> = HashMap::new();
    for candidate in candidates {
        by_table
            .entry(candidate.table.clone())
            .or_default()
            .push(candidate);
    }

    let mut discovered = HashMap::new();
    for table in &expected {
        let Some(mut table_candidates) = by_table.remove(table) else {
            continue;
        };
        table_candidates.sort_by(|a, b| a.rank.cmp(&b.rank).then_with(|| a.path.cmp(&b.path)));
        let exact: Vec<_> = table_candidates.iter().filter(|c| c.rank == 0).collect();
        if exact.len() > 1 {
            let paths = exact
                .iter()
                .map(|c| c.path.to_string())
                .collect::<Vec<_>>()
                .join(" and ");
            return Err(validation_error(format!(
                "ambiguous CSV files for table {}: {}",
                table.display_sql_server(),
                paths
            )));
        }
        if let Some(candidate) = exact.first() {
            discovered.insert(table.clone(), candidate.path.clone());
            continue;
        }
        if table_candidates.len() > 1 {
            let paths = table_candidates
                .iter()
                .map(|c| c.path.to_string())
                .collect::<Vec<_>>()
                .join(" and ");
            return Err(validation_error(format!(
                "ambiguous fallback CSV files for table {}: {}",
                table.display_sql_server(),
                paths
            )));
        }
        discovered.insert(table.clone(), table_candidates[0].path.clone());
    }

    for (_table, extras) in by_table {
        for extra in extras {
            diagnostics.push(ManifestDiagnostic::ExtraCsv {
                path: extra.path,
                table: extra.table,
            });
        }
    }

    for table in &expected {
        if !discovered.contains_key(table) {
            diagnostics.push(ManifestDiagnostic::MissingCsv {
                table: table.clone(),
            });
        }
    }

    diagnostics.sort_by(|a, b| format!("{:?}", a).cmp(&format!("{:?}", b)));
    let fatal: Vec<String> = diagnostics
        .iter()
        .filter_map(|diagnostic| match diagnostic {
            ManifestDiagnostic::MissingCsv { table }
                if options.strict && !options.allow_missing_csv =>
            {
                Some(format!("missing CSV for {}", table.display_sql_server()))
            }
            ManifestDiagnostic::ExtraCsv { path, table } if options.strict => Some(format!(
                "extra CSV file {} does not map to any schema table (parsed as {})",
                path,
                table.display_sql_server()
            )),
            _ => None,
        })
        .collect();

    if !fatal.is_empty() {
        return Err(validation_error(fatal.join("; ")));
    }

    Ok(Manifest {
        tables: discovered,
        diagnostics,
    })
}

fn csv_stem<'a>(file_name: &'a str, path: &Utf8Path) -> Result<&'a str> {
    file_name
        .strip_suffix(".csv")
        .or_else(|| file_name.strip_suffix(".CSV"))
        .ok_or_else(|| validation_error(format!("CSV file must end with .csv: {}", path)))
}

fn table_name_from_exact_stem(stem: &str) -> Option<TableName> {
    let parts: Vec<&str> = stem.split('.').collect();
    match parts.as_slice() {
        [schema, table] if !schema.is_empty() && !table.is_empty() => Some(TableName::new(
            Some((*schema).to_owned()),
            (*table).to_owned(),
        )),
        _ => None,
    }
}

fn table_name_from_csv_path_lossy(path: &Utf8Path) -> Result<TableName> {
    let file_name = path
        .file_name()
        .ok_or_else(|| validation_error(format!("CSV path has no filename component: {}", path)))?;
    let stem = csv_stem(file_name, path)?;
    Ok(table_name_from_exact_stem(stem)
        .or_else(|| {
            stem.split_once('_').and_then(|(schema, table)| {
                (!schema.is_empty() && !table.is_empty())
                    .then(|| TableName::new(Some(schema.to_owned()), table.to_owned()))
            })
        })
        .unwrap_or_else(|| TableName::new(None, stem.to_owned())))
}

#[cfg(test)]
fn table_name_from_csv_path(path: &Utf8Path) -> Result<TableName> {
    let file_name = path
        .file_name()
        .ok_or_else(|| validation_error(format!("CSV path has no filename component: {}", path)))?;
    let stem = csv_stem(file_name, path)?;
    table_name_from_exact_stem(stem).ok_or_else(|| {
        if stem.split('.').count() == 1 {
            validation_error(format!(
                "ambiguous CSV filename {}; use schema-qualified form like dbo.Customer.csv",
                file_name
            ))
        } else {
            validation_error(format!(
                "ambiguous CSV filename {}; expected exactly schema.table.csv",
                file_name
            ))
        }
    })
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

        let expected = HashSet::from([TableName::new(Some("dbo".to_owned()), "Customer")]);
        let error = manifest_from_csv_paths(
            vec![
                Utf8PathBuf::from("first/dbo.Customer.csv"),
                Utf8PathBuf::from("second/dbo.Customer.csv"),
            ],
            expected,
            ManifestOptions::default(),
        )
        .unwrap_err();
        assert!(error.to_string().contains("ambiguous CSV files"));
    }
    #[test]
    fn discovers_schema_underscore_fallback_after_exact_absent() {
        let expected = HashSet::from([TableName::new(Some("dbo".to_owned()), "Customer")]);
        let manifest = manifest_from_csv_paths(
            vec![Utf8PathBuf::from("data/dbo_Customer.csv")],
            expected,
            ManifestOptions::default(),
        )
        .unwrap();
        assert_eq!(
            manifest.tables[&TableName::new(Some("dbo".to_owned()), "Customer")],
            Utf8PathBuf::from("data/dbo_Customer.csv")
        );
    }

    #[test]
    fn discovers_table_only_fallback_when_unambiguous() {
        let expected = HashSet::from([TableName::new(Some("dbo".to_owned()), "Customer")]);
        let manifest = manifest_from_csv_paths(
            vec![Utf8PathBuf::from("data/Customer.csv")],
            expected,
            ManifestOptions::default(),
        )
        .unwrap();
        assert_eq!(
            manifest.tables[&TableName::new(Some("dbo".to_owned()), "Customer")],
            Utf8PathBuf::from("data/Customer.csv")
        );
    }

    #[test]
    fn exact_match_wins_over_fallback() {
        let expected = HashSet::from([TableName::new(Some("dbo".to_owned()), "Customer")]);
        let manifest = manifest_from_csv_paths(
            vec![
                Utf8PathBuf::from("data/Customer.csv"),
                Utf8PathBuf::from("data/dbo.Customer.csv"),
            ],
            expected,
            ManifestOptions::default(),
        )
        .unwrap();
        assert_eq!(
            manifest.tables[&TableName::new(Some("dbo".to_owned()), "Customer")],
            Utf8PathBuf::from("data/dbo.Customer.csv")
        );
    }

    #[test]
    fn ambiguous_fallback_names_fail() {
        let expected = HashSet::from([TableName::new(Some("dbo".to_owned()), "Customer")]);
        let error = manifest_from_csv_paths(
            vec![
                Utf8PathBuf::from("data/Customer.csv"),
                Utf8PathBuf::from("data/dbo_Customer.csv"),
            ],
            expected,
            ManifestOptions::default(),
        )
        .unwrap_err();
        assert!(error.to_string().contains("ambiguous fallback CSV files"));
    }

    #[test]
    fn table_only_fallback_ambiguous_across_schemas() {
        let expected = HashSet::from([
            TableName::new(Some("dbo".to_owned()), "Customer"),
            TableName::new(Some("sales".to_owned()), "Customer"),
        ]);
        let error = manifest_from_csv_paths(
            vec![Utf8PathBuf::from("data/Customer.csv")],
            expected,
            ManifestOptions {
                strict: false,
                allow_missing_csv: true,
            },
        )
        .unwrap_err();
        assert!(error
            .to_string()
            .contains("table-only fallback matches multiple schema tables"));
    }
}
