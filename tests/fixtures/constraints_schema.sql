CREATE TABLE [dbo].[Customer] (
    [Id] int NOT NULL,
    [Email] nvarchar(100) NOT NULL,
    CONSTRAINT [PK_Customer] PRIMARY KEY ([Id]),
    CONSTRAINT [UQ_Customer_Email] UNIQUE ([Email])
);
CREATE TABLE [dbo].[Order] (
    [Id] int NOT NULL,
    [CustomerId] int NOT NULL,
    CONSTRAINT [PK_Order] PRIMARY KEY ([Id])
);
ALTER TABLE [dbo].[Order] ADD CONSTRAINT [FK_Order_Customer]
    FOREIGN KEY ([CustomerId]) REFERENCES [dbo].[Customer] ([Id]);
