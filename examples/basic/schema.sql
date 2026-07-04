CREATE TABLE [dbo].[Customer] (
    [CustomerId] int NOT NULL,
    [CustomerName] nvarchar(200) NOT NULL,
    [Email] nvarchar(255) NULL,
    [CreditLimit] decimal(10, 2) NOT NULL DEFAULT (0),
    [CreatedAt] datetime2(0) NOT NULL,
    [IsActive] bit NOT NULL DEFAULT (1),
    CONSTRAINT [PK_Customer] PRIMARY KEY ([CustomerId])
);

CREATE TABLE [dbo].[Order] (
    [OrderId] int NOT NULL,
    [CustomerId] int NOT NULL,
    [OrderTotal] decimal(10, 2) NOT NULL,
    [OrderedAt] datetime2(0) NOT NULL,
    [IsPaid] bit NOT NULL DEFAULT (0),
    [Notes] nvarchar(500) NULL,
    CONSTRAINT [PK_Order] PRIMARY KEY ([OrderId])
);

ALTER TABLE [dbo].[Order] ADD CONSTRAINT [FK_Order_Customer]
    FOREIGN KEY ([CustomerId]) REFERENCES [dbo].[Customer] ([CustomerId]);
