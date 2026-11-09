use egui::{Color32, RichText, Window};
use crate::models::{AddColumnForm, ConfirmAction, ConfirmDialogState, CreateTableForm};

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

        Window::new(format!("➕ Add Column to {}.{}", self.form.schema, self.form.table))
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
                                        if ui.selectable_value(&mut self.custom_type, t.to_string(), *t).clicked() {
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
                        ui.checkbox(&mut self.form.is_nullable, "Allow NULL (uncheck for NOT NULL)");
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
                ui.label(RichText::new("Generated SQL:").size(11.0).color(Color32::GRAY));
                egui::Frame::none()
                    .fill(Color32::from_rgb(25, 28, 35))
                    .stroke(egui::Stroke::new(1.0_f32, Color32::from_rgb(50, 55, 65)))
                    .rounding(4.0)
                    .inner_margin(6.0)
                    .show(ui, |ui| {
                        ui.label(RichText::new(&sql_preview).monospace().color(Color32::from_rgb(100, 200, 255)));
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
                            egui::Button::new(RichText::new("➕ Add Column").strong().color(Color32::WHITE))
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
    let col = if form.column_name.is_empty() { "new_column" } else { form.column_name.trim() };
    let mut sql = format!("ALTER TABLE \"{}\".\"{}\" ADD COLUMN \"{}\" {}", form.schema, form.table, col, form.data_type.trim());
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
                let tbl_name = if self.form.table_name.is_empty() { "new_table" } else { self.form.table_name.trim() };
                let sql_preview = if self.form.add_id_pk {
                    format!("CREATE TABLE \"{}\".\"{}\" (\n    id SERIAL PRIMARY KEY\n);", self.form.schema, tbl_name)
                } else {
                    format!("CREATE TABLE \"{}\".\"{}\" ();", self.form.schema, tbl_name)
                };

                ui.label(RichText::new("Generated SQL:").size(11.0).color(Color32::GRAY));
                egui::Frame::none()
                    .fill(Color32::from_rgb(25, 28, 35))
                    .stroke(egui::Stroke::new(1.0_f32, Color32::from_rgb(50, 55, 65)))
                    .rounding(4.0)
                    .inner_margin(6.0)
                    .show(ui, |ui| {
                        ui.label(RichText::new(&sql_preview).monospace().color(Color32::from_rgb(100, 200, 255)));
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
                            egui::Button::new(RichText::new("➕ Create Table").strong().color(Color32::WHITE))
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
                ui.label(RichText::new(if is_restore { "⚠ Confirm Restore" } else { "⚠ Warning" })
                    .color(Color32::LIGHT_RED).strong().size(16.0));
            });
            ui.separator();
            ui.add_space(6.0);

            ui.label(&state.message);
            let is_restore = matches!(
                state.action.as_ref(),
                Some(crate::models::ConfirmAction::RestoreDatabase { .. })
            );
            ui.label(RichText::new(if is_restore {
                "Restoring may permanently change or delete database data."
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
                    let confirm_btn = ui.add_enabled(
                        !is_busy,
                        egui::Button::new(RichText::new(if is_restore { "Restore Database" } else { "Yes, Delete" })
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
