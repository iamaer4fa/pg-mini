use egui::{CentralPanel, Color32, RichText, SidePanel, TopBottomPanel};
use std::time::{Duration, Instant};

use crate::config::ConfigManager;
use crate::db::{DbRequest, DbResponse, DbWorker};
use crate::models::{
    AppConfig, CellValue, ColumnItem, ConfirmAction, ConfirmDialogState, DatabaseItem,
    DbObjectKind, DbObjectRef, EditableCell, QueryResult, TableItem,
};
use crate::ui::columns_view::{ColumnsViewAction, render_columns_view};
use crate::ui::connection_modal::ConnectionModal;
use crate::ui::data_table::{DataTableAction, DataTableState, export_to_csv, render_data_table};
use crate::ui::dialogs::{AddColumnModal, CreateTableModal, render_confirm_dialog};
use crate::ui::sidebar::{Sidebar, SidebarAction};
use crate::ui::sql_editor::{SqlEditor, SqlEditorAction};
use crate::ui::theme::setup_custom_theme;

#[derive(PartialEq)]
pub enum ActiveTab {
    TableView,
    SqlEditor,
    SchemaView,
}

pub struct Toast {
    pub message: String,
    pub created_at: Instant,
    pub is_error: bool,
}

pub struct PgMiniApp {
    config: AppConfig,
    db_worker: DbWorker,

    // Connection state
    is_connected: bool,
    is_busy: bool,
    current_db: String,
    host_port: String,
    connection_modal: ConnectionModal,

    // Data
    databases: Vec<DatabaseItem>,
    tables: Vec<TableItem>,
    selected_table: Option<(String, String)>, // (schema, table_name)
    current_table_data: Option<QueryResult>,
    table_data_state: DataTableState,
    current_columns: Vec<ColumnItem>,
    sql_editor: SqlEditor,
    last_query_result: Option<Result<QueryResult, String>>,

    // Modals
    add_column_modal: AddColumnModal,
    create_table_modal: CreateTableModal,
    confirm_dialog: ConfirmDialogState,
    rename_object: Option<DbObjectRef>,
    rename_object_name: String,

    // UI State
    active_tab: ActiveTab,
    sidebar: Sidebar,
    toast: Option<Toast>,
    show_about: bool,
}

