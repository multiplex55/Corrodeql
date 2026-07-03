/*
    Sample SQL Server / SSMS-style schema export for testing MSSQL -> SQLite conversion.

    This file intentionally includes common and awkward SQL Server schema features:
    - Multiple schemas
    - Identity columns
    - Simple and composite primary keys
    - Unique constraints
    - Foreign keys emitted after CREATE TABLE
    - Multiple self-referencing foreign keys
    - Default constraints emitted as ALTER TABLE
    - Check constraints
    - Computed columns
    - rowversion/timestamp
    - decimal/money/numeric
    - date/time/datetime/datetime2/datetimeoffset
    - uniqueidentifier
    - varbinary(max)
    - xml
    - nvarchar(max)
    - filtered indexes
    - included-column indexes
    - a view and trigger that a data-only SQLite converter may choose to skip
*/

SET ANSI_NULLS ON
GO
SET QUOTED_IDENTIFIER ON
GO

CREATE SCHEMA [ref]
GO
CREATE SCHEMA [audit]
GO

CREATE TYPE [dbo].[PhoneNumber] FROM [nvarchar](25) NULL
GO

CREATE SEQUENCE [dbo].[GlobalSequence]
    AS [bigint]
    START WITH 1000
    INCREMENT BY 1
    MINVALUE 1000
    NO MAXVALUE
    CACHE 50
GO

CREATE TABLE [dbo].[Tenant](
    [TenantId] [int] IDENTITY(1,1) NOT NULL,
    [TenantGuid] [uniqueidentifier] NOT NULL,
    [TenantCode] [nvarchar](50) NOT NULL,
    [TenantName] [nvarchar](200) NOT NULL,
    [IsActive] [bit] NOT NULL,
    [CreatedAt] [datetime2](7) NOT NULL,
    [UpdatedAt] [datetime2](7) NULL,
    [RowVer] [rowversion] NOT NULL,
    CONSTRAINT [PK_Tenant] PRIMARY KEY CLUSTERED
    (
        [TenantId] ASC
    ) WITH (PAD_INDEX = OFF, STATISTICS_NORECOMPUTE = OFF, IGNORE_DUP_KEY = OFF,
            ALLOW_ROW_LOCKS = ON, ALLOW_PAGE_LOCKS = ON, OPTIMIZE_FOR_SEQUENTIAL_KEY = OFF) ON [PRIMARY],
    CONSTRAINT [UQ_Tenant_TenantCode] UNIQUE NONCLUSTERED
    (
        [TenantCode] ASC
    ) ON [PRIMARY],
    CONSTRAINT [UQ_Tenant_TenantGuid] UNIQUE NONCLUSTERED
    (
        [TenantGuid] ASC
    ) ON [PRIMARY]
) ON [PRIMARY]
GO

CREATE TABLE [dbo].[AppUser](
    [UserId] [int] IDENTITY(1,1) NOT NULL,
    [TenantId] [int] NOT NULL,
    [ExternalUserGuid] [uniqueidentifier] NOT NULL,
    [UserName] [nvarchar](100) NOT NULL,
    [EmailAddress] [nvarchar](320) NULL,
    [DisplayName] [nvarchar](200) NOT NULL,
    [Phone] [dbo].[PhoneNumber] NULL,
    [PasswordHash] [varbinary](256) NULL,
    [IsLocked] [bit] NOT NULL,
    [FailedLoginCount] [tinyint] NOT NULL,
    [LastLoginAt] [datetimeoffset](7) NULL,
    [CreatedAt] [datetime2](7) NOT NULL,
    [RowVer] [rowversion] NOT NULL,
    CONSTRAINT [PK_AppUser] PRIMARY KEY CLUSTERED
    (
        [UserId] ASC
    ) ON [PRIMARY],
    CONSTRAINT [UQ_AppUser_Tenant_UserName] UNIQUE NONCLUSTERED
    (
        [TenantId] ASC,
        [UserName] ASC
    ) ON [PRIMARY],
    CONSTRAINT [UQ_AppUser_Tenant_Email] UNIQUE NONCLUSTERED
    (
        [TenantId] ASC,
        [EmailAddress] ASC
    ) ON [PRIMARY]
) ON [PRIMARY]
GO

CREATE TABLE [ref].[NodeType](
    [NodeTypeId] [smallint] NOT NULL,
    [NodeTypeCode] [varchar](40) NOT NULL,
    [NodeTypeName] [nvarchar](100) NOT NULL,
    [SortOrder] [int] NOT NULL,
    CONSTRAINT [PK_NodeType] PRIMARY KEY CLUSTERED
    (
        [NodeTypeId] ASC
    ) ON [PRIMARY],
    CONSTRAINT [UQ_NodeType_Code] UNIQUE NONCLUSTERED
    (
        [NodeTypeCode] ASC
    ) ON [PRIMARY]
) ON [PRIMARY]
GO

