use std::fs;

use camino::Utf8Path;
use rusqlite::Connection;

use crate::error::{Error, Result};
use crate::sqlite::ddl::GeneratedDdl;

/// SQLite database handle returned after schema creation.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Database;

/// Opens a SQLite output database and executes generated DDL inside a transaction.
///
/// Existing output files are rejected unless `overwrite` is true. When overwrite
/// is enabled, the existing file is removed only after path validation and DDL
/// generation have already succeeded.
pub fn open_output_database(
    path: impl AsRef<Utf8Path>,
    overwrite: bool,
    ddl: &GeneratedDdl,
) -> Result<Database> {
    drop(open_output_connection(path, overwrite, ddl)?);
    Ok(Database)
}

/// Creates an empty SQLite output database and returns the live connection.
pub fn create_output_connection(path: impl AsRef<Utf8Path>, overwrite: bool) -> Result<Connection> {
    let path = path.as_ref();
    validate_output_path(path, overwrite)?;

    if path.exists() {
        fs::remove_file(path.as_std_path())?;
    }

    let connection = Connection::open(path.as_std_path())?;
    connection.pragma_update(None, "foreign_keys", "ON")?;
    Ok(connection)
}

/// Opens a SQLite output database and returns the live connection after schema creation.
pub fn open_output_connection(
    path: impl AsRef<Utf8Path>,
    overwrite: bool,
    ddl: &GeneratedDdl,
) -> Result<Connection> {
    let path = path.as_ref();
    validate_output_path(path, overwrite)?;

    if path.exists() {
        fs::remove_file(path.as_std_path())?;
    }

    let mut connection = Connection::open(path.as_std_path())?;
    connection.pragma_update(None, "foreign_keys", "ON")?;

    let transaction = connection.transaction()?;
    for statement in &ddl.statements {
        transaction.execute_batch(&statement.0)?;
    }
    transaction.commit()?;

    Ok(connection)
}

fn validate_output_path(path: &Utf8Path, overwrite: bool) -> Result<()> {
    if let Some(parent) = path.parent().filter(|parent| !parent.as_str().is_empty()) {
        if !parent.exists() {
            return Err(Error::InvalidPath {
                kind: "output_db_path",
                path: path.to_path_buf(),
                reason: "parent directory does not exist",
            });
        }
        if !parent.is_dir() {
            return Err(Error::InvalidPath {
                kind: "output_db_path",
                path: path.to_path_buf(),
                reason: "parent path is not a directory",
            });
        }
    }

    if path.exists() && !overwrite {
        return Err(Error::InvalidPath {
            kind: "output_db_path",
            path: path.to_path_buf(),
            reason: "path already exists and overwrite is false",
        });
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    use camino::Utf8PathBuf;
    use rusqlite::Connection;

    use super::*;
    use crate::config::options::ConvertOptions;
    use crate::schema::parser;
    use crate::sqlite::ddl;

    fn temp_root(name: &str) -> Utf8PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = Utf8PathBuf::from_path_buf(
            std::env::temp_dir().join(format!("corrodeql-db-{name}-{unique}")),
        )
        .unwrap();
        fs::create_dir_all(&path).unwrap();
        path
    }

    fn generated_ddl() -> GeneratedDdl {
        let schema = parser::parse(
            "CREATE TABLE Parent (Id int NOT NULL, CONSTRAINT PK_Parent PRIMARY KEY (Id));\
             CREATE TABLE Child (Id int NOT NULL, ParentId int NOT NULL,\
             CONSTRAINT FK_Child_Parent FOREIGN KEY (ParentId) REFERENCES Parent (Id));",
        );
        ddl::generate(&schema, &ConvertOptions::default()).unwrap()
    }

    #[test]
    fn executes_generated_ddl_in_memory() {
        let generated = generated_ddl();
        let mut connection = Connection::open_in_memory().unwrap();
        connection
            .pragma_update(None, "foreign_keys", "ON")
            .unwrap();
        let transaction = connection.transaction().unwrap();
        for statement in &generated.statements {
            transaction.execute_batch(&statement.0).unwrap();
        }
        transaction.commit().unwrap();

        let count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name IN ('dbo_Parent', 'dbo_Child')",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 2);
    }

    #[test]
    fn enables_foreign_keys() {
        let root = temp_root("foreign-keys");
        let db = root.join("out.sqlite");
        let connection = open_output_connection(&db, false, &generated_ddl()).unwrap();
        let enabled: i64 = connection
            .query_row("PRAGMA foreign_keys", [], |row| row.get(0))
            .unwrap();
        assert_eq!(enabled, 1);
    }

    #[test]
    fn rejects_existing_output_without_overwrite() {
        let root = temp_root("reject-existing");
        let db = root.join("out.sqlite");
        fs::write(&db, b"existing").unwrap();

        assert!(open_output_database(&db, false, &generated_ddl()).is_err());
        assert_eq!(fs::read(&db).unwrap(), b"existing");
    }

    #[test]
    fn overwrites_existing_output_after_validation() {
        let root = temp_root("overwrite-existing");
        let db = root.join("out.sqlite");
        fs::write(&db, b"existing").unwrap();

        let connection = open_output_connection(&db, true, &generated_ddl()).unwrap();
        let count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 2);
    }
}
