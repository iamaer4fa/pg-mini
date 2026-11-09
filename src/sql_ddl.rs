#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DdlFormKind {
    Index,
    ForeignKey,
    Constraint,
    View,
    Sequence,
    Grant,
}

impl DdlFormKind {
    pub fn title(self) -> &'static str {
        match self {
            Self::Index => "Create Index",
            Self::ForeignKey => "Create Foreign Key",
            Self::Constraint => "Add Constraint",
            Self::View => "Create View",
            Self::Sequence => "Create Sequence",
            Self::Grant => "Manage Table Grants",
        }
    }

    pub fn is_table_scoped(self) -> bool {
        matches!(
            self,
            Self::Index | Self::ForeignKey | Self::Constraint | Self::Grant
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConstraintKind {
    Check,
    Unique,
    PrimaryKey,
}

#[derive(Debug, Clone)]
pub struct DdlForm {
    pub kind: DdlFormKind,
    pub schema: String,
    pub table: String,
    pub name: String,
    pub columns: String,
    pub unique_index: bool,
    pub index_method: String,
    pub referenced_schema: String,
    pub referenced_table: String,
    pub referenced_columns: String,
    pub on_delete: String,
    pub constraint_kind: ConstraintKind,
    pub check_expression: String,
    pub view_query: String,
    pub sequence_start: String,
    pub sequence_increment: String,
    pub sequence_min: String,
    pub sequence_max: String,
    pub sequence_cache: String,
    pub sequence_cycle: bool,
    pub grantee: String,
    pub privileges: Vec<String>,
    pub grant_option: bool,
    pub revoke: bool,
    pub revoke_cascade: bool,
}

impl DdlForm {
    pub fn new(kind: DdlFormKind, schema: &str, table: &str) -> Self {
        Self {
            kind,
            schema: schema.to_string(),
            table: table.to_string(),
            name: String::new(),
            columns: String::new(),
            unique_index: false,
            index_method: "btree".to_string(),
            referenced_schema: schema.to_string(),
            referenced_table: String::new(),
            referenced_columns: String::new(),
            on_delete: "NO ACTION".to_string(),
            constraint_kind: ConstraintKind::Check,
            check_expression: String::new(),
            view_query: "SELECT ".to_string(),
            sequence_start: "1".to_string(),
            sequence_increment: "1".to_string(),
            sequence_min: String::new(),
            sequence_max: String::new(),
            sequence_cache: "1".to_string(),
            sequence_cycle: false,
            grantee: String::new(),
            privileges: vec!["SELECT".to_string()],
            grant_option: false,
            revoke: false,
            revoke_cascade: false,
        }
    }

    pub fn generate_sql(&self) -> Result<String, String> {
        match self.kind {
            DdlFormKind::Index => self.index_sql(),
            DdlFormKind::ForeignKey => self.foreign_key_sql(),
            DdlFormKind::Constraint => self.constraint_sql(),
            DdlFormKind::View => self.view_sql(),
            DdlFormKind::Sequence => self.sequence_sql(),
            DdlFormKind::Grant => self.grant_sql(),
        }
    }

    fn index_sql(&self) -> Result<String, String> {
        let name = required(&self.name, "Index name")?;
        let columns = parse_ident_list(&self.columns, "Index columns")?;
        let method = match self.index_method.as_str() {
            "btree" | "hash" | "gist" | "spgist" | "gin" | "brin" => self.index_method.as_str(),
            _ => return Err("Select a supported PostgreSQL index method".to_string()),
        };
        Ok(format!(
            "CREATE {}INDEX {} ON {} USING {} ({});",
            if self.unique_index { "UNIQUE " } else { "" },
            quote_ident(name),
            qualified_name(&self.schema, &self.table)?,
            method,
            columns.join(", ")
        ))
    }

    fn foreign_key_sql(&self) -> Result<String, String> {
        let name = required(&self.name, "Constraint name")?;
        let columns = parse_ident_list(&self.columns, "Local columns")?;
        let referenced_columns = parse_ident_list(&self.referenced_columns, "Referenced columns")?;
        if columns.len() != referenced_columns.len() {
            return Err("Local and referenced column counts must match".to_string());
        }
        let on_delete = match self.on_delete.as_str() {
            "NO ACTION" | "RESTRICT" | "CASCADE" | "SET NULL" | "SET DEFAULT" => {
                self.on_delete.as_str()
            }
            _ => return Err("Select a supported ON DELETE action".to_string()),
        };
        Ok(format!(
            "ALTER TABLE {} ADD CONSTRAINT {} FOREIGN KEY ({}) REFERENCES {} ({}) ON DELETE {};",
            qualified_name(&self.schema, &self.table)?,
            quote_ident(name),
            columns.join(", "),
            qualified_name(&self.referenced_schema, &self.referenced_table)?,
            referenced_columns.join(", "),
            on_delete
        ))
    }

    fn constraint_sql(&self) -> Result<String, String> {
        let name = required(&self.name, "Constraint name")?;
        let target = qualified_name(&self.schema, &self.table)?;
        let clause = match self.constraint_kind {
            ConstraintKind::Check => {
                let expression = validate_expression(&self.check_expression)?;
                format!("CHECK ({expression})")
            }
            ConstraintKind::Unique => {
                format!(
                    "UNIQUE ({})",
                    parse_ident_list(&self.columns, "Columns")?.join(", ")
                )
            }
            ConstraintKind::PrimaryKey => format!(
                "PRIMARY KEY ({})",
                parse_ident_list(&self.columns, "Columns")?.join(", ")
            ),
        };
        Ok(format!(
            "ALTER TABLE {target} ADD CONSTRAINT {} {clause};",
            quote_ident(name)
        ))
    }

    fn view_sql(&self) -> Result<String, String> {
        let name = required(&self.name, "View name")?;
        let query = validate_single_select(&self.view_query)?;
        Ok(format!(
            "CREATE VIEW {} AS {query};",
            qualified_name(&self.schema, name)?
        ))
    }

    fn sequence_sql(&self) -> Result<String, String> {
        let name = required(&self.name, "Sequence name")?;
        let start = parse_i64(&self.sequence_start, "Start value")?;
        let increment = parse_i64(&self.sequence_increment, "Increment")?;
        if increment == 0 {
            return Err("Sequence increment cannot be zero".to_string());
        }
        let min = optional_i64(&self.sequence_min, "Minimum value")?;
        let max = optional_i64(&self.sequence_max, "Maximum value")?;
        if let (Some(min), Some(max)) = (min, max) {
            if min > max {
                return Err("Minimum value must not exceed maximum value".to_string());
            }
        }
        if min.is_some_and(|min| start < min) || max.is_some_and(|max| start > max) {
            return Err("Start value must be within the specified sequence bounds".to_string());
        }
        let cache = parse_i64(&self.sequence_cache, "Cache value")?;
        if cache < 1 {
            return Err("Cache value must be at least 1".to_string());
        }

        let mut options = vec![
            format!("INCREMENT BY {increment}"),
            format!("START WITH {start}"),
            format!("CACHE {cache}"),
        ];
        if let Some(min) = min {
            options.push(format!("MINVALUE {min}"));
        } else {
            options.push("NO MINVALUE".to_string());
        }
        if let Some(max) = max {
            options.push(format!("MAXVALUE {max}"));
        } else {
            options.push("NO MAXVALUE".to_string());
        }
        options.push(
            if self.sequence_cycle {
                "CYCLE"
            } else {
                "NO CYCLE"
            }
            .to_string(),
        );
        Ok(format!(
            "CREATE SEQUENCE {} {};",
            qualified_name(&self.schema, name)?,
            options.join(" ")
        ))
    }

    fn grant_sql(&self) -> Result<String, String> {
        let grantee = required(&self.grantee, "Grantee")?;
        if grantee.contains('\0') {
            return Err("Grantee must not contain a NUL character".to_string());
        }
        let valid_privileges = [
            "SELECT",
            "INSERT",
            "UPDATE",
            "DELETE",
            "TRUNCATE",
            "REFERENCES",
            "TRIGGER",
        ];
        if self.privileges.is_empty() {
            return Err("Select at least one table privilege".to_string());
        }
        if self
            .privileges
            .iter()
            .any(|privilege| !valid_privileges.contains(&privilege.as_str()))
        {
            return Err("One or more selected table privileges are invalid".to_string());
        }
        let privileges = &self.privileges;
        let grantee = if grantee.eq_ignore_ascii_case("public") {
            "PUBLIC".to_string()
        } else {
            quote_ident(grantee)
        };
        let table = qualified_name(&self.schema, &self.table)?;
        if self.revoke {
            Ok(format!(
                "REVOKE {} ON TABLE {table} FROM {grantee} {};",
                privileges.join(", "),
                if self.revoke_cascade {
                    "CASCADE"
                } else {
                    "RESTRICT"
                }
            ))
        } else {
            Ok(format!(
                "GRANT {} ON TABLE {table} TO {grantee}{};",
                privileges.join(", "),
                if self.grant_option {
                    " WITH GRANT OPTION"
                } else {
                    ""
                }
            ))
        }
    }
}

fn required<'a>(value: &'a str, label: &str) -> Result<&'a str, String> {
    let value = value.trim();
    if value.is_empty() || value.contains('\0') {
        Err(format!("{label} must not be empty"))
    } else {
        Ok(value)
    }
}

