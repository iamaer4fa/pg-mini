use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::Instant;
use std::{
    path::Path,
    process::{Command, Stdio},
};
use tokio_postgres::{Client, NoTls, SimpleQueryMessage};

use crate::models::{
    AddColumnForm, CellValue, ColumnItem, ConnectionConfig, CreateTableForm, DatabaseItem,
    DbObjectItem, DbObjectKind, DbObjectRef, EditableCell, ObjectTreeKey, QueryResult, RoleForm,
    RoleItem, TableItem,
};

#[derive(Debug)]
pub enum DbRequest {
    Connect(ConnectionConfig),
    Disconnect,
    FetchDatabases,
    FetchRoles,
    SaveRole(RoleForm),
    DropRole(String),
    ExecuteDdl(String),
    SwitchDatabase(String),
    FetchSchemas {
        database: String,
    },
    FetchObjects {
        database: String,
        key: ObjectTreeKey,
    },
    FetchTables,
    FetchTableData {
        schema: String,
        table: String,
        limit: usize,
        offset: usize,
    },
    FetchColumns {
        schema: String,
        table: String,
    },
    ExecuteSql(String),
    AddColumn(AddColumnForm),
    CreateTable(CreateTableForm),
    DropTable {
        schema: String,
        table: String,
    },
    TruncateTable {
        schema: String,
        table: String,
    },
    DropColumn {
        schema: String,
        table: String,
        column: String,
    },
    DropDatabase {
        db_name: String,
    },
    BackupDatabase {
        db_name: String,
        file_path: std::path::PathBuf,
    },
    RestoreDatabase {
        db_name: String,
        file_path: std::path::PathBuf,
    },
    DropObject(DbObjectRef),
    RenameObject {
        object: DbObjectRef,
        new_name: String,
    },
    UpdateRow {
        schema: String,
        table: String,
        primary_key: Vec<(String, CellValue)>,
        values: Vec<(String, EditableCell)>,
    },
    InsertRow {
        schema: String,
        table: String,
        values: Vec<(String, EditableCell)>,
    },
    DeleteRow {
        schema: String,
        table: String,
        primary_key: Vec<(String, CellValue)>,
    },
}

#[derive(Debug)]
pub enum DbResponse {
    Connected {
        server_version: String,
        current_db: String,
        host_port: String,
    },
    ConnectionFailed(String),
    Disconnected,
    DatabasesFetched(Result<Vec<DatabaseItem>, String>),
    RolesFetched(Result<Vec<RoleItem>, String>),
    SchemasFetched {
        database: String,
        result: Result<Vec<String>, String>,
    },
    ObjectsFetched {
        database: String,
        key: ObjectTreeKey,
        result: Result<Vec<DbObjectItem>, String>,
    },
    DatabaseSwitched {
        db_name: String,
        server_version: String,
    },
    TablesFetched(Result<Vec<TableItem>, String>),
    TableDataResult(Result<QueryResult, String>),
    ColumnsFetched(Result<(String, String, Vec<ColumnItem>), String>),
    QueryResult(Result<QueryResult, String>),
    OperationSuccess {
        message: String,
        refresh_tables: bool,
        refresh_columns: bool,
    },
    OperationFailed(String),
    Busy(bool),
}

pub struct DbWorker {
    req_tx: Sender<DbRequest>,
    res_rx: Receiver<DbResponse>,
}

