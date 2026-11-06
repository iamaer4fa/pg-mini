use std::sync::mpsc::{channel, Receiver, Sender};
use std::time::Instant;
use tokio_postgres::{Client, NoTls, SimpleQueryMessage};

use crate::models::{
    AddColumnForm, CellValue, ColumnItem, ConnectionConfig, CreateTableForm, DatabaseItem,
    QueryResult, TableItem,
};

#[derive(Debug)]
pub enum DbRequest {
    Connect(ConnectionConfig),
    Disconnect,
    FetchDatabases,
    SwitchDatabase(String),
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
                                    let result =
                                        fetch_table_data_impl(client, &schema, &table, limit, offset)
                                            .await;
                                    let _ = res_tx.send(DbResponse::TableDataResult(result));
                                    let _ = res_tx.send(DbResponse::Busy(false));
                                }
                            }
                            DbRequest::FetchColumns { schema, table } => {
                                if let Some(client) = &current_client {
                                    let _ = res_tx.send(DbResponse::Busy(true));
                                    let result =
                                        fetch_columns_impl(client, &schema, &table).await;
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
                                                message: format!("Table {}.{} dropped successfully", schema, table),
                                                refresh_tables: true,
                                                refresh_columns: false,
                                            });
                                        }
                                        Err(e) => {
                                            let _ = res_tx.send(DbResponse::OperationFailed(format!("Failed to drop table: {}", e)));
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
                                                message: format!("Table {}.{} truncated successfully", schema, table),
                                                refresh_tables: false,
                                                refresh_columns: false,
                                            });
                                        }
                                        Err(e) => {
                                            let _ = res_tx.send(DbResponse::OperationFailed(format!("Failed to truncate table: {}", e)));
                                        }
                                    }
                                    let _ = res_tx.send(DbResponse::Busy(false));
                                }
                            }
                            DbRequest::DropColumn { schema, table, column } => {
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
                                                message: format!("Column {} dropped from {}.{}", column, schema, table),
                                                refresh_tables: false,
                                                refresh_columns: true,
                                            });
                                        }
                                        Err(e) => {
                                            let _ = res_tx.send(DbResponse::OperationFailed(format!("Failed to drop column: {}", e)));
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
                                                message: format!("Database {} dropped successfully", db_name),
                                                refresh_tables: true,
                                                refresh_columns: false,
                                            });
                                        }
                                        Err(e) => {
                                            let _ = res_tx.send(DbResponse::OperationFailed(format!("Failed to drop database: {}", e)));
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
            let is_nullable = row.get(2).map(|s| s.eq_ignore_ascii_case("YES")).unwrap_or(true);
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
