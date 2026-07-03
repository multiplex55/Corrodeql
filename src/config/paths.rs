use camino::{Utf8Path, Utf8PathBuf};

use crate::config::options::ConvertOptions;
use crate::error::{Error, Result};

/// Filesystem paths used by the application.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Paths;

/// Returns a UTF-8 path buffer with syntactic normalization applied.
pub fn normalize_path(path: impl AsRef<Utf8Path>) -> Utf8PathBuf {
    path.as_ref().to_path_buf()
}

/// Validates all path-bearing conversion options.
pub fn validate_convert_paths(options: &ConvertOptions) -> Result<()> {
    validate_schema_path(&options.schema_path)?;
    validate_data_dir(&options.data_dir)?;
    validate_output_db_path(&options.output_db_path, options.overwrite)?;
    if let Some(path) = &options.emit_ddl_path {
        validate_emit_ddl_path(path)?;
    }
    if let Some(path) = &options.report_dir {
        validate_report_dir(path)?;
    }
    Ok(())
}

pub fn validate_schema_path(path: impl AsRef<Utf8Path>) -> Result<Utf8PathBuf> {
    let path = normalize_path(path);
    if !path.exists() {
        return invalid_path("schema_path", path, "path does not exist");
    }
    if !path.is_file() {
        return invalid_path("schema_path", path, "path is not a file");
    }
    Ok(path)
}

pub fn validate_data_dir(path: impl AsRef<Utf8Path>) -> Result<Utf8PathBuf> {
    let path = normalize_path(path);
    if !path.exists() {
        return invalid_path("data_dir", path, "path does not exist");
    }
    if !path.is_dir() {
        return invalid_path("data_dir", path, "path is not a directory");
    }
    Ok(path)
}

pub fn validate_output_db_path(path: impl AsRef<Utf8Path>, overwrite: bool) -> Result<Utf8PathBuf> {
    let path = normalize_path(path);
    validate_parent_exists("output_db_path", &path)?;
    if path.exists() && !overwrite {
        return invalid_path(
            "output_db_path",
            path,
            "path already exists and overwrite is false",
        );
    }
    Ok(path)
}

pub fn validate_emit_ddl_path(path: impl AsRef<Utf8Path>) -> Result<Utf8PathBuf> {
    let path = normalize_path(path);
    validate_parent_exists("emit_ddl_path", &path)?;
    Ok(path)
}

pub fn validate_report_dir(path: impl AsRef<Utf8Path>) -> Result<Utf8PathBuf> {
    let path = normalize_path(path);
    if path.exists() && !path.is_dir() {
        return invalid_path("report_dir", path, "path exists and is not a directory");
    }
    validate_parent_exists("report_dir", &path)?;
    Ok(path)
}

fn validate_parent_exists(kind: &'static str, path: &Utf8Path) -> Result<()> {
    if let Some(parent) = path.parent().filter(|parent| !parent.as_str().is_empty()) {
        if !parent.exists() {
            return invalid_path(kind, path.to_path_buf(), "parent directory does not exist");
        }
        if !parent.is_dir() {
            return invalid_path(kind, path.to_path_buf(), "parent path is not a directory");
        }
    }
    Ok(())
}

fn invalid_path<T>(kind: &'static str, path: Utf8PathBuf, reason: &'static str) -> Result<T> {
    Err(Error::InvalidPath { kind, path, reason })
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;

    fn temp_root(name: &str) -> Utf8PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = Utf8PathBuf::from_path_buf(
            std::env::temp_dir().join(format!("corrodeql-{name}-{unique}")),
        )
        .unwrap();
        fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn rejects_existing_output_db_without_overwrite() {
        let root = temp_root("existing-output-reject");
        let db = root.join("out.sqlite");
        fs::write(&db, b"").unwrap();

        assert!(validate_output_db_path(&db, false).is_err());
    }

    #[test]
    fn accepts_existing_output_db_with_overwrite() {
        let root = temp_root("existing-output-accept");
        let db = root.join("out.sqlite");
        fs::write(&db, b"").unwrap();

        assert_eq!(validate_output_db_path(&db, true).unwrap(), db);
    }

    #[test]
    fn rejects_missing_schema_file() {
        let root = temp_root("missing-schema");
        assert!(validate_schema_path(root.join("schema.sql")).is_err());
    }

    #[test]
    fn rejects_missing_data_directory() {
        let root = temp_root("missing-data-dir");
        assert!(validate_data_dir(root.join("data")).is_err());
    }
}
