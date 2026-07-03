//! Streaming CSV reader with schema-aware header validation.

use std::fmt;
use std::fs::File;

use camino::Utf8Path;
use rusqlite::types::Value;

use crate::data::conversion::{convert_csv_value, ValueDiagnostic, DEFAULT_NULL_TOKEN};
use crate::error::{Error, Result};
use crate::schema::model::TableDef;

/// Options that control CSV reading and validation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CsvReaderOptions {
    pub null_token: String,
    pub allow_extra_csv_columns: bool,
}

impl Default for CsvReaderOptions {
    fn default() -> Self {
        Self {
            null_token: DEFAULT_NULL_TOKEN.to_owned(),
            allow_extra_csv_columns: false,
        }
    }
}

/// A converted CSV row with its physical CSV row number.
#[derive(Debug, Clone, PartialEq)]
pub struct CsvRow {
    pub row_number: u64,
    pub values: Vec<Value>,
}

/// Streaming CSV reader for one table.
pub struct CsvReader {
    table: TableDef,
    options: CsvReaderOptions,
    expected_to_csv_index: Vec<usize>,
    records: csv::StringRecordsIntoIter<File>,
}

impl fmt::Debug for CsvReader {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CsvReader")
            .field("table", &self.table)
            .field("options", &self.options)
            .field("expected_to_csv_index", &self.expected_to_csv_index)
            .finish_non_exhaustive()
    }
}

impl CsvReader {
    /// Opens a CSV file, reads and validates its header, and prepares streaming row conversion.
    pub fn from_path(
        path: impl AsRef<Utf8Path>,
        table: &TableDef,
        options: CsvReaderOptions,
    ) -> Result<Self> {
        let file = File::open(path.as_ref())?;
        let mut reader = csv::ReaderBuilder::new()
            .has_headers(true)
            .from_reader(file);
        let headers = reader.headers()?.clone();
        let expected_to_csv_index =
            validate_headers(&headers, table, options.allow_extra_csv_columns)?;

        Ok(Self {
            table: table.clone(),
            options,
            expected_to_csv_index,
            records: reader.into_records(),
        })
    }
}

impl Iterator for CsvReader {
    type Item = Result<CsvRow>;

    fn next(&mut self) -> Option<Self::Item> {
        let record = match self.records.next()? {
            Ok(record) => record,
            Err(error) => return Some(Err(error.into())),
        };
        let row_number = record
            .position()
            .map(|position| position.line())
            .unwrap_or(0);
        let mut values = Vec::with_capacity(self.table.columns.len());

        for (column, csv_index) in self.table.columns.iter().zip(&self.expected_to_csv_index) {
            let value = record.get(*csv_index).unwrap_or_default();
            match convert_csv_value(
                &self.table.name,
                column,
                row_number,
                value,
                Some(&self.options.null_token),
            ) {
                Ok(value) => values.push(value),
                Err(diagnostic) => return Some(Err(diagnostic_error(diagnostic))),
            }
        }

        Some(Ok(CsvRow { row_number, values }))
    }
}

fn validate_headers(
    headers: &csv::StringRecord,
    table: &TableDef,
    allow_extra_csv_columns: bool,
) -> Result<Vec<usize>> {
    let mut expected_to_csv_index = Vec::with_capacity(table.columns.len());
    let mut used = vec![false; headers.len()];

    for column in &table.columns {
        let index = headers.iter().position(|header| header == column.name);
        match index {
            Some(index) => {
                expected_to_csv_index.push(index);
                used[index] = true;
            }
            None => {
                return Err(validation_error(format!(
                    "missing required CSV column {} for table {}",
                    column.name,
                    table.name.display_sql_server()
                )));
            }
        }
    }

    if !allow_extra_csv_columns {
        let extras: Vec<&str> = headers
            .iter()
            .enumerate()
            .filter_map(|(index, header)| (!used[index]).then_some(header))
            .collect();
        if !extras.is_empty() {
            return Err(validation_error(format!(
                "extra CSV columns for table {}: {}",
                table.name.display_sql_server(),
                extras.join(", ")
            )));
        }
    }

    Ok(expected_to_csv_index)
}

fn diagnostic_error(diagnostic: ValueDiagnostic) -> Error {
    validation_error(format!(
        "invalid CSV value for table {}, column {}, row {}: {:?} ({})",
        diagnostic.table.display_sql_server(),
        diagnostic.column,
        diagnostic.row_number,
        diagnostic.original_value,
        diagnostic.reason
    ))
}

