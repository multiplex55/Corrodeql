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
    // Preserve decimal/numeric/money precision by storing the original textual representation.
    // Validate only a simple SQL-style decimal lexical grammar instead of parsing through f64.
    let rest = value
        .strip_prefix('+')
        .or_else(|| value.strip_prefix('-'))
        .unwrap_or(value);
    let Some((left, right)) = rest.split_once('.').or(Some((rest, ""))) else {
        unreachable!();
    };
    let has_digits =
        left.bytes().any(|b| b.is_ascii_digit()) || right.bytes().any(|b| b.is_ascii_digit());
    let valid = has_digits
        && left.bytes().all(|b| b.is_ascii_digit())
        && right.bytes().all(|b| b.is_ascii_digit())
        && rest.matches('.').count() <= 1;
    if valid {
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
    fn converts_requested_csv_value_boundaries() {
        let cases = [
            (
                "Name",
                SqlServerType::Text,
                "hello",
                Value::Text("hello".to_owned()),
            ),
            ("Id", SqlServerType::Int, "42", Value::Integer(42)),
            (
                "Amount",
                SqlServerType::Money,
                "12.3400",
                Value::Text("12.3400".to_owned()),
            ),
            (
                "When",
                SqlServerType::Date,
                "2024-01-02",
                Value::Text("2024-01-02".to_owned()),
            ),
            ("Flag", SqlServerType::Bit, "1", Value::Integer(1)),
            ("Flag", SqlServerType::Bit, "false", Value::Integer(0)),
            (
                "Guid",
                SqlServerType::UniqueIdentifier,
                "00000000-0000-0000-0000-000000000000",
                Value::Text("00000000-0000-0000-0000-000000000000".to_owned()),
            ),
        ];
        for (name, data_type, raw, expected) in cases {
            assert_eq!(
                convert_csv_value(&table(), &column(name, data_type), 2, raw, None).unwrap(),
                expected
            );
        }
        assert_eq!(
            convert_csv_value(&table(), &column("Name", SqlServerType::Text), 2, "", None).unwrap(),
            Value::Text(String::new())
        );
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
    #[test]
    fn blank_string_is_not_implicitly_null_for_text() {
        let value =
            convert_csv_value(&table(), &column("Name", SqlServerType::Text), 2, "", None).unwrap();
        assert_eq!(value, Value::Text(String::new()));
    }

    #[test]
    fn invalid_integer_and_bit_fail() {
        assert!(
            convert_csv_value(&table(), &column("Id", SqlServerType::BigInt), 2, "x", None)
                .is_err()
        );
        assert!(convert_csv_value(
            &table(),
            &column("Flag", SqlServerType::Bit),
            2,
            "yes",
            None
        )
        .is_err());
        assert_eq!(
            convert_csv_value(
                &table(),
                &column("Flag", SqlServerType::Bit),
                2,
                "FALSE",
                None
            )
            .unwrap(),
            Value::Integer(0)
        );
    }

    #[test]
    fn high_precision_decimal_and_money_are_preserved_as_text() {
        let decimal = "12345678901234567890.123400";
        assert_eq!(
            convert_csv_value(
                &table(),
                &column(
                    "Amount",
                    SqlServerType::Numeric {
                        precision: Some(38),
                        scale: Some(6)
                    }
                ),
                2,
                decimal,
                None
            )
            .unwrap(),
            Value::Text(decimal.to_owned())
        );
        assert_eq!(
            convert_csv_value(
                &table(),
                &column("Money", SqlServerType::Money),
                2,
                "123.4500",
                None
            )
            .unwrap(),
            Value::Text("123.4500".to_owned())
        );
    }

    #[test]
    fn float_date_guid_and_rowversion_behaviors() {
        assert_eq!(
            convert_csv_value(&table(), &column("F", SqlServerType::Real), 2, "1.5", None).unwrap(),
            Value::Real(1.5)
        );
        assert!(convert_csv_value(
            &table(),
            &column("F", SqlServerType::Float { precision: None }),
            2,
            "nan?",
            None
        )
        .is_err());
        assert_eq!(
            convert_csv_value(
                &table(),
                &column("D", SqlServerType::DateTime2 { scale: None }),
                2,
                "2024-01-02T03:04:05",
                None
            )
            .unwrap(),
            Value::Text("2024-01-02T03:04:05".to_owned())
        );
        assert_eq!(
            convert_csv_value(
                &table(),
                &column("G", SqlServerType::UniqueIdentifier),
                2,
                "00000000-0000-0000-0000-000000000000",
                None
            )
            .unwrap(),
            Value::Text("00000000-0000-0000-0000-000000000000".to_owned())
        );
        assert_eq!(
            convert_csv_value(
                &table(),
                &column("Rv", SqlServerType::RowVersion),
                2,
                "0x0102",
                None
            )
            .unwrap(),
            Value::Blob(vec![1, 2])
        );
        assert!(convert_csv_value(
            &table(),
            &column("Rv", SqlServerType::Timestamp),
            2,
            "base64==",
            None
        )
        .is_err());
    }
}
