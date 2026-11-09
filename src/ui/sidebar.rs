use crate::models::{
    DatabaseItem, DbObjectItem, DbObjectKind, DbObjectRef, ObjectTreeKey, TableItem,
};
use crate::sql_ddl::{DdlForm, DdlFormKind};
use egui::{Color32, RichText, ScrollArea, Ui};
use std::collections::{BTreeMap, HashMap, HashSet};

#[derive(Default)]
pub struct Sidebar {
    pub search_query: String,
    pub selected_table: Option<(String, String)>, // (schema, table_name)
    schemas: Option<Vec<String>>,
    schemas_loading: bool,
    schemas_error: Option<String>,
    objects: HashMap<ObjectTreeKey, Vec<DbObjectItem>>,
    objects_loading: HashSet<ObjectTreeKey>,
    objects_errors: HashMap<ObjectTreeKey, String>,
}

pub enum SidebarAction {
    SwitchDatabase(String),
    RefreshDatabases,
    SelectTable {
        schema: String,
        table: String,
        limit: usize,
    },
    CountRows {
        schema: String,
        table: String,
    },
    ViewColumns {
        schema: String,
        table: String,
    },
    OpenInEditor {
        schema: String,
        table: String,
    },
    OpenQueryToolForDb(String),
    OpenAddColumn {
        schema: String,
        table: String,
    },
    OpenCreateTable {
        schema: String,
    },
    OpenDdlForm(DdlForm),
    ConfirmDropTable {
        schema: String,
        table: String,
    },
    ConfirmTruncateTable {
        schema: String,
        table: String,
    },
    ConfirmDropDatabase {
        db_name: String,
    },
    BackupDatabase {
        db_name: String,
    },
    RestoreDatabase {
        db_name: String,
    },
    RefreshTables,
    RequestSchemas {
        database: String,
    },
    RequestObjects {
        database: String,
        key: ObjectTreeKey,
    },
    RefreshDatabaseTree,
    RefreshObjects {
        database: String,
        key: ObjectTreeKey,
    },
    OpenSql(String),
    RenameObject(DbObjectRef),
    ConfirmDropObject(DbObjectRef),
}

impl Sidebar {
    pub fn reset_object_tree(&mut self) {
        self.schemas = None;
        self.schemas_loading = false;
        self.schemas_error = None;
        self.objects.clear();
        self.objects_loading.clear();
        self.objects_errors.clear();
    }

    pub fn invalidate_object_tree(&mut self) {
        self.objects.clear();
        self.objects_loading.clear();
        self.objects_errors.clear();
    }

    pub fn receive_schemas(&mut self, result: Result<Vec<String>, String>) {
        self.schemas_loading = false;
        match result {
            Ok(schemas) => {
                self.schemas = Some(schemas);
                self.schemas_error = None;
            }
            Err(error) => {
                self.schemas = None;
                self.schemas_error = Some(error);
            }
        }
    }

    pub fn receive_objects(
        &mut self,
        key: &ObjectTreeKey,
        result: Result<Vec<DbObjectItem>, String>,
    ) {
        self.objects_loading.remove(key);
        match result {
            Ok(objects) => {
                self.objects.insert(key.clone(), objects);
                self.objects_errors.remove(key);
            }
            Err(error) => {
                self.objects.remove(key);
                self.objects_errors.insert(key.clone(), error);
            }
        }
    }

