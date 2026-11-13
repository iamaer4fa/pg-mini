use crate::models::{AddColumnForm, ConfirmAction, ConfirmDialogState, CreateTableForm, RoleItem};
use crate::sql_ddl::{ConstraintKind, DdlForm, DdlFormKind};
use egui::{Color32, RichText, ScrollArea, Window};

pub struct AddColumnModal {
    pub is_open: bool,
    pub form: AddColumnForm,
    pub custom_type: String,
    pub error_message: Option<String>,
}

impl Default for AddColumnModal {
    fn default() -> Self {
        Self {
            is_open: false,
            form: AddColumnForm::new("public", ""),
            custom_type: "VARCHAR(255)".to_string(),
            error_message: None,
        }
    }
}

impl AddColumnModal {
    pub fn open_for(&mut self, schema: &str, table: &str) {
        self.form = AddColumnForm::new(schema, table);
        self.custom_type = "VARCHAR(255)".to_string();
        self.error_message = None;
        self.is_open = true;
    }

    pub fn show(&mut self, ctx: &egui::Context, is_busy: bool) -> Option<AddColumnForm> {
        let mut submit_form: Option<AddColumnForm> = None;
        if !self.is_open {
            return None;
        }

        Window::new(format!(
            "➕ Add Column to {}.{}",
            self.form.schema, self.form.table
        ))
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
        .show(ctx, |ui| {
            ui.set_min_width(460.0);

            ui.heading("Add New Column");
            ui.separator();
            ui.add_space(4.0);

            egui::Grid::new("add_col_grid")
                .num_columns(2)
                .spacing([12.0, 8.0])
                .show(ui, |ui| {
                    ui.label("Column Name:");
                    ui.text_edit_singleline(&mut self.form.column_name);
                    ui.end_row();

                    ui.label("Data Type:");
                    ui.horizontal(|ui| {
                        egui::ComboBox::from_id_salt("data_type_select")
                            .width(150.0)
                            .selected_text(&self.custom_type)
                            .show_ui(ui, |ui| {
                                for t in &[
                                    "VARCHAR(255)",
                                    "TEXT",
                                    "INTEGER",
                                    "BIGINT",
                                    "BOOLEAN",
                                    "TIMESTAMP",
                                    "TIMESTAMPTZ",
                                    "DATE",
                                    "TIME",
                                    "NUMERIC(10,2)",
                                    "DOUBLE PRECISION",
                                    "UUID",
                                    "JSONB",
                                    "BYTEA",
                                ] {
                                    if ui
                                        .selectable_value(&mut self.custom_type, t.to_string(), *t)
                                        .clicked()
                                    {
                                        self.form.data_type = self.custom_type.clone();
                                    }
                                }
                            });

                        if ui.text_edit_singleline(&mut self.custom_type).changed() {
                            self.form.data_type = self.custom_type.clone();
                        }
                    });
                    ui.end_row();

                    ui.label("Nullable:");
                    ui.checkbox(
                        &mut self.form.is_nullable,
                        "Allow NULL (uncheck for NOT NULL)",
                    );
                    ui.end_row();

                    ui.label("Default Value:");
                    ui.text_edit_singleline(&mut self.form.default_value);
                    ui.end_row();

                    ui.label("Primary Key:");
                    ui.checkbox(&mut self.form.is_primary_key, "Set as Primary Key");
                    ui.end_row();
                });

            ui.add_space(8.0);

            // SQL Preview
            let sql_preview = generate_sql_preview(&self.form);
            ui.label(
                RichText::new("Generated SQL:")
                    .size(11.0)
                    .color(Color32::GRAY),
            );
            egui::Frame::none()
                .fill(Color32::from_rgb(25, 28, 35))
                .stroke(egui::Stroke::new(1.0_f32, Color32::from_rgb(50, 55, 65)))
                .rounding(4.0)
                .inner_margin(6.0)
                .show(ui, |ui| {
                    ui.label(
                        RichText::new(&sql_preview)
                            .monospace()
                            .color(Color32::from_rgb(100, 200, 255)),
                    );
                });

            if let Some(err) = &self.error_message {
                ui.add_space(6.0);
                ui.label(RichText::new(err).color(Color32::LIGHT_RED).strong());
            }

            ui.add_space(10.0);
            ui.separator();
            ui.add_space(4.0);

            ui.horizontal(|ui| {
                if ui.button("Cancel").clicked() {
                    self.is_open = false;
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let is_valid = !self.form.column_name.trim().is_empty();
                    let btn = ui.add_enabled(
                        is_valid && !is_busy,
                        egui::Button::new(
                            RichText::new("➕ Add Column")
                                .strong()
                                .color(Color32::WHITE),
                        )
                        .fill(Color32::from_rgb(16, 145, 90))
                        .rounding(egui::Rounding::same(6.0)),
                    );

                    if btn.clicked() {
                        submit_form = Some(self.form.clone());
                        self.is_open = false;
                    }
                });
            });
        });

