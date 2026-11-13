pub fn sql_requires_review(sql: &str) -> bool {
    let statements = sql_statements(sql);
    statements.iter().any(|tokens| {
        let Some(command) = tokens.first().map(String::as_str) else {
            return false;
        };
        tokens.iter().any(|token| {
            matches!(
                token.as_str(),
                "INSERT"
                    | "UPDATE"
                    | "DELETE"
                    | "MERGE"
                    | "CREATE"
                    | "ALTER"
                    | "DROP"
                    | "TRUNCATE"
                    | "GRANT"
                    | "REVOKE"
                    | "COMMENT"
                    | "REFRESH"
                    | "VACUUM"
                    | "ANALYZE"
                    | "REINDEX"
                    | "COPY"
                    | "CALL"
                    | "DO"
                    | "SET"
                    | "RESET"
                    | "DISCARD"
                    | "INTO"
                    | "PREPARE"
                    | "EXECUTE"
                    | "DECLARE"
                    | "ABORT"
                    | "PG_CANCEL_BACKEND"
                    | "PG_TERMINATE_BACKEND"
                    | "PG_RELOAD_CONF"
                    | "PG_ROTATE_LOGFILE"
                    | "PG_SWITCH_WAL"
                    | "PG_PROMOTE"
                    | "NEXTVAL"
                    | "SETVAL"
                    | "PG_NOTIFY"
                    | "PG_ADVISORY_LOCK"
                    | "PG_ADVISORY_XACT_LOCK"
                    | "PG_ADVISORY_UNLOCK"
                    | "PG_TRY_ADVISORY_LOCK"
                    | "PG_TRY_ADVISORY_XACT_LOCK"
                    | "SET_CONFIG"
                    | "LO_IMPORT"
                    | "LO_EXPORT"
            )
        }) || !matches!(
            command,
            "SELECT"
                | "SHOW"
                | "VALUES"
                | "TABLE"
                | "EXPLAIN"
                | "WITH"
                | "BEGIN"
                | "START"
                | "COMMIT"
                | "END"
                | "ROLLBACK"
                | "SAVEPOINT"
                | "RELEASE"
        )
    })
}

pub fn is_explain_analyze(sql: &str) -> bool {
    sql_statements(sql).iter().any(|tokens| {
        tokens.first().map(String::as_str) == Some("EXPLAIN")
            && tokens.iter().any(|token| token == "ANALYZE")
    })
}

pub fn validate_read_only_sql(sql: &str) -> Result<(), String> {
    let statements = sql_statements(sql);
    if statements.is_empty() {
        return Err("Enter a query to run in read-only mode.".to_string());
    }

    for tokens in statements {
        let Some(command) = tokens.first().map(String::as_str) else {
            continue;
        };
        if !matches!(
            command,
            "SELECT" | "SHOW" | "VALUES" | "TABLE" | "EXPLAIN" | "WITH"
        ) {
            return Err(format!(
                "Read-only mode only allows SELECT, WITH, VALUES, TABLE, SHOW, and EXPLAIN statements; found {command}."
            ));
        }
    }
    Ok(())
}

pub fn transaction_state_after(sql: &str, mut is_open: bool) -> bool {
    for tokens in sql_statements(sql) {
        match tokens.first().map(String::as_str) {
            Some("BEGIN" | "START") => is_open = true,
            Some("COMMIT" | "END") if !tokens.iter().skip(1).any(|token| token == "PREPARED") => {
                is_open = transaction_chains(&tokens);
            }
            Some("ROLLBACK" | "ABORT")
                if !tokens
                    .iter()
                    .skip(1)
                    .any(|token| matches!(token.as_str(), "TO" | "PREPARED")) =>
            {
                is_open = tokens.first().map(String::as_str) == Some("ROLLBACK")
                    && transaction_chains(&tokens);
            }
            Some("PREPARE") if tokens.get(1).map(String::as_str) == Some("TRANSACTION") => {
                is_open = false;
            }
            _ => {}
        }
    }
    is_open
}

fn transaction_chains(tokens: &[String]) -> bool {
    tokens
        .windows(2)
        .any(|pair| pair[0] == "AND" && pair[1] == "CHAIN")
}

pub fn transaction_state_after_error(sql: &str, is_open: bool) -> bool {
    is_open
        || sql_statements(sql)
            .iter()
            .any(|tokens| matches!(tokens.first().map(String::as_str), Some("BEGIN" | "START")))
}

