use std::fs;
use std::path::Path;

#[test]
fn required_documentation_files_exist_with_key_content() {
    let required_files = [
        "docs/architecture.md",
        "docs/module-layout.md",
        "docs/development.md",
    ];

    for file in required_files {
        assert!(Path::new(file).exists(), "missing required documentation file: {file}");
    }

    let architecture = fs::read_to_string("docs/architecture.md").unwrap();
    assert!(architecture.contains("Schema Parser"));
    assert!(architecture.contains("SQLite DDL Generator"));

    let module_layout = fs::read_to_string("docs/module-layout.md").unwrap();
    assert!(module_layout.contains("src/schema/mod.rs"));

    let development = fs::read_to_string("docs/development.md").unwrap();
    assert!(development.contains("cargo clippy --all-targets --all-features -- -D warnings"));
}