CREATE TABLE [ref].[NodeStatus](
    [NodeStatusId] [tinyint] NOT NULL,
    [StatusCode] [varchar](40) NOT NULL,
    [StatusName] [nvarchar](100) NOT NULL,
    [IsTerminal] [bit] NOT NULL,
    CONSTRAINT [PK_NodeStatus] PRIMARY KEY CLUSTERED
    (
        [NodeStatusId] ASC
    ) ON [PRIMARY],
    CONSTRAINT [UQ_NodeStatus_Code] UNIQUE NONCLUSTERED
    (
        [StatusCode] ASC
    ) ON [PRIMARY]
) ON [PRIMARY]
GO

CREATE TABLE [ref].[Category](
    [CategoryId] [int] IDENTITY(1,1) NOT NULL,
    [TenantId] [int] NOT NULL,
    [ParentCategoryId] [int] NULL,
    [CategoryCode] [nvarchar](50) NOT NULL,
    [CategoryName] [nvarchar](200) NOT NULL,
    [PathText] [nvarchar](900) NULL,
    CONSTRAINT [PK_Category] PRIMARY KEY CLUSTERED
    (
        [CategoryId] ASC
    ) ON [PRIMARY],
    CONSTRAINT [UQ_Category_Tenant_Code] UNIQUE NONCLUSTERED
    (
        [TenantId] ASC,
        [CategoryCode] ASC
    ) ON [PRIMARY]
) ON [PRIMARY]
GO

CREATE TABLE [ref].[Currency](
    [CurrencyCode] [char](3) NOT NULL,
    [CurrencyName] [nvarchar](100) NOT NULL,
    [MinorUnit] [tinyint] NOT NULL,
    CONSTRAINT [PK_Currency] PRIMARY KEY CLUSTERED
    (
        [CurrencyCode] ASC
    ) ON [PRIMARY]
) ON [PRIMARY]
GO

CREATE TABLE [dbo].[Location](
    [LocationId] [int] IDENTITY(1,1) NOT NULL,
    [TenantId] [int] NOT NULL,
    [LocationCode] [nvarchar](50) NOT NULL,
    [LocationName] [nvarchar](200) NOT NULL,
    [AddressLine1] [nvarchar](200) NULL,
    [AddressLine2] [nvarchar](200) NULL,
    [City] [nvarchar](100) NULL,
    [StateProvince] [nvarchar](100) NULL,
    [PostalCode] [nvarchar](30) NULL,
    [CountryCode] [char](2) NULL,
    [Latitude] [decimal](9,6) NULL,
    [Longitude] [decimal](9,6) NULL,
    [GeoJson] [nvarchar](max) NULL,
    CONSTRAINT [PK_Location] PRIMARY KEY CLUSTERED
    (
        [LocationId] ASC
    ) ON [PRIMARY],
    CONSTRAINT [UQ_Location_Tenant_Code] UNIQUE NONCLUSTERED
    (
        [TenantId] ASC,
        [LocationCode] ASC
    ) ON [PRIMARY]
) ON [PRIMARY] TEXTIMAGE_ON [PRIMARY]
GO

