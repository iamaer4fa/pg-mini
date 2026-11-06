use std::collections::BTreeMap;
use egui::{Color32, RichText, ScrollArea, Ui};
use crate::models::{DatabaseItem, TableItem};

#[derive(Default)]
pub struct Sidebar {
    pub search_query: String,
    pub selected_table: Option<(String, String)>, // (schema, table_name)
}

pub enum SidebarAction {
    SwitchDatabase(String),
    RefreshDatabases,
    SelectTable { schema: String, table: String, limit: usize },
    CountRows { schema: String, table: String },
    ViewColumns { schema: String, table: String },
    OpenInEditor { schema: String, table: String },
    OpenQueryToolForDb(String),
    OpenAddColumn { schema: String, table: String },
    OpenCreateTable { schema: String },
    ConfirmDropTable { schema: String, table: String },
    ConfirmTruncateTable { schema: String, table: String },
    ConfirmDropDatabase { db_name: String },
    RefreshTables,
}

impl Sidebar {
    pub fn show(
        &mut self,
        ui: &mut Ui,
        databases: &[DatabaseItem],
        tables: &[TableItem],
        current_db: &str,
        is_connected: bool,
        is_busy: bool,
    ) -> Option<SidebarAction> {
        let mut action: Option<SidebarAction> = None;

        ui.vertical(|ui| {
            // === DATABASES SECTION ===
            ui.horizontal(|ui| {
                ui.heading(RichText::new("🗄️ Databases").size(15.0));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.add_enabled(is_connected && !is_busy, egui::Button::new("🔄").small())
                        .on_hover_text("Refresh databases list")
                        .clicked()
                    {
                        action = Some(SidebarAction::RefreshDatabases);
                    }
                });
            });

            if !is_connected {
                ui.add_space(8.0);
                ui.vertical_centered(|ui| {
                    ui.label(RichText::new("Disconnected").color(Color32::GRAY));
                    ui.label(RichText::new("Connect to browse databases").size(11.0).color(Color32::DARK_GRAY));
                });
                return;
            }

            // List of databases
            egui::Frame::none()
                .fill(Color32::from_rgb(18, 21, 28))
                .stroke(egui::Stroke::new(1.0_f32, Color32::from_rgb(38, 43, 56)))
                .rounding(egui::Rounding::same(6.0))
                .inner_margin(4.0)
                .show(ui, |ui| {
                    for db in databases {
                        let is_current = db.name == current_db || db.is_current;
                        let dot_color = if is_current {
                            Color32::from_rgb(34, 197, 94)
                        } else {
                            Color32::from_rgb(120, 125, 140)
                        };

                        ui.horizontal(|ui| {
                            ui.label(RichText::new("●").size(10.0).color(dot_color));

                            let db_btn = ui.selectable_label(
                                is_current,
                                RichText::new(&db.name).strong(),
                            );

                            if !is_current && db_btn.clicked() {
                                action = Some(SidebarAction::SwitchDatabase(db.name.clone()));
                            }

                            if let Some(ref sz) = db.size {
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    ui.label(RichText::new(sz).size(10.0).color(Color32::GRAY));
                                });
                            }

                            // Right-click context menu on Database
                            db_btn.context_menu(|ui| {
                                ui.label(RichText::new(format!("🗄️ Database: {}", db.name)).strong());
                                ui.separator();

                                if !is_current {
                                    if ui.button("🔌 Switch to this Database").clicked() {
                                        action = Some(SidebarAction::SwitchDatabase(db.name.clone()));
                                        ui.close_menu();
                                    }
                                }

                                if ui.button("⚡ Open Query Tool").clicked() {
                                    action = Some(SidebarAction::OpenQueryToolForDb(db.name.clone()));
                                    ui.close_menu();
                                }

                                if ui.button("➕ Create Table...").clicked() {
                                    action = Some(SidebarAction::OpenCreateTable { schema: "public".to_string() });
                                    ui.close_menu();
                                }

                                if ui.button("🔄 Refresh Tables").clicked() {
                                    action = Some(SidebarAction::RefreshTables);
                                    ui.close_menu();
                                }

                                if !is_current {
                                    ui.separator();
                                    if ui.button(RichText::new("🗑️ Drop Database...").color(Color32::LIGHT_RED)).clicked() {
                                        action = Some(SidebarAction::ConfirmDropDatabase { db_name: db.name.clone() });
                                        ui.close_menu();
                                    }
                                }
                            });
                        });
                    }
                });

            ui.add_space(8.0);
            ui.separator();
            ui.add_space(4.0);

            // === TABLES & VIEWS SECTION ===
            ui.horizontal(|ui| {
                ui.heading(RichText::new("📦 Tables & Views").size(15.0));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.small_button("➕").on_hover_text("Create new table").clicked() {
                        action = Some(SidebarAction::OpenCreateTable { schema: "public".to_string() });
                    }
                    if ui.add_enabled(is_connected && !is_busy, egui::Button::new("🔄").small())
                        .on_hover_text("Refresh tables list")
                        .clicked()
                    {
                        action = Some(SidebarAction::RefreshTables);
                    }
                });
            });

            ui.add_space(4.0);

            // Search filter box
            ui.horizontal(|ui| {
                ui.label("🔍");
                ui.add(
                    egui::TextEdit::singleline(&mut self.search_query)
                        .hint_text("Filter tables...")
                        .desired_width(ui.available_width() - 25.0),
                );
                if !self.search_query.is_empty() && ui.small_button("✖").clicked() {
                    self.search_query.clear();
                }
            });

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
                    RichText::new(format!("{} table(s) in {}", filtered.len(), current_db))
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

            if grouped.is_empty() {
                ui.add_space(16.0);
                ui.vertical(|ui| {
                    ui.label(RichText::new("No tables found").color(Color32::GRAY));
                    ui.add_space(6.0);
                    if ui.button("➕ Create Table").clicked() {
                        action = Some(SidebarAction::OpenCreateTable { schema: "public".to_string() });
                    }
                });
            } else {
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

                                    let btn = ui.selectable_label(is_selected, label_text);

                                    if btn.clicked() {
                                        self.selected_table =
                                            Some((item.schema.clone(), item.name.clone()));
                                        // Default limit: 100 rows for fast viewing
                                        action = Some(SidebarAction::SelectTable {
                                            schema: item.schema.clone(),
                                            table: item.name.clone(),
                                            limit: 100,
                                        });
                                    }

                                    // Right-click context menu on Table (PGAdmin-style)
                                    btn.context_menu(|ui| {
                                        ui.label(RichText::new(format!("📄 Table: {}.{}", item.schema, item.name)).strong());
                                        ui.separator();

                                        // View / Edit Data submenu with limits
                                        ui.menu_button("📊 View Data ▾", |ui| {
                                            if ui.button("⚡ First 100 Rows (Default, Fast)").clicked() {
                                                self.selected_table = Some((item.schema.clone(), item.name.clone()));
                                                action = Some(SidebarAction::SelectTable {
                                                    schema: item.schema.clone(),
                                                    table: item.name.clone(),
                                                    limit: 100,
                                                });
                                                ui.close_menu();
                                            }
                                            if ui.button("📊 First 500 Rows").clicked() {
                                                self.selected_table = Some((item.schema.clone(), item.name.clone()));
                                                action = Some(SidebarAction::SelectTable {
                                                    schema: item.schema.clone(),
                                                    table: item.name.clone(),
                                                    limit: 500,
                                                });
                                                ui.close_menu();
                                            }
                                            if ui.button("📊 First 1,000 Rows").clicked() {
                                                self.selected_table = Some((item.schema.clone(), item.name.clone()));
                                                action = Some(SidebarAction::SelectTable {
                                                    schema: item.schema.clone(),
                                                    table: item.name.clone(),
                                                    limit: 1000,
                                                });
                                                ui.close_menu();
                                            }
                                            if ui.button("📊 All Rows (Unlimited)").clicked() {
                                                self.selected_table = Some((item.schema.clone(), item.name.clone()));
                                                action = Some(SidebarAction::SelectTable {
                                                    schema: item.schema.clone(),
                                                    table: item.name.clone(),
                                                    limit: 100000,
                                                });
                                                ui.close_menu();
                                            }
                                        });

                                        if ui.button("⚡ Query Tool (in SQL Editor)").clicked() {
                                            action = Some(SidebarAction::OpenInEditor {
                                                schema: item.schema.clone(),
                                                table: item.name.clone(),
                                            });
                                            ui.close_menu();
                                        }

                                        if ui.button("➕ Add Column...").clicked() {
                                            action = Some(SidebarAction::OpenAddColumn {
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

                                        if ui.button("🔢 Count Rows (SELECT COUNT)").clicked() {
                                            action = Some(SidebarAction::CountRows {
                                                schema: item.schema.clone(),
                                                table: item.name.clone(),
                                            });
                                            ui.close_menu();
                                        }

                                        ui.separator();

                                        if ui.button("🧹 Truncate Table...").clicked() {
                                            action = Some(SidebarAction::ConfirmTruncateTable {
                                                schema: item.schema.clone(),
                                                table: item.name.clone(),
                                            });
                                            ui.close_menu();
                                        }

                                        if ui.button(RichText::new("🗑️ Drop Table...").color(Color32::LIGHT_RED)).clicked() {
                                            action = Some(SidebarAction::ConfirmDropTable {
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
            }
        });

        action
    }
}