impl DbWorker {
    pub fn new() -> Self {
        let (req_tx, req_rx) = channel::<DbRequest>();
        let (res_tx, res_rx) = channel::<DbResponse>();

        std::thread::Builder::new()
            .name("pg-mini-db-worker".to_string())
            .spawn(move || {
                let rt = tokio::runtime::Builder::new_multi_thread()
                    .worker_threads(2)
                    .enable_all()
                    .build()
                    .expect("Failed to create Tokio runtime");

                rt.block_on(async move {
                    let mut current_client: Option<Client> = None;
                    let mut current_cfg: Option<ConnectionConfig> = None;

                    while let Ok(req) = req_rx.recv() {
                        match req {
                            DbRequest::Connect(cfg) => {
                                let _ = res_tx.send(DbResponse::Busy(true));
                                match connect_client(&cfg).await {
                                    Ok((client, version)) => {
                                        let current_db = cfg.database.clone();
                                        let host_port = format!("{}:{}", cfg.host, cfg.port);
                                        current_client = Some(client);
                                        current_cfg = Some(cfg);

                                        let _ = res_tx.send(DbResponse::Connected {
                                            server_version: version,
                                            current_db,
                                            host_port,
                                        });
                                    }
                                    Err(err) => {
                                        current_client = None;
                                        current_cfg = None;
                                        let _ = res_tx.send(DbResponse::ConnectionFailed(err));
                                    }
                                }
                                let _ = res_tx.send(DbResponse::Busy(false));
                            }
                            DbRequest::Disconnect => {
                                current_client = None;
                                current_cfg = None;
                                let _ = res_tx.send(DbResponse::Disconnected);
                            }
                            DbRequest::FetchDatabases => {
                                if let Some(client) = &current_client {
                                    let active_db = current_cfg
                                        .as_ref()
                                        .map(|c| c.database.as_str())
                                        .unwrap_or("");
                                    let result = fetch_databases_impl(client, active_db).await;
                                    let _ = res_tx.send(DbResponse::DatabasesFetched(result));
                                }
                            }
                            DbRequest::FetchRoles => {
                                if let Some(client) = &current_client {
                                    let _ = res_tx.send(DbResponse::Busy(true));
                                    let result = fetch_roles_impl(client).await;
                                    let _ = res_tx.send(DbResponse::RolesFetched(result));
                                    let _ = res_tx.send(DbResponse::Busy(false));
                                }
                            }
                            DbRequest::SaveRole(form) => {
                                if let Some(client) = &current_client {
                                    let _ = res_tx.send(DbResponse::Busy(true));
                                    match save_role_impl(client, &form).await {
                                        Ok(()) => {
                                            let _ = res_tx.send(DbResponse::OperationSuccess {
                                                message: format!(
                                                    "Role '{}' saved successfully",
                                                    form.name.trim()
                                                ),
                                                refresh_tables: false,
                                                refresh_columns: false,
                                            });
                                            let _ = res_tx.send(DbResponse::RolesFetched(
                                                fetch_roles_impl(client).await,
                                            ));
                                        }
                                        Err(error) => {
                                            let _ = res_tx.send(DbResponse::OperationFailed(
                                                format!("Failed to save role: {error}"),
                                            ));
                                        }
                                    }
                                    let _ = res_tx.send(DbResponse::Busy(false));
                                }
                            }
                            DbRequest::DropRole(role_name) => {
                                if let Some(client) = &current_client {
                                    let _ = res_tx.send(DbResponse::Busy(true));
                                    let sql = format!(
                                        "DROP ROLE {};",
                                        quote_role_ident(role_name.trim())
                                    );
                                    match client.simple_query(&sql).await {
                                        Ok(_) => {
                                            let _ = res_tx.send(DbResponse::OperationSuccess {
                                                message: format!(
                                                    "Role '{}' dropped successfully",
                                                    role_name
                                                ),
                                                refresh_tables: false,
                                                refresh_columns: false,
                                            });
                                            let _ = res_tx.send(DbResponse::RolesFetched(
                                                fetch_roles_impl(client).await,
                                            ));
                                        }
                                        Err(error) => {
                                            let _ = res_tx.send(DbResponse::OperationFailed(
                                                format!("Failed to drop role: {error}"),
                                            ));
                                        }
                                    }
                                    let _ = res_tx.send(DbResponse::Busy(false));
                                }
                            }
                            DbRequest::ExecuteDdl(sql) => {
                                if let Some(client) = &current_client {
                                    let _ = res_tx.send(DbResponse::Busy(true));
                                    match client.simple_query(&sql).await {
                                        Ok(_) => {
                                            let _ = res_tx.send(DbResponse::OperationSuccess {
                                                message: "Database object operation completed"
                                                    .to_string(),
                                                refresh_tables: true,
                                                refresh_columns: false,
                                            });
                                        }
                                        Err(error) => {
                                            let _ = res_tx.send(DbResponse::OperationFailed(
                                                format!("Failed to execute generated SQL: {error}"),
                                            ));
                                        }
                                    }
                                    let _ = res_tx.send(DbResponse::Busy(false));
                                }
                            }
                            DbRequest::SwitchDatabase(db_name) => {
                                if let Some(mut cfg) = current_cfg.clone() {
                                    let _ = res_tx.send(DbResponse::Busy(true));
                                    cfg.database = db_name.clone();
                                    match connect_client(&cfg).await {
                                        Ok((client, version)) => {
                                            current_client = Some(client);
                                            current_cfg = Some(cfg);
                                            let _ = res_tx.send(DbResponse::DatabaseSwitched {
                                                db_name,
                                                server_version: version,
                                            });
                                        }
                                        Err(err) => {
                                            let _ = res_tx.send(DbResponse::OperationFailed(
                                                format!("Failed to switch database: {}", err),
                                            ));
                                        }
                                    }
                                    let _ = res_tx.send(DbResponse::Busy(false));
                                }
                            }
                            DbRequest::FetchSchemas { database } => {
                                let result = match (&current_client, &current_cfg) {
                                    (Some(client), Some(cfg)) if cfg.database == database => {
                                        let _ = res_tx.send(DbResponse::Busy(true));
                                        fetch_schemas_impl(client).await
                                    }
                                    _ => Err(format!(
                                        "Connect to database '{}' before browsing its schemas",
                                        database
                                    )),
                                };
                                let _ =
                                    res_tx.send(DbResponse::SchemasFetched { database, result });
                                let _ = res_tx.send(DbResponse::Busy(false));
                            }
                            DbRequest::FetchObjects { database, key } => {
                                let result = match (&current_client, &current_cfg) {
                                    (Some(client), Some(cfg)) if cfg.database == database => {
                                        let _ = res_tx.send(DbResponse::Busy(true));
                                        fetch_objects_impl(client, &key).await
                                    }
                                    _ => Err(format!(
                                        "Connect to database '{}' before browsing its objects",
                                        database
                                    )),
                                };
                                let _ = res_tx.send(DbResponse::ObjectsFetched {
                                    database,
                                    key,
                                    result,
                                });
                                let _ = res_tx.send(DbResponse::Busy(false));
                            }
                            DbRequest::FetchTables => {
                                if let Some(client) = &current_client {
                                    let _ = res_tx.send(DbResponse::Busy(true));
                                    let result = fetch_tables_impl(client).await;
                                    let _ = res_tx.send(DbResponse::TablesFetched(result));
                                    let _ = res_tx.send(DbResponse::Busy(false));
                                }
                            }
                            DbRequest::FetchTableData {
                                schema,
                                table,
                                limit,
                                offset,
                            } => {
                                if let Some(client) = &current_client {
                                    let _ = res_tx.send(DbResponse::Busy(true));
                                    let result = fetch_table_data_impl(
                                        client, &schema, &table, limit, offset,
                                    )
                                    .await;
                                    let _ = res_tx.send(DbResponse::TableDataResult(result));
                                    let _ = res_tx.send(DbResponse::Busy(false));
                                }
                            }
                            DbRequest::FetchColumns { schema, table } => {
                                if let Some(client) = &current_client {
                                    let _ = res_tx.send(DbResponse::Busy(true));
                                    let result = fetch_columns_impl(client, &schema, &table).await;
                                    let _ = res_tx.send(DbResponse::ColumnsFetched(result));
                                    let _ = res_tx.send(DbResponse::Busy(false));
                                }
                            }
                            DbRequest::ExecuteSql(sql) => {
                                if let Some(client) = &current_client {
                                    let _ = res_tx.send(DbResponse::Busy(true));
                                    let result = execute_sql_impl(client, &sql).await;
                                    let _ = res_tx.send(DbResponse::QueryResult(result));
                                    let _ = res_tx.send(DbResponse::Busy(false));
                                } else {
                                    let _ = res_tx.send(DbResponse::QueryResult(Err(
                                        "Not connected to any PostgreSQL database".to_string(),
                                    )));
                                }
                            }
                            DbRequest::AddColumn(form) => {
                                if let Some(client) = &current_client {
                                    let _ = res_tx.send(DbResponse::Busy(true));
                                    let result = add_column_impl(client, &form).await;
                                    match result {
                                        Ok(msg) => {
                                            let _ = res_tx.send(DbResponse::OperationSuccess {
                                                message: msg,
                                                refresh_tables: false,
                                                refresh_columns: true,
                                            });
                                        }
                                        Err(err) => {
                                            let _ = res_tx.send(DbResponse::OperationFailed(err));
                                        }
                                    }
                                    let _ = res_tx.send(DbResponse::Busy(false));
                                }
                            }
                            DbRequest::CreateTable(form) => {
                                if let Some(client) = &current_client {
                                    let _ = res_tx.send(DbResponse::Busy(true));
                                    let result = create_table_impl(client, &form).await;
                                    match result {
                                        Ok(msg) => {
                                            let _ = res_tx.send(DbResponse::OperationSuccess {
                                                message: msg,
                                                refresh_tables: true,
                                                refresh_columns: false,
                                            });
                                        }
                                        Err(err) => {
                                            let _ = res_tx.send(DbResponse::OperationFailed(err));
                                        }
                                    }
                                    let _ = res_tx.send(DbResponse::Busy(false));
                                }
                            }
                            DbRequest::DropTable { schema, table } => {
                                if let Some(client) = &current_client {
                                    let _ = res_tx.send(DbResponse::Busy(true));
                                    let sql = format!(
                                        "DROP TABLE IF EXISTS \"{}\".\"{}\" CASCADE;",
                                        escape_sql_ident(&schema),
                                        escape_sql_ident(&table)
                                    );
                                    match client.simple_query(&sql).await {
                                        Ok(_) => {
                                            let _ = res_tx.send(DbResponse::OperationSuccess {
                                                message: format!(
                                                    "Table {}.{} dropped successfully",
                                                    schema, table
                                                ),
                                                refresh_tables: true,
                                                refresh_columns: false,
                                            });
                                        }
                                        Err(e) => {
                                            let _ = res_tx.send(DbResponse::OperationFailed(
                                                format!("Failed to drop table: {}", e),
                                            ));
                                        }
                                    }
                                    let _ = res_tx.send(DbResponse::Busy(false));
                                }
                            }
                            DbRequest::TruncateTable { schema, table } => {
                                if let Some(client) = &current_client {
                                    let _ = res_tx.send(DbResponse::Busy(true));
                                    let sql = format!(
                                        "TRUNCATE TABLE \"{}\".\"{}\" RESTART IDENTITY CASCADE;",
                                        escape_sql_ident(&schema),
                                        escape_sql_ident(&table)
                                    );
                                    match client.simple_query(&sql).await {
                                        Ok(_) => {
                                            let _ = res_tx.send(DbResponse::OperationSuccess {
                                                message: format!(
                                                    "Table {}.{} truncated successfully",
                                                    schema, table
                                                ),
                                                refresh_tables: false,
                                                refresh_columns: false,
                                            });
                                        }
                                        Err(e) => {
                                            let _ = res_tx.send(DbResponse::OperationFailed(
                                                format!("Failed to truncate table: {}", e),
                                            ));
                                        }
                                    }
                                    let _ = res_tx.send(DbResponse::Busy(false));
                                }
                            }
                            DbRequest::DropColumn {
                                schema,
                                table,
                                column,
                            } => {
                                if let Some(client) = &current_client {
                                    let _ = res_tx.send(DbResponse::Busy(true));
                                    let sql = format!(
                                        "ALTER TABLE \"{}\".\"{}\" DROP COLUMN \"{}\" CASCADE;",
                                        escape_sql_ident(&schema),
                                        escape_sql_ident(&table),
                                        escape_sql_ident(&column)
                                    );
                                    match client.simple_query(&sql).await {
                                        Ok(_) => {
                                            let _ = res_tx.send(DbResponse::OperationSuccess {
                                                message: format!(
                                                    "Column {} dropped from {}.{}",
                                                    column, schema, table
                                                ),
                                                refresh_tables: false,
                                                refresh_columns: true,
                                            });
                                        }
                                        Err(e) => {
                                            let _ = res_tx.send(DbResponse::OperationFailed(
                                                format!("Failed to drop column: {}", e),
                                            ));
                                        }
                                    }
                                    let _ = res_tx.send(DbResponse::Busy(false));
                                }
                            }
                            DbRequest::DropDatabase { db_name } => {
                                if let Some(client) = &current_client {
                                    let _ = res_tx.send(DbResponse::Busy(true));
                                    let sql = format!(
                                        "DROP DATABASE IF EXISTS \"{}\";",
                                        escape_sql_ident(&db_name)
                                    );
                                    match client.simple_query(&sql).await {
                                        Ok(_) => {
                                            let _ = res_tx.send(DbResponse::OperationSuccess {
                                                message: format!(
                                                    "Database {} dropped successfully",
                                                    db_name
                                                ),
                                                refresh_tables: true,
                                                refresh_columns: false,
                                            });
                                        }
                                        Err(e) => {
                                            let _ = res_tx.send(DbResponse::OperationFailed(
                                                format!("Failed to drop database: {}", e),
                                            ));
                                        }
                                    }
                                    let _ = res_tx.send(DbResponse::Busy(false));
                                }
                            }
                            DbRequest::BackupDatabase { db_name, file_path } => {
                                if let Some(cfg) = &current_cfg {
                                    let _ = res_tx.send(DbResponse::Busy(true));
                                    match backup_database(cfg, &db_name, &file_path) {
                                        Ok(()) => {
                                            let _ = res_tx.send(DbResponse::OperationSuccess {
                                                message: format!(
                                                    "Database '{}' backed up to {}",
                                                    db_name,
                                                    file_path.display()
                                                ),
                                                refresh_tables: false,
                                                refresh_columns: false,
                                            });
                                        }
                                        Err(error) => {
                                            let _ =
                                                res_tx.send(DbResponse::OperationFailed(error));
                                        }
                                    }
                                    let _ = res_tx.send(DbResponse::Busy(false));
                                } else {
                                    let _ = res_tx.send(DbResponse::OperationFailed(
                                        "Connect to a PostgreSQL server before backing up a database"
                                            .to_string(),
                                    ));
                                }
                            }
                            DbRequest::RestoreDatabase { db_name, file_path } => {
                                if let Some(cfg) = &current_cfg {
                                    let _ = res_tx.send(DbResponse::Busy(true));
                                    match restore_database(cfg, &db_name, &file_path) {
                                        Ok(()) => {
                                            let _ = res_tx.send(DbResponse::OperationSuccess {
                                                message: format!(
                                                    "Database '{}' restored from {}",
                                                    db_name,
                                                    file_path.display()
                                                ),
                                                refresh_tables: cfg.database == db_name,
                                                refresh_columns: false,
                                            });
                                        }
                                        Err(error) => {
                                            let _ =
                                                res_tx.send(DbResponse::OperationFailed(error));
                                        }
                                    }
                                    let _ = res_tx.send(DbResponse::Busy(false));
                                } else {
                                    let _ = res_tx.send(DbResponse::OperationFailed(
                                        "Connect to a PostgreSQL server before restoring a database"
                                            .to_string(),
                                    ));
                                }
                            }
                            DbRequest::DropObject(object) => {
                                if let Some(client) = &current_client {
                                    let _ = res_tx.send(DbResponse::Busy(true));
                                    match client.simple_query(&object.drop_sql()).await {
                                        Ok(_) => {
                                            let _ = res_tx.send(DbResponse::OperationSuccess {
                                                message: format!(
                                                    "{} '{}' deleted successfully",
                                                    object.kind.singular(),
                                                    object.qualified_name()
                                                ),
                                                refresh_tables: true,
                                                refresh_columns: false,
                                            });
                                        }
                                        Err(err) => {
                                            let _ =
                                                res_tx.send(DbResponse::OperationFailed(format!(
                                                    "Failed to delete {}: {}",
                                                    object.kind.singular(),
                                                    err
                                                )));
                                        }
                                    }
                                    let _ = res_tx.send(DbResponse::Busy(false));
                                }
                            }
                            DbRequest::RenameObject { object, new_name } => {
                                if let Some(client) = &current_client {
                                    let _ = res_tx.send(DbResponse::Busy(true));
                                    let result = match object.rename_sql(new_name.trim()) {
                                        Some(sql) if !new_name.trim().is_empty() => client
                                            .simple_query(&sql)
                                            .await
                                            .map(|_| ())
                                            .map_err(|err| err.to_string()),
                                        Some(_) => {
                                            Err("The new object name cannot be empty".to_string())
                                        }
                                        None => Err(format!(
                                            "PostgreSQL does not support renaming {} objects",
                                            object.kind.singular().to_lowercase()
                                        )),
                                    };
                                    match result {
                                        Ok(()) => {
                                            let _ = res_tx.send(DbResponse::OperationSuccess {
                                                message: format!(
                                                    "{} '{}' renamed to '{}'",
                                                    object.kind.singular(),
                                                    object.name,
                                                    new_name.trim()
                                                ),
                                                refresh_tables: true,
                                                refresh_columns: false,
                                            });
                                        }
                                        Err(err) => {
                                            let _ =
                                                res_tx.send(DbResponse::OperationFailed(format!(
                                                    "Failed to rename {}: {}",
                                                    object.kind.singular().to_lowercase(),
                                                    err
                                                )));
                                        }
                                    }
                                    let _ = res_tx.send(DbResponse::Busy(false));
                                }
                            }
                            DbRequest::UpdateRow {
                                schema,
                                table,
                                primary_key,
                                values,
                            } => {
                                if let Some(client) = &current_client {
                                    let _ = res_tx.send(DbResponse::Busy(true));
                                    match update_row_impl(
                                        client,
                                        &schema,
                                        &table,
                                        &primary_key,
                                        &values,
                                    )
                                    .await
                                    {
                                        Ok(()) => {
                                            let _ = res_tx.send(DbResponse::OperationSuccess {
                                                message: format!(
                                                    "Row updated in {}.{}",
                                                    schema, table
                                                ),
                                                refresh_tables: false,
                                                refresh_columns: true,
                                            });
                                        }
                                        Err(err) => {
                                            let _ = res_tx.send(DbResponse::OperationFailed(err));
                                        }
                                    }
                                    let _ = res_tx.send(DbResponse::Busy(false));
                                }
                            }
                            DbRequest::InsertRow {
                                schema,
                                table,
                                values,
                            } => {
                                if let Some(client) = &current_client {
                                    let _ = res_tx.send(DbResponse::Busy(true));
                                    match insert_row_impl(client, &schema, &table, &values).await {
                                        Ok(()) => {
                                            let _ = res_tx.send(DbResponse::OperationSuccess {
                                                message: format!(
                                                    "Row inserted into {}.{}",
                                                    schema, table
                                                ),
                                                refresh_tables: false,
                                                refresh_columns: true,
                                            });
                                        }
                                        Err(err) => {
                                            let _ = res_tx.send(DbResponse::OperationFailed(err));
                                        }
                                    }
                                    let _ = res_tx.send(DbResponse::Busy(false));
                                }
                            }
                            DbRequest::DeleteRow {
                                schema,
                                table,
                                primary_key,
                            } => {
                                if let Some(client) = &current_client {
                                    let _ = res_tx.send(DbResponse::Busy(true));
                                    match delete_row_impl(client, &schema, &table, &primary_key)
                                        .await
                                    {
                                        Ok(()) => {
                                            let _ = res_tx.send(DbResponse::OperationSuccess {
                                                message: format!(
                                                    "Row deleted from {}.{}",
                                                    schema, table
                                                ),
                                                refresh_tables: false,
                                                refresh_columns: true,
                                            });
                                        }
                                        Err(err) => {
                                            let _ = res_tx.send(DbResponse::OperationFailed(err));
                                        }
                                    }
                                    let _ = res_tx.send(DbResponse::Busy(false));
                                }
                            }
                        }
                    }
                });
            })
            .expect("Failed to spawn db worker thread");

        Self { req_tx, res_rx }
    }