CREATE TABLE [dbo].[Node](
    [NodeId] [bigint] IDENTITY(1,1) NOT NULL,
    [TenantId] [int] NOT NULL,
    [NodeGuid] [uniqueidentifier] NOT NULL,
    [NodeTypeId] [smallint] NOT NULL,
    [NodeStatusId] [tinyint] NOT NULL,
    [CategoryId] [int] NULL,
    [OwnerUserId] [int] NULL,
    [CreatedByUserId] [int] NOT NULL,
    [UpdatedByUserId] [int] NULL,
    [PrimaryLocationId] [int] NULL,

    [ParentNodeId] [bigint] NULL,
    [RootNodeId] [bigint] NULL,
    [PreviousNodeId] [bigint] NULL,
    [ReplacementNodeId] [bigint] NULL,
    [TemplateNodeId] [bigint] NULL,

    [NodeCode] [nvarchar](80) NOT NULL,
    [NodeName] [nvarchar](300) NOT NULL,
    [ShortName] [nvarchar](80) NULL,
    [Description] [nvarchar](max) NULL,
    [LegacyText] [text] NULL,

    [QuantityOnHand] [decimal](18,4) NOT NULL,
    [QuantityReserved] [numeric](18,4) NOT NULL,
    [UnitCost] [money] NULL,
    [UnitPrice] [decimal](19,4) NULL,
    [WeightKg] [float] NULL,
    [TemperatureCelsius] [real] NULL,

    [IsActive] [bit] NOT NULL,
    [IsDeleted] [bit] NOT NULL,
    [Priority] [tinyint] NOT NULL,
    [Rating] [smallint] NULL,

    [EffectiveDate] [date] NULL,
    [StartTime] [time](7) NULL,
    [CreatedAt] [datetime2](7) NOT NULL,
    [UpdatedAt] [datetime] NULL,
    [ClosedAt] [smalldatetime] NULL,
    [ExternalTimestamp] [datetimeoffset](7) NULL,

    [AttributesXml] [xml] NULL,
    [MetadataJson] [nvarchar](max) NULL,
    [Payload] [varbinary](max) NULL,
    [FixedHash] [binary](32) NULL,

    [ComputedAvailableQty] AS ([QuantityOnHand]-[QuantityReserved]) PERSISTED,
    [CodeAndName] AS (([NodeCode]+N' - ')+[NodeName]),

    [RowVer] [rowversion] NOT NULL,

    CONSTRAINT [PK_Node] PRIMARY KEY CLUSTERED
    (
        [NodeId] ASC
    ) WITH (PAD_INDEX = OFF, STATISTICS_NORECOMPUTE = OFF, IGNORE_DUP_KEY = OFF,
            ALLOW_ROW_LOCKS = ON, ALLOW_PAGE_LOCKS = ON) ON [PRIMARY],

    CONSTRAINT [UQ_Node_Tenant_NodeCode] UNIQUE NONCLUSTERED
    (
        [TenantId] ASC,
        [NodeCode] ASC
    ) ON [PRIMARY],

    CONSTRAINT [UQ_Node_NodeGuid] UNIQUE NONCLUSTERED
    (
        [NodeGuid] ASC
    ) ON [PRIMARY],

    CONSTRAINT [CK_Node_Quantity_NonNegative] CHECK
    (
        [QuantityOnHand] >= (0) AND [QuantityReserved] >= (0)
    ),

    CONSTRAINT [CK_Node_Priority_Range] CHECK
    (
        [Priority] BETWEEN (0) AND (9)
    )
) ON [PRIMARY] TEXTIMAGE_ON [PRIMARY]
GO

CREATE TABLE [dbo].[NodeAudit](
    [NodeAuditId] [bigint] IDENTITY(1,1) NOT NULL,
    [NodeId] [bigint] NOT NULL,
    [AuditAction] [varchar](20) NOT NULL,
    [ChangedByUserId] [int] NULL,
    [ChangedAt] [datetime2](7) NOT NULL,
    [BeforeJson] [nvarchar](max) NULL,
    [AfterJson] [nvarchar](max) NULL,
    CONSTRAINT [PK_NodeAudit] PRIMARY KEY CLUSTERED
    (
        [NodeAuditId] ASC
    ) ON [PRIMARY],
    CONSTRAINT [CK_NodeAudit_Action] CHECK
    (
        [AuditAction] IN ('INSERT','UPDATE','DELETE')
    )
) ON [PRIMARY] TEXTIMAGE_ON [PRIMARY]
GO

CREATE TABLE [dbo].[Tag](
    [TagId] [int] IDENTITY(1,1) NOT NULL,
    [TenantId] [int] NOT NULL,
    [TagName] [nvarchar](100) NOT NULL,
    CONSTRAINT [PK_Tag] PRIMARY KEY CLUSTERED
    (
        [TagId] ASC
    ) ON [PRIMARY],
    CONSTRAINT [UQ_Tag_Tenant_Name] UNIQUE NONCLUSTERED
    (
        [TenantId] ASC,
        [TagName] ASC
    ) ON [PRIMARY]
) ON [PRIMARY]
GO

CREATE TABLE [dbo].[NodeTag](
    [NodeId] [bigint] NOT NULL,
    [TagId] [int] NOT NULL,
    [TaggedAt] [datetime2](7) NOT NULL,
    [TaggedByUserId] [int] NULL,
    CONSTRAINT [PK_NodeTag] PRIMARY KEY CLUSTERED
    (
        [NodeId] ASC,
        [TagId] ASC
    ) ON [PRIMARY]
) ON [PRIMARY]
GO

