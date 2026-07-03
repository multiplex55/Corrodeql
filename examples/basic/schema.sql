CREATE TABLE [dbo].[Customer] (
    [CustomerId] int NOT NULL,
    [Email] nvarchar(255) NOT NULL,
    [FullName] nvarchar(200) NOT NULL,
    [Phone] nvarchar(50) NULL,
    [IsActive] tinyint NOT NULL DEFAULT (1),
    [CreatedDate] datetime2(0) NOT NULL DEFAULT ('2024-01-01T00:00:00'),
    CONSTRAINT [PK_Customer] PRIMARY KEY ([CustomerId]),
    CONSTRAINT [UQ_Customer_Email] UNIQUE ([Email])
);

CREATE TABLE [dbo].[Order] (
    [OrderId] int NOT NULL,
    [CustomerId] int NOT NULL,
    [OrderDate] date NOT NULL DEFAULT ('2024-01-01'),
    [Status] nvarchar(20) NOT NULL DEFAULT ('Pending'),
    [Notes] nvarchar(500) NULL,
    CONSTRAINT [PK_Order] PRIMARY KEY ([OrderId])
);

CREATE TABLE [dbo].[OrderLine] (
    [OrderId] int NOT NULL,
    [LineNumber] int NOT NULL,
    [Sku] nvarchar(40) NOT NULL,
    [Description] nvarchar(200) NULL,
    [Quantity] int NOT NULL DEFAULT (1),
    [UnitPrice] decimal(10, 2) NOT NULL DEFAULT (0),
    CONSTRAINT [PK_OrderLine] PRIMARY KEY ([OrderId], [LineNumber])
);

ALTER TABLE [dbo].[Order] ADD CONSTRAINT [FK_Order_Customer]
    FOREIGN KEY ([CustomerId]) REFERENCES [dbo].[Customer] ([CustomerId]);

ALTER TABLE [dbo].[OrderLine] ADD CONSTRAINT [FK_OrderLine_Order]
    FOREIGN KEY ([OrderId]) REFERENCES [dbo].[Order] ([OrderId]);

CREATE INDEX [IX_Order_CustomerId] ON [dbo].[Order] ([CustomerId]);
