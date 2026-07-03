/// SQLite storage classes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StorageClass {
    Null,
    Integer,
    Real,
    Text,
    Blob,
}