fn quote_ident(value: &str) -> String {
    format!("\"{}\"", value.replace('"', "\"\""))
}

fn qualified_name(schema: &str, name: &str) -> Result<String, String> {
    Ok(format!(
        "{}.{}",
        quote_ident(required(schema, "Schema")?),
        quote_ident(required(name, "Object name")?)
    ))
}

fn parse_ident_list(value: &str, label: &str) -> Result<Vec<String>, String> {
    let identifiers = value
        .split(',')
        .map(str::trim)
        .map(|identifier| {
            if identifier.is_empty() || identifier.contains('\0') {
                Err(format!("{label} must be a comma-separated list of names"))
            } else {
                Ok(quote_ident(identifier))
            }
        })
        .collect::<Result<Vec<_>, _>>()?;
    if identifiers.is_empty() {
        Err(format!("{label} must not be empty"))
    } else {
        Ok(identifiers)
    }
}

fn validate_expression(expression: &str) -> Result<&str, String> {
    let expression = required(expression, "Check expression")?;
    if expression.contains(';')
        || expression.contains("--")
        || expression.contains("/*")
        || expression.contains("*/")
    {
        return Err("Check expression cannot contain statement separators or SQL comments".into());
    }
    Ok(expression)
}

fn validate_single_select(query: &str) -> Result<String, String> {
    let query = required(query, "View query")?.trim_end_matches(';').trim();
    let upper = query.to_ascii_uppercase();
    if !(upper.starts_with("SELECT ")
        || upper.starts_with("SELECT\n")
        || upper.starts_with("WITH "))
    {
        return Err("View query must begin with SELECT or WITH".to_string());
    }
    if query.contains(';') || query.contains("--") || query.contains("/*") || query.contains("*/") {
        return Err("View query must be a single statement without SQL comments".to_string());
    }
    Ok(query.to_string())
}

