use std::fs;
use std::path::{Path, PathBuf};

#[test]
fn repository_does_not_contain_mod_rs_files() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut offenders = Vec::new();

    collect_mod_rs_files(&root, &mut offenders);

    assert!(
        offenders.is_empty(),
        "mod.rs files are not allowed; found: {}",
        offenders
            .iter()
            .map(|path| path.display().to_string())
            .collect::<Vec<_>>()
            .join(", ")
    );
}

fn collect_mod_rs_files(path: &Path, offenders: &mut Vec<PathBuf>) {
    let entries = fs::read_dir(path)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()));

    for entry in entries {
        let entry = entry.unwrap_or_else(|error| panic!("failed to read directory entry: {error}"));
        let path = entry.path();
        let file_type = entry
            .file_type()
            .unwrap_or_else(|error| panic!("failed to inspect {}: {error}", path.display()));

        if file_type.is_dir() {
            collect_mod_rs_files(&path, offenders);
        } else if file_type.is_file() && entry.file_name() == "mod.rs" {
            offenders.push(path);
        }
    }
}