CREATE TABLE [dbo].[PriceBook](
    [TenantId] [int] NOT NULL,
    [PriceBookCode] [nvarchar](50) NOT NULL,
    [CurrencyCode] [char](3) NOT NULL,
    [Description] [nvarchar](200) NULL,
    [ValidFrom] [date] NOT NULL,
    [ValidTo] [date] NULL,
    CONSTRAINT [PK_PriceBook] PRIMARY KEY CLUSTERED
    (
        [TenantId] ASC,
        [PriceBookCode] ASC
    ) ON [PRIMARY]
) ON [PRIMARY]
GO

CREATE TABLE [dbo].[PriceBookLine](
    [TenantId] [int] NOT NULL,
    [PriceBookCode] [nvarchar](50) NOT NULL,
    [NodeId] [bigint] NOT NULL,
    [UnitPrice] [decimal](19,4) NOT NULL,
    [MinimumQuantity] [decimal](18,4) NOT NULL,
    CONSTRAINT [PK_PriceBookLine] PRIMARY KEY CLUSTERED
    (
        [TenantId] ASC,
        [PriceBookCode] ASC,
        [NodeId] ASC
    ) ON [PRIMARY],
    CONSTRAINT [CK_PriceBookLine_MinQty] CHECK ([MinimumQuantity] > (0))
) ON [PRIMARY]
GO

CREATE TABLE [audit].[ImportBatch](
    [ImportBatchId] [bigint] IDENTITY(1,1) NOT NULL,
    [BatchGuid] [uniqueidentifier] NOT NULL,
    [SourceSystem] [nvarchar](100) NOT NULL,
    [FileName] [nvarchar](260) NULL,
    [StartedAt] [datetime2](7) NOT NULL,
    [CompletedAt] [datetime2](7) NULL,
    [RowsRead] [int] NOT NULL,
    [RowsInserted] [int] NOT NULL,
    [RowsFailed] [int] NOT NULL,
    [ErrorSummary] [nvarchar](max) NULL,
    CONSTRAINT [PK_ImportBatch] PRIMARY KEY CLUSTERED
    (
        [ImportBatchId] ASC
    ) ON [PRIMARY],
    CONSTRAINT [UQ_ImportBatch_BatchGuid] UNIQUE NONCLUSTERED
    (
        [BatchGuid] ASC
    ) ON [PRIMARY]
) ON [PRIMARY] TEXTIMAGE_ON [PRIMARY]
GO

CREATE TABLE [dbo].[AppSetting](
    [SettingKey] [nvarchar](150) NOT NULL,
    [SettingValue] [nvarchar](max) NULL,
    [IsSecret] [bit] NOT NULL,
    [UpdatedAt] [datetime2](7) NOT NULL,
    CONSTRAINT [PK_AppSetting] PRIMARY KEY CLUSTERED
    (
        [SettingKey] ASC
    ) ON [PRIMARY]
) ON [PRIMARY] TEXTIMAGE_ON [PRIMARY]
GO

ALTER TABLE [dbo].[Tenant] ADD CONSTRAINT [DF_Tenant_TenantGuid] DEFAULT (newid()) FOR [TenantGuid]
GO
ALTER TABLE [dbo].[Tenant] ADD CONSTRAINT [DF_Tenant_IsActive] DEFAULT ((1)) FOR [IsActive]
GO
ALTER TABLE [dbo].[Tenant] ADD CONSTRAINT [DF_Tenant_CreatedAt] DEFAULT (sysutcdatetime()) FOR [CreatedAt]
GO

ALTER TABLE [dbo].[AppUser] ADD CONSTRAINT [DF_AppUser_ExternalUserGuid] DEFAULT (newid()) FOR [ExternalUserGuid]
GO
ALTER TABLE [dbo].[AppUser] ADD CONSTRAINT [DF_AppUser_IsLocked] DEFAULT ((0)) FOR [IsLocked]
GO
ALTER TABLE [dbo].[AppUser] ADD CONSTRAINT [DF_AppUser_FailedLoginCount] DEFAULT ((0)) FOR [FailedLoginCount]
GO
ALTER TABLE [dbo].[AppUser] ADD CONSTRAINT [DF_AppUser_CreatedAt] DEFAULT (sysutcdatetime()) FOR [CreatedAt]
GO

ALTER TABLE [ref].[NodeStatus] ADD CONSTRAINT [DF_NodeStatus_IsTerminal] DEFAULT ((0)) FOR [IsTerminal]
GO