    pub fn send(&self, req: DbRequest) {
        let _ = self.req_tx.send(req);
    }

    pub fn try_recv(&self) -> Option<DbResponse> {
        self.res_rx.try_recv().ok()
    }
}

async fn connect_client(cfg: &ConnectionConfig) -> Result<(Client, String), String> {
    let mut pg_config = tokio_postgres::Config::new();
    pg_config.host(&cfg.host);
    pg_config.port(cfg.port);
    pg_config.user(&cfg.username);
    if !cfg.password.is_empty() {
        pg_config.password(&cfg.password);
    }
    pg_config.dbname(&cfg.database);
    pg_config.connect_timeout(std::time::Duration::from_secs(8));

    let (client, connection) = pg_config
        .connect(NoTls)
        .await
        .map_err(|e| format!("Failed to connect: {}", e))?;

    tokio::spawn(async move {
        if let Err(e) = connection.await {
            eprintln!("PostgreSQL connection error: {}", e);
        }
    });

    let version_rows = client
        .simple_query("SELECT version();")
        .await
        .map_err(|e| format!("Failed to fetch version: {}", e))?;

    let version = version_rows
        .into_iter()
        .find_map(|msg| match msg {
            SimpleQueryMessage::Row(row) => row.get(0).map(|s| s.to_string()),
            _ => None,
        })
        .unwrap_or_else(|| "PostgreSQL (Unknown version)".to_string());

    Ok((client, version))
}