fn parse_i64(value: &str, label: &str) -> Result<i64, String> {
    value
        .trim()
        .parse()
        .map_err(|_| format!("{label} must be an integer"))
}

fn optional_i64(value: &str, label: &str) -> Result<Option<i64>, String> {
    if value.trim().is_empty() {
        Ok(None)
    } else {
        parse_i64(value, label).map(Some)
    }
}

#[cfg(test)]
mod tests {
    use super::{ConstraintKind, DdlForm, DdlFormKind};

    #[test]
    fn index_sql_quotes_names_and_lists_columns() {
        let mut form = DdlForm::new(DdlFormKind::Index, "sales", "order");
        form.name = "order_idx".to_string();
        form.columns = "customer_id, created_at".to_string();
        form.unique_index = true;
        assert_eq!(
            form.generate_sql().unwrap(),
            "CREATE UNIQUE INDEX \"order_idx\" ON \"sales\".\"order\" USING btree (\"customer_id\", \"created_at\");"
        );
    }

    #[test]
    fn foreign_key_requires_matching_column_counts() {
        let mut form = DdlForm::new(DdlFormKind::ForeignKey, "public", "orders");
        form.name = "orders_customer_fk".to_string();
        form.columns = "customer_id, region_id".to_string();
        form.referenced_table = "customers".to_string();
        form.referenced_columns = "id".to_string();
        assert!(
            form.generate_sql()
                .unwrap_err()
                .contains("counts must match")
        );
    }

