//! CSV field conversion into SQLite-compatible values.

use rusqlite::types::Value;

use crate::schema::model::{ColumnDef, SqlServerType, TableName};

/// Default token that represents SQL `NULL` in CSV input.
pub const DEFAULT_NULL_TOKEN: &str = r"\N";

/// Backwards-compatible no-op marker for module-tree smoke tests.
pub fn convert() {}

/// Row-level conversion diagnostic for a single invalid CSV value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValueDiagnostic {
    pub table: TableName,
    pub column: String,
    /// 1-based physical CSV row number. The header row is row 1.
    pub row_number: u64,
    pub original_value: String,
    pub reason: String,
}

/// Converts one CSV field into the SQLite value used for insertion.
pub fn convert_csv_value(
    table: &TableName,
    column: &ColumnDef,
    row_number: u64,
    value: &str,
    null_token: Option<&str>,
) -> Result<Value, ValueDiagnostic> {
    let null_token = null_token.unwrap_or(DEFAULT_NULL_TOKEN);
    if value == null_token {
        return Ok(Value::Null);
    }

    convert_non_null_value(&column.data_type, value).map_err(|reason| ValueDiagnostic {
        table: table.clone(),
        column: column.name.clone(),
        row_number,
        original_value: value.to_owned(),
        reason,
    })
}

fn convert_non_null_value(data_type: &SqlServerType, value: &str) -> Result<Value, String> {
    use SqlServerType::*;

    match data_type {
        Int | BigInt | SmallInt | TinyInt => parse_integer(value),
        Bit => parse_bit(value),
        Decimal { .. } | Numeric { .. } | Money | SmallMoney => parse_numeric_text(value),
        Float { .. } | Real => value
            .parse::<f64>()
            .map(Value::Real)
            .map_err(|_| "expected floating-point number".to_owned()),
        Date
        | Time { .. }
        | DateTime
        | DateTime2 { .. }
        | SmallDateTime
        | DateTimeOffset { .. }
        | UniqueIdentifier
        | Char { .. }
        | VarChar { .. }
        | NChar { .. }
        | NVarChar { .. }
        | Text
        | NText
        | Xml
        | Other { .. } => Ok(Value::Text(value.to_owned())),
        Binary { .. } | VarBinary { .. } | Image | RowVersion | Timestamp => parse_hex_blob(value),
    }
}

fn parse_integer(value: &str) -> Result<Value, String> {
    value
        .parse::<i64>()
        .map(Value::Integer)
        .map_err(|_| "expected integer".to_owned())
}

fn parse_bit(value: &str) -> Result<Value, String> {
    match value.to_ascii_lowercase().as_str() {
        "0" | "false" => Ok(Value::Integer(0)),
        "1" | "true" => Ok(Value::Integer(1)),
        _ => Err("expected bit value (0, 1, true, or false)".to_owned()),
    }
}

fn parse_numeric_text(value: &str) -> Result<Value, String> {
    if value.parse::<f64>().is_ok() {
        // Preserve decimal/numeric precision by storing the original textual representation.
        Ok(Value::Text(value.to_owned()))
    } else {
        Err("expected decimal or numeric value".to_owned())
    }
}

fn parse_hex_blob(value: &str) -> Result<Value, String> {
    let hex = value
        .strip_prefix("0x")
        .or_else(|| value.strip_prefix("0X"))
        .unwrap_or(value);
    if hex.len() % 2 != 0 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(
            "expected binary value encoded as hexadecimal, optionally prefixed with 0x".to_owned(),
        );
    }

    let mut bytes = Vec::with_capacity(hex.len() / 2);
    for chunk in hex.as_bytes().chunks_exact(2) {
        let pair =
            std::str::from_utf8(chunk).map_err(|_| "expected valid hexadecimal".to_owned())?;
        let byte =
            u8::from_str_radix(pair, 16).map_err(|_| "expected valid hexadecimal".to_owned())?;
        bytes.push(byte);
    }
    Ok(Value::Blob(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn column(name: &str, data_type: SqlServerType) -> ColumnDef {
        ColumnDef {
            name: name.to_owned(),
            data_type,
            nullable: true,
            identity: false,
            primary_key: false,
            unique: false,
            default: None,
            check: None,
        }
    }

    fn table() -> TableName {
        TableName::new(Some("dbo".to_owned()), "Widget")
    }

    #[test]
    fn converts_null_token_before_type_parsing() {
        let value =
            convert_csv_value(&table(), &column("Id", SqlServerType::Int), 2, r"\N", None).unwrap();
        assert_eq!(value, Value::Null);
    }

    #[test]
    fn converts_supported_scalar_values() {
        assert_eq!(
            convert_csv_value(&table(), &column("Id", SqlServerType::Int), 2, "42", None).unwrap(),
            Value::Integer(42)
        );
        assert_eq!(
            convert_csv_value(
                &table(),
                &column("Flag", SqlServerType::Bit),
                2,
                "true",
                None
            )
            .unwrap(),
            Value::Integer(1)
        );
        assert_eq!(
            convert_csv_value(
                &table(),
                &column(
                    "Amount",
                    SqlServerType::Decimal {
                        precision: None,
                        scale: None
                    }
                ),
                2,
                "12.3400",
                None,
            )
            .unwrap(),
            Value::Text("12.3400".to_owned())
        );
        assert_eq!(
            convert_csv_value(
                &table(),
                &column(
                    "Payload",
                    SqlServerType::VarBinary {
                        length: None,
                        max: true
                    }
                ),
                2,
                "0x0A0b",
                None,
            )
            .unwrap(),
            Value::Blob(vec![10, 11])
        );
    }

    #[test]
    fn reports_column_and_row_for_invalid_value() {
        let error = convert_csv_value(&table(), &column("Id", SqlServerType::Int), 7, "oops", None)
            .unwrap_err();
        assert_eq!(error.table, table());
        assert_eq!(error.column, "Id");
        assert_eq!(error.row_number, 7);
        assert_eq!(error.original_value, "oops");
        assert_eq!(error.reason, "expected integer");
    }
}
