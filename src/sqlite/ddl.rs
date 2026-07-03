use std::collections::HashMap;

use crate::config::options::ConvertOptions;
use crate::error::Result;
use crate::schema::model::{
    CheckConstraintDef, DatabaseSchema, DiagnosticSeverity, ForeignKeyDef, IndexDef,
    ReferentialAction, SchemaDiagnostic, SqlServerType, TableDef, TableName, UniqueConstraintDef,
};
use crate::sqlite::names::{table_names_for_schema, Name};
use crate::sqlite::types::sqlite_affinity;

/// A SQLite DDL statement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Statement(pub String);

/// Generated DDL plus diagnostics for constructs that could not be represented safely.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GeneratedDdl {
    pub statements: Vec<Statement>,
    pub diagnostics: Vec<SchemaDiagnostic>,
}

/// Generates deterministic SQLite DDL from the parsed SQL Server schema.
pub fn generate(schema: &DatabaseSchema, options: &ConvertOptions) -> Result<GeneratedDdl> {
    let table_names = table_names_for_schema(schema, options.table_name_mode)?;
    let mut generated = GeneratedDdl::default();

    for table in schema.tables() {
        generated.statements.push(create_table_statement(
            table,
            &table_names,
            &mut generated.diagnostics,
        ));
    }

    for index in &schema.indexes {
        if let Some(statement) = index_statement(index, &table_names, &mut generated.diagnostics) {
            generated.statements.push(statement);
        }
    }

    Ok(generated)
}

/// Returns the full converted SQLite schema SQL as a deterministic string.
pub fn schema_sql(schema: &DatabaseSchema, options: &ConvertOptions) -> Result<String> {
    Ok(generate(schema, options)?.to_sql())
}

impl GeneratedDdl {
    /// Renders all generated statements into executable SQLite SQL.
    pub fn to_sql(&self) -> String {
        if self.statements.is_empty() {
            return String::new();
        }

        let mut sql = self
            .statements
            .iter()
            .map(|statement| statement.0.as_str())
            .collect::<Vec<_>>()
            .join("\n\n");
        sql.push('\n');
        sql
    }
}

/// Quotes a SQLite identifier using double quotes.
pub fn quote_identifier(identifier: &str) -> String {
    format!("\"{}\"", identifier.replace('"', "\"\""))
}

fn create_table_statement(
    table: &TableDef,
    table_names: &HashMap<TableName, Name>,
    diagnostics: &mut Vec<SchemaDiagnostic>,
) -> Statement {
    let table_name = quote_identifier(&table_names[&table.name].0);
    let mut definitions = Vec::new();

    for column in &table.columns {
        let mut definition = format!(
            "{} {}",
            quote_identifier(&column.name),
            sqlite_affinity(&column.data_type).ddl_name()
        );
        if !column.nullable {
            definition.push_str(" NOT NULL");
        }
        if column.identity {
            diagnostics.push(warning(format!(
                "IDENTITY property on {}.{} was not emitted because SQLite rowid/autoincrement semantics differ from SQL Server",
                table.name.display_sql_server(),
                column.name
            )));
        }
        if column.default.is_some() {
            diagnostics.push(warning(format!(
                "default on {}.{} was not emitted because SQL Server defaults are not yet safely portable to SQLite DDL",
                table.name.display_sql_server(),
                column.name
            )));
        }
        if let SqlServerType::Other { name, .. } = &column.data_type {
            diagnostics.push(unsupported(format!(
                "unrecognized SQL Server type '{}' on {}.{} was emitted with SQLite TEXT affinity",
                name,
                table.name.display_sql_server(),
                column.name
            )));
        }
        definitions.push(definition);

        if matches!(column.data_type, SqlServerType::Bit) {
            definitions.push(format!(
                "CHECK ({} IN (0, 1))",
                quote_identifier(&column.name)
            ));
        }
        if let Some(check) = &column.check {
            push_check(check, &mut definitions, diagnostics);
        }
    }

    if let Some(primary_key) = &table.primary_key {
        if !primary_key.columns.is_empty() {
            warn_clustered(
                primary_key.clustered,
                diagnostics,
                "primary key",
                &table.name,
            );
            definitions.push(format!(
                "PRIMARY KEY ({})",
                quote_column_list(&primary_key.columns)
            ));
        } else {
            diagnostics.push(unsupported(format!(
                "primary key on {} has no columns and was not emitted",
                table.name.display_sql_server()
            )));
        }
    }

    for unique in &table.unique_constraints {
        push_unique(unique, table, &mut definitions, diagnostics);
    }
    for foreign_key in &table.foreign_keys {
        push_foreign_key(
            foreign_key,
            table,
            table_names,
            &mut definitions,
            diagnostics,
        );
    }
    for check in &table.check_constraints {
        push_check(check, &mut definitions, diagnostics);
    }

    Statement(format!(
        "CREATE TABLE {} (\n    {}\n);",
        table_name,
        definitions.join(",\n    ")
    ))
}

