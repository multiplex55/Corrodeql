use std::collections::{HashMap, HashSet};

use crate::config::options::ConvertOptions;
use crate::error::Result;
use crate::mssql::defaults::normalize_default;
use crate::schema::model::{
    CheckConstraintDef, DatabaseSchema, DiagnosticSeverity, ForeignKeyDef, IndexDef,
    ReferentialAction, SchemaDiagnostic, SqlServerType, TableDef, TableName, UniqueConstraintDef,
};
use crate::sqlite::names::{quote_identifier, table_names_for_schema, Name};
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
    let mut generated = generate_tables(schema, options)?;
    let mut indexes = generate_indexes(schema, options)?;
    generated.statements.append(&mut indexes.statements);
    generated.diagnostics.append(&mut indexes.diagnostics);
    Ok(generated)
}

/// Generates only table DDL. Indexes are intentionally omitted so imports can
/// load data before creating secondary indexes.
pub fn generate_tables(schema: &DatabaseSchema, options: &ConvertOptions) -> Result<GeneratedDdl> {
    let table_names = table_names_for_schema(schema, options.table_name_mode)?;
    let mut generated = GeneratedDdl::default();

    for table in schema.tables() {
        generated.statements.push(create_table_statement(
            table,
            &table_names,
            &mut generated.diagnostics,
        ));
    }

    Ok(generated)
}

