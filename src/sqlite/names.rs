use std::collections::HashMap;

use crate::config::options::TableNameMode;
use crate::error::{Error, Result};
use crate::schema::model::{DatabaseSchema, TableName};

/// A SQLite object name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Name(pub String);

/// Generates the SQLite table name for a SQL Server table name and naming mode.
pub fn table_name(table_name: &TableName, mode: TableNameMode) -> Name {
    let name = match mode {
        TableNameMode::SchemaPrefix => prefixed_table_name(table_name),
        TableNameMode::DropDbo => match &table_name.schema {
            Some(schema) if schema.eq_ignore_ascii_case("dbo") => table_name.table.clone(),
            Some(_) => prefixed_table_name(table_name),
            None => table_name.table.clone(),
        },
        TableNameMode::TableOnly => table_name.table.clone(),
    };

    Name(name)
}

/// Generates table names for every table in a schema, failing if a mode collides.
pub fn table_names_for_schema(
    schema: &DatabaseSchema,
    mode: TableNameMode,
) -> Result<HashMap<TableName, Name>> {
    let mut generated_to_source: HashMap<String, TableName> = HashMap::new();
    let mut generated = HashMap::new();

    for table in schema.tables() {
        let sqlite_name = table_name(&table.name, mode);
        if let Some(existing) = generated_to_source.get(&sqlite_name.0) {
            return Err(Error::Validation {
                message: format!(
                    "SQLite table name collision under table-name-mode '{}': {} and {} both generate '{}'",
                    mode,
                    existing.display_sql_server(),
                    table.name.display_sql_server(),
                    sqlite_name.0
                ),
            });
        }

        generated_to_source.insert(sqlite_name.0.clone(), table.name.clone());
        generated.insert(table.name.clone(), sqlite_name);
    }

    Ok(generated)
}

fn prefixed_table_name(table_name: &TableName) -> String {
    match &table_name.schema {
        Some(schema) => format!("{}_{}", schema, table_name.table),
        None => table_name.table.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::model::TableDef;

    fn table(schema: &str, name: &str) -> TableDef {
        TableDef {
            name: TableName::new(Some(schema.to_owned()), name),
            columns: Vec::new(),
            primary_key: None,
            unique_constraints: Vec::new(),
            foreign_keys: Vec::new(),
            check_constraints: Vec::new(),
        }
    }

    #[test]
    fn schema_prefix_mode_prefixes_schema() {
        let name = TableName::new(Some("dbo".to_owned()), "Customer");
        assert_eq!(
            table_name(&name, TableNameMode::SchemaPrefix),
            Name("dbo_Customer".to_owned())
        );
    }

    #[test]
    fn drop_dbo_mode_drops_only_dbo_schema() {
        let dbo = TableName::new(Some("dbo".to_owned()), "Customer");
        let sales = TableName::new(Some("sales".to_owned()), "Invoice");

        assert_eq!(
            table_name(&dbo, TableNameMode::DropDbo),
            Name("Customer".to_owned())
        );
        assert_eq!(
            table_name(&sales, TableNameMode::DropDbo),
            Name("sales_Invoice".to_owned())
        );
    }

    #[test]
    fn table_only_mode_drops_all_schemas() {
        let name = TableName::new(Some("dbo".to_owned()), "Customer");
        assert_eq!(
            table_name(&name, TableNameMode::TableOnly),
            Name("Customer".to_owned())
        );
    }

    #[test]
    fn detects_collision_under_table_only() {
        let schema = DatabaseSchema {
            tables: vec![table("dbo", "Customer"), table("sales", "Customer")],
            indexes: Vec::new(),
            diagnostics: Vec::new(),
        };

        let error = table_names_for_schema(&schema, TableNameMode::TableOnly).unwrap_err();
        assert!(error.to_string().contains("collision"));
        assert!(error.to_string().contains("[dbo].[Customer]"));
        assert!(error.to_string().contains("[sales].[Customer]"));
    }

    #[test]
    fn schema_prefix_mode_avoids_schema_collisions() {
        let schema = DatabaseSchema {
            tables: vec![table("dbo", "Customer"), table("sales", "Customer")],
            indexes: Vec::new(),
            diagnostics: Vec::new(),
        };

        let names = table_names_for_schema(&schema, TableNameMode::SchemaPrefix).unwrap();
        assert_eq!(names.len(), 2);
        assert!(names
            .values()
            .any(|name| name == &Name("dbo_Customer".to_owned())));
        assert!(names
            .values()
            .any(|name| name == &Name("sales_Customer".to_owned())));
    }
}
