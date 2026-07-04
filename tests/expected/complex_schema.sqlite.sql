CREATE TABLE "dbo_Customer" (
    "CustomerId" INTEGER NOT NULL,
    "CustomerGuid" TEXT NOT NULL,
    "Email" TEXT NOT NULL,
    "FullName" TEXT NOT NULL,
    "CreditLimit" TEXT NOT NULL DEFAULT 0,
    "CreatedAt" TEXT NOT NULL DEFAULT '2024-01-01T00:00:00',
    "IsActive" INTEGER NOT NULL DEFAULT 1,
    CHECK ("IsActive" IN (0, 1)),
    PRIMARY KEY ("CustomerId"),
    UNIQUE ("Email"),
    CHECK (CreditLimit >=(0))
);

CREATE TABLE "dbo_Order" (
    "OrderId" INTEGER NOT NULL,
    "CustomerId" INTEGER NOT NULL,
    "OrderNumber" TEXT NOT NULL,
    "OrderTotal" TEXT NOT NULL DEFAULT 0,
    "OrderedAt" TEXT NOT NULL,
    "Status" TEXT NOT NULL DEFAULT 'Pending',
    PRIMARY KEY ("OrderId"),
    UNIQUE ("OrderNumber"),
    FOREIGN KEY ("CustomerId") REFERENCES "dbo_Customer" ("CustomerId"),
    CHECK (OrderTotal >=(0))
);

CREATE TABLE "dbo_OrderLine" (
    "OrderId" INTEGER NOT NULL,
    "LineNumber" INTEGER NOT NULL,
    "Sku" TEXT NOT NULL,
    "Description" TEXT,
    "Quantity" INTEGER NOT NULL DEFAULT 1,
    "UnitPrice" TEXT NOT NULL DEFAULT 0,
    PRIMARY KEY ("OrderId", "LineNumber"),
    FOREIGN KEY ("OrderId") REFERENCES "dbo_Order" ("OrderId"),
    CHECK (Quantity >(0)),
    CHECK (UnitPrice >=(0))
);

CREATE TABLE "sales_Invoice" (
    "InvoiceId" INTEGER NOT NULL,
    "OrderId" INTEGER NOT NULL,
    "InvoiceNumber" TEXT NOT NULL,
    "InvoiceTotal" TEXT NOT NULL DEFAULT 0,
    "IssuedAt" TEXT NOT NULL,
    "PaidAt" TEXT,
    PRIMARY KEY ("InvoiceId"),
    UNIQUE ("InvoiceNumber"),
    FOREIGN KEY ("OrderId") REFERENCES "dbo_Order" ("OrderId"),
    CHECK (InvoiceTotal >=(0))
);

CREATE INDEX "IX_Order_CustomerId" ON "dbo_Order" ("CustomerId");

CREATE INDEX "IX_OrderLine_Sku" ON "dbo_OrderLine" ("Sku");

CREATE UNIQUE INDEX "UX_Invoice_OrderId" ON "sales_Invoice" ("OrderId");