ALTER TABLE [dbo].[Node] ADD CONSTRAINT [DF_Node_NodeGuid] DEFAULT (newid()) FOR [NodeGuid]
GO
ALTER TABLE [dbo].[Node] ADD CONSTRAINT [DF_Node_QtyOnHand] DEFAULT ((0)) FOR [QuantityOnHand]
GO
ALTER TABLE [dbo].[Node] ADD CONSTRAINT [DF_Node_QtyReserved] DEFAULT ((0)) FOR [QuantityReserved]
GO
ALTER TABLE [dbo].[Node] ADD CONSTRAINT [DF_Node_IsActive] DEFAULT ((1)) FOR [IsActive]
GO
ALTER TABLE [dbo].[Node] ADD CONSTRAINT [DF_Node_IsDeleted] DEFAULT ((0)) FOR [IsDeleted]
GO
ALTER TABLE [dbo].[Node] ADD CONSTRAINT [DF_Node_Priority] DEFAULT ((5)) FOR [Priority]
GO
ALTER TABLE [dbo].[Node] ADD CONSTRAINT [DF_Node_CreatedAt] DEFAULT (sysutcdatetime()) FOR [CreatedAt]
GO

ALTER TABLE [dbo].[NodeAudit] ADD CONSTRAINT [DF_NodeAudit_ChangedAt] DEFAULT (sysutcdatetime()) FOR [ChangedAt]
GO

ALTER TABLE [dbo].[NodeTag] ADD CONSTRAINT [DF_NodeTag_TaggedAt] DEFAULT (sysutcdatetime()) FOR [TaggedAt]
GO

ALTER TABLE [audit].[ImportBatch] ADD CONSTRAINT [DF_ImportBatch_BatchGuid] DEFAULT (newid()) FOR [BatchGuid]
GO
ALTER TABLE [audit].[ImportBatch] ADD CONSTRAINT [DF_ImportBatch_StartedAt] DEFAULT (sysutcdatetime()) FOR [StartedAt]
GO
ALTER TABLE [audit].[ImportBatch] ADD CONSTRAINT [DF_ImportBatch_RowsRead] DEFAULT ((0)) FOR [RowsRead]
GO
ALTER TABLE [audit].[ImportBatch] ADD CONSTRAINT [DF_ImportBatch_RowsInserted] DEFAULT ((0)) FOR [RowsInserted]
GO
ALTER TABLE [audit].[ImportBatch] ADD CONSTRAINT [DF_ImportBatch_RowsFailed] DEFAULT ((0)) FOR [RowsFailed]
GO

ALTER TABLE [dbo].[AppSetting] ADD CONSTRAINT [DF_AppSetting_IsSecret] DEFAULT ((0)) FOR [IsSecret]
GO
ALTER TABLE [dbo].[AppSetting] ADD CONSTRAINT [DF_AppSetting_UpdatedAt] DEFAULT (sysutcdatetime()) FOR [UpdatedAt]
GO

ALTER TABLE [dbo].[AppUser] WITH CHECK ADD CONSTRAINT [FK_AppUser_Tenant]
FOREIGN KEY([TenantId]) REFERENCES [dbo].[Tenant] ([TenantId])
ON DELETE CASCADE
GO
ALTER TABLE [dbo].[AppUser] CHECK CONSTRAINT [FK_AppUser_Tenant]
GO

ALTER TABLE [ref].[Category] WITH CHECK ADD CONSTRAINT [FK_Category_Tenant]
FOREIGN KEY([TenantId]) REFERENCES [dbo].[Tenant] ([TenantId])
GO
ALTER TABLE [ref].[Category] CHECK CONSTRAINT [FK_Category_Tenant]
GO

ALTER TABLE [ref].[Category] WITH CHECK ADD CONSTRAINT [FK_Category_ParentCategory]
FOREIGN KEY([ParentCategoryId]) REFERENCES [ref].[Category] ([CategoryId])
GO
ALTER TABLE [ref].[Category] CHECK CONSTRAINT [FK_Category_ParentCategory]
GO

ALTER TABLE [dbo].[Location] WITH CHECK ADD CONSTRAINT [FK_Location_Tenant]
FOREIGN KEY([TenantId]) REFERENCES [dbo].[Tenant] ([TenantId])
GO
ALTER TABLE [dbo].[Location] CHECK CONSTRAINT [FK_Location_Tenant]
GO

ALTER TABLE [dbo].[Node] WITH CHECK ADD CONSTRAINT [FK_Node_Tenant]
FOREIGN KEY([TenantId]) REFERENCES [dbo].[Tenant] ([TenantId])
GO
ALTER TABLE [dbo].[Node] CHECK CONSTRAINT [FK_Node_Tenant]
GO

