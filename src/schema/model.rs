use serde::{Deserialize, Serialize};

use super::classifier::ClassificationSummary;

/// Backwards-compatible name for the parsed database schema model.
pub type Schema = DatabaseSchema;

/// A database-neutral representation of a parsed schema.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DatabaseSchema {
    pub tables: Vec<TableDef>,
    pub indexes: Vec<IndexDef>,
    pub diagnostics: Vec<SchemaDiagnostic>,
    #[serde(skip)]
    pub statement_summary: ClassificationSummary,
}

impl DatabaseSchema {
    /// Returns all tables in declaration order.
    pub fn tables(&self) -> &[TableDef] {
        &self.tables
    }

    /// Finds a table by its schema-qualified name.
    pub fn find_table(&self, name: &TableName) -> Option<&TableDef> {
        self.tables.iter().find(|table| &table.name == name)
    }
}

/// A schema-qualified table name.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TableName {
    pub schema: Option<String>,
    pub table: String,
}

impl TableName {
    /// Creates a table name from an optional schema component and a table component.
    pub fn new(schema: impl Into<Option<String>>, table: impl Into<String>) -> Self {
        Self {
            schema: schema.into(),
            table: table.into(),
        }
    }

    /// Displays the name using SQL Server bracket quoting.
    pub fn display_sql_server(&self) -> String {
        match &self.schema {
            Some(schema) => format!(
                "[{}].[{}]",
                escape_sql_server_identifier(schema),
                escape_sql_server_identifier(&self.table)
            ),
            None => format!("[{}]", escape_sql_server_identifier(&self.table)),
        }
    }
}

/// A database table and the constraints declared with it or attached later.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TableDef {
    pub name: TableName,
    pub columns: Vec<ColumnDef>,
    pub primary_key: Option<PrimaryKeyDef>,
    pub unique_constraints: Vec<UniqueConstraintDef>,
    pub foreign_keys: Vec<ForeignKeyDef>,
    pub check_constraints: Vec<CheckConstraintDef>,
}

impl TableDef {
    /// Returns the table primary key, if one was declared.
    pub fn primary_key(&self) -> Option<&PrimaryKeyDef> {
        self.primary_key.as_ref()
    }

    /// Returns foreign keys declared inline or added after table creation.
    pub fn foreign_keys(&self) -> &[ForeignKeyDef] {
        &self.foreign_keys
    }
}

/// A database column definition.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ColumnDef {
    pub name: String,
    pub data_type: SqlServerType,
    /// Whether this column allows NULL values. SQL Server columns are parsed as
    /// nullable by default when neither NULL nor NOT NULL is specified.
    pub nullable: bool,
    pub identity: bool,
    pub primary_key: bool,
    pub unique: bool,
    pub default: Option<DefaultConstraintDef>,
    pub check: Option<CheckConstraintDef>,
}

