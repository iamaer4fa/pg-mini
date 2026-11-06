use egui::{Color32, RichText, Ui};
use egui_extras::{Column, TableBuilder};
use crate::models::ColumnItem;

pub fn render_columns_view(
    ui: &mut Ui,
    schema: &str,
    table: &str,
    columns: &[ColumnItem],
    is_busy: bool,
) {
    ui.horizontal(|ui| {
        ui.heading(RichText::new(format!("📐 Schema: {}.{}", schema, table)).size(16.0));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
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
        });
        return;
    }

    let text_height = egui::TextStyle::Body.resolve(ui.style()).size;
    let row_height = text_height + 8.0;

    TableBuilder::new(ui)
        .striped(true)
        .resizable(true)
        .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
        .column(Column::auto().at_least(40.0)) // #
        .column(Column::initial(160.0).at_least(80.0).resizable(true)) // Column Name
        .column(Column::initial(140.0).at_least(70.0).resizable(true)) // Data Type
        .column(Column::auto().at_least(60.0)) // Primary Key
        .column(Column::auto().at_least(60.0)) // Nullable
        .column(Column::remainder().at_least(100.0)) // Default
        .header(24.0, |mut header| {
            header.col(|ui| { ui.strong("#"); });
            header.col(|ui| { ui.strong("Column Name"); });
            header.col(|ui| { ui.strong("Data Type"); });
            header.col(|ui| { ui.strong("PK"); });
            header.col(|ui| { ui.strong("Nullable"); });
            header.col(|ui| { ui.strong("Default Value"); });
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
                        ui.label(RichText::new(&col.name).strong().monospace());
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
                }
            });
        });
}
