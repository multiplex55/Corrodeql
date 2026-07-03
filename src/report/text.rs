use super::model::Report;

/// Renders a human-readable text report.
pub fn render(report: &Report) -> String {
    let Some(import) = &report.import else {
        return String::new();
    };
    let mut output = format!(
        "Import: rows read={}, inserted={}, rejected={}\n",
        import.rows_read, import.rows_inserted, import.rows_rejected
    );
    for table in &import.tables {
        output.push_str(&format!(
            "{} -> {}: {:?} (read={}, inserted={}, rejected={})\n",
            table.source_table,
            table.sqlite_table,
            table.status,
            table.rows_read,
            table.rows_inserted,
            table.rows_rejected
        ));
        for diagnostic in &table.diagnostics {
            output.push_str(&format!("  - {diagnostic}\n"));
        }
    }
    output
}
