use crate::system_stats::SystemStatsSnapshot;
use egui::{Color32, RichText, Ui};
use std::time::Instant;

pub fn render_system_stats_view(
    ui: &mut Ui,
    snapshot: Option<&SystemStatsSnapshot>,
    is_busy: bool,
    updated_at: Option<Instant>,
) -> bool {
    let mut refresh = false;
    ui.horizontal(|ui| {
        ui.heading("System Stats");
        ui.label(
            RichText::new("Host-level metrics from the system_stats PostgreSQL extension")
                .small()
                .color(Color32::GRAY),
        );
        if let Some(updated_at) = updated_at {
            ui.label(
                RichText::new(format!("Updated {}s ago", updated_at.elapsed().as_secs()))
                    .small()
                    .color(Color32::GRAY),
            );
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui
                .add_enabled(!is_busy, egui::Button::new("↻ Refresh"))
                .clicked()
            {
                refresh = true;
            }
        });
    });
    ui.separator();

    let Some(snapshot) = snapshot else {
        ui.centered_and_justified(|ui| {
            ui.label(if is_busy {
                "Loading system statistics..."
            } else {
                "System statistics have not been loaded."
            });
        });
        return refresh;
    };

    let Some(schema) = snapshot.extension_schema.as_deref() else {
        ui.vertical(|ui| {
            ui.label(
                RichText::new("The system_stats extension is not installed in this database.")
                    .strong(),
            );
            ui.label(
                "The extension must first be installed on the PostgreSQL server by an administrator. \
                 Then enable it in this database:",
            );
            egui::Frame::group(ui.style()).show(ui, |ui| {
                ui.label(RichText::new("CREATE EXTENSION system_stats;").monospace());
            });
            ui.label(
                RichText::new(
                    "Access is restricted by the extension. A DBA can grant the monitor_system_stats \
                     role to the monitoring account. Some metrics may be NULL or unavailable depending \
                     on the operating system and server permissions.",
                )
                .color(Color32::GRAY),
            );
            ui.hyperlink_to(
                "system_stats installation and function documentation",
                "https://github.com/EnterpriseDB/system_stats",
            );
            ui.add_space(8.0);
            if ui
                .add_enabled(!is_busy, egui::Button::new("↻ Check again"))
                .clicked()
            {
                refresh = true;
            }
        });
        return refresh;
    };

    ui.label(
        RichText::new(format!(
            "Extension found in schema '{schema}'. Measurements describe the host running PostgreSQL, not the desktop running pg-mini."
        ))
        .color(Color32::GRAY),
    );
    ui.hyperlink_to(
        "system_stats documentation",
        "https://github.com/EnterpriseDB/system_stats",
    );

    egui::ScrollArea::vertical()
        .auto_shrink([false; 2])
        .show(ui, |ui| {
            for section in &snapshot.sections {
                egui::CollapsingHeader::new(
                    RichText::new(section.title).strong().size(16.0),
                )
                .id_salt(("system_stats_section", section.function_name))
                .default_open(section.function_name != "pg_sys_cpu_memory_by_process")
                .show(ui, |ui| {
                    match &section.result {
                    Ok(result) if result.rows.is_empty() => {
                        ui.label(
                            RichText::new("No measurements returned.")
                                .color(Color32::GRAY)
                                .italics(),
                        );
                    }
                    Ok(result) => {
                        if section.is_truncated {
                            ui.label(
                                RichText::new("Showing the first 200 process rows.")
                                    .small()
                                    .color(Color32::GRAY),
                            );
                        }
                        egui::ScrollArea::horizontal()
                            .id_salt(("system_stats_table", section.function_name))
                            .show(ui, |ui| {
                                egui::Grid::new(("system_stats_grid", section.function_name))
                                    .striped(true)
                                    .show(ui, |ui| {
                                        for column in &result.columns {
                                            ui.label(RichText::new(column).strong());
                                        }
                                        ui.end_row();
                                        for row in &result.rows {
                                            for cell in row {
                                                let label = if cell.is_null {
                                                    RichText::new("NULL")
                                                        .color(Color32::GRAY)
                                                        .italics()
                                                } else if cell.is_number {
                                                    RichText::new(&cell.text).monospace()
                                                } else {
                                                    RichText::new(&cell.text)
                                                };
                                                ui.label(label).on_hover_text(&cell.text);
                                            }
                                            ui.end_row();
                                        }
                                    });
                            });
                    }
                    Err(error) => {
                        ui.label(
                            RichText::new(format!("{}: {error}", section.function_name))
                                .color(Color32::LIGHT_RED)
                                .monospace(),
                        );
                        ui.label(
                            RichText::new(format!(
                                "Check that the connected role can execute {}.{}() (for example, through monitor_system_stats).",
                                schema, section.function_name
                            ))
                            .color(Color32::GRAY)
                            .small(),
                        );
                    }
                    }
                });
            }
        });
    refresh
}