fn push_unique(
    unique: &UniqueConstraintDef,
    table: &TableDef,
    definitions: &mut Vec<String>,
    diagnostics: &mut Vec<SchemaDiagnostic>,
) {
    if !unique.columns.is_empty() {
        definitions.push(format!("UNIQUE ({})", quote_column_list(&unique.columns)));
    } else {
        diagnostics.push(unsupported(format!(
            "unique constraint on {} has no columns and was not emitted",
            table.name.display_sql_server()
        )));
    }
}

fn push_foreign_key(
    foreign_key: &ForeignKeyDef,
    table: &TableDef,
    table_names: &HashMap<TableName, Name>,
    definitions: &mut Vec<String>,
    diagnostics: &mut Vec<SchemaDiagnostic>,
) {
    if foreign_key.columns.is_empty() || foreign_key.referenced_columns.is_empty() {
        diagnostics.push(unsupported(format!(
            "foreign key on {} has no local or referenced columns and was not emitted",
            table.name.display_sql_server()
        )));
        return;
    }
    let Some(referenced_table) = table_names.get(&foreign_key.referenced_table) else {
        diagnostics.push(unsupported(format!(
            "foreign key on {} references missing table {} and was not emitted",
            table.name.display_sql_server(),
            foreign_key.referenced_table.display_sql_server()
        )));
        return;
    };

    let mut clause = format!(
        "FOREIGN KEY ({}) REFERENCES {} ({})",
        quote_column_list(&foreign_key.columns),
        quote_identifier(&referenced_table.0),
        quote_column_list(&foreign_key.referenced_columns)
    );
    if let Some(action) = &foreign_key.on_delete {
        clause.push_str(" ON DELETE ");
        clause.push_str(action_sql(action));
    }
    if let Some(action) = &foreign_key.on_update {
        clause.push_str(" ON UPDATE ");
        clause.push_str(action_sql(action));
    }
    definitions.push(clause);
}

fn push_check(
    check: &CheckConstraintDef,
    definitions: &mut Vec<String>,
    _diagnostics: &mut Vec<SchemaDiagnostic>,
) {
    if !check.expression.trim().is_empty() {
        definitions.push(format!("CHECK ({})", check.expression.trim()));
    }
}

fn index_statement(
    index: &IndexDef,
    table_names: &HashMap<TableName, Name>,
    diagnostics: &mut Vec<SchemaDiagnostic>,
) -> Option<Statement> {
    if index.columns.is_empty() {
        diagnostics.push(unsupported(format!(
            "index {} has no columns and was not emitted",
            index.name
        )));
        return None;
    }
    if index.filter.is_some() {
        diagnostics.push(unsupported(format!(
            "filtered index {} was not emitted because SQL Server filter expressions are not yet safely portable",
            index.name
        )));
        return None;
    }
    warn_clustered(index.clustered, diagnostics, "index", &index.table);
    let table_name = table_names.get(&index.table)?;
    Some(Statement(format!(
        "CREATE {}INDEX {} ON {} ({});",
        if index.unique { "UNIQUE " } else { "" },
        quote_identifier(&index.name),
        quote_identifier(&table_name.0),
        quote_column_list(&index.columns)
    )))
}

