use egui::{Color32, Key, RichText, ScrollArea, Ui};
use crate::models::QueryResult;
use crate::ui::data_table::{render_data_table, DataTableAction, DataTableState};

pub struct SqlEditor {
    pub sql_text: String,
    pub history: Vec<String>,
    pub selected_history_idx: Option<usize>,
    pub data_state: DataTableState,
    pub editor_height: f32,
}

impl Default for SqlEditor {
    fn default() -> Self {
        Self {
            sql_text: "SELECT table_schema, table_name, table_type \nFROM information_schema.tables \nWHERE table_schema = 'public' \nORDER BY table_name;".to_string(),
            history: Vec::new(),
            selected_history_idx: None,
            data_state: DataTableState::default(),
            editor_height: 180.0,
        }
    }
}

pub enum SqlEditorAction {
    ExecuteQuery(String),
    CopyCell(String),
    ExportCsv,
}

impl SqlEditor {
    pub fn show(
        &mut self,
        ui: &mut Ui,
        last_result: &Option<Result<QueryResult, String>>,
        is_connected: bool,
        is_busy: bool,
    ) -> Option<SqlEditorAction> {
        let mut action: Option<SqlEditorAction> = None;

        // Check keyboard shortcut (Ctrl+Enter or F5)
        let ctrl_enter = ui.input(|i| {
            (i.modifiers.command || i.modifiers.ctrl) && i.key_pressed(Key::Enter)
                || i.key_pressed(Key::F5)
        });

        if ctrl_enter && is_connected && !is_busy && !self.sql_text.trim().is_empty() {
            let query = self.sql_text.trim().to_string();
            self.add_history(&query);
            return Some(SqlEditorAction::ExecuteQuery(query));
        }

        // Toolbar
        ui.horizontal(|ui| {
            // Run Button
            let run_btn = ui.add_enabled(
                is_connected && !is_busy && !self.sql_text.trim().is_empty(),
                egui::Button::new(
                    RichText::new(if is_busy { "⏳ Running..." } else { "▶ Run Query (Ctrl+Enter)" })
                        .strong()
                        .color(Color32::WHITE),
                )
                .fill(Color32::from_rgb(16, 145, 90)),
            );

            if run_btn.clicked() {
                let query = self.sql_text.trim().to_string();
                self.add_history(&query);
                action = Some(SqlEditorAction::ExecuteQuery(query));
            }

            if ui.button("Clear").clicked() {
                self.sql_text.clear();
            }

            ui.separator();

            // Quick SQL Templates
            egui::ComboBox::from_id_salt("sql_templates")
                .selected_text("Quick Snippets ▾")
                .width(130.0)
                .show_ui(ui, |ui| {
                    if ui.button("SELECT * FROM public.<table_name> LIMIT 50;").clicked() {
                        self.sql_text = "SELECT * FROM public.your_table LIMIT 50;".to_string();
                        ui.close_menu();
                    }
                    if ui.button("SELECT count(*) FROM public.<table_name>;").clicked() {
                        self.sql_text = "SELECT count(*) FROM public.your_table;".to_string();
                        ui.close_menu();
                    }
                    if ui.button("List All Tables in Public Schema").clicked() {
                        self.sql_text = "SELECT table_name FROM information_schema.tables WHERE table_schema = 'public' ORDER BY table_name;".to_string();
                        ui.close_menu();
                    }
                    if ui.button("Show PostgreSQL Server Info").clicked() {
                        self.sql_text = "SELECT current_database(), current_user, version(), inet_server_addr(), inet_server_port();".to_string();
                        ui.close_menu();
                    }
                    if ui.button("Show Database Sizes").clicked() {
                        self.sql_text = "SELECT datname, pg_size_pretty(pg_database_size(datname)) FROM pg_database ORDER BY pg_database_size(datname) DESC;".to_string();
                        ui.close_menu();
                    }
                });

            // History dropdown
            if !self.history.is_empty() {
                egui::ComboBox::from_id_salt("sql_history")
                    .selected_text("History ▾")
                    .width(110.0)
                    .show_ui(ui, |ui| {
                        for (_idx, q) in self.history.iter().enumerate().rev() {
                            let preview = if q.len() > 40 {
                                format!("{}...", &q[..40].replace('\n', " "))
                            } else {
                                q.replace('\n', " ")
                            };
                            if ui.button(preview).clicked() {
                                self.sql_text = q.clone();
                                ui.close_menu();
                            }
                        }
                    });
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if !is_connected {
                    ui.label(RichText::new("Disconnected").color(Color32::GRAY).size(12.0));
                }
            });
        });

        ui.add_space(4.0);

        // Editor Box
        let available_width = ui.available_width();
        egui::Frame::none()
            .stroke(egui::Stroke::new(1.0_f32, Color32::from_rgb(60, 65, 75)))
            .rounding(4.0)
            .inner_margin(4.0)
            .show(ui, |ui| {
                ui.add(
                    egui::TextEdit::multiline(&mut self.sql_text)
                        .font(egui::TextStyle::Monospace)
                        .desired_width(available_width)
                        .desired_rows(6)
                        .hint_text("-- Type SQL query here (e.g. SELECT * FROM my_table;)\n-- Press Ctrl+Enter to execute"),
                );
            });

        ui.add_space(6.0);
        ui.separator();
        ui.add_space(4.0);

        // Results Section
        match last_result {
            Some(Ok(res)) => {
                if let Some(table_act) = render_data_table(
                    ui,
                    &mut self.data_state,
                    res,
                    false,
                    None,
                ) {
                    match table_act {
                        DataTableAction::CopyCell(text) => {
                            action = Some(SqlEditorAction::CopyCell(text));
                        }
                        DataTableAction::ExportCsv => {
                            action = Some(SqlEditorAction::ExportCsv);
                        }
                        _ => {}
                    }
                }
            }
            Some(Err(err)) => {
                egui::Frame::none()
                    .fill(Color32::from_rgb(50, 15, 15))
                    .stroke(egui::Stroke::new(1.0_f32, Color32::from_rgb(220, 50, 50)))
                    .rounding(6.0)
                    .inner_margin(12.0)
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new("❌ Query Execution Error").color(Color32::LIGHT_RED).strong().size(15.0));
                        });
                        ui.add_space(4.0);
                        ScrollArea::vertical()
                            .max_height(150.0)
                            .show(ui, |ui| {
                                ui.label(RichText::new(err).monospace().color(Color32::from_rgb(255, 180, 180)));
                            });
                    });
            }
            None => {
                ui.vertical_centered(|ui| {
                    ui.add_space(40.0);
                    ui.label(RichText::new("No query executed yet").color(Color32::GRAY).size(14.0));
                    ui.label(RichText::new("Press Ctrl+Enter or click 'Run Query' to execute.").color(Color32::DARK_GRAY).size(12.0));
                });
            }
        }

        action
    }

    fn add_history(&mut self, query: &str) {
        if self.history.last().map(|s| s.as_str()) != Some(query) {
            self.history.push(query.to_string());
            if self.history.len() > 30 {
                self.history.remove(0);
            }
        }
    }
}
