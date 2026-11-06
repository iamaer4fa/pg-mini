use std::collections::BTreeMap;
use egui::{Color32, RichText, ScrollArea, Ui};
use crate::models::TableItem;

#[derive(Default)]
pub struct Sidebar {
    pub search_query: String,
    pub selected_table: Option<(String, String)>, // (schema, table_name)
}

pub enum SidebarAction {
    SelectTable { schema: String, table: String },
    ViewColumns { schema: String, table: String },
    OpenInEditor { schema: String, table: String },
    RefreshTables,
}

impl Sidebar {
    pub fn show(
        &mut self,
        ui: &mut Ui,
        tables: &[TableItem],
        is_connected: bool,
        is_busy: bool,
    ) -> Option<SidebarAction> {
        let mut action: Option<SidebarAction> = None;

        ui.vertical(|ui| {
            // Header with Refresh
            ui.horizontal(|ui| {
                ui.heading(RichText::new("📦 Tables & Views").size(16.0));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.add_enabled(is_connected && !is_busy, egui::Button::new("🔄").small())
                        .on_hover_text("Refresh tables list")
                        .clicked()
                    {
                        action = Some(SidebarAction::RefreshTables);
                    }
                });
            });

            ui.add_space(4.0);

            // Filter box
            ui.horizontal(|ui| {
                ui.label("🔍");
                ui.add(
                    egui::TextEdit::singleline(&mut self.search_query)
                        .hint_text("Filter tables...")
                        .desired_width(ui.available_width() - 20.0),
                );
                if !self.search_query.is_empty() && ui.small_button("✖").clicked() {
                    self.search_query.clear();
                }
            });

            ui.separator();

            if !is_connected {
                ui.add_space(20.0);
                ui.vertical_centered(|ui| {
                    ui.label(RichText::new("Disconnected").color(Color32::GRAY));
                    ui.label(RichText::new("Connect to browse database tables").size(11.0).color(Color32::DARK_GRAY));
                });
                return;
            }

            // Filter tables
            let query = self.search_query.trim().to_lowercase();
            let filtered: Vec<&TableItem> = tables
                .iter()
                .filter(|t| {
                    if query.is_empty() {
                        true
                    } else {
                        t.name.to_lowercase().contains(&query)
                            || t.schema.to_lowercase().contains(&query)
                    }
                })
                .collect();

            // Total count badge
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(format!("Total: {} item(s)", filtered.len()))
                        .size(11.0)
                        .color(Color32::LIGHT_GRAY),
                );
            });

            ui.add_space(4.0);

            // Group tables by schema
            let mut grouped: BTreeMap<&str, Vec<&TableItem>> = BTreeMap::new();
            for t in &filtered {
                grouped.entry(&t.schema).or_default().push(t);
            }

            ScrollArea::vertical()
                .auto_shrink([false; 2])
                .show(ui, |ui| {
                    for (schema, items) in grouped {
                        egui::CollapsingHeader::new(
                            RichText::new(format!("📁 {} ({})", schema, items.len())).strong(),
                        )
                        .default_open(true)
                        .show(ui, |ui| {
                            for item in items {
                                let is_selected = self.selected_table.as_ref()
                                    == Some(&(item.schema.clone(), item.name.clone()));

                                let is_view = item.table_type.contains("VIEW");
                                let icon = if is_view { "👁" } else { "📄" };
                                let label_text = format!("{} {}", icon, item.name);

                                let row_resp = ui.horizontal(|ui| {
                                    let btn = ui.selectable_label(is_selected, label_text);
                                    if btn.clicked() {
                                        self.selected_table =
                                            Some((item.schema.clone(), item.name.clone()));
                                        action = Some(SidebarAction::SelectTable {
                                            schema: item.schema.clone(),
                                            table: item.name.clone(),
                                        });
                                    }

                                    // Context buttons on hover / select
                                    if btn.hovered() || is_selected {
                                        if ui.small_button("📋")
                                            .on_hover_text("View Columns / Structure")
                                            .clicked()
                                        {
                                            action = Some(SidebarAction::ViewColumns {
                                                schema: item.schema.clone(),
                                                table: item.name.clone(),
                                            });
                                        }
                                        if ui.small_button("⚡")
                                            .on_hover_text("Query in SQL Editor")
                                            .clicked()
                                        {
                                            action = Some(SidebarAction::OpenInEditor {
                                                schema: item.schema.clone(),
                                                table: item.name.clone(),
                                            });
                                        }
                                    }
                                });

                                row_resp.response.context_menu(|ui| {
                                    if ui.button("📊 View Data (Top 100)").clicked() {
                                        self.selected_table =
                                            Some((item.schema.clone(), item.name.clone()));
                                        action = Some(SidebarAction::SelectTable {
                                            schema: item.schema.clone(),
                                            table: item.name.clone(),
                                        });
                                        ui.close_menu();
                                    }
                                    if ui.button("📋 View Table Schema").clicked() {
                                        action = Some(SidebarAction::ViewColumns {
                                            schema: item.schema.clone(),
                                            table: item.name.clone(),
                                        });
                                        ui.close_menu();
                                    }
                                    if ui.button("⚡ Open in SQL Editor").clicked() {
                                        action = Some(SidebarAction::OpenInEditor {
                                            schema: item.schema.clone(),
                                            table: item.name.clone(),
                                        });
                                        ui.close_menu();
                                    }
                                });
                            }
                        });
                    }
                });
        });

        action
    }
}
