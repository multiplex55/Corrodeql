use super::model::ConversionReport;

/// Renders a JSON conversion report.
pub fn render(report: &ConversionReport) -> String {
    serde_json::to_string_pretty(report).expect("report model should serialize")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::report::model::{
        ConversionReport, CsvIssueReport, Diagnostic, DiagnosticSeverity,
        ForeignKeyValidationReport, IntegrityCheckReport, RowCountValidationReport, SchemaSummary,
        TableReport, ValidationReport,
    };

    #[test]
    fn json_report_serialization_is_pretty_and_complete() {
        let report = ConversionReport {
            input_schema_path: "schema.sql".to_owned(),
            data_directory: "data".to_owned(),
            output_database_path: "out.sqlite".to_owned(),
            table_name_mode: "schema-prefix".to_owned(),
            null_token: r"\N".to_owned(),
            schema: SchemaSummary {
                tables_detected: 1,
                tables: vec![TableReport {
                    source_table: "[dbo].[A]".to_owned(),
                    sqlite_table: "dbo_A".to_owned(),
                    ..TableReport::default()
                }],
                ..SchemaSummary::default()
            },
            diagnostics: vec![Diagnostic {
                severity: DiagnosticSeverity::Unsupported,
                message: "filegroup ignored".to_owned(),
                ..Default::default()
            }],
            row_count_validation: RowCountValidationReport::default(),
            foreign_key_validation: ForeignKeyValidationReport {
                attempted: true,
                skipped: false,
                violations: Vec::new(),
            },
            integrity_check: IntegrityCheckReport {
                success: true,
                results: vec!["ok".to_owned()],
            },
            validation: ValidationReport {
                integrity_check: IntegrityCheckReport {
                    success: true,
                    results: vec!["ok".to_owned()],
                },
                ..ValidationReport::default()
            },
            type_mapping_warnings: vec![Diagnostic {
                severity: DiagnosticSeverity::Warning,
                message: "unrecognized SQL Server type x".to_owned(),
                ..Default::default()
            }],
            default_mapping_warnings: vec![Diagnostic {
                severity: DiagnosticSeverity::Warning,
                message: "default on [dbo].[A].C was not emitted".to_owned(),
                ..Default::default()
            }],
            skipped_objects: vec!["1 trigger statement".to_owned()],
            unsupported_sql_server_features: vec!["filegroup ignored".to_owned()],
            csv_issues: vec![CsvIssueReport {
                source_table: "[dbo].[A]".to_owned(),
                sqlite_table: "dbo_A".to_owned(),
                csv_path: Some("data/dbo.A.csv".to_owned()),
                message: "bad value".to_owned(),
            }],
            ..ConversionReport::default()
        };

        let json = render(&report);
        assert!(json.contains("\"input_schema_path\": \"schema.sql\""));
        assert!(json.contains("\"source_table\": \"[dbo].[A]\""));
        assert!(json.contains("\"sqlite_table\": \"dbo_A\""));
        assert!(json.contains("\"severity\": \"unsupported\""));
        assert!(json.contains("\"table_name_mode\": \"schema-prefix\""));
        assert!(json.contains("\"null_token\": \"\\\\N\""));
        assert!(json.contains("\"row_count_validation\""));
        assert!(json.contains("\"foreign_key_validation\""));
        assert!(json.contains("\"integrity_check\""));
        assert!(json.contains("\"type_mapping_warnings\""));
        assert!(json.contains("\"default_mapping_warnings\""));
        assert!(json.contains("\"skipped_objects\""));
        assert!(json.contains("\"unsupported_sql_server_features\""));
        assert!(json.contains("\"csv_issues\""));
        assert!(json.contains("\"results\": ["));
        assert!(json.contains("\"ok\""));
        assert!(serde_json::from_str::<ConversionReport>(&json).is_ok());
    }
}