/// Generates only index DDL. Call this after data import for faster loads.
pub fn generate_indexes(schema: &DatabaseSchema, options: &ConvertOptions) -> Result<GeneratedDdl> {
    let table_names = table_names_for_schema(schema, options.table_name_mode)?;
    let mut generated = GeneratedDdl::default();

    let mut used_index_names = HashSet::new();
    let mut duplicate_counts: HashMap<String, usize> = HashMap::new();
    for index in &schema.indexes {
        if let Some(statement) = index_statement(
            index,
            &table_names,
            &mut used_index_names,
            &mut duplicate_counts,
            &mut generated.diagnostics,
        ) {
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

fn create_table_statement(
    table: &TableDef,
    table_names: &HashMap<TableName, Name>,
    diagnostics: &mut Vec<SchemaDiagnostic>,
) -> Statement {
    let table_name = quote_identifier(&table_names[&table.name].0);
    let mut definitions = Vec::new();
    let mut column_constraints = Vec::new();

    let inline_identity_pk = inline_identity_pk_column(table);

    for column in &table.columns {
        let is_inline_identity_pk = inline_identity_pk == Some(column.name.as_str());
        let mut definition = if is_inline_identity_pk {
            format!("{} INTEGER PRIMARY KEY", quote_identifier(&column.name))
        } else {
            format!(
                "{} {}",
                quote_identifier(&column.name),
                sqlite_affinity(&column.data_type).ddl_name()
            )
        };
        if !column.nullable && !is_inline_identity_pk {
            definition.push_str(" NOT NULL");
        }
        if column.unique {
            definition.push_str(" UNIQUE");
        }
        if column.identity && !is_inline_identity_pk {
            diagnostics.push(warning(format!(
                "IDENTITY property on {}.{} was not emitted because SQLite rowid/autoincrement semantics differ from SQL Server",
                table.name.display_sql_server(),
                column.name
            )));
        }
        if let Some(default) = &column.default {
            push_default(
                &mut definition,
                table,
                column,
                &default.expression,
                diagnostics,
            );
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
            column_constraints.push(format!(
                "CHECK ({} IN (0, 1))",
                quote_identifier(&column.name)
            ));
        }
        if let Some(check) = &column.check {
            push_check(check, &mut column_constraints, diagnostics);
        }
    }
    definitions.extend(column_constraints);

    if let Some(primary_key) = &table.primary_key {
        if inline_identity_pk.is_some() {
            warn_clustered(
                primary_key.clustered,
                diagnostics,
                "primary key",
                &table.name,
            );
        } else if !primary_key.columns.is_empty() {
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
    used_index_names: &mut HashSet<String>,
    duplicate_counts: &mut HashMap<String, usize>,
    diagnostics: &mut Vec<SchemaDiagnostic>,
) -> Option<Statement> {
    if index.columns.is_empty() {
        diagnostics.push(unsupported(format!(
            "index {} has no columns and was not emitted",
            index.name
        )));
        return None;
    }
    warn_clustered(index.clustered, diagnostics, "index", &index.table);
    if index
        .filter
        .as_deref()
        .map(str::trim)
        .is_some_and(|f| !f.is_empty())
    {
        diagnostics.push(warning(format!(
            "filter on index {} for {} was not emitted because SQL Server filter expressions may not be portable to SQLite",
            index.name,
            index.table.display_sql_server()
        )));
    }
    let table_name = table_names.get(&index.table)?;
    let emitted_name =
        unique_index_name(&index.name, used_index_names, duplicate_counts, diagnostics);
    let sql = format!(
        "CREATE {}INDEX {} ON {} ({});",
        if index.unique { "UNIQUE " } else { "" },
        quote_identifier(&emitted_name),
        quote_identifier(&table_name.0),
        quote_column_list(&index.columns)
    );
    Some(Statement(sql))
}

fn inline_identity_pk_column(table: &TableDef) -> Option<&str> {
    let primary_key = table.primary_key.as_ref()?;
    if primary_key.columns.len() != 1 {
        return None;
    }
    let pk_column = primary_key.columns[0].as_str();
    let column = table
        .columns
        .iter()
        .find(|column| column.name == pk_column)?;
    if column.identity && matches!(column.data_type, SqlServerType::Int) {
        Some(pk_column)
    } else {
        None
    }
}

fn push_default(
    definition: &mut String,
    table: &TableDef,
    column: &crate::schema::model::ColumnDef,
    expression: &str,
    diagnostics: &mut Vec<SchemaDiagnostic>,
) {
    let normalized = normalize_default(expression);
    if normalized.expression.is_empty() {
        diagnostics.push(warning(format!(
            "default on {}.{} was not emitted because expression is not safely portable to SQLite: {}",
            table.name.display_sql_server(),
            column.name,
            expression
        )));
    } else {
        definition.push_str(" DEFAULT ");
        definition.push_str(&normalized.expression);
    }
}

fn unique_index_name(
    base: &str,
    used: &mut HashSet<String>,
    counts: &mut HashMap<String, usize>,
    diagnostics: &mut Vec<SchemaDiagnostic>,
) -> String {
    if used.insert(base.to_owned()) {
        counts.entry(base.to_owned()).or_insert(1);
        return base.to_owned();
    }
    let mut next = *counts.get(base).unwrap_or(&1) + 1;
    loop {
        let candidate = format!("{}_{}", base, next);
        if used.insert(candidate.clone()) {
            counts.insert(base.to_owned(), next);
            diagnostics.push(warning(format!(
                "duplicate SQLite index name {} was emitted as {}",
                base, candidate
            )));
            return candidate;
        }
        next += 1;
    }
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
        line: None,
        column: None,
    }
}

fn unsupported(message: String) -> SchemaDiagnostic {
    SchemaDiagnostic {
        severity: DiagnosticSeverity::Unsupported,
        message,
        line: None,
        column: None,
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
            primary_key: false,
            unique: false,
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
            statement_summary: Default::default(),
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
            diagnostics: vec![],
            statement_summary: Default::default(),
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
            diagnostics: vec![],
            statement_summary: Default::default(),
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
            diagnostics: vec![],
            statement_summary: Default::default(),
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
                diagnostics: vec![],
                statement_summary: Default::default(),
            })
            .statements[1]
                .0,
            "CREATE INDEX \"IX Customer Name\" ON \"dbo_Customer\" (\"Name\");"
        );
    }

    #[test]
    fn unique_index_ddl() {
        let t = table(
            "dbo",
            "Customer",
            vec![col("Email", SqlServerType::Text, true)],
        );
        let index = IndexDef {
            name: "UX_Customer_Email".into(),
            table: t.name.clone(),
            columns: vec!["Email".into()],
            unique: true,
            clustered: None,
            filter: None,
        };
        assert_eq!(
            ddl(DatabaseSchema {
                tables: vec![t],
                indexes: vec![index],
                diagnostics: vec![],
                statement_summary: Default::default(),
            })
            .statements[1]
                .0,
            "CREATE UNIQUE INDEX \"UX_Customer_Email\" ON \"dbo_Customer\" (\"Email\");"
        );
    }

    #[test]
    fn filtered_index_ddl_warns_and_skips_filter() {
        let t = table(
            "dbo",
            "Order",
            vec![col("Status", SqlServerType::Text, true)],
        );
        let index = IndexDef {
            name: "IX_Order_Open".into(),
            table: t.name.clone(),
            columns: vec!["Status".into()],
            unique: false,
            clustered: None,
            filter: Some("Status = 'Open'".into()),
        };
        let generated = ddl(DatabaseSchema {
            tables: vec![t],
            indexes: vec![index],
            diagnostics: vec![],
            statement_summary: Default::default(),
        });
        assert_eq!(
            generated.statements[1].0,
            "CREATE INDEX \"IX_Order_Open\" ON \"dbo_Order\" (\"Status\");"
        );
        assert!(generated
            .diagnostics
            .iter()
            .any(|d| d.message.contains("filter")));
    }

    #[test]
    fn generated_index_sql_quotes_identifiers() {
        let t = table(
            "sales",
            "Order Detail",
            vec![col("Customer \"Id\"", SqlServerType::Int, true)],
        );
        let index = IndexDef {
            name: "IX \"odd\"".into(),
            table: t.name.clone(),
            columns: vec!["Customer \"Id\"".into()],
            unique: false,
            clustered: None,
            filter: None,
        };
        assert_eq!(
            ddl(DatabaseSchema {
                tables: vec![t],
                indexes: vec![index],
                diagnostics: vec![],
                statement_summary: Default::default(),
            })
            .statements[1]
                .0,
            "CREATE INDEX \"IX \"\"odd\"\"\" ON \"sales_Order Detail\" (\"Customer \"\"Id\"\"\");"
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
            statement_summary: Default::default(),
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
            statement_summary: Default::default(),
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
    fn safe_getdate_default_behavior() {
        let mut c = col("Created", SqlServerType::DateTime, false);
        c.default = Some(crate::schema::model::DefaultConstraintDef {
            name: None,
            expression: "GETDATE()".into(),
        });
        let generated = ddl(DatabaseSchema {
            tables: vec![table("dbo", "Audit", vec![c])],
            indexes: vec![],
            diagnostics: vec![],
            statement_summary: Default::default(),
        });
        assert!(generated.statements[0]
            .0
            .contains("DEFAULT CURRENT_TIMESTAMP"));
        assert!(generated.diagnostics.is_empty());
    }

    #[test]
    fn example_style_table_inline_identity_pk_and_bit_check() {
        let mut id = col("CustomerId", SqlServerType::Int, false);
        id.identity = true;
        let mut t = table(
            "dbo",
            "Customer",
            vec![
                id,
                col(
                    "Name",
                    SqlServerType::NVarChar {
                        length: None,
                        max: false,
                    },
                    false,
                ),
                col(
                    "CreditLimit",
                    SqlServerType::Decimal {
                        precision: None,
                        scale: None,
                    },
                    true,
                ),
                col("IsActive", SqlServerType::Bit, false),
            ],
        );
        t.primary_key = Some(PrimaryKeyDef {
            name: None,
            columns: vec!["CustomerId".into()],
            clustered: None,
        });
        assert_eq!(ddl(DatabaseSchema { tables: vec![t], indexes: vec![], diagnostics: vec![], statement_summary: Default::default() }).statements[0].0,
            "CREATE TABLE \"dbo_Customer\" (\n    \"CustomerId\" INTEGER PRIMARY KEY,\n    \"Name\" TEXT NOT NULL,\n    \"CreditLimit\" TEXT,\n    \"IsActive\" INTEGER NOT NULL,\n    CHECK (\"IsActive\" IN (0, 1))\n);".replace("\\\"", "\""));
    }

    #[test]
    fn foreign_key_actions_and_missing_table_diagnostic() {
        let parent = table("dbo", "Parent", vec![col("Id", SqlServerType::Int, false)]);
        let mut child = table(
            "dbo",
            "Child",
            vec![col("ParentId", SqlServerType::Int, true)],
        );
        child.foreign_keys.push(ForeignKeyDef {
            name: None,
            columns: vec!["ParentId".into()],
            referenced_table: parent.name.clone(),
            referenced_columns: vec!["Id".into()],
            on_delete: Some(ReferentialAction::Cascade),
            on_update: Some(ReferentialAction::SetNull),
        });
        child.foreign_keys.push(ForeignKeyDef {
            name: None,
            columns: vec!["MissingId".into()],
            referenced_table: TableName::new(Some("dbo".into()), "Missing"),
            referenced_columns: vec!["Id".into()],
            on_delete: None,
            on_update: None,
        });
        let generated = ddl(DatabaseSchema {
            tables: vec![parent, child],
            indexes: vec![],
            diagnostics: vec![],
            statement_summary: Default::default(),
        });
        assert!(generated.statements[1].0.contains("FOREIGN KEY (\"ParentId\") REFERENCES \"dbo_Parent\" (\"Id\") ON DELETE CASCADE ON UPDATE SET NULL"));
        assert!(generated
            .diagnostics
            .iter()
            .any(|d| d.message.contains("references missing table")));
    }

    #[test]
    fn safe_defaults_and_unsupported_defaults() {
        let expressions = [
            "((0))",
            "((1))",
            "('abc')",
            "(N'a''bc')",
            "(getdate())",
            "(sysutcdatetime())",
        ];
        let mut columns = Vec::new();
        for (i, expression) in expressions.iter().enumerate() {
            let mut c = col(&format!("C{i}"), SqlServerType::Text, true);
            c.default = Some(crate::schema::model::DefaultConstraintDef {
                name: None,
                expression: (*expression).into(),
            });
            columns.push(c);
        }
        let generated = ddl(DatabaseSchema {
            tables: vec![table("dbo", "Defaults", columns)],
            indexes: vec![],
            diagnostics: vec![],
            statement_summary: Default::default(),
        });
        let sql = &generated.statements[0].0;
        assert!(sql.contains("\"C0\" TEXT DEFAULT 0"));
        assert!(sql.contains("\"C1\" TEXT DEFAULT 1"));
        assert!(sql.contains("\"C2\" TEXT DEFAULT 'abc'"));
        assert!(sql.contains("\"C3\" TEXT DEFAULT 'a''bc'"));
        assert_eq!(sql.matches("DEFAULT CURRENT_TIMESTAMP").count(), 2);
        assert!(generated.diagnostics.is_empty());

        let unsupported = [
            "(newid())",
            "(newsequentialid())",
            "(suser_sname())",
            "(host_name())",
            "(dateadd(day, 1, getdate()))",
        ];
        let mut columns = Vec::new();
        for (i, expression) in unsupported.iter().enumerate() {
            let mut c = col(&format!("U{i}"), SqlServerType::Text, true);
            c.default = Some(crate::schema::model::DefaultConstraintDef {
                name: None,
                expression: (*expression).into(),
            });
            columns.push(c);
        }
        let generated = ddl(DatabaseSchema {
            tables: vec![table("dbo", "UnsupportedDefaults", columns)],
            indexes: vec![],
            diagnostics: vec![],
            statement_summary: Default::default(),
        });
        assert!(!generated.statements[0].0.contains("DEFAULT"));
        assert_eq!(generated.diagnostics.len(), unsupported.len());
        assert!(generated
            .diagnostics
            .iter()
            .all(|d| d.message.contains("[dbo].[UnsupportedDefaults]")
                && d.message.contains("was not emitted")));
    }

    #[test]
    fn separate_index_generation_duplicates_and_features() {
        let t1 = table("dbo", "A", vec![col("Name", SqlServerType::Text, true)]);
        let t2 = table("dbo", "B", vec![col("Name", SqlServerType::Text, true)]);
        let schema = DatabaseSchema {
            tables: vec![t1.clone(), t2.clone()],
            indexes: vec![
                IndexDef {
                    name: "IX_Name".into(),
                    table: t1.name.clone(),
                    columns: vec!["Name".into()],
                    unique: false,
                    clustered: Some(false),
                    filter: Some("Name IS NOT NULL".into()),
                },
                IndexDef {
                    name: "IX_Name".into(),
                    table: t2.name.clone(),
                    columns: vec!["Name".into()],
                    unique: true,
                    clustered: None,
                    filter: None,
                },
            ],
            diagnostics: vec![],
            statement_summary: Default::default(),
        };
        let tables = generate_tables(&schema, &ConvertOptions::default())
            .unwrap()
            .to_sql();
        assert!(!tables.contains("CREATE INDEX"));
        let indexes = generate_indexes(&schema, &ConvertOptions::default()).unwrap();
        assert_eq!(indexes.to_sql(), "CREATE INDEX \"IX_Name\" ON \"dbo_A\" (\"Name\");\n\nCREATE UNIQUE INDEX \"IX_Name_2\" ON \"dbo_B\" (\"Name\");\n".replace("\\\"", "\""));
        assert!(indexes
            .diagnostics
            .iter()
            .any(|d| d.message.contains("clustered/nonclustered")));
        assert!(indexes
            .diagnostics
            .iter()
            .any(|d| d.message.contains("filter")));
        assert!(indexes
            .diagnostics
            .iter()
            .any(|d| d.message.contains("duplicate SQLite index name")));
    }

    #[test]
    fn generated_ddl_executes_in_sqlite() {
        let mut id = col("Id", SqlServerType::Int, false);
        id.identity = true;
        let mut t = table(
            "dbo",
            "Runnable",
            vec![id, col("Name", SqlServerType::Text, false)],
        );
        t.primary_key = Some(PrimaryKeyDef {
            name: None,
            columns: vec!["Id".into()],
            clustered: None,
        });
        let schema = DatabaseSchema {
            tables: vec![t.clone()],
            indexes: vec![IndexDef {
                name: "IX_Runnable_Name".into(),
                table: t.name.clone(),
                columns: vec!["Name".into()],
                unique: false,
                clustered: None,
                filter: None,
            }],
            diagnostics: vec![],
            statement_summary: Default::default(),
        };
        let connection = rusqlite::Connection::open_in_memory().unwrap();
        for statement in generate_tables(&schema, &ConvertOptions::default())
            .unwrap()
            .statements
        {
            connection.execute_batch(&statement.0).unwrap();
        }
        for statement in generate_indexes(&schema, &ConvertOptions::default())
            .unwrap()
            .statements
        {
            connection.execute_batch(&statement.0).unwrap();
        }
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
            statement_summary: Default::default(),
        };
        let first = schema_sql(&schema, &ConvertOptions::default()).unwrap();
        let second = schema_sql(&schema, &ConvertOptions::default()).unwrap();
        assert_eq!(first, second);
        assert!(first.ends_with('\n'));
    }
}