impl PgMiniApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let config = ConfigManager::load();

        setup_custom_theme(&cc.egui_ctx, config.dark_mode);

        let initial_cfg = config
            .saved_connections
            .iter()
            .find(|c| Some(&c.id) == config.last_connection_id.as_ref())
            .cloned()
            .unwrap_or_else(|| {
                config
                    .saved_connections
                    .first()
                    .cloned()
                    .unwrap_or_default()
            });

        let connection_modal = ConnectionModal::new(initial_cfg);
        let mut sql_editor = SqlEditor::default();
        sql_editor.history = config.query_history.clone();

        Self {
            config,
            db_worker: DbWorker::new(),
            is_connected: false,
            is_busy: false,
            current_db: String::new(),
            host_port: String::new(),
            connection_modal,
            databases: Vec::new(),
            tables: Vec::new(),
            selected_table: None,
            current_table_data: None,
            table_data_state: DataTableState::default(),
            current_columns: Vec::new(),
            sql_editor,
            last_query_result: None,
            add_column_modal: AddColumnModal::default(),
            create_table_modal: CreateTableModal::default(),
            confirm_dialog: ConfirmDialogState::default(),
            rename_object: None,
            rename_object_name: String::new(),
            active_tab: ActiveTab::SqlEditor,
            sidebar: Sidebar::default(),
            toast: None,
            show_about: false,
        }
    }

    fn show_toast(&mut self, message: impl Into<String>, is_error: bool) {
        self.toast = Some(Toast {
            message: message.into(),
            created_at: Instant::now(),
            is_error,
        });
    }

    fn poll_db_responses(&mut self, ctx: &egui::Context) {
        while let Some(resp) = self.db_worker.try_recv() {
            match resp {
                DbResponse::Connected {
                    server_version: _,
                    current_db,
                    host_port,
                } => {
                    self.is_connected = true;
                    self.current_db = current_db.clone();
                    self.host_port = host_port.clone();
                    self.connection_modal.is_open = false;
                    self.show_toast(
                        format!("Connected to {} on {}", current_db, host_port),
                        false,
                    );
                    self.sidebar.reset_object_tree();
                    self.db_worker.send(DbRequest::FetchDatabases);
                }
                DbResponse::ConnectionFailed(err) => {
                    self.is_connected = false;
                    self.connection_modal.error_message = Some(err.clone());
                    self.show_toast(format!("Connection error: {}", err), true);
                }
                DbResponse::Disconnected => {
                    self.is_connected = false;
                    self.databases.clear();
                    self.tables.clear();
                    self.selected_table = None;
                    self.current_table_data = None;
                    self.current_columns.clear();
                    self.last_query_result = None;
                    self.sidebar.reset_object_tree();
                    self.show_toast("Disconnected from PostgreSQL", false);
                }
                DbResponse::DatabasesFetched(res) => match res {
                    Ok(dbs) => {
                        self.databases = dbs;
                    }
                    Err(e) => {
                        self.show_toast(format!("Error fetching databases: {}", e), true);
                    }
                },
                DbResponse::SchemasFetched { database, result } => {
                    if database == self.current_db {
                        if let Err(error) = &result {
                            self.show_toast(format!("Error fetching schemas: {}", error), true);
                        }
                        self.sidebar.receive_schemas(result);
                    }
                }
                DbResponse::ObjectsFetched {
                    database,
                    key,
                    result,
                } => {
                    if database == self.current_db {
                        if let Err(error) = &result {
                            self.show_toast(format!("Error fetching objects: {}", error), true);
                        }
                        if let Ok(objects) = &result {
                            if let Some(schema) = key.schema.as_deref() {
                                let table_type = match key.kind {
                                    DbObjectKind::Table => Some("BASE TABLE"),
                                    DbObjectKind::View => Some("VIEW"),
                                    DbObjectKind::MaterializedView => Some("MATERIALIZED VIEW"),
                                    _ => None,
                                };
                                if let Some(table_type) = table_type {
                                    self.tables.retain(|table| {
                                        table.schema != schema || table.table_type != table_type
                                    });
                                    self.tables.extend(objects.iter().map(|object| TableItem {
                                        schema: schema.to_string(),
                                        name: object.name.clone(),
                                        table_type: table_type.to_string(),
                                    }));
                                }
                            }
                        }
                        self.sidebar.receive_objects(&key, result);
                    }
                }
                DbResponse::DatabaseSwitched {
                    db_name,
                    server_version: _,
                } => {
                    self.current_db = db_name.clone();
                    self.selected_table = None;
                    self.current_table_data = None;
                    self.current_columns.clear();
                    self.table_data_state.editing_row = None;
                    self.table_data_state.adding_row = None;
                    self.table_data_state.mutation_pending = false;
                    self.tables.clear();
                    self.sidebar.reset_object_tree();
                    self.show_toast(format!("Switched to database '{}'", db_name), false);
                    // Object catalogs are fetched when their tree nodes are expanded.
                    self.db_worker.send(DbRequest::FetchDatabases);
                }
                DbResponse::TablesFetched(res) => match res {
                    Ok(tables) => {
                        self.tables = tables;
                    }
                    Err(e) => {
                        self.show_toast(format!("Error fetching tables: {}", e), true);
                    }
                },
                DbResponse::TableDataResult(res) => match res {
                    Ok(data) => {
                        self.current_table_data = Some(data);
                        self.active_tab = ActiveTab::TableView;
                    }
                    Err(e) => {
                        self.show_toast(format!("Error fetching data: {}", e), true);
                    }
                },
                DbResponse::ColumnsFetched(res) => match res {
                    Ok((schema, table, columns)) => {
                        self.selected_table = Some((schema, table));
                        self.current_columns = columns;
                    }
                    Err(e) => {
                        self.show_toast(format!("Error fetching schema: {}", e), true);
                    }
                },
                DbResponse::QueryResult(res) => {
                    self.last_query_result = Some(res);
                    self.active_tab = ActiveTab::SqlEditor;
                }
                DbResponse::OperationSuccess {
                    message,
                    refresh_tables,
                    refresh_columns,
                } => {
                    self.show_toast(message, false);
                    if refresh_tables {
                        self.tables.clear();
                        self.sidebar.reset_object_tree();
                        self.db_worker.send(DbRequest::FetchDatabases);
                    }
                    if refresh_columns {
                        self.table_data_state.editing_row = None;
                        self.table_data_state.adding_row = None;
                        self.table_data_state.mutation_pending = false;
                        if let Some((ref s, ref t)) = self.selected_table {
                            self.db_worker.send(DbRequest::FetchColumns {
                                schema: s.clone(),
                                table: t.clone(),
                            });
                            // Also refresh table data if open
                            let offset = self.table_data_state.current_page
                                * self.table_data_state.page_limit;
                            self.db_worker.send(DbRequest::FetchTableData {
                                schema: s.clone(),
                                table: t.clone(),
                                limit: self.table_data_state.page_limit,
                                offset,
                            });
                        }
                    }
                }
                DbResponse::OperationFailed(err) => {
                    self.table_data_state.mutation_pending = false;
                    self.show_toast(err, true);
                }
                DbResponse::Busy(busy) => {
                    self.is_busy = busy;
                }
            }
            ctx.request_repaint();
        }
    }
}

