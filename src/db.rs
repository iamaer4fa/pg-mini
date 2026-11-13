use futures_util::TryStreamExt;
use std::collections::VecDeque;
use std::future::Future;
use std::io::{BufRead, BufReader, Write};
use std::pin::Pin;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::Instant;
use std::{
    fs::{self, File, OpenOptions},
    path::{Path, PathBuf},
    process::{Command, Stdio},
};
use tokio::sync::watch;
use tokio_postgres::{CancelToken, Client, NoTls, SimpleQueryMessage, SimpleQueryStream};

use crate::activity::{ActivityLock, ActivitySession, ActivitySnapshot, SizeEntry};
use crate::ha::provisioning::ProvisioningPreflight;
use crate::ha::{
    HaNodeStatus, NodeRole, PrimaryStandbyStatus, ReplicationStatus, WalReceiverStatus,
};
use crate::models::{
    AddColumnForm, BackupRestorePreflight, CellValue, ColumnItem, ConnectionConfig,
    CreateTableForm, DatabaseItem, DbObjectItem, DbObjectKind, DbObjectRef, EditableCell,
    ObjectTreeKey, QueryResult, RestoreArchiveEntry, RoleForm, RoleItem, TableItem, TlsMode,
};
use crate::sql_safety::{
    transaction_state_after, transaction_state_after_error, validate_read_only_sql,
};
use crate::system_stats::{SYSTEM_STATS_FUNCTIONS, SystemStatsSection, SystemStatsSnapshot};

const QUERY_RESULT_PAGE_SIZE: usize = 1_000;