fn quote_column_list(columns: &[String]) -> String {
    columns
        .iter()
        .map(|column| quote_identifier(column))
        .collect::<Vec<_>>()
        .join(", ")
}

fn action_sql(action: &ReferentialAction) -> &'static str {
    match action {
        ReferentialAction::NoAction => "NO ACTION",
        ReferentialAction::Cascade => "CASCADE",
        ReferentialAction::SetNull => "SET NULL",
        ReferentialAction::SetDefault => "SET DEFAULT",
    }
}

fn warn_clustered(
    clustered: Option<bool>,
    diagnostics: &mut Vec<SchemaDiagnostic>,
    kind: &str,
    table: &TableName,
) {
    if clustered.is_some() {
        diagnostics.push(warning(format!(
            "SQL Server clustered/nonclustered setting on {} for {} was ignored because SQLite does not support it",
            kind,
            table.display_sql_server()
        )));
    }
}

fn warning(message: String) -> SchemaDiagnostic {
    SchemaDiagnostic {
        severity: DiagnosticSeverity::Warning,
        message,
    }
}

fn unsupported(message: String) -> SchemaDiagnostic {
    SchemaDiagnostic {
        severity: DiagnosticSeverity::Unsupported,
        message,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::options::TableNameMode;
    use crate::schema::model::{ColumnDef, PrimaryKeyDef};

    fn col(name: &str, data_type: SqlServerType, nullable: bool) -> ColumnDef {
        ColumnDef {
            name: name.to_owned(),
            data_type,
            nullable,
            identity: false,
            default: None,
            check: None,
        }
    }

    fn table(schema: &str, name: &str, columns: Vec<ColumnDef>) -> TableDef {
        TableDef {
            name: TableName::new(Some(schema.to_owned()), name),
            columns,
            primary_key: None,
            unique_constraints: vec![],
            foreign_keys: vec![],
            check_constraints: vec![],
        }
    }

    fn ddl(schema: DatabaseSchema) -> GeneratedDdl {
        generate(&schema, &ConvertOptions::default()).unwrap()
    }

    #[test]
    fn simple_table_ddl() {
        let schema = DatabaseSchema {
            tables: vec![table(
                "dbo",
                "Customer",
                vec![
                    col("Id", SqlServerType::Int, false),
                    col(
                        "Name",
                        SqlServerType::NVarChar {
                            length: Some(50),
                            max: false,
                        },
                        true,
                    ),
                ],
            )],
            indexes: vec![],
            diagnostics: vec![],
        };
        assert_eq!(
            ddl(schema).statements[0].0,
            "CREATE TABLE \"dbo_Customer\" (\n    \"Id\" INTEGER NOT NULL,\n    \"Name\" TEXT\n);"
        );
    }

    #[test]
    fn composite_primary_key_ddl() {
        let mut t = table(
            "dbo",
            "Line",
            vec![
                col("OrderId", SqlServerType::Int, false),
                col("LineNo", SqlServerType::Int, false),
            ],
        );
        t.primary_key = Some(PrimaryKeyDef {
            name: None,
            columns: vec!["OrderId".into(), "LineNo".into()],
            clustered: None,
        });
        assert!(ddl(DatabaseSchema {
            tables: vec![t],
            indexes: vec![],
            diagnostics: vec![]
        })
        .statements[0]
            .0
            .contains("PRIMARY KEY (\"OrderId\", \"LineNo\")"));
    }

    #[test]
    fn foreign_key_ddl() {
        let parent = table("dbo", "Parent", vec![col("Id", SqlServerType::Int, false)]);
        let mut child = table(
            "dbo",
            "Child",
            vec![col("ParentId", SqlServerType::Int, false)],
        );
        child.foreign_keys.push(ForeignKeyDef {
            name: None,
            columns: vec!["ParentId".into()],
            referenced_table: parent.name.clone(),
            referenced_columns: vec!["Id".into()],
            on_delete: Some(ReferentialAction::Cascade),
            on_update: None,
        });
        assert!(ddl(DatabaseSchema {
            tables: vec![parent, child],
            indexes: vec![],
            diagnostics: vec![]
        })
        .statements[1]
            .0
            .contains(
                "FOREIGN KEY (\"ParentId\") REFERENCES \"dbo_Parent\" (\"Id\") ON DELETE CASCADE"
            ));
    }

    #[test]
    fn unique_constraint_ddl() {
        let mut t = table(
            "dbo",
            "User",
            vec![col(
                "Email",
                SqlServerType::VarChar {
                    length: Some(255),
                    max: false,
                },
                false,
            )],
        );
        t.unique_constraints.push(UniqueConstraintDef {
            name: None,
            columns: vec!["Email".into()],
        });
        assert!(ddl(DatabaseSchema {
            tables: vec![t],
            indexes: vec![],
            diagnostics: vec![]
        })
        .statements[0]
            .0
            .contains("UNIQUE (\"Email\")"));
    }

    #[test]
    fn index_ddl() {
        let t = table(
            "dbo",
            "Customer",
            vec![col(
                "Name",
                SqlServerType::NVarChar {
                    length: Some(50),
                    max: false,
                },
                true,
            )],
        );
        let index = IndexDef {
            name: "IX Customer Name".into(),
            table: t.name.clone(),
            columns: vec!["Name".into()],
            unique: false,
            clustered: None,
            filter: None,
        };
        assert_eq!(
            ddl(DatabaseSchema {
                tables: vec![t],
                indexes: vec![index],
                diagnostics: vec![]
            })
            .statements[1]
                .0,
            "CREATE INDEX \"IX Customer Name\" ON \"dbo_Customer\" (\"Name\");"
        );
    }

    #[test]
    fn identifier_quoting() {
        assert_eq!(quote_identifier("weird\"name"), "\"weird\"\"name\"");
        let schema = DatabaseSchema {
            tables: vec![table(
                "dbo",
                "select",
                vec![col("from", SqlServerType::Int, false)],
            )],
            indexes: vec![],
            diagnostics: vec![],
        };
        assert!(ddl(schema).statements[0]
            .0
            .contains("\"from\" INTEGER NOT NULL"));
    }

    #[test]
    fn table_naming_modes() {
        let schema = DatabaseSchema {
            tables: vec![table(
                "dbo",
                "Customer",
                vec![col("Id", SqlServerType::Int, false)],
            )],
            indexes: vec![],
            diagnostics: vec![],
        };
        let mut options = ConvertOptions::default();
        options.table_name_mode = TableNameMode::DropDbo;
        assert!(generate(&schema, &options).unwrap().statements[0]
            .0
            .starts_with("CREATE TABLE \"Customer\""));
        options.table_name_mode = TableNameMode::TableOnly;
        assert!(generate(&schema, &options).unwrap().statements[0]
            .0
            .starts_with("CREATE TABLE \"Customer\""));
    }

    #[test]
    fn unsupported_default_warning_behavior() {
        let mut c = col("Created", SqlServerType::DateTime, false);
        c.default = Some(crate::schema::model::DefaultConstraintDef {
            name: None,
            expression: "GETDATE()".into(),
        });
        let generated = ddl(DatabaseSchema {
            tables: vec![table("dbo", "Audit", vec![c])],
            indexes: vec![],
            diagnostics: vec![],
        });
        assert!(!generated.statements[0].0.contains("DEFAULT"));
        assert!(generated
            .diagnostics
            .iter()
            .any(|d| d.severity == DiagnosticSeverity::Warning && d.message.contains("default")));
    }

    #[test]
    fn schema_sql_is_deterministic_across_runs() {
        let schema = DatabaseSchema {
            tables: vec![table(
                "dbo",
                "Customer",
                vec![
                    col("Id", SqlServerType::Int, false),
                    col(
                        "Name",
                        SqlServerType::NVarChar {
                            length: Some(50),
                            max: false,
                        },
                        true,
                    ),
                ],
            )],
            indexes: vec![],
            diagnostics: vec![],
        };
        let first = schema_sql(&schema, &ConvertOptions::default()).unwrap();
        let second = schema_sql(&schema, &ConvertOptions::default()).unwrap();
        assert_eq!(first, second);
        assert!(first.ends_with('\n'));
    }
}
