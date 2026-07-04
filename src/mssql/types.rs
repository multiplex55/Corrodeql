use crate::schema::model::{DiagnosticSeverity, SchemaDiagnostic, SqlServerType, TableName};
use crate::sqlite::types::StorageClass;

/// A SQL Server type name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeName(pub String);

/// Required normalization bucket for SQL Server data types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SqlServerTypeGroup {
    Integer,
    Booleanish,
    ExactDecimal,
    FloatingPoint,
    Text,
    DateTime,
    Guid,
    Binary,
    RowVersion,
    Xml,
    Unknown,
}

/// Normalized SQL Server type plus its SQLite affinity mapping.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NormalizedType {
    pub sql_server: SqlServerType,
    pub group: SqlServerTypeGroup,
    pub sqlite_affinity: StorageClass,
    pub diagnostics: Vec<SchemaDiagnostic>,
}

pub fn type_group(data_type: &SqlServerType) -> SqlServerTypeGroup {
    use SqlServerType::*;
    match data_type {
        Int | BigInt | SmallInt | TinyInt => SqlServerTypeGroup::Integer,
        Bit => SqlServerTypeGroup::Booleanish,
        Decimal { .. } | Numeric { .. } | Money | SmallMoney => SqlServerTypeGroup::ExactDecimal,
        Float { .. } | Real => SqlServerTypeGroup::FloatingPoint,
        Char { .. } | VarChar { .. } | NChar { .. } | NVarChar { .. } | Text | NText => {
            SqlServerTypeGroup::Text
        }
        Date
        | Time { .. }
        | DateTime
        | DateTime2 { .. }
        | SmallDateTime
        | DateTimeOffset { .. } => SqlServerTypeGroup::DateTime,
        UniqueIdentifier => SqlServerTypeGroup::Guid,
        Binary { .. } | VarBinary { .. } | Image => SqlServerTypeGroup::Binary,
        RowVersion | Timestamp => SqlServerTypeGroup::RowVersion,
        Xml => SqlServerTypeGroup::Xml,
        Other { .. } => SqlServerTypeGroup::Unknown,
    }
}

/// Maps a parsed SQL Server type to a SQLite storage class and reports lossy or
/// affinity-based conversion decisions.
pub fn normalize_type(data_type: &SqlServerType) -> NormalizedType {
    normalize_type_with_context(data_type, None, None)
}

pub fn normalize_type_with_context(
    data_type: &SqlServerType,
    table: Option<&TableName>,
    column: Option<&str>,
) -> NormalizedType {
    let group = type_group(data_type);
    let sqlite_affinity = match group {
        SqlServerTypeGroup::Integer | SqlServerTypeGroup::Booleanish => StorageClass::Integer,
        SqlServerTypeGroup::ExactDecimal => StorageClass::Text,
        SqlServerTypeGroup::FloatingPoint => StorageClass::Real,
        SqlServerTypeGroup::Text
        | SqlServerTypeGroup::DateTime
        | SqlServerTypeGroup::Guid
        | SqlServerTypeGroup::Xml
        | SqlServerTypeGroup::Unknown => StorageClass::Text,
        SqlServerTypeGroup::Binary | SqlServerTypeGroup::RowVersion => StorageClass::Blob,
    };

    let diagnostics = diagnostic_message(data_type, group, table, column)
        .map(|message| SchemaDiagnostic {
            severity: DiagnosticSeverity::Warning,
            message,
            line: None,
            column: None,
        })
        .into_iter()
        .collect();

    NormalizedType {
        sql_server: data_type.clone(),
        group,
        sqlite_affinity,
        diagnostics,
    }
}

