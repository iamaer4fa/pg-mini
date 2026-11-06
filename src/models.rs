use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ConnectionConfig {
    pub id: String,
    pub name: String,
    pub host: String,
    pub port: u16,
    pub database: String,
    pub username: String,
    #[serde(default)]
    pub password: String,
}

impl Default for ConnectionConfig {
    fn default() -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            name: "Local PostgreSQL".to_string(),
            host: "localhost".to_string(),
            port: 5432,
            database: "postgres".to_string(),
            username: "postgres".to_string(),
            password: String::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct CellValue {
    pub text: String,
    pub is_null: bool,
    pub is_number: bool,
}

impl CellValue {
    pub fn new_null() -> Self {
        Self {
            text: "NULL".to_string(),
            is_null: true,
            is_number: false,
        }
    }

    pub fn new_text(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            is_null: false,
            is_number: false,
        }
    }

    pub fn new_number(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            is_null: false,
            is_number: true,
        }
    }
}

#[derive(Debug, Clone)]
pub struct QueryResult {
    pub query: String,
    pub columns: Vec<String>,
    pub column_types: Vec<String>,
    pub rows: Vec<Vec<CellValue>>,
    pub rows_affected: Option<u64>,
    pub execution_time_ms: f64,
    pub is_select: bool,
}

impl QueryResult {
    pub fn empty() -> Self {
        Self {
            query: String::new(),
            columns: Vec::new(),
            column_types: Vec::new(),
            rows: Vec::new(),
            rows_affected: None,
            execution_time_ms: 0.0,
            is_select: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct TableItem {
    pub schema: String,
    pub name: String,
    pub table_type: String, // "BASE TABLE", "VIEW", "FOREIGN TABLE"
}

#[derive(Debug, Clone)]
pub struct ColumnItem {
    pub name: String,
    pub data_type: String,
    pub is_nullable: bool,
    pub default_value: Option<String>,
    pub is_primary_key: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    #[serde(default)]
    pub saved_connections: Vec<ConnectionConfig>,
    pub last_connection_id: Option<String>,
    #[serde(default = "default_true")]
    pub dark_mode: bool,
}

fn default_true() -> bool {
    true
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            saved_connections: vec![ConnectionConfig::default()],
            last_connection_id: None,
            dark_mode: true,
        }
    }
}