    pub fn show_object_browser(
        &mut self,
        ui: &mut Ui,
        databases: &[DatabaseItem],
        current_db: &str,
        server: &str,
        is_connected: bool,
        is_busy: bool,
    ) -> Option<SidebarAction> {
        let mut action = None;

        ui.horizontal(|ui| {
            ui.heading(RichText::new("🧭 Object Browser").size(15.0));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .add_enabled(is_connected && !is_busy, egui::Button::new("🔄").small())
                    .on_hover_text("Refresh database objects")
                    .clicked()
                {
                    self.reset_object_tree();
                    action = Some(SidebarAction::RefreshDatabaseTree);
                }
            });
        });

        if !is_connected {
            ui.add_space(8.0);
            ui.vertical_centered(|ui| {
                ui.label(RichText::new("Disconnected").color(Color32::GRAY));
                ui.label(
                    RichText::new("Connect to browse PostgreSQL objects")
                        .size(11.0)
                        .color(Color32::DARK_GRAY),
                );
            });
            return action;
        }

        ui.horizontal(|ui| {
            ui.label("🔍");
            ui.add(
                egui::TextEdit::singleline(&mut self.search_query)
                    .hint_text("Filter objects...")
                    .desired_width(ui.available_width() - 24.0),
            );
            if !self.search_query.is_empty() && ui.small_button("✖").clicked() {
                self.search_query.clear();
            }
        });
        let filter = self.search_query.trim().to_lowercase();

        ScrollArea::vertical()
            .auto_shrink([false; 2])
            .show(ui, |ui| {
                egui::CollapsingHeader::new(RichText::new(format!("🖥 Servers ({})", 1)).strong())
                    .id_salt("object_browser_servers")
                    .default_open(true)
                    .show(ui, |ui| {
                        let server_name = if server.is_empty() {
                            "PostgreSQL".to_string()
                        } else {
                            format!("PostgreSQL ({server})")
                        };
                        egui::CollapsingHeader::new(
                            RichText::new(format!("🐘 {server_name}")).strong(),
                        )
                        .id_salt(("object_browser_server", server_name.clone()))
                        .default_open(true)
                        .show(ui, |ui| {
                            let databases_header = egui::CollapsingHeader::new(
                                RichText::new(format!("🗄 Databases ({})", databases.len()))
                                    .strong(),
                            )
                            .id_salt("object_browser_databases")
                            .default_open(true)
                            .show(ui, |ui| {
                                for database in databases {
                                    let is_current =
                                        database.name == current_db || database.is_current;
                                    let db_header = egui::CollapsingHeader::new(
                                        RichText::new(format!(
                                            "{} {}{}",
                                            if is_current { "🟢" } else { "🗄" },
                                            database.name,
                                            database
                                                .size
                                                .as_deref()
                                                .map(|size| format!(" ({size})"))
                                                .unwrap_or_default()
                                        ))
                                        .strong(),
                                    )
                                    .id_salt(("object_browser_database", database.name.clone()))
                                    .default_open(false)
                                    .show(ui, |ui| {
                                        if !is_current {
                                            if ui
                                                .button(format!(
                                                    "Connect to {} to browse",
                                                    database.name
                                                ))
                                                .clicked()
                                            {
                                                action = Some(SidebarAction::SwitchDatabase(
                                                    database.name.clone(),
                                                ));
                                            }
                                            return;
                                        }

                                        self.render_schemas(ui, current_db, &filter, &mut action);
                                        self.render_object_group(
                                            ui,
                                            current_db,
                                            ObjectTreeKey {
                                                schema: None,
                                                kind: DbObjectKind::Extension,
                                            },
                                            &filter,
                                            is_busy,
                                            &mut action,
                                        );
                                    });

                                    db_header.header_response.context_menu(|ui| {
                                        ui.label(
                                            RichText::new(format!("Database: {}", database.name))
                                                .strong(),
                                        );
                                        ui.separator();
                                        if !is_current && ui.button("Connect / Browse").clicked() {
                                            action = Some(SidebarAction::SwitchDatabase(
                                                database.name.clone(),
                                            ));
                                            ui.close_menu();
                                        }
                                        let database_ref = DbObjectRef {
                                            kind: DbObjectKind::Database,
                                            schema: None,
                                            name: database.name.clone(),
                                            identity_arguments: None,
                                        };
                                        ui.menu_button("Create", |ui| {
                                            for kind in [
                                                DbObjectKind::Database,
                                                DbObjectKind::Schema,
                                                DbObjectKind::Extension,
                                            ] {
                                                if ui
                                                    .button(format!(
                                                        "{} {} SQL",
                                                        kind.icon(),
                                                        kind.singular()
                                                    ))
                                                    .clicked()
                                                {
                                                    action = Some(SidebarAction::OpenSql(
                                                        kind.create_sql(
                                                            None,
                                                            &format!(
                                                                "new_{}",
                                                                kind.singular().to_lowercase()
                                                            ),
                                                        ),
                                                    ));
                                                    ui.close_menu();
                                                }
                                            }
                                            if ui.button("Table...").clicked() {
                                                action = Some(SidebarAction::OpenCreateTable {
                                                    schema: "public".to_string(),
                                                });
                                                ui.close_menu();
                                            }
                                        });
                                        if ui.button("Properties").clicked() {
                                            action = Some(SidebarAction::OpenSql(
                                                database_ref.properties_sql(),
                                            ));
                                            ui.close_menu();
                                        }
                                        ui.separator();
                                        if ui.button("Backup...").clicked() {
                                            action = Some(SidebarAction::BackupDatabase {
                                                db_name: database.name.clone(),
                                            });
                                            ui.close_menu();
                                        }
                                        if ui.button("Restore...").clicked() {
                                            action = Some(SidebarAction::RestoreDatabase {
                                                db_name: database.name.clone(),
                                            });
                                            ui.close_menu();
                                        }
                                        if !is_current && ui.button("Rename").clicked() {
                                            action = Some(SidebarAction::RenameObject(
                                                database_ref.clone(),
                                            ));
                                            ui.close_menu();
                                        }
                                        if ui.button("Generate SQL").clicked() {
                                            action = Some(SidebarAction::OpenSql(
                                                database_ref.generate_sql_query(),
                                            ));
                                            ui.close_menu();
                                        }
                                        if ui.button("Refresh").clicked() {
                                            self.reset_object_tree();
                                            action = Some(SidebarAction::RefreshDatabaseTree);
                                            ui.close_menu();
                                        }
                                        ui.separator();
                                        if !is_current
                                            && ui
                                                .button(
                                                    RichText::new("Delete Database...")
                                                        .color(Color32::LIGHT_RED),
                                                )
                                                .clicked()
                                        {
                                            action = Some(SidebarAction::ConfirmDropDatabase {
                                                db_name: database.name.clone(),
                                            });
                                            ui.close_menu();
                                        }
                                    });
                                }
                            });
                            databases_header.header_response.context_menu(|ui| {
                                if ui.button("Refresh Databases").clicked() {
                                    action = Some(SidebarAction::RefreshDatabaseTree);
                                    ui.close_menu();
                                }
                            });
                        });
                    });
            });

        action
    }

    fn render_schemas(
        &mut self,
        ui: &mut Ui,
        database: &str,
        filter: &str,
        action: &mut Option<SidebarAction>,
    ) {
        let schemas_header = egui::CollapsingHeader::new(RichText::new("📁 Schemas").strong())
            .id_salt(("object_browser_schemas", database))
            .default_open(false)
            .show(ui, |ui| {
                if self.schemas.is_none() && !self.schemas_loading {
                    if action.is_none() {
                        self.schemas_loading = true;
                        *action = Some(SidebarAction::RequestSchemas {
                            database: database.to_string(),
                        });
                    }
                }

                if self.schemas_loading {
                    ui.label(RichText::new("Loading schemas...").color(Color32::GRAY));
                    return;
                }
                if let Some(error) = &self.schemas_error {
                    ui.label(RichText::new(error).color(Color32::LIGHT_RED));
                    if ui.button("Retry").clicked() {
                        self.schemas_error = None;
                        self.schemas_loading = true;
                        *action = Some(SidebarAction::RequestSchemas {
                            database: database.to_string(),
                        });
                    }
                    return;
                }

                let schemas: Vec<String> = self
                    .schemas
                    .as_deref()
                    .unwrap_or_default()
                    .iter()
                    .filter(|schema| filter.is_empty() || schema.to_lowercase().contains(filter))
                    .cloned()
                    .collect();
                for schema in schemas {
                    let schema_ref = DbObjectRef {
                        kind: DbObjectKind::Schema,
                        schema: None,
                        name: schema.clone(),
                        identity_arguments: None,
                    };
                    let schema_header =
                        egui::CollapsingHeader::new(RichText::new(format!("📁 {schema}")).strong())
                            .id_salt(("object_browser_schema", database, schema.clone()))
                            .default_open(false)
                            .show(ui, |ui| {
                                for kind in [
                                    DbObjectKind::Table,
                                    DbObjectKind::View,
                                    DbObjectKind::MaterializedView,
                                    DbObjectKind::Sequence,
                                    DbObjectKind::Function,
                                    DbObjectKind::Procedure,
                                    DbObjectKind::Type,
                                ] {
                                    self.render_object_group(
                                        ui,
                                        database,
                                        ObjectTreeKey {
                                            schema: Some(schema.clone()),
                                            kind,
                                        },
                                        filter,
                                        false,
                                        action,
                                    );
                                }
                            });
                    schema_header.header_response.context_menu(|ui| {
                        let create_table = ui.button("Create Table...");
                        if create_table.clicked() {
                            *action = Some(SidebarAction::OpenCreateTable {
                                schema: schema.clone(),
                            });
                            ui.close_menu();
                        }
                        ui.menu_button("Create", |ui| {
                            for kind in [
                                DbObjectKind::View,
                                DbObjectKind::MaterializedView,
                                DbObjectKind::Sequence,
                                DbObjectKind::Function,
                                DbObjectKind::Procedure,
                                DbObjectKind::Type,
                            ] {
                                if ui
                                    .button(format!("{} {}", kind.icon(), kind.singular()))
                                    .clicked()
                                {
                                    *action = Some(SidebarAction::OpenSql(kind.create_sql(
                                        Some(&schema),
                                        &format!(
                                            "new_{}",
                                            kind.singular().to_lowercase().replace(' ', "_")
                                        ),
                                    )));
                                    ui.close_menu();
                                }
                            }
                        });
                        if ui.button("Properties").clicked() {
                            *action = Some(SidebarAction::OpenSql(schema_ref.properties_sql()));
                            ui.close_menu();
                        }
                        if ui.button("Rename").clicked() {
                            *action = Some(SidebarAction::RenameObject(schema_ref.clone()));
                            ui.close_menu();
                        }
                        if ui.button("Refresh").clicked() {
                            for kind in [
                                DbObjectKind::Table,
                                DbObjectKind::View,
                                DbObjectKind::MaterializedView,
                                DbObjectKind::Sequence,
                                DbObjectKind::Function,
                                DbObjectKind::Procedure,
                                DbObjectKind::Type,
                            ] {
                                let key = ObjectTreeKey {
                                    schema: Some(schema.clone()),
                                    kind,
                                };
                                self.objects.remove(&key);
                                self.objects_errors.remove(&key);
                            }
                        }
                        if ui
                            .button(RichText::new("Delete Schema...").color(Color32::LIGHT_RED))
                            .clicked()
                        {
                            *action = Some(SidebarAction::ConfirmDropObject(schema_ref.clone()));
                            ui.close_menu();
                        }
                        if ui.button("Generate SQL").clicked() {
                            *action = Some(SidebarAction::OpenSql(schema_ref.generate_sql_query()));
                            ui.close_menu();
                        }
                    });
                }
            });
        schemas_header.header_response.context_menu(|ui| {
            if ui.button("Create Schema SQL").clicked() {
                *action = Some(SidebarAction::OpenSql(
                    DbObjectKind::Schema.create_sql(None, "new_schema"),
                ));
                ui.close_menu();
            }
            if ui.button("Refresh").clicked() {
                self.schemas = None;
                self.schemas_error = None;
                self.schemas_loading = true;
                self.invalidate_object_tree();
                *action = Some(SidebarAction::RequestSchemas {
                    database: database.to_string(),
                });
                ui.close_menu();
            }
        });
    }

    fn render_object_group(
        &mut self,
        ui: &mut Ui,
        database: &str,
        key: ObjectTreeKey,
        filter: &str,
        is_busy: bool,
        action: &mut Option<SidebarAction>,
    ) {
        let count = self.objects.get(&key).map(Vec::len);
        let heading = match count {
            Some(count) => format!("{} {} ({count})", key.kind.icon(), key.kind.label()),
            None => format!("{} {}", key.kind.icon(), key.kind.label()),
        };
        let group = egui::CollapsingHeader::new(heading)
            .id_salt((
                "object_browser_group",
                database,
                key.schema.clone(),
                key.kind,
            ))
            .default_open(false)
            .show(ui, |ui| {
                if !self.objects.contains_key(&key) && !self.objects_loading.contains(&key) {
                    if action.is_none() {
                        self.objects_loading.insert(key.clone());
                        *action = Some(SidebarAction::RequestObjects {
                            database: database.to_string(),
                            key: key.clone(),
                        });
                    }
                }
                if self.objects_loading.contains(&key) {
                    ui.label(RichText::new("Loading...").color(Color32::GRAY));
                    return;
                }
                if let Some(error) = self.objects_errors.get(&key) {
                    ui.label(RichText::new(error).color(Color32::LIGHT_RED));
                    if ui
                        .add_enabled(!is_busy, egui::Button::new("Retry"))
                        .clicked()
                    {
                        self.objects_errors.remove(&key);
                        self.objects_loading.insert(key.clone());
                        *action = Some(SidebarAction::RequestObjects {
                            database: database.to_string(),
                            key: key.clone(),
                        });
                    }
                    return;
                }
                let objects = self.objects.get(&key).cloned().unwrap_or_default();
                if objects.is_empty() {
                    ui.label(RichText::new("No objects").color(Color32::GRAY));
                }
                for item in objects
                    .into_iter()
                    .filter(|item| filter.is_empty() || item.name.to_lowercase().contains(filter))
                {
                    let object = DbObjectRef {
                        kind: key.kind,
                        schema: key.schema.clone(),
                        name: item.name.clone(),
                        identity_arguments: item.identity_arguments.clone(),
                    };
                    let shown_name = item
                        .identity_arguments
                        .as_deref()
                        .map(|arguments| format!("{}({arguments})", item.name))
                        .unwrap_or_else(|| item.name.clone());
                    let response =
                        ui.selectable_label(false, format!("{} {}", key.kind.icon(), shown_name));
                    if matches!(
                        key.kind,
                        DbObjectKind::Table | DbObjectKind::View | DbObjectKind::MaterializedView
                    ) && response.clicked()
                    {
                        if let Some(schema) = key.schema.clone() {
                            *action = Some(SidebarAction::SelectTable {
                                schema,
                                table: item.name.clone(),
                                limit: 100,
                            });
                        }
                    }
                    response.context_menu(|ui| {
                        if matches!(
                            key.kind,
                            DbObjectKind::Table
                                | DbObjectKind::View
                                | DbObjectKind::MaterializedView
                        ) {
                            if ui.button("View Data (Top 100)").clicked() {
                                if let Some(schema) = key.schema.clone() {
                                    *action = Some(SidebarAction::SelectTable {
                                        schema,
                                        table: item.name.clone(),
                                        limit: 100,
                                    });
                                }
                                ui.close_menu();
                            }
                            if ui.button("Query in SQL Editor").clicked() {
                                *action = Some(SidebarAction::OpenSql(format!(
                                    "SELECT * FROM {} LIMIT 100;",
                                    object.qualified_name()
                                )));
                                ui.close_menu();
                            }
                            if key.kind == DbObjectKind::Table
                                && ui.button("Add Column...").clicked()
                            {
                                if let Some(schema) = key.schema.clone() {
                                    *action = Some(SidebarAction::OpenAddColumn {
                                        schema,
                                        table: item.name.clone(),
                                    });
                                }
                                ui.close_menu();
                            }
                            if key.kind == DbObjectKind::Table {
                                ui.menu_button("Create", |ui| {
                                    for (label, kind) in [
                                        ("Index...", DdlFormKind::Index),
                                        ("Foreign Key...", DdlFormKind::ForeignKey),
                                        ("Constraint...", DdlFormKind::Constraint),
                                    ] {
                                        if ui.button(label).clicked() {
                                            if let Some(schema) = key.schema.as_deref() {
                                                *action = Some(SidebarAction::OpenDdlForm(
                                                    DdlForm::new(kind, schema, &item.name),
                                                ));
                                            }
                                            ui.close_menu();
                                        }
                                    }
                                });
                                if ui.button("Manage Grants...").clicked() {
                                    if let Some(schema) = key.schema.as_deref() {
                                        *action = Some(SidebarAction::OpenDdlForm(DdlForm::new(
                                            DdlFormKind::Grant,
                                            schema,
                                            &item.name,
                                        )));
                                    }
                                    ui.close_menu();
                                }
                            }
                            if ui.button("View Schema").clicked() {
                                if let Some(schema) = key.schema.clone() {
                                    *action = Some(SidebarAction::ViewColumns {
                                        schema,
                                        table: item.name.clone(),
                                    });
                                }
                                ui.close_menu();
                            }
                            if key.kind == DbObjectKind::Table && ui.button("Count Rows").clicked()
                            {
                                if let Some(schema) = key.schema.clone() {
                                    *action = Some(SidebarAction::CountRows {
                                        schema,
                                        table: item.name.clone(),
                                    });
                                }
                                ui.close_menu();
                            }
                            if key.kind == DbObjectKind::Table
                                && ui
                                    .button(
                                        RichText::new("Truncate Table...")
                                            .color(Color32::LIGHT_RED),
                                    )
                                    .clicked()
                            {
                                if let Some(schema) = key.schema.clone() {
                                    *action = Some(SidebarAction::ConfirmTruncateTable {
                                        schema,
                                        table: item.name.clone(),
                                    });
                                }
                                ui.close_menu();
                            }
                            ui.separator();
                        }
                        if ui.button("Properties").clicked() {
                            *action = Some(SidebarAction::OpenSql(object.properties_sql()));
                            ui.close_menu();
                        }
                        if key.kind != DbObjectKind::Extension && ui.button("Rename").clicked() {
                            *action = Some(SidebarAction::RenameObject(object.clone()));
                            ui.close_menu();
                        }
                        if ui
                            .button(RichText::new("Delete...").color(Color32::LIGHT_RED))
                            .clicked()
                        {
                            *action = Some(SidebarAction::ConfirmDropObject(object.clone()));
                            ui.close_menu();
                        }
                        if ui.button("Refresh").clicked() {
                            self.objects.remove(&key);
                            self.objects_loading.insert(key.clone());
                            *action = Some(SidebarAction::RefreshObjects {
                                database: database.to_string(),
                                key: key.clone(),
                            });
                            ui.close_menu();
                        }
                        if ui.button("Generate SQL").clicked() {
                            *action = Some(SidebarAction::OpenSql(object.generate_sql_query()));
                            ui.close_menu();
                        }
                    });
                }
            });
        group.header_response.context_menu(|ui| {
            ui.menu_button("Create", |ui| {
                if key.kind == DbObjectKind::Table {
                    if ui.button("Table...").clicked() {
                        *action = Some(SidebarAction::OpenCreateTable {
                            schema: key.schema.clone().unwrap_or_else(|| "public".to_string()),
                        });
                        ui.close_menu();
                    }
                } else if key.kind == DbObjectKind::View {
                    if ui.button("View...").clicked() {
                        *action = Some(SidebarAction::OpenDdlForm(DdlForm::new(
                            DdlFormKind::View,
                            key.schema.as_deref().unwrap_or("public"),
                            "",
                        )));
                        ui.close_menu();
                    }
                } else if key.kind == DbObjectKind::Sequence {
                    if ui.button("Sequence...").clicked() {
                        *action = Some(SidebarAction::OpenDdlForm(DdlForm::new(
                            DdlFormKind::Sequence,
                            key.schema.as_deref().unwrap_or("public"),
                            "",
                        )));
                        ui.close_menu();
                    }
                } else {
                    let name = format!(
                        "new_{}",
                        key.kind.singular().to_lowercase().replace(' ', "_")
                    );
                    if ui.button(format!("{} SQL", key.kind.singular())).clicked() {
                        *action = Some(SidebarAction::OpenSql(
                            key.kind.create_sql(key.schema.as_deref(), &name),
                        ));
                        ui.close_menu();
                    }
                }
            });
            if ui
                .add_enabled(!is_busy, egui::Button::new("Refresh"))
                .clicked()
            {
                self.objects.remove(&key);
                self.objects_errors.remove(&key);
                self.objects_loading.insert(key.clone());
                *action = Some(SidebarAction::RefreshObjects {
                    database: database.to_string(),
                    key: key.clone(),
                });
                ui.close_menu();
            }
        });
    }

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
                    if ui
                        .add_enabled(is_connected && !is_busy, egui::Button::new("🔄").small())
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
                    ui.label(
                        RichText::new("Connect to browse databases")
                            .size(11.0)
                            .color(Color32::DARK_GRAY),
                    );
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

                            let db_btn =
                                ui.selectable_label(is_current, RichText::new(&db.name).strong());

                            if !is_current && db_btn.clicked() {
                                action = Some(SidebarAction::SwitchDatabase(db.name.clone()));
                            }

                            if let Some(ref sz) = db.size {
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        ui.label(RichText::new(sz).size(10.0).color(Color32::GRAY));
                                    },
                                );
                            }

                            // Right-click context menu on Database
                            db_btn.context_menu(|ui| {
                                ui.label(
                                    RichText::new(format!("🗄️ Database: {}", db.name)).strong(),
                                );
                                ui.separator();

                                if !is_current {
                                    if ui.button("🔌 Switch to this Database").clicked() {
                                        action =
                                            Some(SidebarAction::SwitchDatabase(db.name.clone()));
                                        ui.close_menu();
                                    }
                                }

                                if ui.button("⚡ Open Query Tool").clicked() {
                                    action =
                                        Some(SidebarAction::OpenQueryToolForDb(db.name.clone()));
                                    ui.close_menu();
                                }

                                if ui.button("➕ Create Table...").clicked() {
                                    action = Some(SidebarAction::OpenCreateTable {
                                        schema: "public".to_string(),
                                    });
                                    ui.close_menu();
                                }

                                if ui.button("🔄 Refresh Tables").clicked() {
                                    action = Some(SidebarAction::RefreshTables);
                                    ui.close_menu();
                                }

                                ui.separator();
                                if ui.button("Backup...").clicked() {
                                    action = Some(SidebarAction::BackupDatabase {
                                        db_name: db.name.clone(),
                                    });
                                    ui.close_menu();
                                }
                                if ui.button("Restore...").clicked() {
                                    action = Some(SidebarAction::RestoreDatabase {
                                        db_name: db.name.clone(),
                                    });
                                    ui.close_menu();
                                }

                                if !is_current {
                                    ui.separator();
                                    if ui
                                        .button(
                                            RichText::new("🗑️ Drop Database...")
                                                .color(Color32::LIGHT_RED),
                                        )
                                        .clicked()
                                    {
                                        action = Some(SidebarAction::ConfirmDropDatabase {
                                            db_name: db.name.clone(),
                                        });
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
                    if ui
                        .small_button("➕")
                        .on_hover_text("Create new table")
                        .clicked()
                    {
                        action = Some(SidebarAction::OpenCreateTable {
                            schema: "public".to_string(),
                        });
                    }
                    if ui
                        .add_enabled(is_connected && !is_busy, egui::Button::new("🔄").small())
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
                        action = Some(SidebarAction::OpenCreateTable {
                            schema: "public".to_string(),
                        });
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
                                        ui.label(
                                            RichText::new(format!(
                                                "📄 Table: {}.{}",
                                                item.schema, item.name
                                            ))
                                            .strong(),
                                        );
                                        ui.separator();

                                        // View / Edit Data submenu with limits
                                        ui.menu_button("📊 View Data ▾", |ui| {
                                            if ui
                                                .button("⚡ First 100 Rows (Default, Fast)")
                                                .clicked()
                                            {
                                                self.selected_table =
                                                    Some((item.schema.clone(), item.name.clone()));
                                                action = Some(SidebarAction::SelectTable {
                                                    schema: item.schema.clone(),
                                                    table: item.name.clone(),
                                                    limit: 100,
                                                });
                                                ui.close_menu();
                                            }
                                            if ui.button("📊 First 500 Rows").clicked() {
                                                self.selected_table =
                                                    Some((item.schema.clone(), item.name.clone()));
                                                action = Some(SidebarAction::SelectTable {
                                                    schema: item.schema.clone(),
                                                    table: item.name.clone(),
                                                    limit: 500,
                                                });
                                                ui.close_menu();
                                            }
                                            if ui.button("📊 First 1,000 Rows").clicked() {
                                                self.selected_table =
                                                    Some((item.schema.clone(), item.name.clone()));
                                                action = Some(SidebarAction::SelectTable {
                                                    schema: item.schema.clone(),
                                                    table: item.name.clone(),
                                                    limit: 1000,
                                                });
                                                ui.close_menu();
                                            }
                                            if ui.button("📊 All Rows (Unlimited)").clicked() {
                                                self.selected_table =
                                                    Some((item.schema.clone(), item.name.clone()));
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

                                        if item.table_type == "BASE TABLE" {
                                            ui.menu_button("Create", |ui| {
                                                for (label, kind) in [
                                                    ("Index...", DdlFormKind::Index),
                                                    ("Foreign Key...", DdlFormKind::ForeignKey),
                                                    ("Constraint...", DdlFormKind::Constraint),
                                                ] {
                                                    if ui.button(label).clicked() {
                                                        action = Some(SidebarAction::OpenDdlForm(
                                                            DdlForm::new(
                                                                kind,
                                                                &item.schema,
                                                                &item.name,
                                                            ),
                                                        ));
                                                        ui.close_menu();
                                                    }
                                                }
                                            });
                                            if ui.button("Manage Grants...").clicked() {
                                                action = Some(SidebarAction::OpenDdlForm(
                                                    DdlForm::new(
                                                        DdlFormKind::Grant,
                                                        &item.schema,
                                                        &item.name,
                                                    ),
                                                ));
                                                ui.close_menu();
                                            }
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

                                        if ui
                                            .button(
                                                RichText::new("🗑️ Drop Table...")
                                                    .color(Color32::LIGHT_RED),
                                            )
                                            .clicked()
                                        {
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