fn backup_database(cfg: &ConnectionConfig, database: &str, file_path: &Path) -> Result<(), String> {
    let mut command = postgres_tool("pg_dump");
    command
        .arg("--format=custom")
        .arg("--no-password")
        .arg("--host")
        .arg(&cfg.host)
        .arg("--port")
        .arg(cfg.port.to_string())
        .arg("--username")
        .arg(&cfg.username)
        .arg("--dbname")
        .arg(database)
        .arg("--file")
        .arg(file_path);
    set_tool_password(&mut command, &cfg.password);
    run_postgres_tool(command, "pg_dump")
}

fn restore_database(
    cfg: &ConnectionConfig,
    database: &str,
    file_path: &Path,
) -> Result<(), String> {
    if !file_path.is_file() {
        return Err(format!(
            "Backup file does not exist or is not a regular file: {}",
            file_path.display()
        ));
    }

    let is_sql_file = is_plain_sql_backup(file_path);
    let tool = if is_sql_file { "psql" } else { "pg_restore" };
    let mut command = postgres_tool(tool);
    command
        .arg("--no-password")
        .arg("--host")
        .arg(&cfg.host)
        .arg("--port")
        .arg(cfg.port.to_string())
        .arg("--username")
        .arg(&cfg.username)
        .arg("--dbname")
        .arg(database);

    if is_sql_file {
        command
            .arg("--set=ON_ERROR_STOP=on")
            .arg("--file")
            .arg(file_path);
    } else {
        command.arg("--exit-on-error").arg(file_path);
    }
    set_tool_password(&mut command, &cfg.password);
    run_postgres_tool(command, tool)
}