ALTER TABLE [dbo].[Node] WITH CHECK ADD CONSTRAINT [FK_Node_NodeType]
FOREIGN KEY([NodeTypeId]) REFERENCES [ref].[NodeType] ([NodeTypeId])
GO
ALTER TABLE [dbo].[Node] CHECK CONSTRAINT [FK_Node_NodeType]
GO

ALTER TABLE [dbo].[Node] WITH CHECK ADD CONSTRAINT [FK_Node_NodeStatus]
FOREIGN KEY([NodeStatusId]) REFERENCES [ref].[NodeStatus] ([NodeStatusId])
GO
ALTER TABLE [dbo].[Node] CHECK CONSTRAINT [FK_Node_NodeStatus]
GO

ALTER TABLE [dbo].[Node] WITH CHECK ADD CONSTRAINT [FK_Node_Category]
FOREIGN KEY([CategoryId]) REFERENCES [ref].[Category] ([CategoryId])
GO
ALTER TABLE [dbo].[Node] CHECK CONSTRAINT [FK_Node_Category]
GO

ALTER TABLE [dbo].[Node] WITH CHECK ADD CONSTRAINT [FK_Node_OwnerUser]
FOREIGN KEY([OwnerUserId]) REFERENCES [dbo].[AppUser] ([UserId])
GO
ALTER TABLE [dbo].[Node] CHECK CONSTRAINT [FK_Node_OwnerUser]
GO

ALTER TABLE [dbo].[Node] WITH CHECK ADD CONSTRAINT [FK_Node_CreatedByUser]
FOREIGN KEY([CreatedByUserId]) REFERENCES [dbo].[AppUser] ([UserId])
GO
ALTER TABLE [dbo].[Node] CHECK CONSTRAINT [FK_Node_CreatedByUser]
GO

ALTER TABLE [dbo].[Node] WITH CHECK ADD CONSTRAINT [FK_Node_UpdatedByUser]
FOREIGN KEY([UpdatedByUserId]) REFERENCES [dbo].[AppUser] ([UserId])
GO
ALTER TABLE [dbo].[Node] CHECK CONSTRAINT [FK_Node_UpdatedByUser]
GO

ALTER TABLE [dbo].[Node] WITH CHECK ADD CONSTRAINT [FK_Node_PrimaryLocation]
FOREIGN KEY([PrimaryLocationId]) REFERENCES [dbo].[Location] ([LocationId])
GO
ALTER TABLE [dbo].[Node] CHECK CONSTRAINT [FK_Node_PrimaryLocation]
GO

ALTER TABLE [dbo].[Node] WITH CHECK ADD CONSTRAINT [FK_Node_ParentNode]
FOREIGN KEY([ParentNodeId]) REFERENCES [dbo].[Node] ([NodeId])
GO
ALTER TABLE [dbo].[Node] CHECK CONSTRAINT [FK_Node_ParentNode]
GO

ALTER TABLE [dbo].[Node] WITH CHECK ADD CONSTRAINT [FK_Node_RootNode]
FOREIGN KEY([RootNodeId]) REFERENCES [dbo].[Node] ([NodeId])
GO
ALTER TABLE [dbo].[Node] CHECK CONSTRAINT [FK_Node_RootNode]
GO

ALTER TABLE [dbo].[Node] WITH CHECK ADD CONSTRAINT [FK_Node_PreviousNode]
FOREIGN KEY([PreviousNodeId]) REFERENCES [dbo].[Node] ([NodeId])
GO
ALTER TABLE [dbo].[Node] CHECK CONSTRAINT [FK_Node_PreviousNode]
GO

ALTER TABLE [dbo].[Node] WITH CHECK ADD CONSTRAINT [FK_Node_ReplacementNode]
FOREIGN KEY([ReplacementNodeId]) REFERENCES [dbo].[Node] ([NodeId])
GO
ALTER TABLE [dbo].[Node] CHECK CONSTRAINT [FK_Node_ReplacementNode]
GO

ALTER TABLE [dbo].[Node] WITH CHECK ADD CONSTRAINT [FK_Node_TemplateNode]
FOREIGN KEY([TemplateNodeId]) REFERENCES [dbo].[Node] ([NodeId])
GO
ALTER TABLE [dbo].[Node] CHECK CONSTRAINT [FK_Node_TemplateNode]
GO

ALTER TABLE [dbo].[NodeAudit] WITH CHECK ADD CONSTRAINT [FK_NodeAudit_Node]
FOREIGN KEY([NodeId]) REFERENCES [dbo].[Node] ([NodeId])
ON DELETE CASCADE
GO
ALTER TABLE [dbo].[NodeAudit] CHECK CONSTRAINT [FK_NodeAudit_Node]
GO

