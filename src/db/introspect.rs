use crate::db::async_utils::quote_ident;
use std::error::Error;
use tokio_postgres::Client;

/// Column metadata from pg_catalog for type-safe conversions
#[derive(Debug, Clone)]
pub struct ColumnMeta {
    pub name: String,
    pub pg_type: String,       // e.g., "int4", "text", "bool", "float8"
    pub is_nullable: bool,
    pub has_default: bool,
    pub is_primary_key: bool,
}

/// Fetch detailed column metadata from pg_catalog for type-safe CSV import
pub async fn table_schema(
    client: &Client,
    schema: &str,
    table: &str,
) -> Result<Vec<ColumnMeta>, Box<dyn Error>> {
    let schema_quoted = quote_ident(schema)?;
    let table_quoted = quote_ident(table)?;

    let sql = format!(
        r#"
        SELECT 
            a.attname,
            t.typname,
            a.attnotnull,
            a.atthasdef,
            CASE WHEN pk.attname IS NOT NULL THEN true ELSE false END AS is_pk
        FROM pg_attribute a
        JOIN pg_type t ON a.atttypid = t.oid
        LEFT JOIN (
            SELECT kcu.column_name, kcu.attname
            FROM information_schema.table_constraints tc
            JOIN information_schema.key_column_usage kcu
                ON tc.constraint_name = kcu.constraint_name
                AND tc.table_schema = kcu.table_schema
            WHERE tc.constraint_type = 'PRIMARY KEY'
              AND tc.table_schema = $1
              AND tc.table_name = $2
        ) pk ON a.attname = pk.attname
        WHERE a.attrelid = '{schema_quoted}.{table_quoted}'::regclass
          AND a.attnum > 0
          AND NOT a.attisdropped
        ORDER BY a.attnum;
        "#
    );

    let rows = client.query(&sql, &[]).await?;

    let mut columns = Vec::new();
    for row in rows {
        let name: String = row.get(0);
        let pg_type: String = row.get(1);
        let attnotnull: bool = row.get(2);
        let atthasdef: bool = row.get(3);
        let is_pk: bool = row.get(4);

        columns.push(ColumnMeta {
            name,
            pg_type,
            is_nullable: !attnotnull,
            has_default: atthasdef,
            is_primary_key: is_pk,
        });
    }

    Ok(columns)
}

/// Get the primary key column name for a table (single-column PK only)
pub async fn get_primary_key_column(
    client: &Client,
    schema: &str,
    table: &str,
) -> Result<Option<String>, Box<dyn Error>> {
    let _schema_quoted = quote_ident(schema)?;
    let _table_quoted = quote_ident(table)?;

    let sql = format!(
        r#"
        SELECT kcu.column_name
        FROM information_schema.table_constraints tc
        JOIN information_schema.key_column_usage kcu
            ON tc.constraint_name = kcu.constraint_name
            AND tc.table_schema = kcu.table_schema
        WHERE tc.constraint_type = 'PRIMARY KEY'
          AND tc.table_schema = $1
          AND tc.table_name = $2
        ORDER BY kcu.ordinal_position
        LIMIT 1;
        "#
    );

    let rows = client.query(&sql, &[&schema, &table]).await?;

    if let Some(row) = rows.first() {
        let col: String = row.get(0);
        Ok(Some(col))
    } else {
        Ok(None)
    }
}

/// Convert CSV string value to appropriate PostgreSQL type based on column metadata
pub fn csv_value_to_tosql(
    value: &str,
    meta: &ColumnMeta,
    treat_empty_as_null: bool,
) -> Result<Box<dyn tokio_postgres::types::ToSql + Sync>, Box<dyn Error>> {
    // Handle empty string
    if value.is_empty() {
        if treat_empty_as_null && meta.is_nullable {
            return Ok(Box::new(None::<String>));
        } else if !meta.is_nullable && !meta.has_default {
            return Err(format!("Column '{}' is NOT NULL and has no default, but CSV value is empty", meta.name).into());
        } else {
            // Empty string for text types, or will be handled by type conversion
            return csv_value_to_tosql_typed("", meta);
        }
    }

    csv_value_to_tosql_typed(value, meta)
}

fn csv_value_to_tosql_typed(
    value: &str,
    meta: &ColumnMeta,
) -> Result<Box<dyn tokio_postgres::types::ToSql + Sync>, Box<dyn Error>> {
    match meta.pg_type.as_str() {
        "bool" => {
            let v = value.parse::<bool>().map_err(|_| format!("Invalid boolean: {}", value))?;
            Ok(Box::new(v))
        }
        "int2" => {
            let v = value.parse::<i16>().map_err(|_| format!("Invalid smallint: {}", value))?;
            Ok(Box::new(v))
        }
        "int4" => {
            let v = value.parse::<i32>().map_err(|_| format!("Invalid integer: {}", value))?;
            Ok(Box::new(v))
        }
        "int8" => {
            let v = value.parse::<i64>().map_err(|_| format!("Invalid bigint: {}", value))?;
            Ok(Box::new(v))
        }
        "float4" => {
            let v = value.parse::<f32>().map_err(|_| format!("Invalid real: {}", value))?;
            Ok(Box::new(v))
        }
        "float8" => {
            let v = value.parse::<f64>().map_err(|_| format!("Invalid double precision: {}", value))?;
            Ok(Box::new(v))
        }
        "text" | "varchar" | "bpchar" | "name" => {
            Ok(Box::new(value.to_string()))
        }
        "uuid" => {
            let v = value.parse::<uuid::Uuid>().map_err(|_| format!("Invalid UUID: {}", value))?;
            Ok(Box::new(v))
        }
        "date" => {
            let v = value.parse::<chrono::NaiveDate>().map_err(|_| format!("Invalid date: {}", value))?;
            Ok(Box::new(v))
        }
        "time" | "timetz" => {
            let v = value.parse::<chrono::NaiveTime>().map_err(|_| format!("Invalid time: {}", value))?;
            Ok(Box::new(v))
        }
        "timestamp" | "timestamptz" => {
            let v = value.parse::<chrono::DateTime<chrono::Utc>>().map_err(|_| format!("Invalid timestamp: {}", value))?;
            Ok(Box::new(v))
        }
        "json" | "jsonb" => {
            let v: serde_json::Value = serde_json::from_str(value).map_err(|_| format!("Invalid JSON: {}", value))?;
            Ok(Box::new(v))
        }
        _ => {
            // Fallback to text for unknown types
            Ok(Box::new(value.to_string()))
        }
    }
}