fn is_plain_sql_backup(file_path: &Path) -> bool {
    file_path
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("sql"))
}

fn postgres_tool(name: &str) -> Command {
    #[cfg(target_os = "windows")]
    let executable = format!("{name}.exe");
    #[cfg(not(target_os = "windows"))]
    let executable = name.to_string();
    Command::new(executable)
}

fn set_tool_password(command: &mut Command, password: &str) {
    if password.is_empty() {
        command.env_remove("PGPASSWORD");
    } else {
        command.env("PGPASSWORD", password);
    }
}

fn run_postgres_tool(mut command: Command, tool: &str) -> Result<(), String> {
    command.stdout(Stdio::null());
    let output = command.output().map_err(|error| {
        format!(
            "Could not start {}: {}. Install the PostgreSQL client utilities and make sure {} is available on PATH.",
            tool, error, tool
        )
    })?;
    if output.status.success() {
        return Ok(());
    }

    let details = String::from_utf8_lossy(&output.stderr);
    Err(format!(
        "{} failed with status {}{}",
        tool,
        output.status,
        if details.trim().is_empty() {
            String::new()
        } else {
            format!(": {}", details.trim())
        }
    ))
}

async fn fetch_roles_impl(client: &Client) -> Result<Vec<RoleItem>, String> {
    let rows = client
        .simple_query(
            "SELECT rolname, rolcanlogin, rolsuper, rolcreatedb, rolcreaterole, \
             rolinherit, rolreplication, rolbypassrls, rolconnlimit, \
             CASE WHEN rolvaliduntil = 'infinity'::timestamptz THEN NULL \
                  ELSE rolvaliduntil::text END \
             FROM pg_catalog.pg_roles ORDER BY rolname;",
        )
        .await
        .map_err(|error| format!("Failed to fetch PostgreSQL roles: {error}"))?;

    let mut roles: Vec<RoleItem> = rows
        .into_iter()
        .filter_map(|row| match row {
            SimpleQueryMessage::Row(row) => Some(parse_role_row(&row)),
            _ => None,
        })
        .collect::<Result<_, _>>()?;
    let membership_rows = client
        .simple_query(
            "SELECT member.rolname, granted.rolname \
             FROM pg_catalog.pg_auth_members AS membership \
             JOIN pg_catalog.pg_roles AS member ON member.oid = membership.member \
             JOIN pg_catalog.pg_roles AS granted ON granted.oid = membership.roleid \
             ORDER BY member.rolname, granted.rolname;",
        )
        .await
        .map_err(|error| format!("Failed to fetch role memberships: {error}"))?;

    for row in membership_rows {
        if let SimpleQueryMessage::Row(row) = row {
            if let (Some(member), Some(granted)) = (row.get(0), row.get(1)) {
                if let Some(role) = roles.iter_mut().find(|role| role.name == member) {
                    role.memberships.push(granted.to_string());
                }
            }
        }
    }

    Ok(roles)
}

fn parse_role_row(row: &tokio_postgres::SimpleQueryRow) -> Result<RoleItem, String> {
    let name = row
        .get(0)
        .ok_or_else(|| "Role query returned a row without a name".to_string())?;
    let boolean = |index| match row.get(index) {
        Some("t") => Ok(true),
        Some("f") => Ok(false),
        value => Err(format!("Invalid role flag value: {value:?}")),
    };
    let connection_limit = row
        .get(8)
        .and_then(|value| value.parse::<i32>().ok())
        .ok_or_else(|| format!("Invalid connection limit for role '{name}'"))?;

    Ok(RoleItem {
        name: name.to_string(),
        memberships: Vec::new(),
        can_login: boolean(1)?,
        is_superuser: boolean(2)?,
        can_create_db: boolean(3)?,
        can_create_role: boolean(4)?,
        can_inherit: boolean(5)?,
        can_replicate: boolean(6)?,
        can_bypass_rls: boolean(7)?,
        connection_limit,
        valid_until: row.get(9).map(str::to_string),
    })
}

pub fn role_sql_preview(form: &RoleForm) -> Result<String, String> {
    let name = form.name.trim();
    if name.is_empty() || name.contains('\0') {
        return Err("Role name must not be empty or contain a NUL character".to_string());
    }
    if form.password.contains('\0') {
        return Err("Password must not contain a NUL character".to_string());
    }
    if form
        .memberships
        .iter()
        .chain(form.original_memberships.iter())
        .any(|membership| membership.is_empty() || membership.contains('\0'))
    {
        return Err("Role memberships must have valid role names".to_string());
    }
    let connection_limit = form
        .connection_limit
        .trim()
        .parse::<i32>()
        .map_err(|_| "Connection limit must be -1 (unlimited) or a non-negative integer".to_string())?;
    if connection_limit < -1 {
        return Err("Connection limit must be -1 (unlimited) or a non-negative integer".to_string());
    }

    let valid_until = form.valid_until.trim();
    if valid_until.contains('\0') {
        return Err("Validity timestamp must not contain a NUL character".to_string());
    }

    let role_ident = quote_role_ident(name);
    let old_name = form.original_name.as_deref().map(str::trim);
    if old_name.is_some_and(|old| old.is_empty() || old.contains('\0')) {
        return Err("Original role name is invalid".to_string());
    }

    let flags = format!(
        "{} {} {} {} {} {} {} CONNECTION LIMIT {} VALID UNTIL {}",
        if form.can_login { "LOGIN" } else { "NOLOGIN" },
        if form.is_superuser { "SUPERUSER" } else { "NOSUPERUSER" },
        if form.can_create_db { "CREATEDB" } else { "NOCREATEDB" },
        if form.can_create_role { "CREATEROLE" } else { "NOCREATEROLE" },
        if form.can_inherit { "INHERIT" } else { "NOINHERIT" },
        if form.can_replicate { "REPLICATION" } else { "NOREPLICATION" },
        if form.can_bypass_rls { "BYPASSRLS" } else { "NOBYPASSRLS" },
        connection_limit,
        if valid_until.is_empty() {
            "'infinity'".to_string()
        } else {
            role_sql_literal(valid_until)
        }
    );

    let mut statements = Vec::new();
    if let Some(old_name) = old_name.filter(|old| *old != name) {
        statements.push(format!(
            "ALTER ROLE {} RENAME TO {};",
            quote_role_ident(old_name),
            role_ident
        ));
    }

    if let Some(old_name) = old_name {
        let target = if old_name == name {
            role_ident
        } else {
            quote_role_ident(name)
        };
        let password = if form.password.is_empty() {
            String::new()
        } else {
            format!(" PASSWORD {}", role_sql_literal(&form.password))
        };
        statements.push(format!("ALTER ROLE {target} WITH {flags}{password};"));
        for membership in &form.memberships {
            if !form.original_memberships.contains(membership) {
                statements.push(format!(
                    "GRANT {} TO {};",
                    quote_role_ident(membership),
                    quote_role_ident(name)
                ));
            }
        }
        for membership in &form.original_memberships {
            if !form.memberships.contains(membership) {
                statements.push(format!(
                    "REVOKE {} FROM {};",
                    quote_role_ident(membership),
                    quote_role_ident(name)
                ));
            }
        }
    } else {
        let password = if form.password.is_empty() {
            String::new()
        } else {
            format!(" PASSWORD {}", role_sql_literal(&form.password))
        };
        statements.push(format!("CREATE ROLE {role_ident} WITH {flags}{password};"));
        for membership in &form.memberships {
            statements.push(format!(
                "GRANT {} TO {};",
                quote_role_ident(membership),
                quote_role_ident(name)
            ));
        }
    }
    Ok(statements.join("\n"))
}