        submit_form
    }
}

fn generate_sql_preview(form: &AddColumnForm) -> String {
    let col = if form.column_name.is_empty() {
        "new_column"
    } else {
        form.column_name.trim()
    };
    let mut sql = format!(
        "ALTER TABLE \"{}\".\"{}\" ADD COLUMN \"{}\" {}",
        form.schema,
        form.table,
        col,
        form.data_type.trim()
    );
    if !form.default_value.trim().is_empty() {
        sql.push_str(&format!(" DEFAULT {}", form.default_value.trim()));
    }
    if !form.is_nullable {
        sql.push_str(" NOT NULL");
    }
    if form.is_primary_key {
        sql.push_str(" PRIMARY KEY");
    }
    sql.push(';');
    sql
}

pub struct CreateTableModal {
    pub is_open: bool,
    pub form: CreateTableForm,
    pub error_message: Option<String>,
}

impl Default for CreateTableModal {
    fn default() -> Self {
        Self {
            is_open: false,
            form: CreateTableForm::new("public"),
            error_message: None,
        }
    }
}

impl CreateTableModal {
    pub fn open_for(&mut self, schema: &str) {
        self.form = CreateTableForm::new(schema);
        self.error_message = None;
        self.is_open = true;
    }

    pub fn show(&mut self, ctx: &egui::Context, is_busy: bool) -> Option<CreateTableForm> {
        let mut submit: Option<CreateTableForm> = None;
        if !self.is_open {
            return None;
        }

        Window::new(format!("➕ Create New Table in '{}'", self.form.schema))
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .show(ctx, |ui| {
                ui.set_min_width(420.0);

                ui.heading("Create Table");
                ui.separator();
                ui.add_space(4.0);

                egui::Grid::new("create_tbl_grid")
                    .num_columns(2)
                    .spacing([12.0, 8.0])
                    .show(ui, |ui| {
                        ui.label("Schema:");
                        ui.text_edit_singleline(&mut self.form.schema);
                        ui.end_row();

                        ui.label("Table Name:");
                        ui.text_edit_singleline(&mut self.form.table_name);
                        ui.end_row();

                        ui.label("Primary Key:");
                        ui.checkbox(&mut self.form.add_id_pk, "Include `id SERIAL PRIMARY KEY`");
                        ui.end_row();
                    });

                ui.add_space(8.0);
                let tbl_name = if self.form.table_name.is_empty() {
                    "new_table"
                } else {
                    self.form.table_name.trim()
                };
                let sql_preview = if self.form.add_id_pk {
                    format!(
                        "CREATE TABLE \"{}\".\"{}\" (\n    id SERIAL PRIMARY KEY\n);",
                        self.form.schema, tbl_name
                    )
                } else {
                    format!("CREATE TABLE \"{}\".\"{}\" ();", self.form.schema, tbl_name)
                };

                ui.label(
                    RichText::new("Generated SQL:")
                        .size(11.0)
                        .color(Color32::GRAY),
                );
                egui::Frame::none()
                    .fill(Color32::from_rgb(25, 28, 35))
                    .stroke(egui::Stroke::new(1.0_f32, Color32::from_rgb(50, 55, 65)))
                    .rounding(4.0)
                    .inner_margin(6.0)
                    .show(ui, |ui| {
                        ui.label(
                            RichText::new(&sql_preview)
                                .monospace()
                                .color(Color32::from_rgb(100, 200, 255)),
                        );
                    });

                ui.add_space(10.0);
                ui.separator();
                ui.add_space(4.0);

                ui.horizontal(|ui| {
                    if ui.button("Cancel").clicked() {
                        self.is_open = false;
                    }

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let is_valid = !self.form.table_name.trim().is_empty();
                        let btn = ui.add_enabled(
                            is_valid && !is_busy,
                            egui::Button::new(
                                RichText::new("➕ Create Table")
                                    .strong()
                                    .color(Color32::WHITE),
                            )
                            .fill(Color32::from_rgb(16, 145, 90))
                            .rounding(egui::Rounding::same(6.0)),
                        );

                        if btn.clicked() {
                            submit = Some(self.form.clone());
                            self.is_open = false;
                        }
                    });
                });
            });

        submit
    }
}

