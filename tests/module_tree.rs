use corrodeql::{config, data, mssql, report, schema, sqlite};

#[test]
fn public_module_tree_exposes_major_areas() {
    let _options = config::options::Options;
    let _paths = config::paths::Paths;

    let schema = schema::parser::parse(schema::preprocessor::preprocess(""));
    let _schema = schema::normalize::normalize(schema);
    let _token = schema::lexer::lex("SELECT");
    let _diagnostic = schema::diagnostics::Diagnostic {
        severity: schema::model::DiagnosticSeverity::Warning,
        message: String::new(),
    };

    let _identifier = mssql::identifiers::Identifier(String::new());
    let _type_name = mssql::types::TypeName(String::new());
    let _default = mssql::defaults::DefaultExpression(String::new());
    let _constraint = mssql::constraints::ConstraintName(String::new());

    let _name = sqlite::names::Name(String::new());
    let _storage_class = sqlite::types::StorageClass::Text;
    let _statement = sqlite::ddl::Statement(String::new());
    let _database = sqlite::database::Database;
    sqlite::import::import();
    sqlite::validate::validate();

    let _reader_options = data::csv_reader::CsvReaderOptions::default();
    data::conversion::convert();
    let _manifest = data::manifest::Manifest::default();

    let report = report::model::Report::default();
    let _text = report::text::render(&report);
    let _json = report::json::render(&report);
}
