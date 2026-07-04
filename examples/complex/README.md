# Complex CorrodeQL fixture

This fixture exercises a broader set of SQL Server schema features while remaining small enough for integration tests.

## Tables and coverage

- `[dbo].[Customer]` demonstrates a single-column primary key, `uniqueidentifier`, a unique email constraint, decimal credit limits, defaults, a check constraint, and a `bit` flag.
- `[dbo].[Order]` intentionally uses the reserved table name `Order`; it demonstrates a primary key, a unique order number, `money`, defaults, a check constraint, and a foreign key to `[dbo].[Customer]`.
- `[dbo].[OrderLine]` demonstrates a composite primary key on `([OrderId], [LineNumber])`, nullable CSV text, positive quantity validation, non-negative decimal unit prices, and a foreign key to `[dbo].[Order]`.
- `[sales].[Invoice]` demonstrates a second schema, a unique invoice number, nullable datetime values, non-negative totals, and a foreign key to `[dbo].[Order]`.

With the default `schema-prefix` table-name mode, the expected SQLite table names are:

- `dbo_Customer`
- `dbo_Order`
- `dbo_OrderLine`
- `sales_Invoice`

Expected row counts:

| Schema | Table | Rows |
| --- | --- | ---: |
| dbo | Customer | 2 |
| dbo | Order | 2 |
| dbo | OrderLine | 3 |
| sales | Invoice | 2 |

Expected constraints and validation behavior:

- Primary keys, including the composite `dbo_OrderLine` key, should be emitted to SQLite.
- Unique constraints and unique indexes should reject duplicate customer emails, order numbers, invoice numbers, and invoice order references.
- Foreign-key validation should pass for the supplied rows.
- Check constraints should reject negative totals, negative unit prices, and non-positive quantities.
- Default constraints are represented in the converted schema for columns where SQLite can safely preserve them.

Sample conversion command:

```sh
corrodeql convert --schema examples/complex/schema.sql --data-dir examples/complex/data --out /tmp/complex.sqlite --overwrite
```