#[derive(Default)]
pub struct ObjectDdlModal {
    pub is_open: bool,
    pub form: Option<DdlForm>,
}

impl ObjectDdlModal {
    pub fn open(&mut self, form: DdlForm) {
        self.form = Some(form);
        self.is_open = true;
    }

    pub fn show(
        &mut self,
        ctx: &egui::Context,
        is_busy: bool,
        roles: &[RoleItem],
    ) -> Option<String> {
        if !self.is_open {
            return None;
        }
        let form = self.form.as_mut()?;
        let mut keep_open = true;
        let mut request_close = false;
        let mut execute_sql = None;
        Window::new(form.kind.title())
            .collapsible(false)
            .resizable(true)
            .open(&mut keep_open)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .show(ctx, |ui| {
                ui.set_min_width(500.0);
                egui::Grid::new("object_ddl_target")
                    .num_columns(2)
                    .spacing([12.0, 8.0])
                    .show(ui, |ui| {
                        ui.label("Schema:");
                        ui.add(egui::TextEdit::singleline(&mut form.schema).desired_width(300.0));
                        ui.end_row();
                        if form.kind.is_table_scoped() {
                            ui.label("Table:");
                            ui.add(
                                egui::TextEdit::singleline(&mut form.table).desired_width(300.0),
                            );
                            ui.end_row();
                        }
                    });
                ui.separator();

                match form.kind {
                    DdlFormKind::Index => {
                        ui.label("Index name");
                        ui.text_edit_singleline(&mut form.name);
                        ui.label("Columns (comma-separated)");
                        ui.text_edit_singleline(&mut form.columns);
                        ui.horizontal(|ui| {
                            ui.checkbox(&mut form.unique_index, "Unique");
                            ui.label("Method:");
                            egui::ComboBox::from_id_salt("ddl_index_method")
                                .selected_text(&form.index_method)
                                .show_ui(ui, |ui| {
                                    for method in ["btree", "hash", "gist", "spgist", "gin", "brin"]
                                    {
                                        ui.selectable_value(
                                            &mut form.index_method,
                                            method.to_string(),
                                            method,
                                        );
                                    }
                                });
                        });
                    }
                    DdlFormKind::ForeignKey => {
                        ui.label("Constraint name");
                        ui.text_edit_singleline(&mut form.name);
                        ui.label("Local columns (comma-separated)");
                        ui.text_edit_singleline(&mut form.columns);
                        ui.label("Referenced schema and table");
                        ui.horizontal(|ui| {
                            ui.add(
                                egui::TextEdit::singleline(&mut form.referenced_schema)
                                    .desired_width(180.0),
                            );
                            ui.add(
                                egui::TextEdit::singleline(&mut form.referenced_table)
                                    .desired_width(180.0),
                            );
                        });
                        ui.label("Referenced columns (same count/order)");
                        ui.text_edit_singleline(&mut form.referenced_columns);
                        ui.horizontal(|ui| {
                            ui.label("On delete:");
                            egui::ComboBox::from_id_salt("ddl_fk_on_delete")
                                .selected_text(&form.on_delete)
                                .show_ui(ui, |ui| {
                                    for action in [
                                        "NO ACTION",
                                        "RESTRICT",
                                        "CASCADE",
                                        "SET NULL",
                                        "SET DEFAULT",
                                    ] {
                                        ui.selectable_value(
                                            &mut form.on_delete,
                                            action.to_string(),
                                            action,
                                        );
                                    }
                                });
                        });
                    }
                    DdlFormKind::Constraint => {
                        ui.label("Constraint name");
                        ui.text_edit_singleline(&mut form.name);
                        ui.horizontal(|ui| {
                            ui.label("Type:");
                            egui::ComboBox::from_id_salt("ddl_constraint_kind")
                                .selected_text(match form.constraint_kind {
                                    ConstraintKind::Check => "CHECK",
                                    ConstraintKind::Unique => "UNIQUE",
                                    ConstraintKind::PrimaryKey => "PRIMARY KEY",
                                })
                                .show_ui(ui, |ui| {
                                    ui.selectable_value(
                                        &mut form.constraint_kind,
                                        ConstraintKind::Check,
                                        "CHECK",
                                    );
                                    ui.selectable_value(
                                        &mut form.constraint_kind,
                                        ConstraintKind::Unique,
                                        "UNIQUE",
                                    );
                                    ui.selectable_value(
                                        &mut form.constraint_kind,
                                        ConstraintKind::PrimaryKey,
                                        "PRIMARY KEY",
                                    );
                                });
                        });
                        match form.constraint_kind {
                            ConstraintKind::Check => {
                                ui.label("Check expression");
                                ui.text_edit_singleline(&mut form.check_expression);
                            }
                            ConstraintKind::Unique | ConstraintKind::PrimaryKey => {
                                ui.label("Columns (comma-separated)");
                                ui.text_edit_singleline(&mut form.columns);
                            }
                        }
                    }
                    DdlFormKind::View => {
                        ui.label("View name");
                        ui.text_edit_singleline(&mut form.name);
                        ui.label("SELECT query");
                        ui.add(
                            egui::TextEdit::multiline(&mut form.view_query)
                                .desired_rows(6)
                                .desired_width(f32::INFINITY)
                                .code_editor(),
                        );
                    }
                    DdlFormKind::Sequence => {
                        ui.label("Sequence name");
                        ui.text_edit_singleline(&mut form.name);
                        egui::Grid::new("ddl_sequence_options")
                            .num_columns(2)
                            .spacing([12.0, 8.0])
                            .show(ui, |ui| {
                                ui.label("Start with:");
                                ui.text_edit_singleline(&mut form.sequence_start);
                                ui.end_row();
                                ui.label("Increment by:");
                                ui.text_edit_singleline(&mut form.sequence_increment);
                                ui.end_row();
                                ui.label("Minimum (blank = default):");
                                ui.text_edit_singleline(&mut form.sequence_min);
                                ui.end_row();
                                ui.label("Maximum (blank = default):");
                                ui.text_edit_singleline(&mut form.sequence_max);
                                ui.end_row();
                                ui.label("Cache:");
                                ui.text_edit_singleline(&mut form.sequence_cache);
                                ui.end_row();
                            });
                        ui.checkbox(&mut form.sequence_cycle, "Cycle when limit is reached");
                    }
                    DdlFormKind::Grant => {
                        ui.horizontal(|ui| {
                            ui.selectable_value(&mut form.revoke, false, "Grant");
                            ui.selectable_value(&mut form.revoke, true, "Revoke");
                        });
                        ui.label("Role / grantee");
                        ui.horizontal(|ui| {
                            egui::ComboBox::from_id_salt("ddl_grantee_role")
                                .selected_text(if form.grantee.is_empty() {
                                    "Select a role"
                                } else {
                                    &form.grantee
                                })
                                .show_ui(ui, |ui| {
                                    for role in roles {
                                        ui.selectable_value(
                                            &mut form.grantee,
                                            role.name.clone(),
                                            &role.name,
                                        );
                                    }
                                    ui.selectable_value(
                                        &mut form.grantee,
                                        "PUBLIC".to_string(),
                                        "PUBLIC",
                                    );
                                });
                            ui.text_edit_singleline(&mut form.grantee);
                        });
                        ui.label("Privileges");
                        ui.horizontal_wrapped(|ui| {
                            for privilege in [
                                "SELECT",
                                "INSERT",
                                "UPDATE",
                                "DELETE",
                                "TRUNCATE",
                                "REFERENCES",
                                "TRIGGER",
                            ] {
                                let mut selected =
                                    form.privileges.iter().any(|value| value == privilege);
                                if ui.checkbox(&mut selected, privilege).changed() {
                                    if selected {
                                        form.privileges.push(privilege.to_string());
                                    } else {
                                        form.privileges.retain(|value| value != privilege);
                                    }
                                }
                            }
                        });
                        if form.revoke {
                            ui.checkbox(&mut form.revoke_cascade, "CASCADE dependent grants");
                        } else {
                            ui.checkbox(
                                &mut form.grant_option,
                                "Allow grantee to grant these privileges",
                            );
                        }
                    }
                }

                ui.add_space(8.0);
                let preview = form.generate_sql();
                let mut preview_text = preview
                    .clone()
                    .unwrap_or_else(|error| format!("Cannot generate SQL: {error}"));
                ui.label(
                    RichText::new("Generated SQL preview")
                        .size(11.0)
                        .color(Color32::GRAY),
                );
                egui::Frame::none()
                    .fill(Color32::from_rgb(25, 28, 35))
                    .stroke(egui::Stroke::new(1.0_f32, Color32::from_rgb(50, 55, 65)))
                    .rounding(4.0)
                    .inner_margin(6.0)
                    .show(ui, |ui| {
                        ui.add(
                            egui::TextEdit::multiline(&mut preview_text)
                                .font(egui::TextStyle::Monospace)
                                .desired_rows(4)
                                .desired_width(f32::INFINITY)
                                .interactive(false),
                        );
                    });
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.button("Cancel").clicked() {
                        request_close = true;
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .add_enabled(
                                !is_busy && preview.is_ok(),
                                egui::Button::new(if form.revoke { "Revoke" } else { "Execute" })
                                    .fill(Color32::from_rgb(16, 145, 90)),
                            )
                            .clicked()
                        {
                            execute_sql = preview.ok();
                            request_close = true;
                        }
                    });
                });
            });
        if request_close {
            keep_open = false;
        }
        self.is_open = keep_open;
        if !keep_open || execute_sql.is_some() {
            self.form = None;
        }
        execute_sql
    }
}

