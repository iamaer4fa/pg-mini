use egui::{Color32, RichText, Ui};
use egui_extras::{Column, TableBuilder};
use crate::models::ColumnItem;

pub enum ColumnsViewAction {
    OpenAddColumn { schema: String, table: String },
    ConfirmDropColumn { schema: String, table: String, column: String },
    CopyName(String),
}

pub fn render_columns_view(
    ui: &mut Ui,
    schema: &str,
    table: &str,
    columns: &[ColumnItem],
    is_busy: bool,
) -> Option<ColumnsViewAction> {
    let mut action: Option<ColumnsViewAction> = None;

    ui.horizontal(|ui| {
        ui.heading(RichText::new(format!("📐 Schema: {}.{}", schema, table)).size(16.0));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if !table.is_empty() {
                if ui.add(
                    egui::Button::new(RichText::new("➕ Add Column...").strong().color(Color32::WHITE))
                        .fill(Color32::from_rgb(16, 145, 90)),
                )
                .clicked()
                {
                    action = Some(ColumnsViewAction::OpenAddColumn {
                        schema: schema.to_string(),
                        table: table.to_string(),
                    });
                }
            }

            if is_busy {
                ui.label(RichText::new("⏳ Loading columns...").color(Color32::YELLOW));
            } else {
                ui.label(format!("Total: {} columns", columns.len()));
            }
        });
    });

    ui.separator();
    ui.add_space(4.0);

    if columns.is_empty() {
        ui.vertical_centered(|ui| {
            ui.add_space(30.0);
            ui.label(RichText::new("No columns found or table not selected").color(Color32::GRAY));
            if !table.is_empty() {
                if ui.button("➕ Add First Column").clicked() {
                    action = Some(ColumnsViewAction::OpenAddColumn {
                        schema: schema.to_string(),
                        table: table.to_string(),
                    });
                }
            }
        });
        return action;
    }

    let text_height = egui::TextStyle::Body.resolve(ui.style()).size;
    let row_height = text_height + 8.0;

    TableBuilder::new(ui)
        .striped(true)
        .resizable(true)
        .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
        .column(Column::auto().at_least(35.0)) // #
        .column(Column::initial(160.0).at_least(80.0).resizable(true)) // Column Name
        .column(Column::initial(140.0).at_least(70.0).resizable(true)) // Data Type
        .column(Column::auto().at_least(50.0)) // Primary Key
        .column(Column::auto().at_least(60.0)) // Nullable
        .column(Column::initial(150.0).at_least(80.0).resizable(true)) // Default
        .column(Column::auto().at_least(70.0)) // Actions
        .header(24.0, |mut header| {
            header.col(|ui| { ui.strong("#"); });
            header.col(|ui| { ui.strong("Column Name"); });
            header.col(|ui| { ui.strong("Data Type"); });
            header.col(|ui| { ui.strong("PK"); });
            header.col(|ui| { ui.strong("Nullable"); });
            header.col(|ui| { ui.strong("Default Value"); });
            header.col(|ui| { ui.strong("Actions"); });
        })
        .body(|body| {
            body.rows(row_height, columns.len(), |mut row| {
                let idx = row.index();
                if let Some(col) = columns.get(idx) {
                    // #
                    row.col(|ui| {
                        ui.label(RichText::new((idx + 1).to_string()).size(11.0).color(Color32::DARK_GRAY));
                    });

                    // Name
                    row.col(|ui| {
                        let name_label = ui.selectable_label(false, RichText::new(&col.name).strong().monospace());
                        if name_label.clicked() {
                            action = Some(ColumnsViewAction::CopyName(col.name.clone()));
                        }
                    });

                    // Type
                    row.col(|ui| {
                        ui.label(RichText::new(&col.data_type).monospace().color(Color32::from_rgb(100, 180, 240)));
                    });

                    // PK
                    row.col(|ui| {
                        if col.is_primary_key {
                            ui.label(RichText::new("🔑 YES").color(Color32::from_rgb(255, 200, 50)).strong());
                        } else {
                            ui.label(RichText::new("-").color(Color32::DARK_GRAY));
                        }
                    });

                    // Nullable
                    row.col(|ui| {
                        if col.is_nullable {
                            ui.label(RichText::new("YES").color(Color32::LIGHT_GRAY));
                        } else {
                            ui.label(RichText::new("NO").color(Color32::from_rgb(255, 120, 120)).strong());
                        }
                    });

                    // Default
                    row.col(|ui| {
                        if let Some(ref def) = col.default_value {
                            ui.label(RichText::new(def).monospace().color(Color32::LIGHT_BLUE));
                        } else {
                            ui.label(RichText::new("-").color(Color32::DARK_GRAY));
                        }
                    });

                    // Actions
                    row.col(|ui| {
                        if ui.small_button("🗑️ Drop").on_hover_text("Drop this column").clicked() {
                            action = Some(ColumnsViewAction::ConfirmDropColumn {
                                schema: schema.to_string(),
                                table: table.to_string(),
                                column: col.name.clone(),
                            });
                        }
                    });
                }
            });
        });

    action
}
