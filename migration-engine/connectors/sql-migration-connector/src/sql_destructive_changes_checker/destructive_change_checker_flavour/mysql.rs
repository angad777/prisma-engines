use super::DestructiveChangeCheckerFlavour;
use crate::{
    expanded_alter_column::{expand_mysql_alter_column, MysqlAlterColumn},
    flavour::MysqlFlavour,
    sql_destructive_changes_checker::{
        destructive_check_plan::DestructiveCheckPlan, unexecutable_step_check::UnexecutableStepCheck,
        warning_check::SqlMigrationWarning,
    },
    sql_schema_differ::ColumnDiffer,
};
use sql_schema_describer::Table;

impl DestructiveChangeCheckerFlavour for MysqlFlavour {
    fn check_alter_column(&self, previous_table: &Table, columns: &ColumnDiffer<'_>, plan: &mut DestructiveCheckPlan) {
        match expand_mysql_alter_column(columns) {
            MysqlAlterColumn::DropDefault => return, // dropping a default is safe

            // If only the default changed, the step is safe.
            MysqlAlterColumn::Modify { changes, .. } if changes.only_default_changed() => return,

            // Otherwise, case by case.
            MysqlAlterColumn::Modify { .. } => {
                // Column went from optional to required. This is unexecutable unless the table is
                // empty or the column has no existing NULLs.
                if columns.all_changes().arity_changed() && columns.next.tpe.arity.is_required() {
                    plan.push_unexecutable(UnexecutableStepCheck::MadeOptionalFieldRequired {
                        column: columns.previous.name.clone(),
                        table: previous_table.name.clone(),
                    });
                } else {
                    // Not detected as a column migration by us, but it still may be because MODIFY
                    // requires re-stating the column type.
                    if !columns.all_changes().type_changed()
                        && columns.previous.tpe.data_type != columns.next.tpe.data_type
                    {
                        plan.push_warning(SqlMigrationWarning::MysqlColumnTypeRestatement {
                            table: previous_table.name.clone(),
                            column: columns.previous.name.clone(),
                            previous_type: columns.previous.tpe.data_type.clone(),
                            next_type: columns.next.tpe.data_type.clone(),
                        })
                    }

                    plan.push_warning(SqlMigrationWarning::AlterColumn {
                        table: previous_table.name.clone(),
                        column: columns.next.name.clone(),
                    });
                }
            }
        }
    }
}