#[derive(Debug)]
pub enum DbRequest {
    Connect(ConnectionConfig),
    Disconnect,
    FetchDatabases,
    FetchRoles,
    FetchActivity,
    FetchSystemStats,
    FetchProvisioningPreflight,
    FetchHaStatus,
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
    ExecuteSql {
        query_id: u64,
        sql: String,
        read_only: bool,
    },
    FetchMoreQueryRows,
    CancelQuery {
        query_id: u64,
    },
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
    PreflightBackup {
        db_name: String,
        file_path: std::path::PathBuf,
    },
    PreflightRestore {
        db_name: String,
        file_path: std::path::PathBuf,
    },
    RunBackup {
        db_name: String,
        file_path: PathBuf,
    },
    RunRestore {
        db_name: String,
        file_path: PathBuf,
        clean_before_restore: bool,
        selected_entries: Vec<String>,
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

fn parse_optional_activity_f64(
    row: &tokio_postgres::SimpleQueryRow,
    index: usize,
    label: &str,
) -> Result<Option<f64>, String> {
    row.get(index)
        .map(|value| {
            value
                .parse::<f64>()
                .map_err(|_| format!("Activity query returned an invalid {label}"))
        })
        .transpose()
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
    ActivityFetched(Result<ActivitySnapshot, String>),
    SystemStatsFetched(Result<SystemStatsSnapshot, String>),
    ProvisioningPreflightFetched {
        connection_id: String,
        result: Result<ProvisioningPreflight, String>,
    },
    BackupRestorePreflight(Result<BackupRestorePreflight, String>),
    BackupRestoreProgress(String),
    BackupRestoreFinished {
        database: String,
        is_restore: bool,
        result: Result<String, String>,
    },
    HaStatusFetched(Result<HaNodeStatus, String>),
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
    QueryCancelled {
        query_id: u64,
    },
    TransactionState(bool),
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
    cancel_tx: watch::Sender<u64>,
}

struct PendingQuery {
    query_id: u64,
    sql: String,
    stream: Pin<Box<SimpleQueryStream>>,
    result: QueryResult,
    buffered_message: Option<SimpleQueryMessage>,
    cancel_token: CancelToken,
    read_only: bool,
    started_at: Instant,
}

impl DbWorker {
    pub fn new() -> Self {
        let (req_tx, req_rx) = channel::<DbRequest>();
        let (res_tx, res_rx) = channel::<DbResponse>();
        let (cancel_tx, cancel_rx) = watch::channel(0_u64);

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
                    let mut pending_query: Option<PendingQuery> = None;
                    let mut cancel_rx = cancel_rx;
                    let mut transaction_open = false;

                    while let Ok(req) = req_rx.recv() {
                        match req {
                            DbRequest::Connect(cfg) => {
                                let _ = res_tx.send(DbResponse::Busy(true));
                                match connect_client(&cfg).await {
                                    Ok((client, version)) => {
                                        pending_query = None;
                                        transaction_open = false;
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
                                pending_query = None;
                                transaction_open = false;
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
                            DbRequest::FetchActivity => {
                                if let Some(client) = &current_client {
                                    let _ = res_tx.send(DbResponse::Busy(true));
                                    let result = fetch_activity_impl(client).await;
                                    let _ = res_tx.send(DbResponse::ActivityFetched(result));
                                    let _ = res_tx.send(DbResponse::Busy(false));
                                }
                            }
                            DbRequest::FetchSystemStats => {
                                if let Some(client) = &current_client {
                                    let _ = res_tx.send(DbResponse::Busy(true));
                                    let result = fetch_system_stats_impl(client).await;
                                    let _ = res_tx.send(DbResponse::SystemStatsFetched(result));
                                    let _ = res_tx.send(DbResponse::Busy(false));
                                }
                            }
                            DbRequest::FetchProvisioningPreflight => {
                                if let (Some(client), Some(config)) =
                                    (&current_client, &current_cfg)
                                {
                                    let _ = res_tx.send(DbResponse::Busy(true));
                                    let result = fetch_provisioning_preflight_impl(client).await;
                                    let _ = res_tx.send(
                                        DbResponse::ProvisioningPreflightFetched {
                                            connection_id: config.id.clone(),
                                            result,
                                        },
                                    );
                                    let _ = res_tx.send(DbResponse::Busy(false));
                                }
                            }
                            DbRequest::FetchHaStatus => {
                                if let Some(client) = &current_client {
                                    let _ = res_tx.send(DbResponse::Busy(true));
                                    let result = fetch_ha_status_impl(client).await;
                                    let _ = res_tx.send(DbResponse::HaStatusFetched(result));
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
                                            pending_query = None;
                                            transaction_open = false;
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
                            DbRequest::ExecuteSql {
                                query_id,
                                sql,
                                read_only,
                            } => {
                                if pending_query.is_some() {
                                    let _ = res_tx.send(DbResponse::QueryResult(Err(
                                        "Fetch the remaining query rows or cancel the query before running another statement."
                                            .to_string(),
                                    )));
                                    continue;
                                }
                                if read_only && transaction_open {
                                    let _ = res_tx.send(DbResponse::QueryResult(Err(
                                        "Read-only mode cannot start while an explicit transaction is open."
                                            .to_string(),
                                    )));
                                    continue;
                                }
                                if let (Some(client), Some(cfg)) =
                                    (&current_client, &current_cfg)
                                {
                                    let _ = res_tx.send(DbResponse::Busy(true));
                                    if *cancel_rx.borrow() == query_id {
                                        let _ = res_tx.send(DbResponse::QueryCancelled { query_id });
                                        let _ = res_tx.send(DbResponse::Busy(false));
                                        continue;
                                    }
                                    match start_query(client, query_id, &sql, read_only).await {
                                        Ok(mut pending) => {
                                            if pending.read_only {
                                                transaction_open = true;
                                                let _ = res_tx.send(
                                                    DbResponse::TransactionState(true),
                                                );
                                            }
                                            match fetch_query_page(
                                                cfg,
                                                &mut pending,
                                                &mut cancel_rx,
                                            )
                                            .await
                                            {
                                                Ok(has_more) => {
                                                    pending.result.has_more_rows = has_more;
                                                    pending.result.execution_time_ms = pending
                                                        .started_at
                                                        .elapsed()
                                                        .as_secs_f64()
                                                        * 1000.0;
                                                    transaction_open = if pending.read_only {
                                                        has_more
                                                    } else {
                                                        transaction_state_after(
                                                            &pending.sql,
                                                            transaction_open,
                                                        )
                                                    };
                                                    let _ = res_tx.send(
                                                        DbResponse::TransactionState(
                                                            transaction_open,
                                                        ),
                                                    );
                                                    if has_more {
                                                        let _ = res_tx.send(
                                                            DbResponse::QueryResult(Ok(
                                                                pending.result.clone(),
                                                            )),
                                                        );
                                                        pending_query = Some(pending);
                                                        let _ = res_tx.send(DbResponse::Busy(true));
                                                    } else if pending.read_only {
                                                        match finish_read_only_query(client).await {
                                                            Ok(()) => {
                                                                transaction_open = false;
                                                                let _ = res_tx.send(
                                                                    DbResponse::TransactionState(
                                                                        false,
                                                                    ),
                                                                );
                                                                let _ = res_tx.send(
                                                                    DbResponse::QueryResult(Ok(
                                                                        pending.result,
                                                                    )),
                                                                );
                                                                let _ = res_tx.send(
                                                                    DbResponse::Busy(false),
                                                                );
                                                            }
                                                            Err(error) => {
                                                                let _ = res_tx.send(
                                                                    DbResponse::QueryResult(Err(
                                                                        error,
                                                                    )),
                                                                );
                                                                let _ = res_tx.send(
                                                                    DbResponse::Busy(false),
                                                                );
                                                            }
                                                        }
                                                    } else {
                                                        let _ = res_tx.send(
                                                            DbResponse::QueryResult(Ok(
                                                                pending.result,
                                                            )),
                                                        );
                                                        let _ = res_tx.send(DbResponse::Busy(false));
                                                    }
                                                }
                                                Err(error) => {
                                                    if error == "Query cancelled by user." {
                                                        if pending.read_only {
                                                            match finish_read_only_query(client).await {
                                                                Ok(()) => {
                                                                    transaction_open = false;
                                                                    let _ = res_tx.send(
                                                                        DbResponse::TransactionState(
                                                                            false,
                                                                        ),
                                                                    );
                                                                    let _ = res_tx.send(
                                                                        DbResponse::QueryCancelled {
                                                                            query_id,
                                                                        },
                                                                    );
                                                                }
                                                                Err(error) => {
                                                                    let _ = res_tx.send(
                                                                        DbResponse::OperationFailed(
                                                                            format!(
                                                                                "Query was cancelled, but the read-only transaction could not be rolled back: {error}"
                                                                            ),
                                                                        ),
                                                                    );
                                                                }
                                                            }
                                                        } else {
                                                            let was_open = transaction_open;
                                                            transaction_open = transaction_state_after_error(
                                                                &pending.sql,
                                                                transaction_open,
                                                            );
                                                            if transaction_open != was_open {
                                                                let _ = res_tx.send(
                                                                    DbResponse::TransactionState(
                                                                        transaction_open,
                                                                    ),
                                                                );
                                                            }
                                                            let _ = res_tx.send(
                                                                DbResponse::QueryCancelled {
                                                                    query_id,
                                                                },
                                                            );
                                                        }
                                                        let _ =
                                                            res_tx.send(DbResponse::Busy(false));
                                                        continue;
                                                    }
                                                    let error = if pending.read_only {
                                                        match finish_read_only_query(client).await {
                                                            Ok(()) => {
                                                                transaction_open = false;
                                                                let _ = res_tx.send(
                                                                    DbResponse::TransactionState(
                                                                        false,
                                                                    ),
                                                                );
                                                                error
                                                            }
                                                            Err(rollback_error) => format!(
                                                                "{error}; additionally failed to roll back the read-only transaction: {rollback_error}"
                                                            ),
                                                        }
                                                    } else {
                                                        error
                                                    };
                                                    if !pending.read_only {
                                                        transaction_open = transaction_state_after_error(
                                                            &pending.sql,
                                                            transaction_open,
                                                        );
                                                        let _ = res_tx.send(
                                                            DbResponse::TransactionState(
                                                                transaction_open,
                                                            ),
                                                        );
                                                    }
                                                    let _ = res_tx.send(DbResponse::QueryResult(
                                                        Err(error),
                                                    ));
                                                    let _ = res_tx.send(DbResponse::Busy(false));
                                                }
                                            }
                                        }
                                        Err(error) => {
                                            let _ = res_tx.send(DbResponse::QueryResult(Err(
                                                error,
                                            )));
                                            let _ = res_tx.send(DbResponse::Busy(false));
                                        }
                                    }
                                } else {
                                    let _ = res_tx.send(DbResponse::QueryResult(Err(
                                        "Not connected to any PostgreSQL database".to_string(),
                                    )));
                                }
                            }
                            DbRequest::FetchMoreQueryRows => {
                                if let (Some(client), Some(cfg), Some(mut pending)) =
                                    (&current_client, &current_cfg, pending_query.take())
                                {
                                    let _ = res_tx.send(DbResponse::Busy(true));
                                    match fetch_query_page(cfg, &mut pending, &mut cancel_rx).await {
                                        Ok(has_more) => {
                                            pending.result.has_more_rows = has_more;
                                            pending.result.execution_time_ms = pending
                                                .started_at
                                                .elapsed()
                                                .as_secs_f64()
                                                * 1000.0;
                                            if has_more {
                                                let _ = res_tx.send(DbResponse::QueryResult(Ok(
                                                    pending.result.clone(),
                                                )));
                                                pending_query = Some(pending);
                                                let _ = res_tx.send(DbResponse::Busy(true));
                                            } else if pending.read_only {
                                                match finish_read_only_query(client).await {
                                                    Ok(()) => {
                                                        transaction_open = false;
                                                        let _ = res_tx.send(
                                                            DbResponse::TransactionState(false),
                                                        );
                                                        let _ = res_tx.send(
                                                            DbResponse::QueryResult(Ok(
                                                                pending.result,
                                                            )),
                                                        );
                                                        let _ = res_tx.send(
                                                            DbResponse::Busy(false),
                                                        );
                                                    }
                                                    Err(error) => {
                                                        let _ = res_tx.send(
                                                            DbResponse::QueryResult(Err(error)),
                                                        );
                                                        let _ = res_tx.send(
                                                            DbResponse::Busy(false),
                                                        );
                                                    }
                                                }
                                            } else {
                                                let _ = res_tx.send(DbResponse::QueryResult(Ok(
                                                    pending.result,
                                                )));
                                                let _ = res_tx.send(DbResponse::Busy(false));
                                            }
                                        }
                                        Err(error) => {
                                            if error == "Query cancelled by user." {
                                                if pending.read_only {
                                                    match finish_read_only_query(client).await {
                                                        Ok(()) => {
                                                            transaction_open = false;
                                                            let _ = res_tx.send(
                                                                DbResponse::TransactionState(false),
                                                            );
                                                            let _ = res_tx.send(
                                                                DbResponse::QueryCancelled {
                                                                    query_id: pending.query_id,
                                                                },
                                                            );
                                                        }
                                                        Err(error) => {
                                                            let _ = res_tx.send(
                                                                DbResponse::OperationFailed(
                                                                    format!(
                                                                        "Query was cancelled, but the read-only transaction could not be rolled back: {error}"
                                                                    ),
                                                                ),
                                                            );
                                                        }
                                                    }
                                                } else {
                                                    let was_open = transaction_open;
                                                    transaction_open = transaction_state_after_error(
                                                        &pending.sql,
                                                        transaction_open,
                                                    );
                                                    if transaction_open != was_open {
                                                        let _ = res_tx.send(
                                                            DbResponse::TransactionState(
                                                                transaction_open,
                                                            ),
                                                        );
                                                    }
                                                    let _ = res_tx.send(
                                                        DbResponse::QueryCancelled {
                                                            query_id: pending.query_id,
                                                        },
                                                    );
                                                }
                                                let _ = res_tx.send(DbResponse::Busy(false));
                                                continue;
                                            }
                                            let error = if pending.read_only {
                                                match finish_read_only_query(client).await {
                                                    Ok(()) => {
                                                        transaction_open = false;
                                                        let _ = res_tx.send(
                                                            DbResponse::TransactionState(false),
                                                        );
                                                        error
                                                    }
                                                    Err(rollback_error) => format!(
                                                        "{error}; additionally failed to roll back the read-only transaction: {rollback_error}"
                                                    ),
                                                }
                                            } else {
                                                transaction_open = transaction_state_after_error(
                                                    &pending.sql,
                                                    transaction_open,
                                                );
                                                let _ = res_tx.send(
                                                    DbResponse::TransactionState(transaction_open),
                                                );
                                                error
                                            };
                                            let _ = res_tx.send(DbResponse::QueryResult(Err(
                                                error,
                                            )));
                                            let _ = res_tx.send(DbResponse::Busy(false));
                                        }
                                    }
                                }
                            }
                            DbRequest::CancelQuery { query_id } => {
                                if let (Some(client), Some(cfg), Some(mut pending)) =
                                    (&current_client, &current_cfg, pending_query.take())
                                {
                                    if pending.query_id != query_id {
                                        pending_query = Some(pending);
                                        continue;
                                    }
                                    let _ = res_tx.send(DbResponse::Busy(true));
                                    match cancel_pending_query(cfg, &mut pending).await {
                                        Ok(()) => {
                                            if pending.read_only {
                                                match finish_read_only_query(client).await {
                                                    Ok(()) => {
                                                        transaction_open = false;
                                                        let _ = res_tx.send(
                                                            DbResponse::TransactionState(false),
                                                        );
                                                        let _ = res_tx.send(
                                                            DbResponse::QueryCancelled { query_id },
                                                        );
                                                    }
                                                    Err(error) => {
                                                        let _ = res_tx.send(
                                                            DbResponse::OperationFailed(format!(
                                                                "Query was cancelled, but the read-only transaction could not be rolled back: {error}"
                                                            )),
                                                        );
                                                    }
                                                }
                                            } else {
                                                let was_open = transaction_open;
                                                transaction_open = transaction_state_after_error(
                                                    &pending.sql,
                                                    transaction_open,
                                                );
                                                if transaction_open != was_open {
                                                    let _ = res_tx.send(
                                                        DbResponse::TransactionState(
                                                            transaction_open,
                                                        ),
                                                    );
                                                }
                                                let _ = res_tx
                                                    .send(DbResponse::QueryCancelled { query_id });
                                            }
                                            let _ = res_tx.send(DbResponse::Busy(false));
                                        }
                                        Err(error) => {
                                            pending_query = Some(pending);
                                            let _ = res_tx.send(DbResponse::OperationFailed(
                                                format!("Could not cancel query: {error}"),
                                            ));
                                            let _ = res_tx.send(DbResponse::Busy(true));
                                        }
                                    }
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
                            DbRequest::PreflightBackup { db_name, file_path } => {
                                let _ = res_tx.send(DbResponse::Busy(true));
                                let result = if let (Some(client), Some(cfg)) =
                                    (&current_client, &current_cfg)
                                {
                                    preflight_backup_database(client, cfg, &db_name, &file_path)
                                        .await
                                } else {
                                    Err("Connect to a PostgreSQL server before backing up a database".to_string())
                                };
                                let _ = res_tx.send(DbResponse::BackupRestorePreflight(result));
                                let _ = res_tx.send(DbResponse::Busy(false));
                            }
                            DbRequest::PreflightRestore { db_name, file_path } => {
                                let _ = res_tx.send(DbResponse::Busy(true));
                                let result = if let (Some(client), Some(cfg)) =
                                    (&current_client, &current_cfg)
                                {
                                    preflight_restore_database(client, cfg, &db_name, &file_path)
                                        .await
                                } else {
                                    Err("Connect to a PostgreSQL server before restoring a database".to_string())
                                };
                                let _ = res_tx.send(DbResponse::BackupRestorePreflight(result));
                                let _ = res_tx.send(DbResponse::Busy(false));
                            }
                            DbRequest::RunBackup { db_name, file_path } => {
                                let _ = res_tx.send(DbResponse::Busy(true));
                                let result = if let Some(cfg) = &current_cfg {
                                    let progress_tx = res_tx.clone();
                                    backup_database(cfg, &db_name, &file_path, |line| {
                                        let _ = progress_tx.send(
                                            DbResponse::BackupRestoreProgress(line.to_string()),
                                        );
                                    })
                                } else {
                                    Err("Connection was closed before the backup started".to_string())
                                };
                                let _ = res_tx.send(DbResponse::BackupRestoreFinished {
                                    database: db_name,
                                    is_restore: false,
                                    result,
                                });
                                let _ = res_tx.send(DbResponse::Busy(false));
                            }
                            DbRequest::RunRestore {
                                db_name,
                                file_path,
                                clean_before_restore,
                                selected_entries,
                            } => {
                                let _ = res_tx.send(DbResponse::Busy(true));
                                let result = if let Some(cfg) = &current_cfg {
                                    let progress_tx = res_tx.clone();
                                    restore_database(
                                        cfg,
                                        &db_name,
                                        &file_path,
                                        clean_before_restore,
                                        &selected_entries,
                                        |line| {
                                            let _ = progress_tx.send(
                                                DbResponse::BackupRestoreProgress(line.to_string()),
                                            );
                                        },
                                    )
                                } else {
                                    Err("Connection was closed before the restore started".to_string())
                                };
                                let _ = res_tx.send(DbResponse::BackupRestoreFinished {
                                    database: db_name,
                                    is_restore: true,
                                    result,
                                });
                                let _ = res_tx.send(DbResponse::Busy(false));
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

        Self {
            req_tx,
            res_rx,
            cancel_tx,
        }
    }

    pub fn send(&self, req: DbRequest) {
        let _ = self.req_tx.send(req);
    }

    pub fn cancel_query(&self, query_id: u64) {
        self.cancel_tx.send_replace(query_id);
        let _ = self.req_tx.send(DbRequest::CancelQuery { query_id });
    }

    pub fn try_recv(&self) -> Option<DbResponse> {
        self.res_rx.try_recv().ok()
    }
}

async fn fetch_provisioning_preflight_impl(
    client: &Client,
) -> Result<ProvisioningPreflight, String> {
    let rows = client
        .simple_query(
            "SELECT current_setting('server_version_num'), \
             current_setting('server_version'), \
             pg_catalog.pg_is_in_recovery()::text, \
             current_setting('wal_level'), \
             current_setting('max_wal_senders'), \
             current_setting('max_replication_slots'), \
             current_setting('listen_addresses'), \
             current_setting('ssl'), \
             role.rolsuper::text, role.rolcreaterole::text, role.rolreplication::text \
             FROM pg_catalog.pg_roles AS role \
             WHERE role.rolname = current_user;",
        )
        .await
        .map_err(|error| {
            format!("Failed to inspect PostgreSQL provisioning prerequisites: {error}")
        })?;
    let row = rows
        .into_iter()
        .find_map(|message| match message {
            SimpleQueryMessage::Row(row) => Some(row),
            _ => None,
        })
        .ok_or_else(|| {
            "Could not read the connected PostgreSQL role and server settings.".to_string()
        })?;

    let server_version_num = parse_preflight_i32(&row, 0, "server_version_num")?;
    let (system_identifier, system_identifier_error) = if server_version_num >= 100000 {
        client
            .query_one(
                "SELECT system_identifier::text FROM pg_catalog.pg_control_system()",
                &[],
            )
            .await
            .and_then(|row| row.try_get::<_, String>(0).map_err(Into::into))
            .map(|identifier| (Some(identifier), None))
            .unwrap_or_else(|error| {
                (
                    None,
                    Some(format!(
                        "System identifier is unavailable to this account: {error}"
                    )),
                )
            })
    } else {
        (
            None,
            Some("System identifier requires PostgreSQL 10 or later.".to_string()),
        )
    };
    Ok(ProvisioningPreflight {
        server_version_num,
        server_version: preflight_value(&row, 1, "server_version")?,
        is_in_recovery: parse_preflight_bool(&row, 2, "recovery state")?,
        system_identifier,
        system_identifier_error,
        wal_level: preflight_value(&row, 3, "wal_level")?,
        max_wal_senders: parse_preflight_i32(&row, 4, "max_wal_senders")?,
        max_replication_slots: parse_preflight_i32(&row, 5, "max_replication_slots")?,
        listen_addresses: preflight_value(&row, 6, "listen_addresses")?,
        ssl_enabled: parse_preflight_bool(&row, 7, "SSL setting")?,
        is_superuser: parse_preflight_bool(&row, 8, "superuser privilege")?,
        can_create_role: parse_preflight_bool(&row, 9, "CREATEROLE privilege")?,
        can_replication: parse_preflight_bool(&row, 10, "REPLICATION privilege")?,
    })
}

fn preflight_value(
    row: &tokio_postgres::SimpleQueryRow,
    index: usize,
    label: &str,
) -> Result<String, String> {
    row.get(index)
        .map(str::to_string)
        .ok_or_else(|| format!("PostgreSQL preflight returned no {label} value."))
}

fn parse_preflight_i32(
    row: &tokio_postgres::SimpleQueryRow,
    index: usize,
    label: &str,
) -> Result<i32, String> {
    preflight_value(row, index, label)?
        .parse::<i32>()
        .map_err(|_| format!("PostgreSQL preflight returned an invalid {label} value."))
}

fn parse_preflight_bool(
    row: &tokio_postgres::SimpleQueryRow,
    index: usize,
    label: &str,
) -> Result<bool, String> {
    match preflight_value(row, index, label)?.as_str() {
        "true" | "t" | "on" | "1" => Ok(true),
        "false" | "f" | "off" | "0" => Ok(false),
        _ => Err(format!(
            "PostgreSQL preflight returned an invalid {label} value."
        )),
    }
}

async fn fetch_ha_status_impl(client: &Client) -> Result<HaNodeStatus, String> {
    let server_version_num: i32 = client
        .query_one("SELECT current_setting('server_version_num')::integer", &[])
        .await
        .map_err(|error| format!("Could not read PostgreSQL server version: {error}"))?
        .try_get(0)
        .map_err(|error| format!("Could not parse PostgreSQL server version: {error}"))?;
    let row = client
        .query_one("SELECT pg_catalog.pg_is_in_recovery()", &[])
        .await
        .map_err(|error| format!("Failed to detect PostgreSQL node role: {error}"))?;
    let is_in_recovery: bool = row
        .try_get(0)
        .map_err(|error| format!("Could not read PostgreSQL node role: {error}"))?;
    let role = NodeRole::from_recovery_state(is_in_recovery);
    let system_identifier = if server_version_num >= 100000 {
        client
            .query_one(
                "SELECT system_identifier::text FROM pg_catalog.pg_control_system()",
                &[],
            )
            .await
            .and_then(|row| row.try_get::<_, String>(0))
            .map_err(|error| error.to_string())
    } else {
        Err("System identifier query requires PostgreSQL 10 or later".to_string())
    };
    let (system_identifier, system_identifier_error) = match system_identifier {
        Ok(identifier) => (Some(identifier), None),
        Err(error) => (
            None,
            Some(format!("System identifier unavailable: {error}")),
        ),
    };
    let replication = match role {
        NodeRole::Primary => fetch_primary_replication_status(client, server_version_num).await,
        NodeRole::Standby => fetch_standby_replication_status(client, server_version_num).await,
    }
    .unwrap_or_else(|reason| ReplicationStatus::Unknown { reason });
    Ok(HaNodeStatus {
        role,
        replication,
        system_identifier,
        system_identifier_error,
    })
}

async fn fetch_primary_replication_status(
    client: &Client,
    server_version_num: i32,
) -> Result<ReplicationStatus, String> {
    let fields = crate::ha::primary_lsn_fields(server_version_num);
    let rows = client
        .simple_query(&format!(
            "SELECT to_jsonb(replication)->>'application_name', \
             to_jsonb(replication)->>'client_addr', \
             to_jsonb(replication)->>'state', \
             to_jsonb(replication)->>'sync_state', \
             to_jsonb(replication)->>'{sent}', \
             to_jsonb(replication)->>'{written}', \
             to_jsonb(replication)->>'{flushed}', \
             to_jsonb(replication)->>'{replayed}', \
             extract(epoch FROM NULLIF(to_jsonb(replication)->>'write_lag', '')::interval)::text, \
             extract(epoch FROM NULLIF(to_jsonb(replication)->>'flush_lag', '')::interval)::text, \
             extract(epoch FROM NULLIF(to_jsonb(replication)->>'replay_lag', '')::interval)::text \
             FROM pg_catalog.pg_stat_replication AS replication \
             ORDER BY to_jsonb(replication)->>'application_name';",
            sent = fields.sent,
            written = fields.written,
            flushed = fields.flushed,
            replayed = fields.replayed,
        ))
        .await
        .map_err(|error| format!("Failed to fetch primary replication status: {error}"))?;

    let mut standbys = Vec::new();
    for row in rows {
        if let SimpleQueryMessage::Row(row) = row {
            standbys.push(PrimaryStandbyStatus {
                application_name: ha_optional_value(&row, 0),
                client_addr: ha_optional_value(&row, 1),
                state: ha_optional_value(&row, 2),
                sync_state: ha_optional_value(&row, 3),
                sent_lsn: ha_optional_value(&row, 4),
                write_lsn: ha_optional_value(&row, 5),
                flush_lsn: ha_optional_value(&row, 6),
                replay_lsn: ha_optional_value(&row, 7),
                write_lag_seconds: parse_optional_ha_f64(&row, 8, "write lag")?,
                flush_lag_seconds: parse_optional_ha_f64(&row, 9, "flush lag")?,
                replay_lag_seconds: parse_optional_ha_f64(&row, 10, "replay lag")?,
            });
        }
    }
    Ok(ReplicationStatus::Primary { standbys })
}

async fn fetch_standby_replication_status(
    client: &Client,
    server_version_num: i32,
) -> Result<ReplicationStatus, String> {
    if server_version_num < 90600 {
        return Err("WAL receiver statistics require PostgreSQL 9.6 or later".to_string());
    }
    let replay_lsn_function = crate::ha::standby_replay_lsn_function(server_version_num)
        .ok_or_else(|| "WAL receiver statistics require PostgreSQL 9.6 or later".to_string())?;
    let rows = client
        .simple_query(&format!(
            "SELECT to_jsonb(receiver)->>'status', \
             to_jsonb(receiver)->>'sender_host', \
             to_jsonb(receiver)->>'sender_port', \
             to_jsonb(receiver)->>'slot_name', \
             to_jsonb(receiver)->>'receive_start_lsn', \
             to_jsonb(receiver)->>'written_lsn', \
             to_jsonb(receiver)->>'flushed_lsn', \
             to_jsonb(receiver)->>'latest_end_lsn', \
             to_jsonb(receiver)->>'latest_end_time', \
             {replay_lsn_function}::text, \
             pg_catalog.pg_last_xact_replay_timestamp()::text \
             FROM pg_catalog.pg_stat_wal_receiver AS receiver LIMIT 1;",
        ))
        .await
        .map_err(|error| format!("Failed to fetch standby WAL receiver status: {error}"))?;

    let receiver = rows.into_iter().find_map(|row| match row {
        SimpleQueryMessage::Row(row) => Some(WalReceiverStatus {
            status: ha_optional_value(&row, 0),
            sender_host: ha_optional_value(&row, 1),
            sender_port: ha_optional_value(&row, 2),
            slot_name: ha_optional_value(&row, 3),
            receive_start_lsn: ha_optional_value(&row, 4),
            written_lsn: ha_optional_value(&row, 5),
            flushed_lsn: ha_optional_value(&row, 6),
            latest_end_lsn: ha_optional_value(&row, 7),
            latest_end_time: ha_optional_value(&row, 8),
            replay_lsn: ha_optional_value(&row, 9),
            last_replay_at: ha_optional_value(&row, 10),
        }),
        _ => None,
    });
    Ok(ReplicationStatus::Standby { receiver })
}

fn ha_optional_value(row: &tokio_postgres::SimpleQueryRow, index: usize) -> Option<String> {
    row.get(index).map(str::to_string)
}

fn parse_optional_ha_f64(
    row: &tokio_postgres::SimpleQueryRow,
    index: usize,
    label: &str,
) -> Result<Option<f64>, String> {
    parse_ha_f64(row.get(index), label)
}

fn parse_ha_f64(value: Option<&str>, label: &str) -> Result<Option<f64>, String> {
    value
        .map(|value| {
            value
                .parse::<f64>()
                .map_err(|_| format!("Primary replication query returned an invalid {label}"))
        })
        .transpose()
}

async fn connect_client(cfg: &ConnectionConfig) -> Result<(Client, String), String> {
    let pg_config = postgres_config(cfg);
    match cfg.tls_mode {
        TlsMode::Disable => {
            let (client, connection) = pg_config
                .connect(NoTls)
                .await
                .map_err(|error| format!("Failed to connect: {error}"))?;
            finish_connection(client, connection).await
        }
        TlsMode::Prefer => {
            let tls = tls_connector(cfg)?;
            match pg_config.connect(tls).await {
                Ok((client, connection)) => finish_connection(client, connection).await,
                Err(tls_error) if tls_error.as_db_error().is_some() => Err(format!(
                    "PostgreSQL rejected the TLS connection: {tls_error}"
                )),
                Err(tls_error) => {
                    let (client, connection) = pg_config.connect(NoTls).await.map_err(|plain_error| {
                        format!(
                            "TLS connection failed ({tls_error}); plaintext fallback also failed ({plain_error})"
                        )
                    })?;
                    finish_connection(client, connection).await
                }
            }
        }
        TlsMode::Require => {
            let tls = tls_connector(cfg)?;
            let (client, connection) = pg_config
                .connect(tls)
                .await
                .map_err(|error| format!("TLS connection failed: {error}"))?;
            finish_connection(client, connection).await
        }
    }
}

fn postgres_config(cfg: &ConnectionConfig) -> tokio_postgres::Config {
    let mut pg_config = tokio_postgres::Config::new();
    pg_config.host(&cfg.host);
    pg_config.port(cfg.port);
    pg_config.user(&cfg.username);
    if !cfg.password.is_empty() {
        pg_config.password(&cfg.password);
    }
    pg_config.dbname(&cfg.database);
    pg_config.connect_timeout(std::time::Duration::from_secs(8));
    pg_config.options(&format!(
        "-c statement_timeout={}ms -c lock_timeout={}ms",
        cfg.statement_timeout_seconds.saturating_mul(1_000),
        cfg.lock_timeout_seconds.saturating_mul(1_000)
    ));
    pg_config
}

fn tls_connector(
    cfg: &ConnectionConfig,
) -> Result<tokio_postgres_rustls::MakeRustlsConnect, String> {
    let mut roots = rustls::RootCertStore::empty();
    let native_certs = rustls_native_certs::load_native_certs();
    let (added, ignored) = roots.add_parsable_certificates(native_certs.certs);
    for error in &native_certs.errors {
        eprintln!("TLS trust store warning: {error}");
    }
    if ignored > 0 {
        eprintln!("TLS trust store warning: ignored {ignored} invalid native certificate(s)");
    }

    if !cfg.tls_root_cert.trim().is_empty() {
        let file = std::fs::File::open(&cfg.tls_root_cert)
            .map_err(|error| format!("Could not open TLS root certificate: {error}"))?;
        let mut reader = BufReader::new(file);
        let certificates = rustls_pemfile::certs(&mut reader)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("Could not parse TLS root certificate: {error}"))?;
        if certificates.is_empty() {
            return Err("The TLS root certificate file contains no PEM certificates".to_string());
        }
        for certificate in certificates {
            roots
                .add(certificate)
                .map_err(|error| format!("Invalid TLS root certificate: {error}"))?;
        }
    } else if added == 0 {
        let details = native_certs
            .errors
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("; ");
        return Err(format!(
            "No trusted root certificates are available{}",
            if details.is_empty() {
                String::new()
            } else {
                format!(": {details}")
            }
        ));
    }

    let client_config = rustls::ClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth();
    Ok(tokio_postgres_rustls::MakeRustlsConnect::new(client_config))
}

async fn finish_connection<C>(client: Client, connection: C) -> Result<(Client, String), String>
where
    C: Future<Output = Result<(), tokio_postgres::Error>> + Send + 'static,
{
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

async fn preflight_backup_database(
    client: &Client,
    cfg: &ConnectionConfig,
    database: &str,
    file_path: &Path,
) -> Result<BackupRestorePreflight, String> {
    check_backup_destination(file_path)?;
    let (server_version, server_major, database_size_bytes) =
        backup_server_info(client, database).await?;
    let tool_version = postgres_tool_version("pg_dump")?;
    let tool_major = parse_version_major(&tool_version)
        .ok_or_else(|| format!("Could not parse pg_dump version: {tool_version}"))?;
    if tool_major < server_major {
        return Err(format!(
            "pg_dump {tool_version} is older than PostgreSQL server {server_version}. Install a pg_dump client of major version {server_major} or newer."
        ));
    }

    let available_space_bytes = available_disk_space(
        file_path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new(".")),
    );
    let mut warnings = Vec::new();
    if let (Some(database_size), Some(free_space)) = (database_size_bytes, available_space_bytes)
        && database_size > free_space
    {
        warnings.push(
            "The database is larger than the available destination space. Custom-format compression may reduce the backup size, but this estimate is not a guarantee."
                .to_string(),
        );
    }
    if cfg.tls_mode == TlsMode::Prefer {
        warnings.push(
            "PostgreSQL client tools use verified TLS for Prefer profiles; they will not fall back to plaintext. Use Disable explicitly for a plaintext-only server."
                .to_string(),
        );
    }

    Ok(BackupRestorePreflight {
        tool: "pg_dump".to_string(),
        tool_version,
        server_version,
        target_database: database.to_string(),
        format: "Custom archive".to_string(),
        command_preview: format!(
            "{}pg_dump --format=custom --verbose --no-password --host {} --port {} --username {} --dbname {} > {}",
            postgres_tool_environment_preview(cfg),
            shell_quote(&cfg.host),
            cfg.port,
            shell_quote(&cfg.username),
            shell_quote(database),
            shell_quote(&file_path.display().to_string())
        ),
        database_size_bytes,
        available_space_bytes,
        restore_entries: std::sync::Arc::new(Vec::new()),
        warnings,
    })
}

async fn preflight_restore_database(
    client: &Client,
    cfg: &ConnectionConfig,
    database: &str,
    file_path: &Path,
) -> Result<BackupRestorePreflight, String> {
    if !file_path.is_file() {
        return Err(format!(
            "Backup file does not exist or is not a regular file: {}",
            file_path.display()
        ));
    }
    let (server_version, server_major, database_size_bytes) =
        backup_server_info(client, database).await?;
    let is_sql_file = is_plain_sql_backup(file_path);
    let tool = if is_sql_file { "psql" } else { "pg_restore" };
    let tool_version = postgres_tool_version(tool)?;
    let mut warnings = Vec::new();
    let mut restore_entries = Vec::new();

    if is_sql_file {
        if let Some(dump_major) = plain_sql_dump_major(file_path) {
            if dump_major > server_major {
                warnings.push(format!(
                    "This script was produced by PostgreSQL major version {dump_major}, newer than the target server ({server_version}); restore may fail on incompatible SQL."
                ));
            }
        } else {
            warnings.push(
                "The script's source PostgreSQL version could not be determined.".to_string(),
            );
        }
    } else {
        let listing = list_restore_archive(file_path)?;
        if let Some(dump_major) = parse_dumped_from_major(&listing) {
            if dump_major > server_major {
                warnings.push(format!(
                    "This archive was created from PostgreSQL major version {dump_major}, newer than the target server ({server_version}); restore may fail on incompatible objects."
                ));
            }
        } else {
            warnings.push(
                "The archive's source PostgreSQL version could not be determined.".to_string(),
            );
        }
        if let Some(dump_tool_major) = parse_dumped_by_major(&listing)
            && let Some(restore_tool_major) = parse_version_major(&tool_version)
            && restore_tool_major < dump_tool_major
        {
            warnings.push(format!(
                "This archive was created by pg_dump major version {dump_tool_major}, newer than the installed pg_restore ({tool_version}); install a matching or newer pg_restore if restore fails."
            ));
        }
        restore_entries = parse_restore_entries(&listing);
        if restore_entries.is_empty() {
            return Err(
                "The archive contains no selectable restore entries. Verify that it is a valid pg_dump archive."
                    .to_string(),
            );
        }
    }
    if cfg.tls_mode == TlsMode::Prefer {
        warnings.push(
            "PostgreSQL client tools use verified TLS for Prefer profiles; they will not fall back to plaintext. Use Disable explicitly for a plaintext-only server."
                .to_string(),
        );
    }

    Ok(BackupRestorePreflight {
        tool: tool.to_string(),
        tool_version,
        server_version,
        target_database: database.to_string(),
        format: if is_sql_file {
            "Plain SQL script".to_string()
        } else {
            "PostgreSQL archive".to_string()
        },
        command_preview: if is_sql_file {
            format!(
                "{}psql --no-password --set=ON_ERROR_STOP=on --echo-errors --host {} --port {} --username {} --dbname {} --file {}",
                postgres_tool_environment_preview(cfg),
                shell_quote(&cfg.host),
                cfg.port,
                shell_quote(&cfg.username),
                shell_quote(database),
                shell_quote(&file_path.display().to_string())
            )
        } else {
            format!(
                "{}pg_restore --verbose --exit-on-error --no-password --host {} --port {} --username {} --dbname {} {}",
                postgres_tool_environment_preview(cfg),
                shell_quote(&cfg.host),
                cfg.port,
                shell_quote(&cfg.username),
                shell_quote(database),
                shell_quote(&file_path.display().to_string())
            )
        },
        database_size_bytes,
        available_space_bytes: None,
        restore_entries: std::sync::Arc::new(restore_entries),
        warnings,
    })
}

async fn backup_server_info(
    client: &Client,
    database: &str,
) -> Result<(String, u32, Option<u64>), String> {
    let (server_version, server_version_num): (String, i32) = client
        .query_one(
            "SELECT current_setting('server_version'), current_setting('server_version_num')::int",
            &[],
        )
        .await
        .map(|row| (row.get(0), row.get(1)))
        .map_err(|error| format!("Could not determine PostgreSQL server version: {error}"))?;

    let database_exists: bool = client
        .query_one(
            "SELECT EXISTS(SELECT 1 FROM pg_catalog.pg_database WHERE datname = $1)",
            &[&database],
        )
        .await
        .map(|row| row.get(0))
        .map_err(|error| format!("Could not check target database: {error}"))?;
    if !database_exists {
        return Err(format!(
            "Target database '{database}' does not exist on this server."
        ));
    }

    let can_connect: bool = client
        .query_one(
            "SELECT has_database_privilege(current_user, $1, 'CONNECT')",
            &[&database],
        )
        .await
        .map(|row| row.get(0))
        .map_err(|error| format!("Could not check target database access: {error}"))?;
    if !can_connect {
        return Err(format!(
            "The current PostgreSQL role does not have CONNECT permission on '{database}'."
        ));
    }

    let database_size_bytes = client
        .query_one("SELECT pg_database_size($1)::bigint", &[&database])
        .await
        .ok()
        .map(|row| row.get::<_, i64>(0).max(0) as u64);

    let server_major = u32::try_from(server_version_num / 10_000)
        .map_err(|_| "PostgreSQL returned an invalid server_version_num".to_string())?;
    Ok((server_version, server_major, database_size_bytes))
}

fn backup_database(
    cfg: &ConnectionConfig,
    database: &str,
    file_path: &Path,
    mut on_progress: impl FnMut(&str),
) -> Result<String, String> {
    let mut output_options = OpenOptions::new();
    output_options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        output_options.mode(0o600);
    }
    let output_file = output_options.open(file_path).map_err(|error| {
        if error.kind() == std::io::ErrorKind::AlreadyExists {
            format!(
                "Backup destination already exists; pg-mini will not overwrite it: {}",
                file_path.display()
            )
        } else {
            format!(
                "Cannot create backup destination {}: {error}",
                file_path.display()
            )
        }
    })?;
    let mut command = postgres_tool("pg_dump");
    command
        .arg("--format=custom")
        .arg("--verbose")
        .arg("--no-password")
        .arg("--host")
        .arg(&cfg.host)
        .arg("--port")
        .arg(cfg.port.to_string())
        .arg("--username")
        .arg(&cfg.username)
        .arg("--dbname")
        .arg(database)
        .stdout(Stdio::from(output_file));
    apply_postgres_tool_environment(&mut command, cfg);
    match run_postgres_tool(command, "pg_dump", &mut on_progress) {
        Ok(elapsed) => {
            let size = fs::metadata(file_path)
                .map_err(|error| {
                    format!(
                        "pg_dump finished, but the backup file size could not be checked ({}): {error}",
                        file_path.display()
                    )
                })?
                .len();
            if size == 0 {
                return match fs::remove_file(file_path) {
                    Ok(()) => {
                        Err("pg_dump reported success but produced an empty file.".to_string())
                    }
                    Err(error) => Err(format!(
                        "pg_dump reported success but produced an empty file that could not be removed: {error}"
                    )),
                };
            }
            Ok(format!(
                "Backup completed with pg_dump in {} ({})",
                format_elapsed(elapsed),
                format_bytes(size)
            ))
        }
        Err(error) => {
            let cleanup = fs::remove_file(file_path);
            if let Err(cleanup_error) = cleanup {
                Err(format!(
                    "{error}. The incomplete backup file could not be removed: {cleanup_error}"
                ))
            } else {
                Err(error)
            }
        }
    }
}

fn restore_database(
    cfg: &ConnectionConfig,
    database: &str,
    file_path: &Path,
    clean_before_restore: bool,
    selected_entries: &[String],
    mut on_progress: impl FnMut(&str),
) -> Result<String, String> {
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
        .arg(database)
        .stdout(Stdio::null());

    let selection_file = if is_sql_file {
        if clean_before_restore {
            return Err(
                "Clean-before-restore is not supported for plain SQL scripts. Use a PostgreSQL archive or restore into an empty database."
                    .to_string(),
            );
        }
        if !selected_entries.is_empty() {
            return Err("Object selection is only supported for PostgreSQL archives.".to_string());
        }
        command
            .arg("--set=ON_ERROR_STOP=on")
            .arg("--echo-errors")
            .arg("--file")
            .arg(file_path);
        None
    } else {
        let list = list_restore_archive(file_path)?;
        let entries = parse_restore_entries(&list);
        if selected_entries.is_empty() {
            return Err("Select at least one archive entry to restore.".to_string());
        }
        let contents = build_restore_list(&entries, selected_entries)?;
        let selection_file = create_restore_selection_file(&contents)?;
        command.arg("--verbose").arg("--exit-on-error");
        if clean_before_restore {
            command.arg("--clean").arg("--if-exists");
        }
        command
            .arg("--use-list")
            .arg(&selection_file)
            .arg(file_path);
        Some(selection_file)
    };
    apply_postgres_tool_environment(&mut command, cfg);
    let result = run_postgres_tool(command, tool, &mut on_progress).map(|elapsed| {
        format!(
            "Restore completed with {tool} in {}",
            format_elapsed(elapsed)
        )
    });
    if let Some(path) = selection_file {
        if let Err(error) = fs::remove_file(&path) {
            return match result {
                Ok(message) => Err(format!(
                    "{message}, but the temporary selection list could not be removed: {error}"
                )),
                Err(message) => Err(format!(
                    "{message}. The temporary selection list could not be removed: {error}"
                )),
            };
        }
    }
    result
}

fn is_plain_sql_backup(file_path: &Path) -> bool {
    file_path
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("sql"))
}

fn check_backup_destination(file_path: &Path) -> Result<(), String> {
    if file_path.exists() {
        return Err(format!(
            "Backup destination already exists; pg-mini will not overwrite it: {}",
            file_path.display()
        ));
    }
    let parent = file_path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    if !parent.is_dir() {
        return Err(format!(
            "Backup destination directory does not exist: {}",
            parent.display()
        ));
    }
    let probe_path = parent.join(format!(".pg-mini-write-test-{}", uuid::Uuid::new_v4()));
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&probe_path)
        .map_err(|error| {
            format!(
                "Backup destination directory is not writable ({}): {error}",
                parent.display()
            )
        })?;
    fs::remove_file(&probe_path).map_err(|error| {
        format!(
            "Could not remove backup destination access-check file {}: {error}",
            probe_path.display()
        )
    })
}

fn available_disk_space(path: &Path) -> Option<u64> {
    #[cfg(unix)]
    {
        let output = Command::new("df").arg("-Pk").arg(path).output().ok()?;
        if !output.status.success() {
            return None;
        }
        let line = String::from_utf8_lossy(&output.stdout)
            .lines()
            .last()?
            .to_string();
        let available_kib = line.split_whitespace().nth(3)?.parse::<u64>().ok()?;
        Some(available_kib.saturating_mul(1024))
    }
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;

        let path: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        let mut available = 0_u64;
        let result = unsafe {
            GetDiskFreeSpaceExW(
                path.as_ptr(),
                &mut available,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        };
        (result != 0).then_some(available)
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = path;
        None
    }
}

fn postgres_tool_version(tool: &str) -> Result<String, String> {
    let output = postgres_tool(tool)
        .arg("--version")
        .output()
        .map_err(|error| {
            format!(
                "Required client tool '{tool}' is unavailable: {error}. Install the PostgreSQL client utilities and make sure {tool} is on PATH."
            )
        })?;
    if !output.status.success() {
        return Err(format!(
            "Could not determine {tool} version ({}). Install or repair the PostgreSQL client utilities.",
            output.status
        ));
    }
    let version = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if version.is_empty() {
        return Err(format!(
            "{tool} returned no version information. Verify the PostgreSQL client installation."
        ));
    }
    Ok(version)
}

fn parse_version_major(version: &str) -> Option<u32> {
    version
        .split_whitespace()
        .rev()
        .find_map(|part| part.split('.').next()?.parse().ok())
}

fn plain_sql_dump_major(file_path: &Path) -> Option<u32> {
    let file = File::open(file_path).ok()?;
    for line in BufReader::new(file).lines().take(100).flatten() {
        if let Some(version) = line.split_once("Dumped from database version:") {
            return parse_version_major(version.1.trim());
        }
    }
    None
}

fn parse_dumped_from_major(archive_listing: &str) -> Option<u32> {
    archive_listing.lines().find_map(|line| {
        line.split_once("Dumped from database version:")
            .and_then(|(_, version)| parse_version_major(version.trim()))
    })
}

fn parse_dumped_by_major(archive_listing: &str) -> Option<u32> {
    archive_listing.lines().find_map(|line| {
        line.split_once("Dumped by pg_dump version:")
            .and_then(|(_, version)| parse_version_major(version.trim()))
    })
}

fn list_restore_archive(file_path: &Path) -> Result<String, String> {
    let output = postgres_tool("pg_restore")
        .arg("--list")
        .arg(file_path)
        .output()
        .map_err(|error| {
            format!(
                "Could not start pg_restore to inspect the archive: {error}. Install the PostgreSQL client utilities and make sure pg_restore is on PATH."
            )
        })?;
    if !output.status.success() {
        let details = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "pg_restore could not read this archive ({}): {}. Check the file format and that it is not truncated.",
            output.status,
            details.trim()
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn parse_restore_entries(listing: &str) -> Vec<RestoreArchiveEntry> {
    listing
        .lines()
        .filter(|line| {
            let trimmed = line.trim();
            !trimmed.is_empty() && !trimmed.starts_with(';')
        })
        .map(|line| RestoreArchiveEntry {
            line: line.to_string(),
        })
        .collect()
}

fn build_restore_list(
    entries: &[RestoreArchiveEntry],
    selected_entries: &[String],
) -> Result<String, String> {
    let selected: std::collections::BTreeSet<&str> =
        selected_entries.iter().map(String::as_str).collect();
    if selected.is_empty() {
        return Err("Select at least one archive entry to restore.".to_string());
    }
    if let Some(entry) = selected
        .iter()
        .find(|selected_line| !entries.iter().any(|entry| entry.line == **selected_line))
    {
        return Err(format!(
            "Selected archive entry is no longer available ({entry}); inspect the backup again."
        ));
    }
    Ok(entries
        .iter()
        .filter(|entry| selected.contains(entry.line.as_str()))
        .map(|entry| entry.line.as_str())
        .collect::<Vec<_>>()
        .join("\n")
        + "\n")
}

fn create_restore_selection_file(contents: &str) -> Result<PathBuf, String> {
    let path =
        std::env::temp_dir().join(format!("pg-mini-restore-list-{}.txt", uuid::Uuid::new_v4()));
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(&path)
        .map_err(|error| format!("Could not create temporary restore selection list: {error}"))?;
    let write_result = file.write_all(contents.as_bytes());
    drop(file);
    if let Err(error) = write_result {
        return match fs::remove_file(&path) {
            Ok(()) => Err(format!(
                "Could not write temporary restore selection list: {error}"
            )),
            Err(cleanup_error) => Err(format!(
                "Could not write temporary restore selection list: {error}. The incomplete file {} could not be removed: {cleanup_error}",
                path.display()
            )),
        };
    }
    Ok(path)
}

fn shell_quote(argument: &str) -> String {
    format!("'{}'", argument.replace('\'', "'\\''"))
}

fn postgres_tool_environment_preview(cfg: &ConnectionConfig) -> String {
    let sslmode = match cfg.tls_mode {
        TlsMode::Disable => "disable",
        TlsMode::Prefer => "verify-full",
        TlsMode::Require => "verify-full",
    };
    let cert = if cfg.tls_root_cert.is_empty() {
        String::new()
    } else {
        format!(" PGSSLROOTCERT={}", shell_quote(&cfg.tls_root_cert))
    };
    format!("PGSSLMODE={sslmode}{cert} ")
}

fn apply_postgres_tool_environment(command: &mut Command, cfg: &ConnectionConfig) {
    let sslmode = match cfg.tls_mode {
        TlsMode::Disable => "disable",
        TlsMode::Prefer => "verify-full",
        TlsMode::Require => "verify-full",
    };
    command.env("PGSSLMODE", sslmode);
    if cfg.tls_root_cert.is_empty() {
        command.env_remove("PGSSLROOTCERT");
    } else {
        command.env("PGSSLROOTCERT", &cfg.tls_root_cert);
    }
    set_tool_password(command, &cfg.password);
}

fn format_elapsed(elapsed: std::time::Duration) -> String {
    format!("{:.1}s", elapsed.as_secs_f64())
}

fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KiB", "MiB", "GiB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    format!("{value:.1} {}", UNITS[unit])
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

fn run_postgres_tool(
    mut command: Command,
    tool: &str,
    on_progress: &mut impl FnMut(&str),
) -> Result<std::time::Duration, String> {
    let started_at = Instant::now();
    let mut child = command
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| {
            format!(
                "Could not start {tool}: {error}. Install the PostgreSQL client utilities and make sure {tool} is available on PATH."
            )
        })?;
    let stderr = child.stderr.take().ok_or_else(|| {
        format!("{tool} stderr was unavailable; check that the client tool can be started.")
    })?;
    let mut error_lines = VecDeque::new();
    let mut read_error = None;
    for line in BufReader::new(stderr).lines() {
        match line {
            Ok(line) => {
                on_progress(&line);
                if !line.trim().is_empty() {
                    error_lines.push_back(line);
                    while error_lines.len() > 40 {
                        error_lines.pop_front();
                    }
                }
            }
            Err(error) => {
                read_error = Some(error);
                break;
            }
        }
    }
    let status = child
        .wait()
        .map_err(|error| format!("Could not wait for {tool} to finish: {error}"))?;
    let elapsed = started_at.elapsed();
    if let Some(error) = read_error {
        return Err(format!("Could not read {tool} progress output: {error}"));
    }
    if status.success() {
        return Ok(elapsed);
    }
    let details = error_lines.into_iter().collect::<Vec<_>>().join("\n");
    Err(format!(
        "{tool} failed with status {} after {}{}",
        status,
        format_elapsed(elapsed),
        if details.is_empty() {
            String::new()
        } else {
            format!(":\n{details}")
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

async fn fetch_system_stats_impl(client: &Client) -> Result<SystemStatsSnapshot, String> {
    let extension_rows = client
        .simple_query(
            "SELECT namespace.nspname \
             FROM pg_catalog.pg_extension AS extension \
             JOIN pg_catalog.pg_namespace AS namespace \
               ON namespace.oid = extension.extnamespace \
             WHERE extension.extname = 'system_stats';",
        )
        .await
        .map_err(|error| format!("Failed to check for system_stats extension: {error}"))?;

    let extension_schema = extension_rows
        .into_iter()
        .find_map(|message| match message {
            SimpleQueryMessage::Row(row) => row.get(0).map(str::to_string),
            _ => None,
        });
    let Some(extension_schema) = extension_schema else {
        return Ok(SystemStatsSnapshot {
            extension_schema: None,
            sections: Vec::new(),
        });
    };

    let mut sections = Vec::with_capacity(SYSTEM_STATS_FUNCTIONS.len());
    for (title, function_name) in SYSTEM_STATS_FUNCTIONS {
        let is_process_list = function_name == "pg_sys_cpu_memory_by_process";
        let row_limit = if is_process_list { " LIMIT 201" } else { "" };
        let query = format!(
            "SELECT * FROM {}.{}(){};",
            quote_sql_ident(&extension_schema),
            quote_sql_ident(function_name),
            row_limit
        );
        let mut result = execute_sql_impl(client, &query).await;
        let mut is_truncated = false;
        if let Ok(result) = &mut result {
            if is_process_list && result.rows.len() > 200 {
                result.rows.truncate(200);
                is_truncated = true;
            }
        }
        sections.push(SystemStatsSection {
            title,
            function_name,
            result,
            is_truncated,
        });
    }

    Ok(SystemStatsSnapshot {
        extension_schema: Some(extension_schema),
        sections,
    })
}

fn quote_sql_ident(ident: &str) -> String {
    format!("\"{}\"", escape_sql_ident(ident))
}

async fn fetch_activity_impl(client: &Client) -> Result<ActivitySnapshot, String> {
    let session_rows = client
        .simple_query(
            "SELECT pid::text, usename, datname, application_name, \
             COALESCE(client_addr::text, 'local'), state, \
             concat_ws(': ', wait_event_type, wait_event), \
             CASE WHEN state = 'active' AND query_start IS NOT NULL \
                  THEN round(extract(epoch FROM clock_timestamp() - query_start), 3)::text \
                  ELSE NULL END, \
             left(query, 2000) \
             FROM pg_catalog.pg_stat_activity \
             WHERE pid <> pg_backend_pid() \
             ORDER BY (state = 'active') DESC, query_start ASC NULLS LAST \
             LIMIT 100;",
        )
        .await
        .map_err(|error| format!("Failed to fetch active sessions: {error}"))?;

    let mut sessions = Vec::new();
    for row in session_rows {
        if let SimpleQueryMessage::Row(row) = row {
            sessions.push(ActivitySession {
                pid: parse_activity_i32(&row, 0, "session PID")?,
                username: activity_value(&row, 1),
                database: activity_value(&row, 2),
                application: activity_value(&row, 3),
                client_addr: activity_value(&row, 4),
                state: activity_value(&row, 5),
                wait_event: activity_value(&row, 6),
                duration_seconds: parse_optional_activity_f64(&row, 7, "session duration")?,
                query: activity_value(&row, 8),
            });
        }
    }

    let lock_rows = client
        .simple_query(
            "SELECT blocked.pid::text, blocked.usename, blocked.datname, \
             extract(epoch FROM clock_timestamp() - blocked.query_start)::text, \
             blocking.pid::text, blocking.usename, left(blocking.query, 2000), \
             left(blocked.query, 2000) \
             FROM pg_catalog.pg_stat_activity AS blocked \
             CROSS JOIN LATERAL unnest(pg_catalog.pg_blocking_pids(blocked.pid)) AS blocker(pid) \
             JOIN pg_catalog.pg_stat_activity AS blocking ON blocking.pid = blocker.pid \
             WHERE blocked.pid <> pg_backend_pid() \
             ORDER BY blocked.query_start ASC NULLS LAST LIMIT 100;",
        )
        .await
        .map_err(|error| format!("Failed to fetch blocked sessions: {error}"))?;

    let mut locks = Vec::new();
    for row in lock_rows {
        if let SimpleQueryMessage::Row(row) = row {
            locks.push(ActivityLock {
                blocked_pid: parse_activity_i32(&row, 0, "blocked PID")?,
                blocked_user: activity_value(&row, 1),
                database: activity_value(&row, 2),
                wait_seconds: parse_optional_activity_f64(&row, 3, "lock wait duration")?,
                blocking_pid: parse_activity_i32(&row, 4, "blocking PID")?,
                blocking_user: activity_value(&row, 5),
                blocking_query: activity_value(&row, 6),
                blocked_query: activity_value(&row, 7),
            });
        }
    }

    let database_rows = client
        .simple_query(
            "SELECT datname, pg_size_pretty(pg_database_size(datname)), \
             pg_database_size(datname)::text \
             FROM pg_catalog.pg_database \
             WHERE NOT datistemplate AND has_database_privilege(datname, 'CONNECT') \
             ORDER BY pg_database_size(datname) DESC LIMIT 50;",
        )
        .await
        .map_err(|error| format!("Failed to fetch database sizes: {error}"))?;
    let database_sizes = parse_size_rows(database_rows, "database")?;

    let table_rows = client
        .simple_query(
            "SELECT format('%I.%I', schemaname, relname), \
             pg_size_pretty(pg_total_relation_size(relid)), \
             pg_total_relation_size(relid)::text \
             FROM pg_catalog.pg_statio_user_tables \
             ORDER BY pg_total_relation_size(relid) DESC LIMIT 50;",
        )
        .await
        .map_err(|error| format!("Failed to fetch table sizes: {error}"))?;
    let table_sizes = parse_size_rows(table_rows, "table")?;

    Ok(ActivitySnapshot {
        sessions,
        locks,
        database_sizes,
        table_sizes,
    })
}

fn activity_value(row: &tokio_postgres::SimpleQueryRow, index: usize) -> String {
    row.get(index)
        .filter(|value| !value.is_empty())
        .unwrap_or("—")
        .to_string()
}

fn parse_activity_i32(
    row: &tokio_postgres::SimpleQueryRow,
    index: usize,
    label: &str,
) -> Result<i32, String> {
    row.get(index)
        .and_then(|value| value.parse().ok())
        .ok_or_else(|| format!("Activity query returned an invalid {label}"))
}

fn parse_size_rows(
    rows: Vec<SimpleQueryMessage>,
    object_kind: &str,
) -> Result<Vec<SizeEntry>, String> {
    rows.into_iter()
        .filter_map(|row| match row {
            SimpleQueryMessage::Row(row) => Some(row),
            _ => None,
        })
        .map(|row| {
            let name = row
                .get(0)
                .ok_or_else(|| format!("Size query returned a {object_kind} without a name"))?;
            let size = row.get(1).ok_or_else(|| {
                format!("Size query returned a {object_kind} without a formatted size")
            })?;
            let bytes = row
                .get(2)
                .and_then(|value| value.parse::<i64>().ok())
                .ok_or_else(|| format!("Size query returned an invalid {object_kind} size"))?;
            Ok(SizeEntry {
                name: name.to_string(),
                size: size.to_string(),
                bytes,
            })
        })
        .collect()
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
    let connection_limit = form.connection_limit.trim().parse::<i32>().map_err(|_| {
        "Connection limit must be -1 (unlimited) or a non-negative integer".to_string()
    })?;
    if connection_limit < -1 {
        return Err(
            "Connection limit must be -1 (unlimited) or a non-negative integer".to_string(),
        );
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
        if form.is_superuser {
            "SUPERUSER"
        } else {
            "NOSUPERUSER"
        },
        if form.can_create_db {
            "CREATEDB"
        } else {
            "NOCREATEDB"
        },
        if form.can_create_role {
            "CREATEROLE"
        } else {
            "NOCREATEROLE"
        },
        if form.can_inherit {
            "INHERIT"
        } else {
            "NOINHERIT"
        },
        if form.can_replicate {
            "REPLICATION"
        } else {
            "NOREPLICATION"
        },
        if form.can_bypass_rls {
            "BYPASSRLS"
        } else {
            "NOBYPASSRLS"
        },
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
    use super::{
        build_restore_list, check_backup_destination, is_plain_sql_backup, parse_dumped_by_major,
        parse_dumped_from_major, parse_ha_f64, parse_restore_entries, parse_version_major,
        restore_database, role_sql_preview,
    };
    use crate::models::{ConnectionConfig, RoleForm};
    use std::fs;
    use std::path::Path;

    #[test]
    fn identifies_plain_sql_backups_case_insensitively() {
        assert!(is_plain_sql_backup(Path::new("backup.sql")));
        assert!(is_plain_sql_backup(Path::new("backup.SQL")));
        assert!(!is_plain_sql_backup(Path::new("backup.dump")));
        assert!(!is_plain_sql_backup(Path::new("backup.tar")));
    }

    #[test]
    fn backup_destination_is_writable_and_existing_files_are_never_overwritten() {
        let directory =
            std::env::temp_dir().join(format!("pg-mini-backup-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&directory).unwrap();
        let path = directory.join("backup.dump");

        assert!(check_backup_destination(&path).is_ok());
        assert!(!path.exists());
        fs::write(&path, b"existing backup").unwrap();
        assert!(
            check_backup_destination(&path)
                .unwrap_err()
                .contains("will not overwrite")
        );

        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn parses_postgres_client_and_archive_versions() {
        assert_eq!(parse_version_major("pg_dump (PostgreSQL) 17.4"), Some(17));
        assert_eq!(parse_version_major("psql (PostgreSQL) 9.6.24"), Some(9));
        assert_eq!(
            parse_dumped_from_major(
                "; Dumped from database version: 16.3\n; Dumped by pg_dump version: 17.0"
            ),
            Some(16)
        );
        assert_eq!(
            parse_dumped_by_major(
                "; Dumped from database version: 16.3\n; Dumped by pg_dump version: 17.0"
            ),
            Some(17)
        );
    }

    #[test]
    fn restore_selection_contains_only_checked_archive_entries() {
        let entries = parse_restore_entries(
            "; Archive created at 2025-01-01\n1; 0 123 TABLE public first owner\n2; 0 124 TABLE public second owner\n",
        );
        let selected = vec![entries[1].line.clone()];
        assert_eq!(
            build_restore_list(&entries, &selected).unwrap(),
            "2; 0 124 TABLE public second owner\n"
        );
        assert!(build_restore_list(&entries, &[]).is_err());
        assert!(
            build_restore_list(&entries, &["stale entry".to_string()])
                .unwrap_err()
                .contains("no longer available")
        );
    }

    #[test]
    fn clean_restore_is_rejected_for_plain_sql_scripts() {
        let path =
            std::env::temp_dir().join(format!("pg-mini-restore-test-{}.sql", uuid::Uuid::new_v4()));
        fs::write(&path, "-- PostgreSQL database dump\n").unwrap();
        let result = restore_database(
            &ConnectionConfig::default(),
            "test",
            &path,
            true,
            &[],
            |_| {},
        );
        assert!(
            result
                .unwrap_err()
                .contains("not supported for plain SQL scripts")
        );
        fs::remove_file(path).unwrap();
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

    #[test]
    fn optional_replication_lag_preserves_unknown_and_zero_values() {
        assert_eq!(parse_ha_f64(None, "write lag").unwrap(), None);
        assert_eq!(parse_ha_f64(Some("0"), "write lag").unwrap(), Some(0.0));
        assert_eq!(parse_ha_f64(Some("1.25"), "write lag").unwrap(), Some(1.25));
        assert!(parse_ha_f64(Some("unknown"), "write lag").is_err());
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
    let (_cancel_tx, mut cancel_rx) = watch::channel(0_u64);
    let mut pending = start_query(client, 0, sql, false).await?;
    let cfg = ConnectionConfig {
        tls_mode: TlsMode::Disable,
        ..ConnectionConfig::default()
    };
    let has_more = fetch_query_page(&cfg, &mut pending, &mut cancel_rx).await?;
    pending.result.has_more_rows = has_more;
    pending.result.execution_time_ms = pending.started_at.elapsed().as_secs_f64() * 1000.0;
    Ok(pending.result)
}

async fn start_query(
    client: &Client,
    query_id: u64,
    sql: &str,
    read_only: bool,
) -> Result<PendingQuery, String> {
    if read_only {
        validate_read_only_sql(sql)?;
        client
            .batch_execute("BEGIN READ ONLY")
            .await
            .map_err(|error| format!("Could not start read-only transaction: {error}"))?;
    }

    let stream = match client.simple_query_raw(sql).await {
        Ok(stream) => stream,
        Err(error) => {
            if read_only {
                if let Err(rollback_error) = finish_read_only_query(client).await {
                    return Err(format!(
                        "Query error: {error}; additionally failed to roll back the read-only transaction: {rollback_error}"
                    ));
                }
            }
            return Err(format!("Query error: {error}"));
        }
    };

    Ok(PendingQuery {
        query_id,
        sql: sql.to_string(),
        stream: Box::pin(stream),
        result: QueryResult {
            query: sql.to_string(),
            ..QueryResult::empty()
        },
        buffered_message: None,
        cancel_token: client.cancel_token(),
        read_only,
        started_at: Instant::now(),
    })
}

async fn fetch_query_page(
    cfg: &ConnectionConfig,
    pending: &mut PendingQuery,
    cancel_rx: &mut watch::Receiver<u64>,
) -> Result<bool, String> {
    let mut rows_in_page = 0;
    while rows_in_page < QUERY_RESULT_PAGE_SIZE {
        let Some(message) = next_query_message(cfg, pending, cancel_rx).await? else {
            finalize_query_result(&mut pending.result);
            return Ok(false);
        };
        let is_row = matches!(&message, SimpleQueryMessage::Row(_));
        append_query_message(&mut pending.result, message);
        if is_row {
            rows_in_page += 1;
        }
    }

    loop {
        match next_query_message(cfg, pending, cancel_rx).await? {
            Some(message @ SimpleQueryMessage::Row(_)) => {
                pending.buffered_message = Some(message);
                return Ok(true);
            }
            Some(message) => append_query_message(&mut pending.result, message),
            None => {
                finalize_query_result(&mut pending.result);
                return Ok(false);
            }
        }
    }
}

async fn next_query_message(
    cfg: &ConnectionConfig,
    pending: &mut PendingQuery,
    cancel_rx: &mut watch::Receiver<u64>,
) -> Result<Option<SimpleQueryMessage>, String> {
    if let Some(message) = pending.buffered_message.take() {
        return Ok(Some(message));
    }
    loop {
        tokio::select! {
            result = pending.stream.try_next() => {
                return result.map_err(|error| format!("Query error: {error}"));
            }
            changed = cancel_rx.changed() => {
                changed.map_err(|_| "Query cancellation channel closed".to_string())?;
                if *cancel_rx.borrow() != pending.query_id {
                    continue;
                }
                send_query_cancel(cfg, &pending.cancel_token)
                    .await
                    .map_err(|error| format!("Could not send query cancellation request: {error}"))?;
                loop {
                    match pending.stream.try_next().await {
                        Ok(Some(_)) => {}
                        Ok(None) | Err(_) => break,
                    }
                }
                return Err("Query cancelled by user.".to_string());
            }
        }
    }
}

fn append_query_message(result: &mut QueryResult, message: SimpleQueryMessage) {
    match message {
        SimpleQueryMessage::Row(row) => {
            if result.columns.is_empty() {
                result.columns = row
                    .columns()
                    .iter()
                    .map(|column| column.name().to_string())
                    .collect();
                result.column_types = vec!["text".to_string(); result.columns.len()];
            }
            let values = (0..result.columns.len())
                .map(|index| match row.get(index) {
                    Some(value) if value.parse::<f64>().is_ok() => CellValue::new_number(value),
                    Some(value) => CellValue::new_text(value),
                    None => CellValue::new_null(),
                })
                .collect();
            result.rows.push(values);
        }
        SimpleQueryMessage::CommandComplete(count) => result.rows_affected = Some(count),
        _ => {}
    }
}

fn finalize_query_result(result: &mut QueryResult) {
    result.is_select = !result.columns.is_empty() || result.rows_affected.is_none();
    result.has_more_rows = false;
}

async fn finish_read_only_query(client: &Client) -> Result<(), String> {
    client
        .batch_execute("ROLLBACK")
        .await
        .map_err(|error| format!("Failed to roll back read-only transaction: {error}"))
}

async fn cancel_pending_query(
    cfg: &ConnectionConfig,
    pending: &mut PendingQuery,
) -> Result<(), String> {
    send_query_cancel(cfg, &pending.cancel_token)
        .await
        .map_err(|error| error.to_string())?;
    loop {
        match pending.stream.try_next().await {
            Ok(Some(_)) => {}
            Ok(None) | Err(_) => break,
        }
    }
    Ok(())
}

async fn send_query_cancel(
    cfg: &ConnectionConfig,
    cancel_token: &CancelToken,
) -> Result<(), String> {
    match cfg.tls_mode {
        TlsMode::Disable => cancel_token
            .cancel_query(NoTls)
            .await
            .map_err(|error| error.to_string()),
        TlsMode::Prefer => {
            if let Ok(tls) = tls_connector(cfg) {
                if cancel_token.cancel_query(tls).await.is_ok() {
                    return Ok(());
                }
            }
            cancel_token
                .cancel_query(NoTls)
                .await
                .map_err(|error| error.to_string())
        }
        TlsMode::Require => {
            let tls = tls_connector(cfg)?;
            cancel_token
                .cancel_query(tls)
                .await
                .map_err(|error| error.to_string())
        }
    }
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
