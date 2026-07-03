use crate::schema::model::SqlServerType;

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
    use SqlServerType::*;

    match data_type {
        Int | BigInt | SmallInt | TinyInt | Bit => StorageClass::Integer,
        Decimal { .. } | Numeric { .. } | Money => StorageClass::Numeric,
        Float { .. } | Real => StorageClass::Real,
        Date | Time { .. } | DateTime | DateTime2 { .. } | SmallDateTime => StorageClass::Text,
        UniqueIdentifier
        | Char { .. }
        | VarChar { .. }
        | NChar { .. }
        | NVarChar { .. }
        | Text
        | NText
        | Xml => StorageClass::Text,
        Binary { .. } | VarBinary { .. } => StorageClass::Blob,
        Other { .. } => StorageClass::Text,
    }
}