fn sql_statements(sql: &str) -> Vec<Vec<String>> {
    let bytes = sql.as_bytes();
    let mut statements = Vec::new();
    let mut tokens = Vec::new();
    let mut i = 0;

    while i < bytes.len() {
        match bytes[i] {
            byte if byte.is_ascii_whitespace() => i += 1,
            b'-' if bytes.get(i + 1) == Some(&b'-') => {
                i += 2;
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
            }
            b'/' if bytes.get(i + 1) == Some(&b'*') => {
                i += 2;
                let mut depth = 1;
                while i < bytes.len() && depth > 0 {
                    if bytes[i] == b'/' && bytes.get(i + 1) == Some(&b'*') {
                        depth += 1;
                        i += 2;
                    } else if bytes[i] == b'*' && bytes.get(i + 1) == Some(&b'/') {
                        depth -= 1;
                        i += 2;
                    } else {
                        i += 1;
                    }
                }
            }
            b'\'' => i = skip_quoted(bytes, i, b'\'', false),
            b'"' => i = skip_quoted(bytes, i, b'"', false),
            b'$' => {
                if let Some((delimiter, end)) = dollar_quote_delimiter(bytes, i) {
                    i = end;
                    while i < bytes.len() && !bytes[i..].starts_with(&delimiter) {
                        i += 1;
                    }
                    i = (i + delimiter.len()).min(bytes.len());
                } else {
                    i += 1;
                }
            }
            b';' => {
                if !tokens.is_empty() {
                    statements.push(std::mem::take(&mut tokens));
                }
                i += 1;
            }
            byte if byte.is_ascii_alphabetic() || byte == b'_' => {
                let start = i;
                i += 1;
                while i < bytes.len()
                    && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_' || bytes[i] == b'$')
                {
                    i += 1;
                }
                tokens.push(sql[start..i].to_ascii_uppercase());
            }
            _ => i += 1,
        }
    }

    if !tokens.is_empty() {
        statements.push(tokens);
    }
    statements
}

fn skip_quoted(bytes: &[u8], mut i: usize, quote: u8, backslash_escapes: bool) -> usize {
    i += 1;
    while i < bytes.len() {
        if backslash_escapes && bytes[i] == b'\\' {
            i = (i + 2).min(bytes.len());
        } else if bytes[i] == quote {
            if bytes.get(i + 1) == Some(&quote) {
                i += 2;
            } else {
                return i + 1;
            }
        } else {
            i += 1;
        }
    }
    i
}

fn dollar_quote_delimiter(bytes: &[u8], start: usize) -> Option<(Vec<u8>, usize)> {
    let mut end = start + 1;
    if bytes.get(end) == Some(&b'$') {
        return Some((bytes[start..=end].to_vec(), end + 1));
    }
    if !bytes
        .get(end)
        .is_some_and(|byte| byte.is_ascii_alphabetic() || *byte == b'_')
    {
        return None;
    }
    end += 1;
    while end < bytes.len() && (bytes[end].is_ascii_alphanumeric() || bytes[end] == b'_') {
        end += 1;
    }
    (bytes.get(end) == Some(&b'$')).then(|| (bytes[start..=end].to_vec(), end + 1))
}

#[cfg(test)]
mod tests {
    use super::{
        is_explain_analyze, sql_requires_review, transaction_state_after,
        transaction_state_after_error, validate_read_only_sql,
    };

    #[test]
    fn mutation_review_ignores_comments_and_quoted_text() {
        assert!(!sql_requires_review(
            "SELECT 'DROP TABLE users', \"update\" FROM users -- DELETE FROM users"
        ));
        assert!(sql_requires_review(
            "WITH removed AS (DELETE FROM users RETURNING *) SELECT * FROM removed"
        ));
    }

    #[test]
    fn read_only_mode_rejects_mutations_and_transaction_control() {
        assert!(validate_read_only_sql("WITH x AS (SELECT 1) SELECT * FROM x").is_ok());
        assert!(validate_read_only_sql("SELECT 1; DELETE FROM users").is_err());
        assert!(validate_read_only_sql("COMMIT").is_err());
    }

    #[test]
    fn transaction_tracking_handles_control_statements_and_comments() {
        assert!(transaction_state_after("-- start\nBEGIN; SELECT 1", false));
        assert!(!transaction_state_after("ROLLBACK", true));
        assert!(transaction_state_after("ROLLBACK TO SAVEPOINT s", true));
        assert!(!transaction_state_after("COMMIT", true));
        assert!(!transaction_state_after("SELECT 'BEGIN; COMMIT'", false));
        assert!(transaction_state_after("COMMIT AND CHAIN", true));
        assert!(transaction_state_after("COMMIT WORK AND CHAIN", true));
        assert!(transaction_state_after("ROLLBACK AND CHAIN", true));
        assert!(transaction_state_after("ROLLBACK WORK AND CHAIN", true));
        assert!(!transaction_state_after("ABORT", true));
        assert!(transaction_state_after(
            "ROLLBACK WORK TO SAVEPOINT s",
            true
        ));
        assert!(!transaction_state_after("PREPARE TRANSACTION 'tx'", true));
        assert!(transaction_state_after("COMMIT PREPARED 'tx'", true));
    }

    #[test]
    fn failed_multi_statement_scripts_track_uncertain_open_transactions() {
        assert!(transaction_state_after_error(
            "BEGIN; SELECT invalid_column",
            false
        ));
        assert!(transaction_state_after_error("COMMIT", true));
        assert!(!transaction_state_after_error(
            "SELECT invalid_column",
            false
        ));
    }

    #[test]
    fn dollar_quoted_bodies_do_not_create_false_statements() {
        assert!(!sql_requires_review(
            "SELECT $$; DROP TABLE users;$$ AS body"
        ));
    }

    #[test]
    fn reviews_table_creation_and_operational_functions() {
        assert!(sql_requires_review("SELECT * INTO backup_users FROM users"));
        assert!(sql_requires_review("SELECT pg_terminate_backend(123)"));
        assert!(sql_requires_review("EXPLAIN ANALYZE SELECT 1"));
        assert!(is_explain_analyze("EXPLAIN (ANALYZE, BUFFERS) SELECT 1"));
        assert!(!is_explain_analyze("EXPLAIN SELECT 1"));
    }
}