fn role_sql_literal(value: &str) -> String {
    format!("E'{}'", value.replace('\\', "\\\\").replace('\'', "''"))
}

fn quote_role_ident(value: &str) -> String {
    format!("\"{}\"", escape_sql_ident(value))
}

async fn save_role_impl(client: &Client, form: &RoleForm) -> Result<(), String> {
    let sql = role_sql_preview(form)?;
    client
        .simple_query(&sql)
        .await
        .map_err(|error| error.to_string())?;
    Ok(())
}

#[cfg(test)]
mod db_tests {
    use super::{is_plain_sql_backup, role_sql_preview};
    use crate::models::RoleForm;
    use std::path::Path;

    #[test]
    fn identifies_plain_sql_backups_case_insensitively() {
        assert!(is_plain_sql_backup(Path::new("backup.sql")));
        assert!(is_plain_sql_backup(Path::new("backup.SQL")));
        assert!(!is_plain_sql_backup(Path::new("backup.dump")));
        assert!(!is_plain_sql_backup(Path::new("backup.tar")));
    }

    #[test]
    fn role_sql_quotes_names_and_passwords() {
        let mut form = RoleForm::new();
        form.name = "user\" name".to_string();
        form.password = "p\\ass'word".to_string();
        let sql = role_sql_preview(&form).unwrap();
        assert!(sql.starts_with("CREATE ROLE \"user\"\" name\" WITH LOGIN"));
        assert!(sql.contains("PASSWORD E'p\\\\ass''word'"));
        assert!(!sql.contains("p\\ass'word';"));
    }

    #[test]
    fn role_sql_validates_connection_limit() {
        let mut form = RoleForm::new();
        form.connection_limit = "-2".to_string();
        assert!(role_sql_preview(&form).is_err());
    }

    #[test]
    fn role_sql_changes_existing_role_without_resetting_password() {
        let mut form = RoleForm::new();
        form.original_name = Some("old_role".to_string());
        form.name = "new_role".to_string();
        let sql = role_sql_preview(&form).unwrap();
        assert!(sql.contains("RENAME TO \"new_role\""));
        assert!(sql.contains("ALTER ROLE \"new_role\" WITH"));
        assert!(!sql.contains("PASSWORD"));
    }

    #[test]
    fn role_sql_updates_group_memberships() {
        let mut form = RoleForm::new();
        form.original_name = Some("app_user".to_string());
        form.name = "app_user".to_string();
        form.original_memberships = vec!["old_group".to_string()];
        form.memberships = vec!["new_group".to_string()];
        let sql = role_sql_preview(&form).unwrap();
        assert!(sql.contains("GRANT \"new_group\" TO \"app_user\";"));
        assert!(sql.contains("REVOKE \"old_group\" FROM \"app_user\";"));
    }
}

async fn fetch_databases_impl(
    client: &Client,
    current_db: &str,
) -> Result<Vec<DatabaseItem>, String> {
    let sql = "
        SELECT 
            d.datname,
            pg_size_pretty(pg_database_size(d.datname)) AS size
        FROM pg_database d
        WHERE d.datistemplate = false
        ORDER BY d.datname;
    ";

    let messages = client
        .simple_query(sql)
        .await
        .map_err(|e| format!("Failed to fetch databases: {}", e))?;

    let mut databases = Vec::new();
    for msg in messages {
        if let SimpleQueryMessage::Row(row) = msg {
            let name = row.get(0).unwrap_or("").to_string();
            let size = row.get(1).map(|s| s.to_string());
            let is_current = name == current_db;
            databases.push(DatabaseItem {
                name,
                size,
                is_current,
            });
        }
    }

    Ok(databases)
}

async fn fetch_tables_impl(client: &Client) -> Result<Vec<TableItem>, String> {
    let sql = "
        SELECT table_schema, table_name, table_type
        FROM information_schema.tables
        WHERE table_schema NOT IN ('pg_catalog', 'information_schema')
        ORDER BY table_schema, table_name;
    ";

    let messages = client
        .simple_query(sql)
        .await
        .map_err(|e| format!("Failed to fetch tables: {}", e))?;

    let mut tables = Vec::new();
    for msg in messages {
        if let SimpleQueryMessage::Row(row) = msg {
            let schema = row.get(0).unwrap_or("").to_string();
            let name = row.get(1).unwrap_or("").to_string();
            let table_type = row.get(2).unwrap_or("BASE TABLE").to_string();
            tables.push(TableItem {
                schema,
                name,
                table_type,
            });
        }
    }

    Ok(tables)
}

async fn fetch_schemas_impl(client: &Client) -> Result<Vec<String>, String> {
    let rows = client
        .simple_query(
            "SELECT nspname FROM pg_catalog.pg_namespace \
             WHERE nspname NOT IN ('pg_catalog', 'information_schema') \
               AND nspname NOT LIKE 'pg_toast%' \
               AND left(nspname, 8) <> 'pg_temp_' \
             ORDER BY nspname;",
        )
        .await
        .map_err(|err| format!("Failed to fetch schemas: {}", err))?;

    Ok(rows
        .into_iter()
        .filter_map(|message| match message {
            SimpleQueryMessage::Row(row) => row.get(0).map(str::to_string),
            _ => None,
        })
        .collect())
}

