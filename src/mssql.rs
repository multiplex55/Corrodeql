//! Microsoft SQL Server connectivity and extraction.

pub mod constraints;
pub mod defaults;
pub mod identifiers;
pub mod types;

/// Normalizes SQL Server-specific schema constructs before SQLite generation.
pub fn normalize(
    schema: crate::schema::model::DatabaseSchema,
) -> crate::schema::model::DatabaseSchema {
    crate::schema::normalize::normalize(schema)
}
