use super::model::ConversionReport;

/// Renders a JSON conversion report.
pub fn render(report: &ConversionReport) -> String {
    serde_json::to_string_pretty(report).expect("report model should serialize")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::report::model::{
        ConversionReport, Diagnostic, DiagnosticSeverity, IntegrityCheckReport, SchemaSummary,
        TableReport, ValidationReport,
    };

    #[test]
    fn json_report_serialization_is_pretty_and_complete() {
        let report = ConversionReport {
            input_schema_path: "schema.sql".to_owned(),
            data_directory: "data".to_owned(),
            output_database_path: "out.sqlite".to_owned(),
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
            }],
            validation: ValidationReport {
                integrity_check: IntegrityCheckReport {
                    success: true,
                    results: vec!["ok".to_owned()],
                },
                ..ValidationReport::default()
            },
            ..ConversionReport::default()
        };

        let json = render(&report);
        assert!(json.contains("\"input_schema_path\": \"schema.sql\""));
        assert!(json.contains("\"source_table\": \"[dbo].[A]\""));
        assert!(json.contains("\"sqlite_table\": \"dbo_A\""));
        assert!(json.contains("\"severity\": \"unsupported\""));
        assert!(json.contains("\"integrity_check\""));
        assert!(json.contains("\"results\": [\n        \"ok\"\n      ]"));
        assert!(serde_json::from_str::<ConversionReport>(&json).is_ok());
    }
}
