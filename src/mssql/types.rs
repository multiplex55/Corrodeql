use crate::schema::model::{DiagnosticSeverity, SchemaDiagnostic, SqlServerType};
use crate::sqlite::types::StorageClass;

/// A SQL Server type name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeName(pub String);

/// Normalized SQL Server type plus its SQLite affinity mapping.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NormalizedType {
    pub sql_server: SqlServerType,
    pub sqlite_affinity: StorageClass,
    pub diagnostics: Vec<SchemaDiagnostic>,
}

/// Maps a parsed SQL Server type to a SQLite storage class and reports lossy or
/// affinity-based conversion decisions.
pub fn normalize_type(data_type: &SqlServerType) -> NormalizedType {
    use SqlServerType::*;

    let (sqlite_affinity, warning) = match data_type {
        Int | BigInt | SmallInt | TinyInt | Bit => (StorageClass::Integer, None),
        Decimal { .. } | Numeric { .. } | Money => (
            StorageClass::Real,
            Some("exact SQL Server numeric type mapped to SQLite REAL affinity"),
        ),
        Float { .. } | Real => (StorageClass::Real, None),
        Char { .. } | VarChar { .. } | NChar { .. } | NVarChar { .. } | Text | NText => {
            (StorageClass::Text, None)
        }
        Date | DateTime | DateTime2 { .. } | SmallDateTime | Time { .. } => (
            StorageClass::Text,
            Some("SQL Server temporal type mapped to SQLite TEXT affinity"),
        ),
        UniqueIdentifier => (
            StorageClass::Text,
            Some("uniqueidentifier mapped to SQLite TEXT affinity"),
        ),
        Binary { .. } | VarBinary { .. } => (StorageClass::Blob, None),
        Xml => (
            StorageClass::Text,
            Some("xml mapped to SQLite TEXT affinity"),
        ),
        Other { name, .. } => (
            StorageClass::Text,
            Some(if name.is_empty() {
                "unknown SQL Server type mapped to SQLite TEXT affinity"
            } else {
                "unrecognized SQL Server type mapped to SQLite TEXT affinity"
            }),
        ),
    };

    let diagnostics = warning
        .map(|message| SchemaDiagnostic {
            severity: DiagnosticSeverity::Warning,
            message: message.to_owned(),
        })
        .into_iter()
        .collect();

    NormalizedType {
        sql_server: data_type.clone(),
        sqlite_affinity,
        diagnostics,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_integer_family() {
        assert_eq!(
            normalize_type(&SqlServerType::Int).sqlite_affinity,
            StorageClass::Integer
        );
        assert_eq!(
            normalize_type(&SqlServerType::BigInt).sqlite_affinity,
            StorageClass::Integer
        );
        assert_eq!(
            normalize_type(&SqlServerType::SmallInt).sqlite_affinity,
            StorageClass::Integer
        );
        assert_eq!(
            normalize_type(&SqlServerType::TinyInt).sqlite_affinity,
            StorageClass::Integer
        );
        assert_eq!(
            normalize_type(&SqlServerType::Bit).sqlite_affinity,
            StorageClass::Integer
        );
    }

    #[test]
    fn maps_exact_numeric_family_with_warning() {
        let normalized = normalize_type(&SqlServerType::Decimal {
            precision: Some(10),
            scale: Some(2),
        });
        assert_eq!(normalized.sqlite_affinity, StorageClass::Real);
        assert_eq!(
            normalized.diagnostics[0].severity,
            DiagnosticSeverity::Warning
        );
        assert_eq!(
            normalize_type(&SqlServerType::Numeric {
                precision: Some(8),
                scale: Some(3)
            })
            .sqlite_affinity,
            StorageClass::Real
        );
        assert_eq!(
            normalize_type(&SqlServerType::Money).sqlite_affinity,
            StorageClass::Real
        );
    }

    #[test]
    fn maps_character_and_binary_families() {
        assert_eq!(
            normalize_type(&SqlServerType::VarChar {
                length: None,
                max: true
            })
            .sqlite_affinity,
            StorageClass::Text
        );
        assert_eq!(
            normalize_type(&SqlServerType::NVarChar {
                length: Some(50),
                max: false
            })
            .sqlite_affinity,
            StorageClass::Text
        );
        assert_eq!(
            normalize_type(&SqlServerType::Text).sqlite_affinity,
            StorageClass::Text
        );
        assert_eq!(
            normalize_type(&SqlServerType::VarBinary {
                length: None,
                max: true
            })
            .sqlite_affinity,
            StorageClass::Blob
        );
    }

    #[test]
    fn maps_temporal_and_guid_families_with_warnings() {
        assert_eq!(
            normalize_type(&SqlServerType::DateTime2 { scale: None }).sqlite_affinity,
            StorageClass::Text
        );
        assert_eq!(
            normalize_type(&SqlServerType::UniqueIdentifier).sqlite_affinity,
            StorageClass::Text
        );
        assert!(!normalize_type(&SqlServerType::UniqueIdentifier)
            .diagnostics
            .is_empty());
    }
}
