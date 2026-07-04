# Basic CorrodeQL fixture

This intentionally small fixture demonstrates the minimum relational shape needed for conversion tests: one parent table (`[dbo].[Customer]`) and one child table (`[dbo].[Order]`) with primary keys and a foreign key from orders to customers.

The schema includes representative SQL Server column types:

- `nvarchar` text columns (`CustomerName`, `Email`, `Notes`)
- `int` identifiers
- `decimal(10, 2)` monetary values
- `datetime2(0)` timestamps
- `bit` flags
- nullable text values, including blank CSV cells for `Email` and `Notes`

With the default `schema-prefix` table-name mode, the expected SQLite table names are:

- `dbo_Customer`
- `dbo_Order`

Expected row counts:

| Schema | Table | Rows |
| --- | --- | ---: |
| dbo | Customer | 2 |
| dbo | Order | 2 |

Sample conversion command:

```sh
corrodeql convert --schema examples/basic/schema.sql --data-dir examples/basic/data --out /tmp/basic.sqlite --overwrite
```