/// SQL Server type information without SQLite-specific lowering decisions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SqlServerType {
    Int,
    BigInt,
    SmallInt,
    TinyInt,
    Bit,
    Decimal {
        precision: Option<u8>,
        scale: Option<u8>,
    },
    Numeric {
        precision: Option<u8>,
        scale: Option<u8>,
    },
    Money,
    SmallMoney,
    Float {
        precision: Option<u8>,
    },
    Real,
    Date,
    Time {
        scale: Option<u8>,
    },
    DateTime,
    DateTime2 {
        scale: Option<u8>,
    },
    SmallDateTime,
    DateTimeOffset {
        scale: Option<u8>,
    },
    UniqueIdentifier,
    Char {
        length: Option<u32>,
    },
    VarChar {
        length: Option<u32>,
        max: bool,
    },
    NChar {
        length: Option<u32>,
    },
    NVarChar {
        length: Option<u32>,
        max: bool,
    },
    Text,
    NText,
    Image,
    Binary {
        length: Option<u32>,
    },
    VarBinary {
        length: Option<u32>,
        max: bool,
    },
    RowVersion,
    Timestamp,
    Xml,
    Other {
        name: String,
        arguments: Vec<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrimaryKeyDef {
    pub name: Option<String>,
    pub columns: Vec<String>,
    pub clustered: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UniqueConstraintDef {
    pub name: Option<String>,
    pub columns: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ForeignKeyDef {
    pub name: Option<String>,
    pub columns: Vec<String>,
    pub referenced_table: TableName,
    pub referenced_columns: Vec<String>,
    pub on_delete: Option<ReferentialAction>,
    pub on_update: Option<ReferentialAction>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReferentialAction {
    NoAction,
    Cascade,
    SetNull,
    SetDefault,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CheckConstraintDef {
    pub name: Option<String>,
    pub expression: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DefaultConstraintDef {
    pub name: Option<String>,
    pub expression: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IndexDef {
    pub name: String,
    pub table: TableName,
    pub columns: Vec<String>,
    pub unique: bool,
    pub clustered: Option<bool>,
    pub filter: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SchemaDiagnostic {
    pub severity: DiagnosticSeverity,
    pub message: String,
    pub line: Option<usize>,
    pub column: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DiagnosticSeverity {
    Warning,
    Error,
    Unsupported,
}

fn escape_sql_server_identifier(identifier: &str) -> String {
    identifier.replace(']', "]]")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn customer_table(schema: &str) -> TableDef {
        TableDef {
            name: TableName::new(Some(schema.to_owned()), "Customer"),
            columns: vec![ColumnDef {
                name: "Id".to_owned(),
                data_type: SqlServerType::Int,
                nullable: false,
                identity: true,
                primary_key: false,
                unique: false,
                default: None,
                check: None,
            }],
            primary_key: Some(PrimaryKeyDef {
                name: Some("PK_Customer".to_owned()),
                columns: vec!["Id".to_owned()],
                clustered: Some(true),
            }),
            unique_constraints: Vec::new(),
            foreign_keys: Vec::new(),
            check_constraints: Vec::new(),
        }
    }

    #[test]
    fn creates_table_in_dbo_schema() {
        let schema = DatabaseSchema {
            tables: vec![customer_table("dbo")],
            indexes: Vec::new(),
            diagnostics: Vec::new(),
            statement_summary: Default::default(),
        };

        let name = TableName::new(Some("dbo".to_owned()), "Customer");
        assert_eq!(name.display_sql_server(), "[dbo].[Customer]");
        assert_eq!(schema.find_table(&name), Some(&schema.tables()[0]));
    }

    #[test]
    fn supports_same_table_name_in_different_schemas() {
        let schema = DatabaseSchema {
            tables: vec![customer_table("sales"), customer_table("archive")],
            indexes: Vec::new(),
            diagnostics: Vec::new(),
            statement_summary: Default::default(),
        };

        assert!(schema
            .find_table(&TableName::new(Some("sales".to_owned()), "Customer"))
            .is_some());
        assert!(schema
            .find_table(&TableName::new(Some("archive".to_owned()), "Customer"))
            .is_some());
        assert_eq!(schema.tables().len(), 2);
    }

    #[test]
    fn represents_composite_primary_key() {
        let pk = PrimaryKeyDef {
            name: Some("PK_OrderLine".to_owned()),
            columns: vec!["OrderId".to_owned(), "LineNumber".to_owned()],
            clustered: Some(true),
        };
        let mut table = customer_table("dbo");
        table.primary_key = Some(pk.clone());

        assert_eq!(table.primary_key(), Some(&pk));
    }

    #[test]
    fn represents_foreign_key_added_after_create_table() {
        let fk = ForeignKeyDef {
            name: Some("FK_Order_Customer".to_owned()),
            columns: vec!["CustomerId".to_owned()],
            referenced_table: TableName::new(Some("dbo".to_owned()), "Customer"),
            referenced_columns: vec!["Id".to_owned()],
            on_delete: Some(ReferentialAction::Cascade),
            on_update: None,
        };
        let mut table = customer_table("sales");
        table.foreign_keys.push(fk.clone());

        assert_eq!(table.foreign_keys(), &[fk]);
    }

    #[test]
    fn collects_warning_diagnostics() {
        let schema = DatabaseSchema {
            tables: Vec::new(),
            indexes: Vec::new(),
            diagnostics: vec![SchemaDiagnostic {
                severity: DiagnosticSeverity::Warning,
                message: "unsupported filegroup ignored".to_owned(),
                line: None,
                column: None,
            }],
            statement_summary: Default::default(),
        };

        assert_eq!(schema.diagnostics[0].severity, DiagnosticSeverity::Warning);
    }

    #[test]
    fn serializes_schema_model_to_json() {
        let schema = DatabaseSchema {
            tables: vec![customer_table("dbo")],
            indexes: vec![IndexDef {
                name: "IX_Customer_Id".to_owned(),
                table: TableName::new(Some("dbo".to_owned()), "Customer"),
                columns: vec!["Id".to_owned()],
                unique: true,
                clustered: Some(false),
                filter: None,
            }],
            diagnostics: Vec::new(),
            statement_summary: Default::default(),
        };

        let json = serde_json::to_string(&schema).expect("schema should serialize");
        assert!(json.contains("Customer"));
        assert!(json.contains("IX_Customer_Id"));
    }
}