impl eframe::App for PgMiniApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_db_responses(ctx);

        // Top Navigation Bar
        TopBottomPanel::top("top_panel").show(ctx, |ui| {
            ui.add_space(3.0);
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new("⚡ pg-mini")
                        .strong()
                        .size(18.0)
                        .color(Color32::from_rgb(80, 180, 255)),
                );
                ui.label(
                    RichText::new("Lightweight PostgreSQL Manager")
                        .size(11.0)
                        .color(Color32::GRAY),
                );

                ui.separator();

                // Connection badge
                if self.is_connected {
                    ui.label(RichText::new("●").color(Color32::from_rgb(50, 220, 80)));
                    ui.label(
                        RichText::new(format!("{}/{}", self.host_port, self.current_db)).strong(),
                    );
                    if ui.button("🔌 Disconnect").clicked() {
                        self.db_worker.send(DbRequest::Disconnect);
                    }
                } else {
                    ui.label(RichText::new("●").color(Color32::from_rgb(220, 50, 50)));
                    ui.label(RichText::new("Disconnected").color(Color32::GRAY));
                    if ui
                        .add(
                            egui::Button::new(
                                RichText::new("🔌 Connect...")
                                    .strong()
                                    .color(Color32::WHITE),
                            )
                            .fill(Color32::from_rgb(16, 145, 90))
                            .rounding(egui::Rounding::same(6.0)),
                        )
                        .clicked()
                    {
                        self.connection_modal.is_open = true;
                    }
                }

                if self.is_busy {
                    ui.label(RichText::new("⏳ Working...").color(Color32::YELLOW));
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("ℹ About").clicked() {
                        self.show_about = true;
                    }

                    // Theme toggle
                    let theme_icon = if self.config.dark_mode {
                        "☀ Light"
                    } else {
                        "🌙 Dark"
                    };
                    if ui.button(theme_icon).clicked() {
                        self.config.dark_mode = !self.config.dark_mode;
                        setup_custom_theme(ctx, self.config.dark_mode);
                        ConfigManager::save(&self.config);
                    }
                });
            });
            ui.add_space(3.0);
        });

        // Bottom Status Bar
        TopBottomPanel::bottom("bottom_panel").show(ctx, |ui| {
            ui.horizontal(|ui| {
                if let Some(toast) = &self.toast {
                    if toast.created_at.elapsed() < Duration::from_secs(5) {
                        let color = if toast.is_error {
                            Color32::LIGHT_RED
                        } else {
                            Color32::from_rgb(100, 230, 120)
                        };
                        ui.label(RichText::new(&toast.message).color(color).strong());
                    }
                } else {
                    let status_txt = if self.is_connected {
                        format!(
                            "Connected: {} | Active DB: {}",
                            self.host_port, self.current_db
                        )
                    } else {
                        "Ready | Click 'Connect...' to start".to_string()
                    };
                    ui.label(
                        RichText::new(status_txt)
                            .size(11.0)
                            .color(Color32::DARK_GRAY),
                    );
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(RichText::new("v0.1.0").size(11.0).color(Color32::DARK_GRAY));
                });
            });
        });

        // Left Sidebar: lazy PostgreSQL object browser
        SidePanel::left("left_sidebar")
            .resizable(true)
            .default_width(260.0)
            .width_range(180.0..=450.0)
            .show(ctx, |ui| {
                if let Some(action) = self.sidebar.show_object_browser(
                    ui,
                    &self.databases,
                    &self.current_db,
                    &self.host_port,
                    self.is_connected,
                    self.is_busy,
                ) {
                    match action {
                        SidebarAction::SwitchDatabase(db_name) => {
                            self.db_worker.send(DbRequest::SwitchDatabase(db_name));
                        }
                        SidebarAction::RefreshDatabases => {
                            self.db_worker.send(DbRequest::FetchDatabases);
                        }
                        SidebarAction::SelectTable { schema, table, limit } => {
                            self.selected_table = Some((schema.clone(), table.clone()));
                            self.current_columns.clear();
                            self.table_data_state.editing_row = None;
                            self.table_data_state.adding_row = None;
                            self.table_data_state.page_limit = limit;
                            self.table_data_state.current_page = 0;
                            self.db_worker.send(DbRequest::FetchTableData {
                                schema: schema.clone(),
                                table: table.clone(),
                                limit,
                                offset: 0,
                            });
                            self.db_worker.send(DbRequest::FetchColumns { schema, table });
                        }
                        SidebarAction::CountRows { schema, table } => {
                            let object = DbObjectRef {
                                kind: DbObjectKind::Table,
                                schema: Some(schema),
                                name: table,
                                identity_arguments: None,
                            };
                            self.db_worker.send(DbRequest::ExecuteSql(format!(
                                "SELECT count(*) AS total_rows FROM {};",
                                object.qualified_name()
                            )));
                            self.active_tab = ActiveTab::SqlEditor;
                        }
                        SidebarAction::ViewColumns { schema, table } => {
                            self.selected_table = Some((schema.clone(), table.clone()));
                            self.active_tab = ActiveTab::SchemaView;
                            self.db_worker.send(DbRequest::FetchColumns { schema, table });
                        }
                        SidebarAction::OpenInEditor { schema, table } => {
                            self.sql_editor.sql_text = format!("SELECT * FROM \"{}\".\"{}\" LIMIT 100;", schema, table);
                            self.active_tab = ActiveTab::SqlEditor;
                        }
                        SidebarAction::OpenQueryToolForDb(db_name) => {
                            if db_name != self.current_db {
                                self.db_worker.send(DbRequest::SwitchDatabase(db_name));
                            }
                            self.active_tab = ActiveTab::SqlEditor;
                        }
                        SidebarAction::OpenAddColumn { schema, table } => {
                            self.add_column_modal.open_for(&schema, &table);
                        }
                        SidebarAction::OpenCreateTable { schema } => {
                            self.create_table_modal.open_for(&schema);
                        }
                        SidebarAction::ConfirmDropTable { schema, table } => {
                            self.confirm_dialog = ConfirmDialogState {
                                is_open: true,
                                title: "Drop Table".to_string(),
                                message: format!("Are you sure you want to permanently drop table '{}.{}' and delete all its data?", schema, table),
                                action: Some(ConfirmAction::DropTable { schema, table }),
                            };
                        }
                        SidebarAction::ConfirmTruncateTable { schema, table } => {
                            self.confirm_dialog = ConfirmDialogState {
                                is_open: true,
                                title: "Truncate Table".to_string(),
                                message: format!("Are you sure you want to truncate table '{}.{}'? All records will be removed.", schema, table),
                                action: Some(ConfirmAction::TruncateTable { schema, table }),
                            };
                        }
                        SidebarAction::ConfirmDropDatabase { db_name } => {
                            self.confirm_dialog = ConfirmDialogState {
                                is_open: true,
                                title: "Drop Database".to_string(),
                                message: format!("Are you sure you want to permanently delete database '{}'?", db_name),
                                action: Some(ConfirmAction::DropDatabase { db_name }),
                            };
                        }
                        SidebarAction::RefreshTables => {
                            self.tables.clear();
                            self.sidebar.reset_object_tree();
                            self.db_worker.send(DbRequest::FetchDatabases);
                        }
                        SidebarAction::RequestSchemas { database } => {
                            self.db_worker.send(DbRequest::FetchSchemas { database });
                        }
                        SidebarAction::RequestObjects { database, key } => {
                            self.db_worker
                                .send(DbRequest::FetchObjects { database, key });
                        }
                        SidebarAction::RefreshDatabaseTree => {
                            self.tables.clear();
                            self.sidebar.reset_object_tree();
                            self.db_worker.send(DbRequest::FetchDatabases);
                        }
                        SidebarAction::RefreshObjects { database, key } => {
                            self.db_worker
                                .send(DbRequest::FetchObjects { database, key });
                        }
                        SidebarAction::OpenSql(sql) => {
                            self.sql_editor.sql_text = sql;
                            self.active_tab = ActiveTab::SqlEditor;
                        }
                        SidebarAction::RenameObject(object) => {
                            self.rename_object_name = object.name.clone();
                            self.rename_object = Some(object);
                        }
                        SidebarAction::ConfirmDropObject(object) => {
                            self.confirm_dialog = ConfirmDialogState {
                                is_open: true,
                                title: format!("Delete {}", object.kind.singular()),
                                message: format!(
                                    "Delete {} '{}'?",
                                    object.kind.singular().to_lowercase(),
                                    object.qualified_name()
                                ),
                                action: Some(ConfirmAction::DropObject { object }),
                            };
                        }
                    }
                }
            });

        // Main Central Area: Tabs and Content
        CentralPanel::default().show(ctx, |ui| {
            // Tab Header
            ui.horizontal(|ui| {
                let table_tab_label = if let Some((_, ref t)) = self.selected_table {
                    format!("📊 Table ({})", t)
                } else {
                    "📊 Table View".to_string()
                };

                if ui.selectable_label(self.active_tab == ActiveTab::TableView, table_tab_label).clicked() {
                    self.active_tab = ActiveTab::TableView;
                }

                if ui.selectable_label(self.active_tab == ActiveTab::SqlEditor, "⚡ SQL Editor").clicked() {
                    self.active_tab = ActiveTab::SqlEditor;
                }

                let schema_tab_label = if let Some((_, ref t)) = self.selected_table {
                    format!("📐 Schema ({})", t)
                } else {
                    "📐 Schema".to_string()
                };

                if ui.selectable_label(self.active_tab == ActiveTab::SchemaView, schema_tab_label).clicked() {
                    self.active_tab = ActiveTab::SchemaView;
                }
            });

            ui.separator();
            ui.add_space(4.0);

            // Active Tab Body
            match self.active_tab {
                ActiveTab::TableView => {
                    let table_name_label = self.selected_table.as_ref().map(|(s, t)| format!("{}.{}", s, t));
                    let primary_key_columns: Vec<String> = self.current_columns.iter()
                        .filter(|column| column.is_primary_key).map(|column| column.name.clone()).collect();
                    let allow_insert = self.selected_table.as_ref().and_then(|(schema, table)| self.tables.iter()
                        .find(|item| item.schema == *schema && item.name == *table))
                        .map(|item| item.table_type == "BASE TABLE").unwrap_or(false);
                    if let Some(ref data) = self.current_table_data {
                        if let Some(action) = render_data_table(
                            ui,
                            &mut self.table_data_state,
                            data,
                            true,
                            table_name_label.as_deref(),
                            &primary_key_columns,
                            allow_insert,
                        ) {
                            match action {
                                DataTableAction::Refresh => {
                                    if let Some((ref s, ref t)) = self.selected_table {
                                        let offset = self.table_data_state.current_page * self.table_data_state.page_limit;
                                        self.db_worker.send(DbRequest::FetchTableData {
                                            schema: s.clone(),
                                            table: t.clone(),
                                            limit: self.table_data_state.page_limit,
                                            offset,
                                        });
                                    }
                                }
                                DataTableAction::ChangePage { limit, offset } => {
                                    if let Some((ref s, ref t)) = self.selected_table {
                                        self.db_worker.send(DbRequest::FetchTableData {
                                            schema: s.clone(),
                                            table: t.clone(),
                                            limit,
                                            offset,
                                        });
                                    }
                                }
                                DataTableAction::CopyCell(text) => {
                                    ctx.output_mut(|o| o.copied_text = text.clone());
                                    let preview = if text.len() > 30 { format!("{}...", &text[..30]) } else { text };
                                    self.show_toast(format!("Copied: {}", preview), false);
                                }
                                DataTableAction::ExportCsv => {
                                    let default_filename = self
                                        .selected_table
                                        .as_ref()
                                        .map(|(s, t)| format!("{}_{}", s, t))
                                        .unwrap_or_else(|| "export".to_string());

                                    match export_to_csv(data, &default_filename) {
                                        Ok(path) => {
                                            self.show_toast(format!("Exported to {}", path), false);
                                        }
                                        Err(e) => {
                                            if e != "Export cancelled" {
                                                self.show_toast(e, true);
                                            }
                                        }
                                    }
                                }
                                DataTableAction::SaveRow { row_index, values } => {
                                    if let (Some((schema, table)), Some(data_row)) = (&self.selected_table, data.rows.get(row_index)) {
                                        let changed: Vec<(String, EditableCell)> = values.into_iter().enumerate()
                                            .filter_map(|(idx, value)| {
                                                let original = data_row.get(idx)?;
                                                if value.is_null != original.is_null || (!value.is_null && value.text != original.text) {
                                                    Some((data.columns.get(idx)?.clone(), value))
                                                } else { None }
                                            }).collect();
                                        let keys: Vec<(String, CellValue)> = primary_key_columns.iter().filter_map(|name| {
                                            let idx = data.columns.iter().position(|column| column == name)?;
                                            Some((name.clone(), data_row.get(idx)?.clone()))
                                        }).collect();
                                        if changed.is_empty() {
                                            self.table_data_state.editing_row = None;
                                            self.table_data_state.mutation_pending = false;
                                            self.show_toast("No cell values changed", false);
                                        } else {
                                            self.db_worker.send(DbRequest::UpdateRow {
                                                schema: schema.clone(), table: table.clone(), primary_key: keys, values: changed,
                                            });
                                        }
                                    }
                                }
                                DataTableAction::InsertRow { values } => {
                                    if let Some((schema, table)) = &self.selected_table {
                                        let named_values = data.columns.iter().cloned().zip(values).collect();
                                        self.db_worker.send(DbRequest::InsertRow {
                                            schema: schema.clone(), table: table.clone(), values: named_values,
                                        });
                                    }
                                }
                                DataTableAction::DeleteRow { row_index } => {
                                    if let (Some((schema, table)), Some(data_row)) = (&self.selected_table, data.rows.get(row_index)) {
                                        let keys: Vec<(String, CellValue)> = primary_key_columns.iter().filter_map(|name| {
                                            let idx = data.columns.iter().position(|column| column == name)?;
                                            Some((name.clone(), data_row.get(idx)?.clone()))
                                        }).collect();
                                        self.confirm_dialog = ConfirmDialogState {
                                            is_open: true,
                                            title: "Delete Row".to_string(),
                                            message: format!("Delete this row from {}.{}? This cannot be undone.", schema, table),
                                            action: Some(ConfirmAction::DeleteRow { schema: schema.clone(), table: table.clone(), primary_key: keys }),
                                        };
                                        self.table_data_state.mutation_pending = false;
                                    }
                                }
                            }
                        }
                    } else {
                        ui.vertical_centered(|ui| {
                            ui.add_space(50.0);
                            ui.label(RichText::new("Select a table from the sidebar to view data").size(15.0).color(Color32::GRAY));
                            ui.label(RichText::new("Or write a custom SQL query in the SQL Editor tab.").size(12.0).color(Color32::DARK_GRAY));
                        });
                    }
                }
                ActiveTab::SqlEditor => {
                    let previous_history = self.sql_editor.history.clone();
                    if let Some(action) = self.sql_editor.show(
                        ui,
                        &self.last_query_result,
                        self.is_connected,
                        self.is_busy,
                        &self.tables,
                        &self.current_columns,
                    ) {
                        match action {
                            SqlEditorAction::ExecuteQuery(query) => {
                                self.db_worker.send(DbRequest::ExecuteSql(query));
                            }
                            SqlEditorAction::CopyCell(text) => {
                                ctx.output_mut(|o| o.copied_text = text.clone());
                                let preview = if text.len() > 30 { format!("{}...", &text[..30]) } else { text };
                                self.show_toast(format!("Copied: {}", preview), false);
                            }
                            SqlEditorAction::ExportCsv => {
                                if let Some(Ok(ref data)) = self.last_query_result {
                                    match export_to_csv(data, "query_results") {
                                        Ok(path) => {
                                            self.show_toast(format!("Exported to {}", path), false);
                                        }
                                        Err(e) => {
                                            if e != "Export cancelled" {
                                                self.show_toast(e, true);
                                            }
                                        }
                                    }
                                }
                            }
                            SqlEditorAction::CopyQuery(query) => {
                                ctx.output_mut(|o| o.copied_text = query);
                                self.show_toast("Query copied to clipboard", false);
                            }
                        }
                    }
                    if self.sql_editor.history != previous_history {
                        self.config.query_history = self.sql_editor.history.clone();
                        ConfigManager::save(&self.config);
                    }
                }
                ActiveTab::SchemaView => {
                    let (schema, table) = self
                        .selected_table
                        .as_ref()
                        .map(|(s, t)| (s.as_str(), t.as_str()))
                        .unwrap_or(("public", ""));

                    if let Some(action) = render_columns_view(ui, schema, table, &self.current_columns, self.is_busy) {
                        match action {
                            ColumnsViewAction::OpenAddColumn { schema, table } => {
                                self.add_column_modal.open_for(&schema, &table);
                            }
                            ColumnsViewAction::ConfirmDropColumn { schema, table, column } => {
                                self.confirm_dialog = ConfirmDialogState {
                                    is_open: true,
                                    title: "Drop Column".to_string(),
                                    message: format!("Are you sure you want to drop column '{}' from '{}.{}'?", column, schema, table),
                                    action: Some(ConfirmAction::DropColumn { schema, table, column }),
                                };
                            }
                            ColumnsViewAction::CopyName(name) => {
                                ctx.output_mut(|o| o.copied_text = name.clone());
                                self.show_toast(format!("Copied column name: {}", name), false);
                            }
                        }
                    }
                }
            }
        });

        // Modals
        if let Some(cfg) =
            self.connection_modal
                .show(ctx, &mut self.config, self.is_connected, self.is_busy)
        {
            self.db_worker.send(DbRequest::Connect(cfg));
        }

        if let Some(form) = self.add_column_modal.show(ctx, self.is_busy) {
            self.db_worker.send(DbRequest::AddColumn(form));
        }

        if let Some(form) = self.create_table_modal.show(ctx, self.is_busy) {
            self.db_worker.send(DbRequest::CreateTable(form));
        }

        if let Some(action) = render_confirm_dialog(ctx, &mut self.confirm_dialog, self.is_busy) {
            match action {
                ConfirmAction::DropTable { schema, table } => {
                    self.db_worker.send(DbRequest::DropTable { schema, table });
                }
                ConfirmAction::TruncateTable { schema, table } => {
                    self.db_worker
                        .send(DbRequest::TruncateTable { schema, table });
                }
                ConfirmAction::DropColumn {
                    schema,
                    table,
                    column,
                } => {
                    self.db_worker.send(DbRequest::DropColumn {
                        schema,
                        table,
                        column,
                    });
                }
                ConfirmAction::DropDatabase { db_name } => {
                    self.db_worker.send(DbRequest::DropDatabase { db_name });
                }
                ConfirmAction::DropObject { object } => {
                    self.db_worker.send(DbRequest::DropObject(object));
                }
                ConfirmAction::DeleteRow {
                    schema,
                    table,
                    primary_key,
                } => {
                    self.table_data_state.mutation_pending = true;
                    self.db_worker.send(DbRequest::DeleteRow {
                        schema,
                        table,
                        primary_key,
                    });
                }
            }
        }

        if let Some(object) = self.rename_object.clone() {
            let mut keep_open = true;
            let mut submit_rename = false;
            let mut cancel_rename = false;
            egui::Window::new(format!("Rename {}", object.kind.singular()))
                .collapsible(false)
                .resizable(false)
                .open(&mut keep_open)
                .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
                .show(ctx, |ui| {
                    ui.label(format!("Rename '{}':", object.qualified_name()));
                    ui.text_edit_singleline(&mut self.rename_object_name);
                    ui.horizontal(|ui| {
                        if ui.button("Cancel").clicked() {
                            cancel_rename = true;
                        }
                        if ui
                            .add_enabled(
                                !self.is_busy && !self.rename_object_name.trim().is_empty(),
                                egui::Button::new("Rename"),
                            )
                            .clicked()
                        {
                            submit_rename = true;
                        }
                    });
                });
            if submit_rename || cancel_rename {
                keep_open = false;
            }
            if submit_rename {
                self.db_worker.send(DbRequest::RenameObject {
                    object,
                    new_name: self.rename_object_name.trim().to_string(),
                });
                keep_open = false;
            }
            if !keep_open {
                self.rename_object = None;
            }
        }

        // About Modal
        if self.show_about {
            egui::Window::new("ℹ About pg-mini")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
                .show(ctx, |ui| {
                    ui.set_min_width(340.0);
                    ui.heading("⚡ pg-mini v0.1.0");
                    ui.label("Ultra-lightweight native PostgreSQL manager.");
                    ui.add_space(8.0);
                    ui.label("Built with Rust + egui/eframe + tokio-postgres.");
                    ui.label("• Zero web engine or Electron overhead");
                    ui.label("• Tiny memory footprint (~30MB RAM)");
                    ui.label("• Instant startup & async query execution");
                    ui.add_space(12.0);
                    ui.separator();
                    ui.horizontal(|ui| {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.button("Close").clicked() {
                                self.show_about = false;
                            }
                        });
                    });
                });
        }
    }
}
