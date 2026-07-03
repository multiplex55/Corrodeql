use camino::Utf8PathBuf;
use rusqlite::Connection;
use serde::Serialize;

#[derive(Serialize)]
struct SmokeTestPayload {
    name: &'static str,
    count: u8,
}

#[test]
fn dependency_smoke_test() {
    let payload = SmokeTestPayload {
        name: "corrodeql",
        count: 1,
    };

    let serialized = serde_json::to_string(&payload).expect("payload should serialize to JSON");
    assert_eq!(serialized, r#"{"name":"corrodeql","count":1}"#);

    let path = Utf8PathBuf::from("input.csv");
    assert_eq!(path.as_str(), "input.csv");

    Connection::open_in_memory().expect("in-memory SQLite connection should open");
}
