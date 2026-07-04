CREATE TABLE [dbo].[Customer] (
    [CustomerId] int NOT NULL,
    [CustomerGuid] uniqueidentifier NOT NULL DEFAULT (NEWID()),
    [Email] nvarchar(320) NOT NULL,
    [FullName] nvarchar(200) NOT NULL,
    [CreditLimit] decimal(10, 2) NOT NULL DEFAULT (0),
    [CreatedAt] datetime2(0) NOT NULL DEFAULT ('2024-01-01T00:00:00'),
    [IsActive] bit NOT NULL DEFAULT (1),
    CONSTRAINT [PK_Customer] PRIMARY KEY ([CustomerId]),
    CONSTRAINT [UQ_Customer_Email] UNIQUE ([Email]),
    CONSTRAINT [CK_Customer_CreditLimit] CHECK ([CreditLimit] >= (0))
);

CREATE TABLE [dbo].[Order] (
    [OrderId] int NOT NULL,
    [CustomerId] int NOT NULL,
    [OrderNumber] nvarchar(40) NOT NULL,
    [OrderTotal] money NOT NULL DEFAULT (0),
    [OrderedAt] datetime2(0) NOT NULL,
    [Status] nvarchar(20) NOT NULL DEFAULT ('Pending'),
    CONSTRAINT [PK_Order] PRIMARY KEY ([OrderId]),
    CONSTRAINT [UQ_Order_OrderNumber] UNIQUE ([OrderNumber]),
    CONSTRAINT [CK_Order_Total] CHECK ([OrderTotal] >= (0))
);

CREATE TABLE [dbo].[OrderLine] (
    [OrderId] int NOT NULL,
    [LineNumber] int NOT NULL,
    [Sku] nvarchar(40) NOT NULL,
    [Description] nvarchar(200) NULL,
    [Quantity] int NOT NULL DEFAULT (1),
    [UnitPrice] decimal(10, 2) NOT NULL DEFAULT (0),
    CONSTRAINT [PK_OrderLine] PRIMARY KEY ([OrderId], [LineNumber]),
    CONSTRAINT [CK_OrderLine_Quantity] CHECK ([Quantity] > (0)),
    CONSTRAINT [CK_OrderLine_UnitPrice] CHECK ([UnitPrice] >= (0))
);

CREATE TABLE [sales].[Invoice] (
    [InvoiceId] int NOT NULL,
    [OrderId] int NOT NULL,
    [InvoiceNumber] nvarchar(40) NOT NULL,
    [InvoiceTotal] decimal(10, 2) NOT NULL DEFAULT (0),
    [IssuedAt] datetime2(0) NOT NULL,
    [PaidAt] datetime2(0) NULL,
    CONSTRAINT [PK_Invoice] PRIMARY KEY ([InvoiceId]),
    CONSTRAINT [UQ_Invoice_InvoiceNumber] UNIQUE ([InvoiceNumber]),
    CONSTRAINT [CK_Invoice_Total] CHECK ([InvoiceTotal] >= (0))
);

ALTER TABLE [dbo].[Order] ADD CONSTRAINT [FK_Order_Customer]
    FOREIGN KEY ([CustomerId]) REFERENCES [dbo].[Customer] ([CustomerId]);

ALTER TABLE [dbo].[OrderLine] ADD CONSTRAINT [FK_OrderLine_Order]
    FOREIGN KEY ([OrderId]) REFERENCES [dbo].[Order] ([OrderId]);

ALTER TABLE [sales].[Invoice] ADD CONSTRAINT [FK_Invoice_Order]
    FOREIGN KEY ([OrderId]) REFERENCES [dbo].[Order] ([OrderId]);

CREATE INDEX [IX_Order_CustomerId] ON [dbo].[Order] ([CustomerId]);
CREATE INDEX [IX_OrderLine_Sku] ON [dbo].[OrderLine] ([Sku]);
CREATE UNIQUE INDEX [UX_Invoice_OrderId] ON [sales].[Invoice] ([OrderId]);
