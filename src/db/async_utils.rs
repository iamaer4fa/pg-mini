use regex::Regex;
use serde_json::Value;
use std::error::Error;
use tokio_postgres::types::ToSql;

/// Quote an identifier safely – accepts only alphanumeric + underscore, otherwise returns an error.
pub fn quote_ident(ident: &str) -> Result<String, Box<dyn Error>> {
    let re = Regex::new(r"^[a-zA-Z_][a-zA-Z0-9_]*$")?;
    if re.is_match(ident) {
        Ok(format!("\"{}\"", ident))
    } else {
        Err(format!("Invalid identifier: {}", ident).into())
    }
}

/// Convert a JSON value to a boxed `ToSql + Sync` based on a PostgreSQL type OID.
pub fn json_to_tosql(value: &Value, pg_type: &str) -> Result<Box<dyn ToSql + Sync>, Box<dyn Error>> {
    match (value, pg_type) {
        (Value::Null, _) => Ok(Box::new(None::<String>)),
        (Value::Bool(b), "bool") => Ok(Box::new(*b)),
        (Value::Number(num), "int2") => Ok(Box::new(num.as_i64().unwrap_or_default() as i16)),
        (Value::Number(num), "int4") => Ok(Box::new(num.as_i64().unwrap_or_default() as i32)),
        (Value::Number(num), "int8") => Ok(Box::new(num.as_i64().unwrap_or_default())),
        (Value::Number(num), "float4") => Ok(Box::new(num.as_f64().unwrap_or_default() as f32)),
        (Value::Number(num), "float8") => Ok(Box::new(num.as_f64().unwrap_or_default() as f64)),
        (Value::String(s), "text" | "varchar" | "bpchar") => Ok(Box::new(s.clone())),
        // Add more type mappings as needed
        _ => Err(format!("Unsupported conversion for {:?} as {}", value, pg_type).into()),
    }
}

/// Primary key value for a single-column primary key
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PrimaryKeyKey(pub Value);

impl PrimaryKeyKey {
    pub fn from_cell_value(cell: &crate::models::CellValue) -> Self {
        if cell.is_null {
            Self(Value::Null)
        } else if cell.is_number {
            // Try to parse as integer first
            if let Ok(n) = cell.text.parse::<i64>() {
                Self(Value::Number(n.into()))
            } else if let Ok(f) = cell.text.parse::<f64>() {
                Self(Value::Number(serde_json::Number::from_f64(f).unwrap()))
            } else {
                Self(Value::String(cell.text.clone()))
            }
        } else {
            Self(Value::String(cell.text.clone()))
        }
    }
}