async fn fetch_objects_impl(
    client: &Client,
    key: &ObjectTreeKey,
) -> Result<Vec<DbObjectItem>, String> {
    let schema = key.schema.as_deref().unwrap_or("");
    let schema_literal = escape_sql_literal(schema);
    let sql = match key.kind {
        DbObjectKind::Database => {
            return Err("Databases are loaded from the server's database list".to_string());
        }
        DbObjectKind::Table => format!(
            "SELECT c.relname, NULL::text FROM pg_catalog.pg_class AS c \
             JOIN pg_catalog.pg_namespace AS n ON n.oid = c.relnamespace \
             WHERE n.nspname = '{schema_literal}' AND c.relkind IN ('r', 'p') \
             ORDER BY c.relname;"
        ),
        DbObjectKind::View => format!(
            "SELECT c.relname, NULL::text FROM pg_catalog.pg_class AS c \
             JOIN pg_catalog.pg_namespace AS n ON n.oid = c.relnamespace \
             WHERE n.nspname = '{schema_literal}' AND c.relkind = 'v' \
             ORDER BY c.relname;"
        ),
        DbObjectKind::MaterializedView => format!(
            "SELECT c.relname, NULL::text FROM pg_catalog.pg_class AS c \
             JOIN pg_catalog.pg_namespace AS n ON n.oid = c.relnamespace \
             WHERE n.nspname = '{schema_literal}' AND c.relkind = 'm' \
             ORDER BY c.relname;"
        ),
        DbObjectKind::Sequence => format!(
            "SELECT c.relname, NULL::text FROM pg_catalog.pg_class AS c \
             JOIN pg_catalog.pg_namespace AS n ON n.oid = c.relnamespace \
             WHERE n.nspname = '{schema_literal}' AND c.relkind = 'S' \
             ORDER BY c.relname;"
        ),
        DbObjectKind::Function | DbObjectKind::Procedure => {
            let prokind = if key.kind == DbObjectKind::Function {
                "f"
            } else {
                "p"
            };
            format!(
                "SELECT p.proname, pg_catalog.pg_get_function_identity_arguments(p.oid) \
                 FROM pg_catalog.pg_proc AS p \
                 JOIN pg_catalog.pg_namespace AS n ON n.oid = p.pronamespace \
                 WHERE n.nspname = '{schema_literal}' AND p.prokind = '{prokind}' \
                 ORDER BY p.proname, 2;"
            )
        }
        DbObjectKind::Type => format!(
            "SELECT t.typname, NULL::text FROM pg_catalog.pg_type AS t \
             JOIN pg_catalog.pg_namespace AS n ON n.oid = t.typnamespace \
             LEFT JOIN pg_catalog.pg_class AS c ON c.oid = t.typrelid \
             WHERE n.nspname = '{schema_literal}' AND t.typisdefined \
               AND t.typtype IN ('b', 'c', 'd', 'e', 'm', 'r') \
               AND (c.oid IS NULL OR c.relkind NOT IN ('r', 'p', 'v', 'm', 'f')) \
             ORDER BY t.typname;"
        ),
        DbObjectKind::Extension => {
            "SELECT e.extname, NULL::text FROM pg_catalog.pg_extension AS e ORDER BY e.extname;"
                .to_string()
        }
        DbObjectKind::Schema => {
            return Err("Schemas are loaded with the database's schema list".to_string());
        }
    };

    let rows = client.simple_query(&sql).await.map_err(|err| {
        format!(
            "Failed to fetch {}: {}",
            key.kind.label().to_lowercase(),
            err
        )
    })?;

    Ok(rows
        .into_iter()
        .filter_map(|message| match message {
            SimpleQueryMessage::Row(row) => row.get(0).map(|name| DbObjectItem {
                name: name.to_string(),
                identity_arguments: row.get(1).map(str::to_string),
            }),
            _ => None,
        })
        .collect())
}

async fn fetch_columns_impl(
    client: &Client,
    schema: &str,
    table: &str,
) -> Result<(String, String, Vec<ColumnItem>), String> {
    let sql = format!(
        "SELECT 
            c.column_name, 
            c.data_type, 
            c.is_nullable, 
            c.column_default,
            COALESCE(pk.is_pk, false) AS is_primary_key
        FROM information_schema.columns c
        LEFT JOIN (
            SELECT 
                kcu.column_name, 
                true AS is_pk
            FROM information_schema.table_constraints tc
            JOIN information_schema.key_column_usage kcu 
              ON tc.constraint_name = kcu.constraint_name 
              AND tc.table_schema = kcu.table_schema
            WHERE tc.constraint_type = 'PRIMARY KEY' 
              AND tc.table_schema = '{s}' 
              AND tc.table_name = '{t}'
        ) pk ON c.column_name = pk.column_name
        WHERE c.table_schema = '{s}' AND c.table_name = '{t}'
        ORDER BY c.ordinal_position;",
        s = escape_sql_literal(schema),
        t = escape_sql_literal(table)
    );

    let messages = client
        .simple_query(&sql)
        .await
        .map_err(|e| format!("Failed to fetch columns: {}", e))?;

    let mut columns = Vec::new();
    for msg in messages {
        if let SimpleQueryMessage::Row(row) = msg {
            let name = row.get(0).unwrap_or("").to_string();
            let data_type = row.get(1).unwrap_or("").to_string();
            let is_nullable = row
                .get(2)
                .map(|s| s.eq_ignore_ascii_case("YES"))
                .unwrap_or(true);
            let default_value = row.get(3).map(|s| s.to_string());
            let is_primary_key = row.get(4).map(|s| s == "t" || s == "true").unwrap_or(false);

            columns.push(ColumnItem {
                name,
                data_type,
                is_nullable,
                default_value,
                is_primary_key,
            });
        }
    }

    Ok((schema.to_string(), table.to_string(), columns))
}

async fn fetch_table_data_impl(
    client: &Client,
    schema: &str,
    table: &str,
    limit: usize,
    offset: usize,
) -> Result<QueryResult, String> {
    let sql = format!(
        "SELECT * FROM \"{}\".\"{}\" LIMIT {} OFFSET {};",
        escape_sql_ident(schema),
        escape_sql_ident(table),
        limit,
        offset
    );

    execute_sql_impl(client, &sql).await
}

async fn execute_sql_impl(client: &Client, sql: &str) -> Result<QueryResult, String> {
    let start = Instant::now();
    let messages = client
        .simple_query(sql)
        .await
        .map_err(|e| format!("Query error: {}", e))?;

    let elapsed = start.elapsed().as_secs_f64() * 1000.0;

    let mut columns = Vec::new();
    let mut rows = Vec::new();
    let mut rows_affected = None;
    let mut got_headers = false;

    for msg in messages {
        match msg {
            SimpleQueryMessage::Row(row) => {
                if !got_headers {
                    for col in row.columns() {
                        columns.push(col.name().to_string());
                    }
                    got_headers = true;
                }

                let mut row_vals = Vec::with_capacity(columns.len());
                for i in 0..columns.len() {
                    if let Some(val) = row.get(i) {
                        let is_num = val.parse::<f64>().is_ok();
                        if is_num {
                            row_vals.push(CellValue::new_number(val));
                        } else {
                            row_vals.push(CellValue::new_text(val));
                        }
                    } else {
                        row_vals.push(CellValue::new_null());
                    }
                }
                rows.push(row_vals);
            }
            SimpleQueryMessage::CommandComplete(count) => {
                rows_affected = Some(count);
            }
            _ => {}
        }
    }

    let is_select = got_headers || rows_affected.is_none();
    let col_count = columns.len();

    Ok(QueryResult {
        query: sql.to_string(),
        columns,
        column_types: vec!["text".to_string(); col_count],
        rows,
        rows_affected,
        execution_time_ms: elapsed,
        is_select,
    })
}