fn validation_error(message: String) -> Error {
    Error::Validation { message }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    use camino::Utf8PathBuf;
    use rusqlite::types::Value;

    use crate::schema::model::{ColumnDef, SqlServerType, TableDef, TableName};

    use super::*;

    static TEMP_FILE_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn col(name: &str, data_type: SqlServerType) -> ColumnDef {
        ColumnDef {
            name: name.to_owned(),
            data_type,
            nullable: true,
            identity: false,
            default: None,
            check: None,
        }
    }

    fn table(columns: Vec<ColumnDef>) -> TableDef {
        TableDef {
            name: TableName::new(Some("dbo".to_owned()), "Widget"),
            columns,
            primary_key: None,
            unique_constraints: Vec::new(),
            foreign_keys: Vec::new(),
            check_constraints: Vec::new(),
        }
    }

    fn csv_path(name: &str, contents: &str) -> Utf8PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let counter = TEMP_FILE_COUNTER.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("corrodeql-csv-{name}-{unique}-{counter}.csv"));
        fs::write(&path, contents).unwrap();
        Utf8PathBuf::from_path_buf(path).unwrap()
    }

    fn read_all(
        contents: &str,
        table: &TableDef,
        options: CsvReaderOptions,
    ) -> Result<Vec<CsvRow>> {
        CsvReader::from_path(&csv_path("read", contents), table, options)?.collect()
    }

    #[test]
    fn converts_null_token_to_sql_null() {
        let rows = read_all(
            "Name\n\\N\n",
            &table(vec![col("Name", SqlServerType::Text)]),
            Default::default(),
        )
        .unwrap();
        assert_eq!(rows[0].values, vec![Value::Null]);
    }

    #[test]
    fn preserves_empty_strings() {
        let rows = read_all(
            "Name\n\"\"\n",
            &table(vec![col("Name", SqlServerType::Text)]),
            Default::default(),
        )
        .unwrap();
        assert_eq!(rows[0].values, vec![Value::Text(String::new())]);
    }

    #[test]
    fn header_order_is_independent() {
        let rows = read_all(
            "Name,Id\nAlpha,42\n",
            &table(vec![
                col("Id", SqlServerType::Int),
                col("Name", SqlServerType::Text),
            ]),
            Default::default(),
        )
        .unwrap();
        assert_eq!(
            rows[0].values,
            vec![Value::Integer(42), Value::Text("Alpha".to_owned())]
        );
    }

    #[test]
    fn detects_missing_columns() {
        let error = CsvReader::from_path(
            &csv_path("missing", "Id\n1\n"),
            &table(vec![
                col("Id", SqlServerType::Int),
                col("Name", SqlServerType::Text),
            ]),
            Default::default(),
        )
        .unwrap_err();
        assert!(error
            .to_string()
            .contains("missing required CSV column Name"));
    }

    #[test]
    fn detects_extra_columns_unless_allowed() {
        let table = table(vec![col("Id", SqlServerType::Int)]);
        let path = csv_path("extra", "Id,Ignored\n1,x\n");
        let error = CsvReader::from_path(&path, &table, Default::default()).unwrap_err();
        assert!(error.to_string().contains("extra CSV columns"));

        let rows: Vec<CsvRow> = CsvReader::from_path(
            &path,
            &table,
            CsvReaderOptions {
                allow_extra_csv_columns: true,
                ..Default::default()
            },
        )
        .unwrap()
        .collect::<Result<_>>()
        .unwrap();
        assert_eq!(rows[0].values, vec![Value::Integer(1)]);
    }

    #[test]
    fn converts_integers() {
        let rows = read_all(
            "Id\n123\n",
            &table(vec![col("Id", SqlServerType::Int)]),
            Default::default(),
        )
        .unwrap();
        assert_eq!(rows[0].values, vec![Value::Integer(123)]);
    }

    #[test]
    fn converts_bits() {
        let rows = read_all(
            "A,B,C,D\n0,1,true,false\n",
            &table(vec![
                col("A", SqlServerType::Bit),
                col("B", SqlServerType::Bit),
                col("C", SqlServerType::Bit),
                col("D", SqlServerType::Bit),
            ]),
            Default::default(),
        )
        .unwrap();
        assert_eq!(
            rows[0].values,
            vec![
                Value::Integer(0),
                Value::Integer(1),
                Value::Integer(1),
                Value::Integer(0)
            ]
        );
    }

    #[test]
    fn invalid_values_include_row_diagnostics() {
        let error = read_all(
            "Id\n1\nnope\n",
            &table(vec![col("Id", SqlServerType::Int)]),
            Default::default(),
        )
        .unwrap_err();
        let message = error.to_string();
        assert!(message.contains("table [dbo].[Widget]"));
        assert!(message.contains("column Id"));
        assert!(message.contains("row 3"));
        assert!(message.contains("nope"));
        assert!(message.contains("expected integer"));
    }
}
