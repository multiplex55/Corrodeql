use crate::schema::model::{
    CheckConstraintDef, DefaultConstraintDef, ForeignKeyDef, PrimaryKeyDef, UniqueConstraintDef,
};

/// A SQL Server constraint name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConstraintName(pub String);

pub fn normalize_primary_key(
    name: Option<String>,
    columns: Vec<String>,
    clustered: Option<bool>,
) -> PrimaryKeyDef {
    PrimaryKeyDef {
        name,
        columns,
        clustered,
    }
}

pub fn normalize_unique_constraint(
    name: Option<String>,
    columns: Vec<String>,
) -> UniqueConstraintDef {
    UniqueConstraintDef { name, columns }
}

pub fn normalize_default_constraint(
    name: Option<String>,
    expression: String,
) -> DefaultConstraintDef {
    DefaultConstraintDef { name, expression }
}

pub fn normalize_check_constraint(name: Option<String>, expression: String) -> CheckConstraintDef {
    CheckConstraintDef { name, expression }
}

pub fn normalize_foreign_key(foreign_key: ForeignKeyDef) -> ForeignKeyDef {
    foreign_key
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::model::{ReferentialAction, TableName};

    #[test]
    fn keeps_named_constraints() {
        assert_eq!(
            normalize_primary_key(Some("PK_T".into()), vec!["Id".into()], Some(true))
                .name
                .as_deref(),
            Some("PK_T")
        );
        assert_eq!(
            normalize_unique_constraint(Some("UQ_T_Code".into()), vec!["Code".into()])
                .name
                .as_deref(),
            Some("UQ_T_Code")
        );
        assert_eq!(
            normalize_default_constraint(Some("DF_T_Flag".into()), "0".into())
                .name
                .as_deref(),
            Some("DF_T_Flag")
        );
        assert_eq!(
            normalize_check_constraint(Some("CK_T_Flag".into()), "Flag in (0,1)".into())
                .name
                .as_deref(),
            Some("CK_T_Flag")
        );
    }

    #[test]
    fn keeps_foreign_key_shape() {
        let fk = ForeignKeyDef {
            name: Some("FK_Order_Customer".into()),
            columns: vec!["CustomerId".into()],
            referenced_table: TableName::new(Some("dbo".into()), "Customer"),
            referenced_columns: vec!["Id".into()],
            on_delete: Some(ReferentialAction::Cascade),
            on_update: None,
        };
        assert_eq!(normalize_foreign_key(fk.clone()), fk);
    }
}
