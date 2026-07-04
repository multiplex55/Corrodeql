CREATE TABLE "dbo_Customer" (
    "CustomerId" INTEGER NOT NULL,
    "CustomerName" TEXT NOT NULL,
    "Email" TEXT,
    "CreditLimit" TEXT NOT NULL DEFAULT 0,
    "CreatedAt" TEXT NOT NULL,
    "IsActive" INTEGER NOT NULL DEFAULT 1,
    CHECK ("IsActive" IN (0, 1)),
    PRIMARY KEY ("CustomerId")
);

CREATE TABLE "dbo_Order" (
    "OrderId" INTEGER NOT NULL,
    "CustomerId" INTEGER NOT NULL,
    "OrderTotal" TEXT NOT NULL,
    "OrderedAt" TEXT NOT NULL,
    "IsPaid" INTEGER NOT NULL DEFAULT 0,
    "Notes" TEXT,
    CHECK ("IsPaid" IN (0, 1)),
    PRIMARY KEY ("OrderId"),
    FOREIGN KEY ("CustomerId") REFERENCES "dbo_Customer" ("CustomerId")
);
