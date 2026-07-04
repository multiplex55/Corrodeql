use crate::{mssql::types::normalize_type, schema::model::SqlServerType};

/// SQLite storage classes / type affinity names used in generated DDL.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StorageClass {
    Null,
    Integer,
    Real,
    Numeric,
    Text,
    Blob,
}

impl StorageClass {
    /// Returns the SQLite type name used to request this affinity.
    pub const fn ddl_name(self) -> &'static str {
        match self {
            Self::Null => "NULL",
            Self::Integer => "INTEGER",
            Self::Real => "REAL",
            Self::Numeric => "NUMERIC",
            Self::Text => "TEXT",
            Self::Blob => "BLOB",
        }
    }
}

/// Maps SQL Server types to SQLite affinities for DDL generation.
pub fn sqlite_affinity(data_type: &SqlServerType) -> StorageClass {
    normalize_type(data_type).sqlite_affinity
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ddl_names_match_sqlite_affinity_tokens() {
        assert_eq!(StorageClass::Null.ddl_name(), "NULL");
        assert_eq!(StorageClass::Integer.ddl_name(), "INTEGER");
        assert_eq!(StorageClass::Real.ddl_name(), "REAL");
        assert_eq!(StorageClass::Numeric.ddl_name(), "NUMERIC");
        assert_eq!(StorageClass::Text.ddl_name(), "TEXT");
        assert_eq!(StorageClass::Blob.ddl_name(), "BLOB");
    }

    #[test]
    fn maps_core_sql_server_families_to_sqlite_affinity() {
        assert_eq!(sqlite_affinity(&SqlServerType::Int), StorageClass::Integer);
        assert_eq!(sqlite_affinity(&SqlServerType::Bit), StorageClass::Integer);
        assert_eq!(
            sqlite_affinity(&SqlServerType::Decimal {
                precision: Some(18),
                scale: Some(2),
            }),
            StorageClass::Text
        );
        assert_eq!(
            sqlite_affinity(&SqlServerType::Float { precision: None }),
            StorageClass::Real
        );
        assert_eq!(
            sqlite_affinity(&SqlServerType::NVarChar {
                length: Some(50),
                max: false,
            }),
            StorageClass::Text
        );
        assert_eq!(
            sqlite_affinity(&SqlServerType::VarBinary {
                length: Some(16),
                max: false,
            }),
            StorageClass::Blob
        );
    }
}