ALTER TABLE [dbo].[NodeAudit] WITH CHECK ADD CONSTRAINT [FK_NodeAudit_ChangedByUser]
FOREIGN KEY([ChangedByUserId]) REFERENCES [dbo].[AppUser] ([UserId])
GO
ALTER TABLE [dbo].[NodeAudit] CHECK CONSTRAINT [FK_NodeAudit_ChangedByUser]
GO

ALTER TABLE [dbo].[Tag] WITH CHECK ADD CONSTRAINT [FK_Tag_Tenant]
FOREIGN KEY([TenantId]) REFERENCES [dbo].[Tenant] ([TenantId])
ON DELETE CASCADE
GO
ALTER TABLE [dbo].[Tag] CHECK CONSTRAINT [FK_Tag_Tenant]
GO

ALTER TABLE [dbo].[NodeTag] WITH CHECK ADD CONSTRAINT [FK_NodeTag_Node]
FOREIGN KEY([NodeId]) REFERENCES [dbo].[Node] ([NodeId])
ON DELETE CASCADE
GO
ALTER TABLE [dbo].[NodeTag] CHECK CONSTRAINT [FK_NodeTag_Node]
GO

ALTER TABLE [dbo].[NodeTag] WITH CHECK ADD CONSTRAINT [FK_NodeTag_Tag]
FOREIGN KEY([TagId]) REFERENCES [dbo].[Tag] ([TagId])
ON DELETE CASCADE
GO
ALTER TABLE [dbo].[NodeTag] CHECK CONSTRAINT [FK_NodeTag_Tag]
GO

ALTER TABLE [dbo].[NodeTag] WITH CHECK ADD CONSTRAINT [FK_NodeTag_TaggedByUser]
FOREIGN KEY([TaggedByUserId]) REFERENCES [dbo].[AppUser] ([UserId])
GO
ALTER TABLE [dbo].[NodeTag] CHECK CONSTRAINT [FK_NodeTag_TaggedByUser]
GO

ALTER TABLE [dbo].[PriceBook] WITH CHECK ADD CONSTRAINT [FK_PriceBook_Tenant]
FOREIGN KEY([TenantId]) REFERENCES [dbo].[Tenant] ([TenantId])
ON DELETE CASCADE
GO
ALTER TABLE [dbo].[PriceBook] CHECK CONSTRAINT [FK_PriceBook_Tenant]
GO

ALTER TABLE [dbo].[PriceBook] WITH CHECK ADD CONSTRAINT [FK_PriceBook_Currency]
FOREIGN KEY([CurrencyCode]) REFERENCES [ref].[Currency] ([CurrencyCode])
GO
ALTER TABLE [dbo].[PriceBook] CHECK CONSTRAINT [FK_PriceBook_Currency]
GO

ALTER TABLE [dbo].[PriceBookLine] WITH CHECK ADD CONSTRAINT [FK_PriceBookLine_PriceBook]
FOREIGN KEY([TenantId], [PriceBookCode]) REFERENCES [dbo].[PriceBook] ([TenantId], [PriceBookCode])
ON DELETE CASCADE
GO
ALTER TABLE [dbo].[PriceBookLine] CHECK CONSTRAINT [FK_PriceBookLine_PriceBook]
GO

ALTER TABLE [dbo].[PriceBookLine] WITH CHECK ADD CONSTRAINT [FK_PriceBookLine_Node]
FOREIGN KEY([NodeId]) REFERENCES [dbo].[Node] ([NodeId])
GO
ALTER TABLE [dbo].[PriceBookLine] CHECK CONSTRAINT [FK_PriceBookLine_Node]
GO

ALTER TABLE [dbo].[AppUser] WITH CHECK ADD CONSTRAINT [CK_AppUser_FailedLoginCount]
CHECK ([FailedLoginCount] >= (0) AND [FailedLoginCount] <= (10))
GO
ALTER TABLE [dbo].[AppUser] CHECK CONSTRAINT [CK_AppUser_FailedLoginCount]
GO

ALTER TABLE [ref].[Currency] WITH CHECK ADD CONSTRAINT [CK_Currency_MinorUnit]
CHECK ([MinorUnit] BETWEEN (0) AND (4))
GO
ALTER TABLE [ref].[Currency] CHECK CONSTRAINT [CK_Currency_MinorUnit]
GO

