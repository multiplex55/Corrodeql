mod app;
mod config;
mod data;
mod error;
mod logging;
mod mssql;
mod report;
mod schema;
mod sqlite;

fn main() {
    app::run();
}
