use super::model::Report;

/// Renders a JSON report.
pub fn render(report: &Report) -> String {
    serde_json::to_string_pretty(report).expect("report model should serialize")
}