pub fn render_confirm_dialog(
    ctx: &egui::Context,
    state: &mut ConfirmDialogState,
    is_busy: bool,
) -> Option<ConfirmAction> {
    let mut confirmed_action: Option<ConfirmAction> = None;
    if !state.is_open {
        return None;
    }

    Window::new(&state.title)
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
        .show(ctx, |ui| {
            ui.set_min_width(400.0);

            ui.horizontal(|ui| {
                let is_restore = matches!(
                    state.action.as_ref(),
                    Some(crate::models::ConfirmAction::RestoreDatabase { .. })
                );
                let is_sql_review = matches!(
                    state.action.as_ref(),
                    Some(
                        crate::models::ConfirmAction::ExecuteSql { .. }
                            | crate::models::ConfirmAction::ExecuteDdl(_)
                    )
                );
                ui.label(RichText::new(if is_restore {
                    "⚠ Confirm Restore"
                } else if is_sql_review {
                    "⚠ Review SQL"
                } else {
                    "⚠ Warning"
                })
                    .color(Color32::LIGHT_RED).strong().size(16.0));
            });
            ui.separator();
            ui.add_space(6.0);

            ui.label(&state.message);
            let sql_preview = match state.action.as_ref() {
                Some(crate::models::ConfirmAction::ExecuteSql { sql, .. })
                | Some(crate::models::ConfirmAction::ExecuteDdl(sql)) => Some(sql),
                _ => None,
            };
            if let Some(sql) = sql_preview {
                egui::Frame::group(ui.style()).show(ui, |ui| {
                    ScrollArea::vertical().max_height(240.0).show(ui, |ui| {
                        ui.label(RichText::new(sql).monospace());
                    });
                });
            }
            let is_restore = matches!(
                state.action.as_ref(),
                Some(crate::models::ConfirmAction::RestoreDatabase { .. })
            );
            let is_sql_review = matches!(
                state.action.as_ref(),
                Some(
                    crate::models::ConfirmAction::ExecuteSql { .. }
                        | crate::models::ConfirmAction::ExecuteDdl(_)
                )
            );
            let is_session_change = matches!(
                state.action.as_ref(),
                Some(
                    crate::models::ConfirmAction::DisconnectWithOpenTransaction
                        | crate::models::ConfirmAction::SwitchDatabaseWithOpenTransaction { .. }
                )
            );
            ui.label(RichText::new(if is_restore {
                "Restoring may permanently change or delete database data."
            } else if is_sql_review {
                "Confirm the target and SQL before execution. The database may apply only part of a multi-statement script if an error occurs."
            } else if is_session_change {
                "Uncommitted changes will be rolled back when the PostgreSQL session closes."
            } else {
                "This action is permanent and cannot be undone."
            }).size(11.0).color(Color32::GRAY));

            ui.add_space(12.0);
            ui.separator();
            ui.add_space(4.0);

            ui.horizontal(|ui| {
                if ui.button("Cancel").clicked() {
                    state.is_open = false;
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let is_restore = matches!(
                        state.action.as_ref(),
                        Some(crate::models::ConfirmAction::RestoreDatabase { .. })
                    );
                    let is_sql_review = matches!(
                        state.action.as_ref(),
                        Some(
                            crate::models::ConfirmAction::ExecuteSql { .. }
                                | crate::models::ConfirmAction::ExecuteDdl(_)
                        )
                    );
                    let is_session_change = matches!(
                        state.action.as_ref(),
                        Some(
                            crate::models::ConfirmAction::DisconnectWithOpenTransaction
                                | crate::models::ConfirmAction::SwitchDatabaseWithOpenTransaction { .. }
                        )
                    );
                    let confirm_btn = ui.add_enabled(
                        !is_busy,
                        egui::Button::new(RichText::new(if is_restore {
                            "Restore Database"
                        } else if is_sql_review {
                            "Execute SQL"
                        } else if is_session_change {
                            "Continue (roll back)"
                        } else {
                            "Yes, Delete"
                        })
                            .strong().color(Color32::WHITE))
                            .fill(if is_restore { Color32::from_rgb(16, 145, 90) } else { Color32::from_rgb(185, 35, 35) })
                            .rounding(egui::Rounding::same(6.0)),
                    );

                    if confirm_btn.clicked() {
                        confirmed_action = state.action.clone();
                        state.is_open = false;
                    }
                });
            });
        });

    confirmed_action
}
