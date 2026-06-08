//! Static safety analysis of migration operations — the rules behind
//! `molde lint`. Pure functions over [`Operation`], no database access, so they
//! run in CI before a migration is merged.
//!
//! Two severities:
//! - **Destructive** — irreversible data loss (drop table/column). These do *not*
//!   fail at apply time, so they must be caught here.
//! - **Warning** — may fail on existing data or lock the table (add NOT NULL
//!   without a default, add a UNIQUE index, make a column NOT NULL, change a
//!   column type, add a foreign key). These would fail at apply time on a
//!   populated table; the lint surfaces them early.

use crate::diff::Operation;
use crate::model::Column;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    /// Irreversible data loss — should block a merge.
    Destructive,
    /// May fail on existing data or lock the table — review.
    Warning,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    /// Stable rule id (e.g. `drop-column`).
    pub code: &'static str,
    pub severity: Severity,
    /// The affected object (`Table` or `Table.Column`).
    pub object: String,
    pub message: String,
}

/// Analyze the `up` operations of a migration for risky changes.
pub fn lint_operations(ops: &[Operation]) -> Vec<Finding> {
    let mut out = Vec::new();
    for op in ops {
        analyze(op, &mut out);
    }
    out
}

fn analyze(op: &Operation, out: &mut Vec<Finding>) {
    match op {
        Operation::DropTable { name, .. } => out.push(Finding {
            code: "drop-table",
            severity: Severity::Destructive,
            object: name.clone(),
            message: format!("drops table `{name}` — all of its rows are lost"),
        }),
        Operation::DropColumn { table, name, .. } => out.push(Finding {
            code: "drop-column",
            severity: Severity::Destructive,
            object: format!("{table}.{name}"),
            message: format!("drops column `{table}.{name}` — its data is lost"),
        }),
        Operation::AddColumn { table, column, .. } if needs_value_but_has_none(column) => {
            out.push(Finding {
                code: "not-null-no-default",
                severity: Severity::Warning,
                object: format!("{table}.{}", column.name),
                message: format!(
                    "adds NOT NULL column `{table}.{}` without a default — fails if `{table}` has rows",
                    column.name
                ),
            })
        }
        Operation::AlterColumn { table, old, new, .. } => {
            if old.is_nullable && !new.is_nullable {
                out.push(Finding {
                    code: "make-not-null",
                    severity: Severity::Warning,
                    object: format!("{table}.{}", new.name),
                    message: format!(
                        "makes `{table}.{}` NOT NULL — fails if existing rows hold NULL",
                        new.name
                    ),
                });
            }
            if type_changed(old, new) {
                out.push(Finding {
                    code: "alter-column-type",
                    severity: Severity::Warning,
                    object: format!("{table}.{}", new.name),
                    message: format!(
                        "changes the type of `{table}.{}` — may rewrite/lock the table and fail on incompatible data",
                        new.name
                    ),
                });
            }
        }
        Operation::CreateIndex { table, index, .. } if index.is_unique => out.push(Finding {
            code: "add-unique-index",
            severity: Severity::Warning,
            object: index.name.clone(),
            message: format!(
                "adds UNIQUE index `{}` on `{table}` — fails if duplicate values already exist",
                index.name
            ),
        }),
        Operation::AddForeignKey {
            table, foreign_key, ..
        } => out.push(Finding {
            code: "add-foreign-key",
            severity: Severity::Warning,
            object: foreign_key.name.clone(),
            message: format!(
                "adds foreign key `{}` on `{table}` — fails if existing rows reference a missing parent",
                foreign_key.name
            ),
        }),
        _ => {}
    }
}

/// A column that must hold a value on every existing row but provides none.
fn needs_value_but_has_none(c: &Column) -> bool {
    !c.is_nullable && c.default_value_sql.is_none() && !c.is_identity && c.computed_sql.is_none()
}

fn type_changed(old: &Column, new: &Column) -> bool {
    old.store_type != new.store_type
        || old.clr_type != new.clr_type
        || old.max_length != new.max_length
        || old.precision != new.precision
        || old.scale != new.scale
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Column, ForeignKey, Index, ReferentialAction};

    fn col(name: &str, nullable: bool) -> Column {
        Column {
            name: name.into(),
            store_type: None,
            clr_type: Some("System.Int32".into()),
            is_nullable: nullable,
            is_identity: false,
            max_length: None,
            precision: None,
            scale: None,
            default_value_sql: None,
            computed_sql: None,
            computed_stored: false,
            collation: None,
            comment: None,
        }
    }

    #[test]
    fn flags_destructive_and_data_dependent() {
        let ops = vec![
            Operation::DropTable {
                schema: None,
                name: "Customer".into(),
            },
            Operation::DropColumn {
                schema: None,
                table: "Order".into(),
                name: "LegacyId".into(),
            },
            Operation::AddColumn {
                schema: None,
                table: "Order".into(),
                column: col("Code", false), // NOT NULL, no default
            },
            Operation::CreateIndex {
                schema: None,
                table: "User".into(),
                index: Index {
                    name: "ix_user_email".into(),
                    columns: vec!["Email".into()],
                    is_unique: true,
                    filter: None,
                    method: None,
                    operators: vec![],
                    expression: None,
                },
            },
            Operation::AddForeignKey {
                schema: None,
                table: "Order".into(),
                foreign_key: ForeignKey {
                    name: "fk_order_customer".into(),
                    columns: vec!["CustomerId".into()],
                    principal_table: "Customer".into(),
                    principal_schema: None,
                    principal_columns: vec!["Id".into()],
                    on_delete: ReferentialAction::NoAction,
                },
            },
        ];
        let f = lint_operations(&ops);
        let codes: Vec<_> = f.iter().map(|x| x.code).collect();
        assert_eq!(
            codes,
            vec![
                "drop-table",
                "drop-column",
                "not-null-no-default",
                "add-unique-index",
                "add-foreign-key"
            ]
        );
        assert_eq!(f[0].severity, Severity::Destructive);
        assert_eq!(f[2].severity, Severity::Warning);
    }

    #[test]
    fn nullable_or_defaulted_or_identity_column_is_fine() {
        let mut id = col("Id", false);
        id.is_identity = true;
        let mut defaulted = col("Flag", false);
        defaulted.default_value_sql = Some("false".into());
        let ops = vec![
            Operation::AddColumn {
                schema: None,
                table: "T".into(),
                column: col("Nick", true),
            },
            Operation::AddColumn {
                schema: None,
                table: "T".into(),
                column: id,
            },
            Operation::AddColumn {
                schema: None,
                table: "T".into(),
                column: defaulted,
            },
        ];
        assert!(lint_operations(&ops).is_empty());
    }

    #[test]
    fn alter_column_flags_both_not_null_and_type_change() {
        let old = col("Amount", true);
        let mut new = col("Amount", false);
        new.clr_type = Some("System.Decimal".into());
        let f = lint_operations(&[Operation::AlterColumn {
            schema: None,
            table: "Order".into(),
            new,
            old,
        }]);
        let codes: Vec<_> = f.iter().map(|x| x.code).collect();
        assert_eq!(codes, vec!["make-not-null", "alter-column-type"]);
    }
}
