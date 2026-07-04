//! Optional row-count manifest support.

use std::collections::HashMap;

use camino::{Utf8Path, Utf8PathBuf};
use serde::Deserialize;

use crate::error::{Error, Result};
use crate::schema::model::TableName;

pub const ROW_COUNTS_FILE: &str = "row_counts.csv";

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RowCountManifest {
    pub path: Utf8PathBuf,
    pub counts: HashMap<TableName, u64>,
}

#[derive(Debug, Deserialize)]
struct RowCountRecord {
    schema_name: Option<String>,
    table_name: String,
    row_count: u64,
}

pub fn read_row_count_manifest(data_dir: impl AsRef<Utf8Path>) -> Result<Option<RowCountManifest>> {
    let path = data_dir.as_ref().join(ROW_COUNTS_FILE);
    if !path.exists() {
        return Ok(None);
    }

    let mut reader = csv::Reader::from_path(&path)?;
    let mut counts = HashMap::new();
    for record in reader.deserialize::<RowCountRecord>() {
        let record = record?;
        let schema = record
            .schema_name
            .map(|schema| schema.trim().to_owned())
            .filter(|schema| !schema.is_empty());
        let table = record.table_name.trim().to_owned();
        if table.is_empty() {
            return Err(Error::Validation {
                message: format!("row-count manifest {} contains an empty table_name", path),
            });
        }
        let table_name = TableName::new(schema, table);
        if counts
            .insert(table_name.clone(), record.row_count)
            .is_some()
        {
            return Err(Error::Validation {
                message: format!(
                    "row-count manifest {} contains duplicate table {}",
                    path,
                    table_name.display_sql_server()
                ),
            });
        }
    }

    Ok(Some(RowCountManifest { path, counts }))
}