fn diagnostic_message(
    data_type: &SqlServerType,
    group: SqlServerTypeGroup,
    table: Option<&TableName>,
    column: Option<&str>,
) -> Option<String> {
    let context = match (table, column) {
        (Some(table), Some(column)) => format!(" on {}.{}", table.display_sql_server(), column),
        (Some(table), None) => format!(" on {}", table.display_sql_server()),
        (None, Some(column)) => format!(" on column {}", column),
        (None, None) => String::new(),
    };

    match group {
        SqlServerTypeGroup::ExactDecimal => Some(format!(
            "exact SQL Server decimal type{} mapped to SQLite TEXT affinity to preserve precision",
            context
        )),
        SqlServerTypeGroup::DateTime => Some(format!(
            "SQL Server temporal type{} mapped to SQLite TEXT affinity",
            context
        )),
        SqlServerTypeGroup::Guid => Some(format!(
            "uniqueidentifier{} mapped to SQLite TEXT affinity",
            context
        )),
        SqlServerTypeGroup::Xml => Some(format!("xml{} mapped to SQLite TEXT affinity", context)),
        SqlServerTypeGroup::Unknown => {
            let name = match data_type {
                SqlServerType::Other { name, .. } if !name.is_empty() => name.as_str(),
                _ => "<unknown>",
            };
            Some(format!(
                "unknown SQL Server type '{}'{} mapped to SQLite TEXT affinity",
                name, context
            ))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_maps(data_type: SqlServerType, group: SqlServerTypeGroup, storage: StorageClass) {
        let normalized = normalize_type(&data_type);
        assert_eq!(normalized.group, group);
        assert_eq!(normalized.sqlite_affinity, storage);
    }

    #[test]
    fn maps_required_sql_server_types_to_sqlite_affinities() {
        use SqlServerType::*;
        assert_maps(Int, SqlServerTypeGroup::Integer, StorageClass::Integer);
        assert_maps(BigInt, SqlServerTypeGroup::Integer, StorageClass::Integer);
        assert_maps(SmallInt, SqlServerTypeGroup::Integer, StorageClass::Integer);
        assert_maps(TinyInt, SqlServerTypeGroup::Integer, StorageClass::Integer);
        assert_maps(Bit, SqlServerTypeGroup::Booleanish, StorageClass::Integer);
        assert_maps(
            Decimal {
                precision: Some(18),
                scale: Some(2),
            },
            SqlServerTypeGroup::ExactDecimal,
            StorageClass::Text,
        );
        assert_maps(
            Numeric {
                precision: Some(18),
                scale: Some(2),
            },
            SqlServerTypeGroup::ExactDecimal,
            StorageClass::Text,
        );
        assert_maps(Money, SqlServerTypeGroup::ExactDecimal, StorageClass::Text);
        assert_maps(
            SmallMoney,
            SqlServerTypeGroup::ExactDecimal,
            StorageClass::Text,
        );
        assert_maps(
            Float { precision: None },
            SqlServerTypeGroup::FloatingPoint,
            StorageClass::Real,
        );
        assert_maps(Real, SqlServerTypeGroup::FloatingPoint, StorageClass::Real);
        assert_maps(
            Char { length: Some(1) },
            SqlServerTypeGroup::Text,
            StorageClass::Text,
        );
        assert_maps(
            VarChar {
                length: None,
                max: true,
            },
            SqlServerTypeGroup::Text,
            StorageClass::Text,
        );
        assert_maps(
            NChar { length: Some(1) },
            SqlServerTypeGroup::Text,
            StorageClass::Text,
        );
        assert_maps(
            NVarChar {
                length: Some(50),
                max: false,
            },
            SqlServerTypeGroup::Text,
            StorageClass::Text,
        );
        assert_maps(Text, SqlServerTypeGroup::Text, StorageClass::Text);
        assert_maps(NText, SqlServerTypeGroup::Text, StorageClass::Text);
        assert_maps(Date, SqlServerTypeGroup::DateTime, StorageClass::Text);
        assert_maps(
            Time { scale: Some(7) },
            SqlServerTypeGroup::DateTime,
            StorageClass::Text,
        );
        assert_maps(DateTime, SqlServerTypeGroup::DateTime, StorageClass::Text);
        assert_maps(
            DateTime2 { scale: Some(7) },
            SqlServerTypeGroup::DateTime,
            StorageClass::Text,
        );
        assert_maps(
            SmallDateTime,
            SqlServerTypeGroup::DateTime,
            StorageClass::Text,
        );
        assert_maps(
            DateTimeOffset { scale: Some(7) },
            SqlServerTypeGroup::DateTime,
            StorageClass::Text,
        );
        assert_maps(
            UniqueIdentifier,
            SqlServerTypeGroup::Guid,
            StorageClass::Text,
        );
        assert_maps(
            Binary { length: Some(16) },
            SqlServerTypeGroup::Binary,
            StorageClass::Blob,
        );
        assert_maps(
            VarBinary {
                length: None,
                max: true,
            },
            SqlServerTypeGroup::Binary,
            StorageClass::Blob,
        );
        assert_maps(Image, SqlServerTypeGroup::Binary, StorageClass::Blob);
        assert_maps(
            RowVersion,
            SqlServerTypeGroup::RowVersion,
            StorageClass::Blob,
        );
        assert_maps(
            Timestamp,
            SqlServerTypeGroup::RowVersion,
            StorageClass::Blob,
        );
        assert_maps(Xml, SqlServerTypeGroup::Xml, StorageClass::Text);
    }

    #[test]
    fn decimal_18_2_maps_to_text_not_real() {
        let normalized = normalize_type(&SqlServerType::Decimal {
            precision: Some(18),
            scale: Some(2),
        });
        assert_eq!(normalized.sqlite_affinity, StorageClass::Text);
        assert_ne!(normalized.sqlite_affinity, StorageClass::Real);
        assert!(normalized.diagnostics[0]
            .message
            .contains("preserve precision"));
    }

    #[test]
    fn unknown_type_maps_to_text_with_context_warning() {
        let table = TableName::new(Some("dbo".to_owned()), "Widget");
        let normalized = normalize_type_with_context(
            &SqlServerType::Other {
                name: "geography".to_owned(),
                arguments: Vec::new(),
            },
            Some(&table),
            Some("Shape"),
        );
        assert_eq!(normalized.sqlite_affinity, StorageClass::Text);
        assert!(normalized.diagnostics[0].message.contains("geography"));
        assert!(normalized.diagnostics[0]
            .message
            .contains("[dbo].[Widget].Shape"));
    }
}
