# Architecture

CorrodeQL is organized as a layered conversion and validation pipeline. Each layer accepts focused inputs, performs one stage of work, and passes structured output to the next layer. This keeps command-line handling, schema understanding, SQLite rendering, data import, validation, and reporting concerns independent.

```text
CLI / Interactive Input
        ↓
ConvertOptions
        ↓
Schema Preprocessor
        ↓
Schema Parser
        ↓
DatabaseSchema Model
        ↓
SQLite DDL Generator
        ↓
CSV Importer
        ↓
Validator
        ↓
Reports
```

## Layer responsibilities

### CLI / Interactive Input

The CLI and interactive prompts collect user intent: source schema paths, CSV input locations, output database paths, validation preferences, and any flags that alter conversion behavior. This layer should focus on user experience, argument parsing, prompting, and clear error messages for invalid user-supplied options.

### ConvertOptions

`ConvertOptions` is the normalized configuration passed into the conversion workflow. It translates CLI or interactive choices into explicit, programmatic settings so downstream layers do not need to know whether a value came from a flag, prompt, default, or future integration point.

### Schema Preprocessor

The schema preprocessor prepares raw SQL Server schema text for parsing. It may normalize input, remove or handle unsupported wrappers, split batches, and preserve enough context for useful diagnostics. Its job is to make parser input predictable without changing the intended schema semantics.

### Schema Parser

The schema parser reads the preprocessed SQL Server schema and extracts schema intent into structured data. It should identify database objects, tables, columns, data types, nullability, keys, defaults, relationships, and other supported schema features while reporting unsupported or ambiguous constructs clearly.

### DatabaseSchema Model

`DatabaseSchema` is the neutral in-memory representation of the parsed schema. It is not SQLite DDL and should not be treated as a string rendering target. Instead, it captures the SQL Server schema intent in a form that can be inspected, validated, tested, diagnosed, and rendered into one or more output formats.

### SQLite DDL Generator

The SQLite DDL generator renders the neutral `DatabaseSchema` model into target-specific SQLite statements. This layer handles SQLite type mapping, table creation syntax, constraints, indexes, and any SQLite-specific compatibility decisions required to create the output database.

### CSV Importer

The CSV importer loads source CSV data into the generated SQLite schema. It is responsible for reading records, mapping CSV columns to database columns, applying configured import behavior, surfacing row-level import problems, and ensuring data is inserted into the expected target tables.

### Validator

The validator checks the generated database and imported data against expected structural and data-quality rules. It may verify schema shape, table availability, row counts, constraint-related expectations, import completeness, and other conversion correctness signals.

### Reports

Reports summarize the conversion result for users. They should present successful actions, warnings, validation results, unsupported schema features, import issues, and actionable next steps in a form suitable for CLI output, logs, or future report formats.

## Why parsing and SQLite DDL generation are separate

Schema parsing and SQLite DDL generation are intentionally separate stages. Parsing should preserve SQL Server schema intent in a neutral `DatabaseSchema` model rather than immediately producing SQLite-specific text. SQLite generation is a target-specific rendering step that interprets that model for SQLite syntax and behavior.

This separation gives CorrodeQL important flexibility and testability benefits:

- the parsed schema can be inspected before rendering;
- diagnostics can point to SQL Server schema intent instead of only generated SQLite text;
- alternate outputs can be added later without rewriting the parser;
- parser behavior and SQLite rendering behavior can be tested in isolation;
- conversion decisions are easier to review because semantic extraction and target-specific rendering are not mixed together.
