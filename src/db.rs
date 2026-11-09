use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::Instant;
use tokio_postgres::{Client, NoTls, SimpleQueryMessage};

use crate::models::{
    AddColumnForm, CellValue, ColumnItem, ConnectionConfig, CreateTableForm, DatabaseItem,
    DbObjectItem, DbObjectKind, DbObjectRef, EditableCell, ObjectTreeKey, QueryResult, TableItem,
};

#[derive(Debug)]
pub enum DbRequest {
    Connect(ConnectionConfig),
    Disconnect,
    FetchDatabases,
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