    #[test]
    fn foreign_key_generates_reference_and_delete_action() {
        let mut form = DdlForm::new(DdlFormKind::ForeignKey, "public", "orders");
        form.name = "orders_customer_fk".to_string();
        form.columns = "customer_id".to_string();
        form.referenced_table = "customers".to_string();
        form.referenced_columns = "id".to_string();
        form.on_delete = "CASCADE".to_string();
        assert_eq!(
            form.generate_sql().unwrap(),
            "ALTER TABLE \"public\".\"orders\" ADD CONSTRAINT \"orders_customer_fk\" FOREIGN KEY (\"customer_id\") REFERENCES \"public\".\"customers\" (\"id\") ON DELETE CASCADE;"
        );
    }

    #[test]
    fn check_constraint_rejects_multiple_statements() {
        let mut form = DdlForm::new(DdlFormKind::Constraint, "public", "accounts");
        form.name = "balance_check".to_string();
        form.constraint_kind = ConstraintKind::Check;
        form.check_expression = "balance >= 0; DROP TABLE accounts".to_string();
        assert!(form.generate_sql().is_err());
    }

    #[test]
    fn view_requires_a_single_select_statement() {
        let mut form = DdlForm::new(DdlFormKind::View, "public", "");
        form.name = "active_users".to_string();
        form.view_query = "SELECT * FROM users; DROP TABLE users".to_string();
        assert!(form.generate_sql().is_err());
    }

    #[test]
    fn sequence_validates_increment_and_bounds() {
        let mut form = DdlForm::new(DdlFormKind::Sequence, "public", "");
        form.name = "invoice_id_seq".to_string();
        form.sequence_increment = "0".to_string();
        assert!(form.generate_sql().unwrap_err().contains("cannot be zero"));
    }

    #[test]
    fn sequence_generates_options() {
        let mut form = DdlForm::new(DdlFormKind::Sequence, "public", "");
        form.name = "invoice_id_seq".to_string();
        form.sequence_start = "10".to_string();
        form.sequence_increment = "5".to_string();
        form.sequence_max = "500".to_string();
        form.sequence_cycle = true;
        assert_eq!(
            form.generate_sql().unwrap(),
            "CREATE SEQUENCE \"public\".\"invoice_id_seq\" INCREMENT BY 5 START WITH 10 CACHE 1 NO MINVALUE MAXVALUE 500 CYCLE;"
        );
    }

    #[test]
    fn view_generates_create_view_statement() {
        let mut form = DdlForm::new(DdlFormKind::View, "reporting", "");
        form.name = "active_customers".to_string();
        form.view_query = "SELECT id, name FROM customers WHERE active".to_string();
        assert_eq!(
            form.generate_sql().unwrap(),
            "CREATE VIEW \"reporting\".\"active_customers\" AS SELECT id, name FROM customers WHERE active;"
        );
    }

    #[test]
    fn grants_support_grant_option_and_revoke() {
        let mut form = DdlForm::new(DdlFormKind::Grant, "public", "invoices");
        form.grantee = "app_user".to_string();
        form.grant_option = true;
        assert_eq!(
            form.generate_sql().unwrap(),
            "GRANT SELECT ON TABLE \"public\".\"invoices\" TO \"app_user\" WITH GRANT OPTION;"
        );
        form.revoke = true;
        assert_eq!(
            form.generate_sql().unwrap(),
            "REVOKE SELECT ON TABLE \"public\".\"invoices\" FROM \"app_user\" RESTRICT;"
        );
    }
}
