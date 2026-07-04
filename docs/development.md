# Development

This document captures the local CI-like validation sequence for CorrodeQL development.

## Local validation sequence

Run the following commands before opening pull requests and after changes that can affect runtime behavior:

```bash
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test
cargo build
```

Run the full sequence:

- before opening PRs;
- after CLI changes;
- after parser changes;
- after SQLite DDL generation changes;
- after CSV import changes;
- after validation changes;
- after any other Rust code changes that may affect behavior.

Documentation-only changes may not require the full Rust validation sequence, but code changes should run it before review.

## Optional debugging

When tests need additional output for debugging, run:

```bash
cargo test -- --nocapture
```

Use `-- --nocapture` when printed diagnostic output is useful for understanding a failing test or investigating parser, DDL, import, or validation behavior.