ALTER TABLE [dbo].[Location] WITH CHECK ADD CONSTRAINT [CK_Location_Latitude]
CHECK ([Latitude] IS NULL OR ([Latitude] >= (-90) AND [Latitude] <= (90)))
GO
ALTER TABLE [dbo].[Location] CHECK CONSTRAINT [CK_Location_Latitude]
GO

ALTER TABLE [dbo].[Location] WITH CHECK ADD CONSTRAINT [CK_Location_Longitude]
CHECK ([Longitude] IS NULL OR ([Longitude] >= (-180) AND [Longitude] <= (180)))
GO
ALTER TABLE [dbo].[Location] CHECK CONSTRAINT [CK_Location_Longitude]
GO

CREATE NONCLUSTERED INDEX [IX_AppUser_Tenant_DisplayName]
ON [dbo].[AppUser]
(
    [TenantId] ASC,
    [DisplayName] ASC
)
INCLUDE([EmailAddress], [IsLocked]) WITH (SORT_IN_TEMPDB = OFF, DROP_EXISTING = OFF, ONLINE = OFF) ON [PRIMARY]
GO

CREATE NONCLUSTERED INDEX [IX_Category_Tenant_Parent]
ON [ref].[Category]
(
    [TenantId] ASC,
    [ParentCategoryId] ASC
) ON [PRIMARY]
GO

CREATE NONCLUSTERED INDEX [IX_Node_Tenant_Type_Status]
ON [dbo].[Node]
(
    [TenantId] ASC,
    [NodeTypeId] ASC,
    [NodeStatusId] ASC
)
INCLUDE([NodeCode], [NodeName], [OwnerUserId], [CreatedAt]) ON [PRIMARY]
GO

CREATE NONCLUSTERED INDEX [IX_Node_ParentNodeId]
ON [dbo].[Node]
(
    [ParentNodeId] ASC
)
WHERE ([ParentNodeId] IS NOT NULL)
GO

CREATE NONCLUSTERED INDEX [IX_Node_RootNodeId]
ON [dbo].[Node]
(
    [RootNodeId] ASC
)
WHERE ([RootNodeId] IS NOT NULL)
GO

CREATE NONCLUSTERED INDEX [IX_Node_IsActive_NotDeleted]
ON [dbo].[Node]
(
    [TenantId] ASC,
    [IsActive] ASC,
    [IsDeleted] ASC
)
WHERE ([IsDeleted]=(0))
GO

CREATE UNIQUE NONCLUSTERED INDEX [UX_Node_Tenant_ShortName_WhenPresent]
ON [dbo].[Node]
(
    [TenantId] ASC,
    [ShortName] ASC
)
WHERE ([ShortName] IS NOT NULL)
GO

CREATE NONCLUSTERED INDEX [IX_NodeAudit_Node_ChangedAt]
ON [dbo].[NodeAudit]
(
    [NodeId] ASC,
    [ChangedAt] DESC
) ON [PRIMARY]
GO

CREATE NONCLUSTERED INDEX [IX_PriceBookLine_Node]
ON [dbo].[PriceBookLine]
(
    [NodeId] ASC
) ON [PRIMARY]
GO

SET ANSI_NULLS ON
GO
SET QUOTED_IDENTIFIER ON
GO
CREATE VIEW [dbo].[vwActiveNodeSummary]
AS
SELECT
    n.[TenantId],
    n.[NodeId],
    n.[NodeCode],
    n.[NodeName],
    nt.[NodeTypeCode],
    ns.[StatusCode],
    n.[QuantityOnHand],
    n.[QuantityReserved],
    n.[ComputedAvailableQty]
FROM [dbo].[Node] AS n
INNER JOIN [ref].[NodeType] AS nt
    ON nt.[NodeTypeId] = n.[NodeTypeId]
INNER JOIN [ref].[NodeStatus] AS ns
    ON ns.[NodeStatusId] = n.[NodeStatusId]
WHERE n.[IsActive] = 1
  AND n.[IsDeleted] = 0
GO

SET ANSI_NULLS ON
GO
SET QUOTED_IDENTIFIER ON
GO
CREATE TRIGGER [dbo].[trg_Node_Audit_Update]
ON [dbo].[Node]
AFTER UPDATE
AS
BEGIN
    SET NOCOUNT ON;

    INSERT INTO [dbo].[NodeAudit]
    (
        [NodeId],
        [AuditAction],
        [ChangedByUserId],
        [ChangedAt],
        [BeforeJson],
        [AfterJson]
    )
    SELECT
        i.[NodeId],
        'UPDATE',
        i.[UpdatedByUserId],
        sysutcdatetime(),
        NULL,
        NULL
    FROM inserted AS i;
END
GO