async fn add_column_impl(client: &Client, form: &AddColumnForm) -> Result<String, String> {
    if form.column_name.trim().is_empty() {
        return Err("Column name cannot be empty".to_string());
    }

    let mut sql = format!(
        "ALTER TABLE \"{}\".\"{}\" ADD COLUMN \"{}\" {}",
        escape_sql_ident(&form.schema),
        escape_sql_ident(&form.table),
        escape_sql_ident(form.column_name.trim()),
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

    client
        .simple_query(&sql)
        .await
        .map_err(|e| format!("Failed to add column: {}", e))?;

    Ok(format!(
        "Column '{}' successfully added to {}.{}",
        form.column_name, form.schema, form.table
    ))
}

async fn create_table_impl(client: &Client, form: &CreateTableForm) -> Result<String, String> {
    if form.table_name.trim().is_empty() {
        return Err("Table name cannot be empty".to_string());
    }

    let sql = if form.add_id_pk {
        format!(
            "CREATE TABLE \"{}\".\"{}\" (id SERIAL PRIMARY KEY);",
            escape_sql_ident(&form.schema),
            escape_sql_ident(form.table_name.trim())
        )
    } else {
        format!(
            "CREATE TABLE \"{}\".\"{}\" ();",
            escape_sql_ident(&form.schema),
            escape_sql_ident(form.table_name.trim())
        )
    };

    client
        .simple_query(&sql)
        .await
        .map_err(|e| format!("Failed to create table: {}", e))?;

    Ok(format!(
        "Table '{}.{}' created successfully",
        form.schema, form.table_name
    ))
}

fn escape_sql_ident(ident: &str) -> String {
    ident.replace('"', "\"\"")
}

fn escape_sql_literal(literal: &str) -> String {
    literal.replace('\'', "''")
}

fn edited_cell_sql(cell: &EditableCell) -> Result<String, String> {
    if cell.use_default {
        return Ok("DEFAULT".to_string());
    }
    if cell.is_null {
        return Ok("NULL".to_string());
    }
    Ok(escape_sql_value(&cell.text))
}

fn escape_sql_value(value: &str) -> String {
    let escaped = value.replace('\\', "\\\\").replace('\'', "\\'");
    format!("E'{}'", escaped)
}

fn primary_key_predicate(primary_key: &[(String, CellValue)]) -> Result<String, String> {
    if primary_key.is_empty() {
        return Err(
            "This table has no primary key; row updates and deletes are disabled.".to_string(),
        );
    }
    Ok(primary_key
        .iter()
        .map(|(name, value)| {
            let ident = format!("\"{}\"", escape_sql_ident(name));
            if value.is_null {
                format!("{} IS NULL", ident)
            } else {
                format!(
                    "{} IS NOT DISTINCT FROM {}",
                    ident,
                    escape_sql_value(&value.text)
                )
            }
        })
        .collect::<Vec<_>>()
        .join(" AND "))
}

async fn update_row_impl(
    client: &Client,
    schema: &str,
    table: &str,
    primary_key: &[(String, CellValue)],
    values: &[(String, EditableCell)],
) -> Result<(), String> {
    if values.is_empty() {
        return Err("No changed values to save.".to_string());
    }
    let assignments = values
        .iter()
        .map(|(name, value)| {
            Ok(format!(
                "\"{}\" = {}",
                escape_sql_ident(name),
                edited_cell_sql(value)?
            ))
        })
        .collect::<Result<Vec<_>, String>>()?
        .join(", ");
    let predicate = primary_key_predicate(primary_key)?;
    let sql = format!(
        "UPDATE \"{}\".\"{}\" SET {} WHERE {};",
        escape_sql_ident(schema),
        escape_sql_ident(table),
        assignments,
        predicate
    );
    execute_single_row_mutation(client, &sql, "update").await
}

async fn insert_row_impl(
    client: &Client,
    schema: &str,
    table: &str,
    values: &[(String, EditableCell)],
) -> Result<(), String> {
    let supplied: Vec<_> = values
        .iter()
        .filter(|(_, value)| !value.use_default)
        .collect();
    let sql = if supplied.is_empty() {
        format!(
            "INSERT INTO \"{}\".\"{}\" DEFAULT VALUES;",
            escape_sql_ident(schema),
            escape_sql_ident(table)
        )
    } else {
        let names = supplied
            .iter()
            .map(|(name, _)| format!("\"{}\"", escape_sql_ident(name)))
            .collect::<Vec<_>>()
            .join(", ");
        let vals = supplied
            .iter()
            .map(|(_, value)| edited_cell_sql(value))
            .collect::<Result<Vec<_>, _>>()?
            .join(", ");
        format!(
            "INSERT INTO \"{}\".\"{}\" ({}) VALUES ({});",
            escape_sql_ident(schema),
            escape_sql_ident(table),
            names,
            vals
        )
    };
    client
        .simple_query(&sql)
        .await
        .map(|_| ())
        .map_err(|e| format!("Failed to insert row: {}", e))
}

async fn delete_row_impl(
    client: &Client,
    schema: &str,
    table: &str,
    primary_key: &[(String, CellValue)],
) -> Result<(), String> {
    let predicate = primary_key_predicate(primary_key)?;
    let sql = format!(
        "DELETE FROM \"{}\".\"{}\" WHERE {};",
        escape_sql_ident(schema),
        escape_sql_ident(table),
        predicate
    );
    execute_single_row_mutation(client, &sql, "delete").await
}

async fn execute_single_row_mutation(
    client: &Client,
    sql: &str,
    operation: &str,
) -> Result<(), String> {
    let messages = client
        .simple_query(sql)
        .await
        .map_err(|e| format!("Failed to {} row: {}", operation, e))?;
    let affected = messages
        .into_iter()
        .find_map(|message| match message {
            SimpleQueryMessage::CommandComplete(count) => Some(count),
            _ => None,
        })
        .unwrap_or(0);
    if affected != 1 {
        return Err(if affected == 0 {
            format!(
                "Could not {} row: it may have been changed or deleted by another session.",
                operation
            )
        } else {
            format!(
                "Refusing to {} row: primary-key match affected {} rows.",
                operation, affected
            )
        });
    }
    Ok(())
}
