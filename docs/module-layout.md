# Module Layout

CorrodeQL follows a Rust module layout convention that avoids `mod.rs`. Prefer explicit sibling module files because they make module ownership easier to see in directory listings and keep paths consistent as modules grow.

```text
Correct:

src/schema.rs
src/schema/parser.rs
src/schema/lexer.rs

Incorrect:

src/schema/mod.rs
```

## Convention for new modules

New modules should use a sibling `foo.rs` plus `foo/child.rs` layout:

- define the parent module in `src/foo.rs`;
- place child modules under `src/foo/`;
- expose child modules from `src/foo.rs` with normal `pub mod child;` or `mod child;` declarations;
- do not introduce `src/foo/mod.rs` for new module trees.

For example, a new `reports` module with formatter and writer children should use:

```text
src/reports.rs
src/reports/formatter.rs
src/reports/writer.rs
```

Future automation and Codex runs should preserve this convention. When adding, moving, or refactoring Rust modules, keep the non-`mod.rs` layout unless the project explicitly changes this policy.
