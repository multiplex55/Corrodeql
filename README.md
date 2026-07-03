# CorrodeQL

CorrodeQL is a Rust CLI project scaffold for database schema and data tooling.

## Repository conventions

This repository uses modern Rust module layout conventions:

- No `mod.rs` files are allowed anywhere in the repository.
- Every module with child modules must use the sibling-file-plus-directory style.
- For example, a `schema` module with children should be organized as:
  - `src/schema.rs`
  - `src/schema/parser.rs`

## Development

Run the standard checks before submitting changes:

```sh
cargo check
cargo test
find . -name mod.rs
```
