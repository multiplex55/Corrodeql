use super::model::{ConversionReport, DiagnosticSeverity};
use crate::sqlite::names::quote_identifier;

/// Renders a human-readable conversion report.
pub fn render(report: &ConversionReport) -> String {
    let mut output = String::new();
    output.push_str("Conversion Report\n");
    output.push_str("=================\n");
    output.push_str("\nInputs/options\n");
    output.push_str(&format!("Input schema: {}\n", report.input_schema_path));
    output.push_str(&format!("Data directory: {}\n", report.data_directory));
    output.push_str(&format!(
        "Output database: {}\n",
        report.output_database_path
    ));
    output.push_str(&format!("Table naming mode: {}\n", report.table_name_mode));
    output.push_str(&format!("Null token: {}\n\n", report.null_token));

    output.push_str("Schema summary\n");
    output.push_str(&format!(
        "Tables: {}, Columns: {}, Constraints: {}, Indexes: {}\n",
        report.schema.tables_detected,
        report.schema.columns_detected,
        report.schema.constraints_detected,
        report.schema.indexes_detected
    ));
    for table in &report.schema.tables {
        output.push_str(&format!(
            "- {} -> {}: columns={}, constraints={}, indexes={}\n",
            table.source_table,
            quote_identifier(&table.sqlite_table),
            table.columns_detected,
            table.constraints_detected,
            table.indexes_detected
        ));
        if !table.columns.is_empty() {
            output.push_str(&format!("  columns: {}\n", table.columns.join(", ")));
        }
        if !table.constraints.is_empty() {
            output.push_str(&format!(
                "  constraints: {}\n",
                table.constraints.join(", ")
            ));
        }
        if !table.indexes.is_empty() {
            output.push_str(&format!("  indexes: {}\n", table.indexes.join(", ")));
        }
    }

    if !report.statements.detected.is_empty()
        || !report.statements.ignored.is_empty()
        || !report.statements.warnings.is_empty()
    {
        output.push_str("\nStatement Classification\n");
        for entry in &report.statements.detected {
            output.push_str(&format!(
                "Detected: {} {} statement{}\n",
                entry.count,
                entry.kind,
                if entry.count == 1 { "" } else { "s" }
            ));
        }
        for entry in &report.statements.ignored {
            output.push_str(&format!(
                "Ignored: {} {} statement{}\n",
                entry.count,
                entry.kind,
                if entry.count == 1 { "" } else { "s" }
            ));
        }
        for entry in &report.statements.warnings {
            output.push_str(&format!(
                "Warnings: {} {} statement{}\n",
                entry.count,
                entry.kind,
                if entry.count == 1 { "" } else { "s" }
            ));
        }
    }

    output.push_str("\nTables discovered/imported\n");
    output.push_str(&format!(
        "Rows read={}, inserted={}, rejected={}\n",
        report.import.rows_read, report.import.rows_inserted, report.import.rows_rejected
    ));
    for table in &report.import.tables {
        let status = match table.status {
            super::model::TableImportStatus::Imported => "Imported",
            super::model::TableImportStatus::Partial => "Partial",
            super::model::TableImportStatus::Failed => "Failed",
            super::model::TableImportStatus::Skipped => "Skipped",
        };
        output.push_str(&format!(
            "- {} -> {}: {status} (read={}, inserted={}, rejected={})\n",
            table.source_table,
            quote_identifier(&table.sqlite_table),
            table.rows_read,
            table.rows_inserted,
            table.rows_rejected
        ));
        for diagnostic in &table.diagnostics {
            output.push_str(&format!("  - {diagnostic}\n"));
        }
    }

    output.push_str("\nRows imported per table\n");
    for table in &report.import.tables {
        output.push_str(&format!(
            "- {}: {} inserted ({} read, {} rejected)\n",
            table.source_table, table.rows_inserted, table.rows_read, table.rows_rejected
        ));
    }

    output.push_str("\nRow-count validation\n");
    output.push_str(&format!(
        "Status: {:?}\n",
        report.row_count_validation.status
    ));
    for diagnostic in &report.row_count_validation.diagnostics {
        output.push_str(&format!(
            "- {:?}: {}\n",
            diagnostic.severity, diagnostic.message
        ));
    }

    output.push_str("\nForeign-key validation\n");
    output.push_str(&format!(
        "Attempted: {}, skipped: {}, violations: {}\n",
        report.foreign_key_validation.attempted,
        report.foreign_key_validation.skipped,
        report.foreign_key_validation.violations.len()
    ));

    output.push_str("\nIntegrity check\n");
    output.push_str(&format!(
        "Success: {}, results: {}\n",
        report.integrity_check.success,
        if report.integrity_check.results.is_empty() {
            "<no rows>".to_owned()
        } else {
            report.integrity_check.results.join("; ")
        }
    ));

    output.push_str("\nMapping warnings\n");
    for diagnostic in report
        .type_mapping_warnings
        .iter()
        .chain(report.default_mapping_warnings.iter())
    {
        output.push_str(&format!(
            "- {:?}: {}\n",
            diagnostic.severity, diagnostic.message
        ));
    }

    output.push_str("\nSkipped objects\n");
    for object in &report.skipped_objects {
        output.push_str(&format!("- {object}\n"));
    }

    output.push_str("\nUnsupported features\n");
    for feature in &report.unsupported_sql_server_features {
        output.push_str(&format!("- {feature}\n"));
    }

    output.push_str("\nCSV issues\n");
    for issue in &report.csv_issues {
        output.push_str(&format!(
            "- {} ({:?}): {}\n",
            issue.source_table, issue.csv_path, issue.message
        ));
    }

    if !report.diagnostics.is_empty() {
        output.push_str("\nDiagnostics\n");
        for diagnostic in &report.diagnostics {
            let label = match diagnostic.severity {
                DiagnosticSeverity::Warning => "warning",
                DiagnosticSeverity::Error => "error",
                DiagnosticSeverity::Unsupported => "unsupported",
            };
            output.push_str(&format!("- {label}: {}\n", diagnostic.message));
        }
    }

    output.push_str("\nValidation\n");
    output.push_str(&format!(
        "Attempted: {}, Success: {}, Tables validated: {}\n",
        report.validation.attempted, report.validation.success, report.validation.tables_validated
    ));
    output.push_str(&format!(
        "Integrity check: {} ({})\n",
        if report.validation.integrity_check.success {
            "ok"
        } else {
            "failed"
        },
        if report.validation.integrity_check.results.is_empty() {
            "<no rows>".to_owned()
        } else {
            report.validation.integrity_check.results.join("; ")
        }
    ));
    output.push_str(&format!(
        "Foreign-key check: attempted={}, skipped={}, violations={}\n",
        report.validation.foreign_key_check_attempted,
        report.validation.foreign_key_check_skipped,
        report.validation.foreign_key_violations.len()
    ));
    for violation in &report.validation.foreign_key_violations {
        output.push_str(&format!(
            "- foreign key violation: child_table={}, rowid={:?}, parent_table={}, foreign_key_id={}\n",
            violation.child_table, violation.rowid, violation.parent_table, violation.foreign_key_id
        ));
    }
    for diagnostic in &report.validation.diagnostics {
        output.push_str(&format!(
            "- {:?}: {}\n",
            diagnostic.severity, diagnostic.message
        ));
    }

    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::report::model::{
        ConversionReport, Diagnostic, DiagnosticSeverity, IntegrityCheckReport, SchemaSummary,
        TableReport, ValidationReport,
    };

    #[test]
    fn text_report_includes_table_summaries() {
        let report = ConversionReport {
            schema: SchemaSummary {
                tables_detected: 1,
                columns_detected: 2,
                constraints_detected: 1,
                indexes_detected: 1,
                tables: vec![TableReport {
                    source_table: "[dbo].[Widget]".to_owned(),
                    sqlite_table: "dbo_Widget".to_owned(),
                    columns_detected: 2,
                    constraints_detected: 1,
                    indexes_detected: 1,
                    columns: vec!["Id".to_owned(), "Name".to_owned()],
                    constraints: vec!["primary_key".to_owned()],
                    indexes: vec!["IX_Widget_Name".to_owned()],
                }],
            },
            ..ConversionReport::default()
        };
        let text = render(&report);
        assert!(text.contains("[dbo].[Widget] -> \"dbo_Widget\""));
        assert!(text.contains("columns: Id, Name"));
    }

    #[test]
    fn text_report_includes_warnings() {
        let report = ConversionReport {
            diagnostics: vec![Diagnostic {
                severity: DiagnosticSeverity::Warning,
                message: "default was ignored".to_owned(),
            }],
            ..ConversionReport::default()
        };
        assert!(render(&report).contains("warning: default was ignored"));
    }

    #[test]
    fn text_report_includes_integrity_check_results() {
        let report = ConversionReport {
            validation: ValidationReport {
                attempted: true,
                success: false,
                integrity_check: IntegrityCheckReport {
                    success: false,
                    results: vec!["row 1 missing from index".to_owned()],
                },
                ..ValidationReport::default()
            },
            ..ConversionReport::default()
        };
        let text = render(&report);
        assert!(text.contains("Integrity check: failed (row 1 missing from index)"));
    }
}
