-- SSMS-style export with comments, GO batches, and unsupported statements.
CREATE TABLE [dbo].[Customer] (
    [CustomerId] int NOT NULL,
    [Email] nvarchar(320) NOT NULL,
    [CreditLimit] decimal(19, 4) NULL,
    [Status] varchar(20) NOT NULL,
    CONSTRAINT [PK_Customer] PRIMARY KEY CLUSTERED ([CustomerId]),
    CONSTRAINT [UQ_Customer_Email] UNIQUE ([Email])
);
GO
/* A second table receives constraints from ALTER TABLE batches. */
CREATE TABLE [dbo].[Order] (
    [OrderId] int NOT NULL,
    [CustomerId] int NOT NULL,
    [OrderTotal] numeric(12, 2) NOT NULL,
    [CreatedAt] datetime2(7) NOT NULL,
    CONSTRAINT [PK_Order] PRIMARY KEY ([OrderId])
);
GO
ALTER TABLE [dbo].[Order] WITH CHECK ADD CONSTRAINT [FK_Order_Customer]
    FOREIGN KEY ([CustomerId]) REFERENCES [dbo].[Customer] ([CustomerId])
    ON DELETE CASCADE;
GO
ALTER TABLE [dbo].[Customer] ADD CONSTRAINT [DF_Customer_Status]
    DEFAULT ('active') FOR [Status];
GO
ALTER TABLE [dbo].[Order] ADD CONSTRAINT [CK_Order_Total]
    CHECK ([OrderTotal] >= (0));
GO
CREATE INDEX [IX_Order_CustomerId] ON [dbo].[Order] ([CustomerId]);
GO
CREATE UNIQUE INDEX [UX_Order_Customer_Total] ON [dbo].[Order] ([CustomerId], [OrderTotal]);
GO
CREATE VIEW [dbo].[CustomerSummary] AS SELECT [CustomerId] FROM [dbo].[Customer];
GO
FROBULATE [dbo].[Customer];
GO
