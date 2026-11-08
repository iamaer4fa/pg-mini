use crate::models::{ColumnItem, QueryResult, TableItem};
use crate::ui::data_table::{DataTableAction, DataTableState, render_data_table};
use egui::{Color32, Key, RichText, ScrollArea, Ui, text::LayoutJob};

const MAX_HISTORY: usize = 200;
const ADMIN_INSIGHTS: [(&str, &str); 4] = [
    (
        "Server activity",
        "SELECT pid, usename, datname, application_name, client_addr, state, wait_event_type, wait_event, query_start, query\n\
         FROM pg_stat_activity\n\
         WHERE datname = current_database()\n\
         ORDER BY query_start DESC NULLS LAST;",
    ),
    (
        "Blocked sessions",
        "SELECT blocked.pid AS blocked_pid, blocked.usename AS blocked_user,\n\
         now() - blocked.query_start AS wait_duration, blocking.pid AS blocking_pid,\n\
         blocking.usename AS blocking_user, blocked.query AS blocked_query,\n\
         blocking.query AS blocking_query\n\
         FROM pg_stat_activity AS blocked\n\
         CROSS JOIN LATERAL unnest(pg_blocking_pids(blocked.pid)) AS blocker(pid)\n\
         JOIN pg_stat_activity AS blocking ON blocking.pid = blocker.pid\n\
         ORDER BY blocked.query_start;",
    ),
    (
        "Database statistics",
        "SELECT d.datname, s.numbackends, s.xact_commit, s.xact_rollback,\n\
         s.blks_read, s.blks_hit,\n\
         round(100.0 * s.blks_hit / NULLIF(s.blks_hit + s.blks_read, 0), 2) AS cache_hit_pct,\n\
         pg_size_pretty(pg_database_size(d.datname)) AS database_size\n\
         FROM pg_stat_database AS s\n\
         JOIN pg_database AS d ON d.datname = s.datname\n\
         WHERE NOT d.datistemplate\n\
         ORDER BY pg_database_size(d.datname) DESC;",
    ),
    (
        "Database roles",
        "SELECT rolname, rolsuper, rolinherit, rolcreaterole, rolcreatedb,\n\
         rolcanlogin, rolreplication, rolconnlimit, rolvaliduntil\n\
         FROM pg_roles\n\
         ORDER BY rolname;",
    ),
];

const SQL_KEYWORDS: &[&str] = &[
    "SELECT",
    "FROM",
    "WHERE",
    "AND",
    "OR",
    "NOT",
    "NULL",
    "IS",
    "IN",
    "LIKE",
    "ILIKE",
    "BETWEEN",
    "ORDER",
    "BY",
    "ASC",
    "DESC",
    "LIMIT",
    "OFFSET",
    "GROUP",
    "HAVING",
    "JOIN",
    "INNER",
    "LEFT",
    "RIGHT",
    "FULL",
    "OUTER",
    "CROSS",
    "ON",
    "AS",
    "DISTINCT",
    "INSERT",
    "INTO",
    "VALUES",
    "UPDATE",
    "SET",
    "DELETE",
    "RETURNING",
    "CREATE",
    "ALTER",
    "DROP",
    "TABLE",
    "VIEW",
    "INDEX",
    "SCHEMA",
    "DATABASE",
    "CONSTRAINT",
    "PRIMARY",
    "KEY",
    "FOREIGN",
    "REFERENCES",
    "UNIQUE",
    "CHECK",
    "DEFAULT",
    "CASCADE",
    "TRUNCATE",
    "WITH",
    "RECURSIVE",
    "UNION",
    "ALL",
    "EXCEPT",
    "INTERSECT",
    "CASE",
    "WHEN",
    "THEN",
    "ELSE",
    "END",
    "BEGIN",
    "COMMIT",
    "ROLLBACK",
    "EXPLAIN",
    "ANALYZE",
    "VACUUM",
    "GRANT",
    "REVOKE",
    "TRUE",
    "FALSE",
    "OFFSET",
    "FETCH",
    "FOR",
    "INTO",
    "EXISTS",
    "OVER",
    "PARTITION",
    "FILTER",
];

pub struct SqlEditor {
    pub sql_text: String,
    pub history: Vec<String>,
    pub history_search: String,
    pub selected_history_idx: Option<usize>,
    pub data_state: DataTableState,
    pub editor_height: f32,
    show_completions: bool,
}

impl Default for SqlEditor {
    fn default() -> Self {
        Self {
            sql_text: "SELECT table_schema, table_name, table_type \nFROM information_schema.tables \nWHERE table_schema = 'public' \nORDER BY table_name;".to_string(),
            history: Vec::new(),
            history_search: String::new(),
            selected_history_idx: None,
            data_state: DataTableState::default(),
            editor_height: 180.0,
            show_completions: false,
        }
    }
}

pub enum SqlEditorAction {
    ExecuteQuery(String),
    CopyCell(String),
    ExportCsv,
    CopyQuery(String),
}

impl SqlEditor {
    pub fn show(
        &mut self,
        ui: &mut Ui,
        last_result: &Option<Result<QueryResult, String>>,
        is_connected: bool,
        is_busy: bool,
        tables: &[TableItem],
        columns: &[ColumnItem],
    ) -> Option<SqlEditorAction> {
        let mut action: Option<SqlEditorAction> = None;
        let ctrl_enter = ui.input(|i| {
            (i.modifiers.command || i.modifiers.ctrl) && i.key_pressed(Key::Enter)
                || i.key_pressed(Key::F5)
        });

        if ctrl_enter && is_connected && !is_busy && !self.sql_text.trim().is_empty() {
            let query = self.sql_text.trim().to_string();
            self.add_history(&query);
            return Some(SqlEditorAction::ExecuteQuery(query));
        }

        ui.horizontal(|ui| {
            let run = ui.add_enabled(
                is_connected && !is_busy && !self.sql_text.trim().is_empty(),
                egui::Button::new(RichText::new(if is_busy { "⏳ Running..." } else { "▶ Run Query (Ctrl+Enter)" }).strong().color(Color32::WHITE))
                    .fill(Color32::from_rgb(16, 145, 90)),
            );
            if run.clicked() {
                let query = self.sql_text.trim().to_string();
                self.add_history(&query);
                action = Some(SqlEditorAction::ExecuteQuery(query));
            }
            if ui.button("Clear").clicked() { self.sql_text.clear(); }
            if ui.button("Completion (Ctrl+Space)").clicked() { self.show_completions = !self.show_completions; }
            ui.separator();
            egui::ComboBox::from_id_salt("sql_templates")
                .selected_text("Quick Snippets ▾").width(130.0).show_ui(ui, |ui| {
                    for (label, sql) in [
                        ("Select from table", "SELECT * FROM public.your_table LIMIT 50;"),
                        ("Count rows", "SELECT count(*) FROM public.your_table;"),
                        ("List public tables", "SELECT table_name FROM information_schema.tables WHERE table_schema = 'public' ORDER BY table_name;"),
                        ("Server info", "SELECT current_database(), current_user, version(), inet_server_addr(), inet_server_port();"),
                        ("Database sizes", "SELECT datname, pg_size_pretty(pg_database_size(datname)) FROM pg_database ORDER BY pg_database_size(datname) DESC;"),
                    ] {
                        if ui.button(label).clicked() { self.sql_text = sql.to_string(); ui.close_menu(); }
                    }
                });
            if ui.add_enabled(
                is_connected && !is_busy && !self.sql_text.trim().is_empty(),
                egui::Button::new("Explain Query"),
            ).on_hover_text("Show a non-executing PostgreSQL query plan").clicked() {
                if let Some(query) = prepare_query_plan(&self.sql_text) {
                    self.sql_text = query.clone();
                    self.add_history(&query);
                    action = Some(SqlEditorAction::ExecuteQuery(query));
                }
            }
            egui::ComboBox::from_id_salt("admin_insights")
                .selected_text("Admin Insights ▾")
                .width(130.0)
                .show_ui(ui, |ui| {
                    for (label, sql) in ADMIN_INSIGHTS {
                        if ui.button(label).clicked() {
                            let query = sql.to_string();
                            self.sql_text = query.clone();
                            self.add_history(&query);
                            action = Some(SqlEditorAction::ExecuteQuery(query));
                            ui.close_menu();
                        }
                    }
                });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if !is_connected { ui.label(RichText::new("Disconnected").color(Color32::GRAY).size(12.0)); }
            });
        });

        ui.horizontal(|ui| {
            ui.label(RichText::new("SQL History").strong());
            ui.add(
                egui::TextEdit::singleline(&mut self.history_search)
                    .hint_text("Search saved queries...")
                    .desired_width(220.0),
            );
            if !self.history_search.is_empty() && ui.small_button("Clear search").clicked() {
                self.history_search.clear();
            }
            ui.label(
                RichText::new(format!("{} entries", self.history.len()))
                    .color(Color32::GRAY)
                    .small(),
            );
        });
        egui::Frame::group(ui.style()).show(ui, |ui| {
            ScrollArea::vertical()
                .max_height(125.0)
                .auto_shrink([false; 2])
                .show(ui, |ui| {
                    let needle = self.history_search.trim().to_lowercase();
                    let matches: Vec<(usize, String)> = self
                        .history
                        .iter()
                        .enumerate()
                        .rev()
                        .filter(|(_, q)| needle.is_empty() || q.to_lowercase().contains(&needle))
                        .map(|(i, q)| (i, q.clone()))
                        .collect();
                    if matches.is_empty() {
                        ui.label(
                            RichText::new(if self.history.is_empty() {
                                "Executed queries will be saved here."
                            } else {
                                "No matching queries."
                            })
                            .color(Color32::GRAY)
                            .small(),
                        );
                    }
                    for (idx, query) in matches {
                        ui.horizontal(|ui| {
                            let preview = query.lines().next().unwrap_or("").trim();
                            let preview = if preview.chars().count() > 100 {
                                format!("{}…", preview.chars().take(100).collect::<String>())
                            } else {
                                preview.to_string()
                            };
                            if ui
                                .add(
                                    egui::Button::new(RichText::new(preview).monospace())
                                        .frame(false),
                                )
                                .clicked()
                            {
                                self.sql_text = query.clone();
                                self.selected_history_idx = Some(idx);
                            }
                            if ui
                                .small_button("Use")
                                .on_hover_text("Load query into editor")
                                .clicked()
                            {
                                self.sql_text = query.clone();
                                self.selected_history_idx = Some(idx);
                            }
                            if ui
                                .small_button("Copy")
                                .on_hover_text("Copy query to clipboard")
                                .clicked()
                            {
                                action = Some(SqlEditorAction::CopyQuery(query.clone()));
                            }
                        });
                    }
                });
        });

        ui.add_space(4.0);
        let completion_shortcut = ui.input(|i| i.modifiers.ctrl && i.key_pressed(Key::Space));
        if completion_shortcut {
            self.show_completions = true;
        }
        let available_width = ui.available_width();
        egui::Frame::none()
            .stroke(egui::Stroke::new(1.0_f32, Color32::from_rgb(60, 65, 75)))
            .rounding(4.0)
            .inner_margin(4.0)
            .show(ui, |ui| {
                let mut layouter = |ui: &Ui, text: &str, wrap_width: f32| {
                    let job = highlight_sql(text, wrap_width);
                    ui.fonts(|fonts| fonts.layout_job(job))
                };
                ui.add(
                    egui::TextEdit::multiline(&mut self.sql_text)
                        .font(egui::TextStyle::Monospace)
                        .desired_width(available_width)
                        .desired_rows(6)
                        .hint_text(
                            "-- Type SQL here. Ctrl+Space opens completion; Ctrl+Enter executes.",
                        )
                        .layouter(&mut layouter),
                );
            });

        if self.show_completions {
            let prefix = trailing_word(&self.sql_text).to_lowercase();
            let mut candidates: Vec<String> = SQL_KEYWORDS.iter().map(|s| s.to_string()).collect();
            for table in tables {
                candidates.push(table.name.clone());
                candidates.push(format!("{}.{}", table.schema, table.name));
            }
            candidates.extend(columns.iter().map(|c| c.name.clone()));
            candidates.sort_by_key(|s| s.to_lowercase());
            candidates.dedup_by(|a, b| a.eq_ignore_ascii_case(b));
            candidates.retain(|s| prefix.is_empty() || s.to_lowercase().starts_with(&prefix));
            ui.horizontal(|ui| {
                ui.label(RichText::new("Complete:").strong());
                if candidates.is_empty() {
                    ui.label(RichText::new("No matches").color(Color32::GRAY));
                }
                for candidate in candidates.into_iter().take(12) {
                    if ui.small_button(&candidate).clicked() {
                        replace_trailing_word(&mut self.sql_text, &candidate);
                        self.show_completions = false;
                    }
                }
                if ui.small_button("×").clicked() {
                    self.show_completions = false;
                }
            });
        }

        ui.add_space(6.0);
        ui.separator();
        ui.add_space(4.0);
        match last_result {
            Some(Ok(res)) => {
                if let Some(table_act) =
                    render_data_table(ui, &mut self.data_state, res, false, None, &[], false)
                {
                    match table_act {
                        DataTableAction::CopyCell(text) => {
                            action = Some(SqlEditorAction::CopyCell(text))
                        }
                        DataTableAction::ExportCsv => action = Some(SqlEditorAction::ExportCsv),
                        _ => {}
                    }
                }
            }
            Some(Err(err)) => {
                egui::Frame::none()
                    .fill(Color32::from_rgb(50, 15, 15))
                    .stroke(egui::Stroke::new(1.0_f32, Color32::from_rgb(220, 50, 50)))
                    .rounding(6.0)
                    .inner_margin(12.0)
                    .show(ui, |ui| {
                        ui.label(
                            RichText::new("❌ Query Execution Error")
                                .color(Color32::LIGHT_RED)
                                .strong()
                                .size(15.0),
                        );
                        ScrollArea::vertical().max_height(150.0).show(ui, |ui| {
                            ui.label(
                                RichText::new(err)
                                    .monospace()
                                    .color(Color32::from_rgb(255, 180, 180)),
                            );
                        });
                    });
            }
            None => {
                ui.vertical_centered(|ui| {
                    ui.add_space(30.0);
                    ui.label(
                        RichText::new("No query executed yet")
                            .color(Color32::GRAY)
                            .size(14.0),
                    );
                });
            }
        }
        action
    }

    fn add_history(&mut self, query: &str) {
        self.history.retain(|q| q != query);
        self.history.push(query.to_string());
        if self.history.len() > MAX_HISTORY {
            self.history.drain(0..self.history.len() - MAX_HISTORY);
        }
    }
}

fn prepare_query_plan(query: &str) -> Option<String> {
    let query = query.trim().trim_end_matches(';').trim();
    if query.is_empty() {
        None
    } else {
        Some(format!("EXPLAIN (FORMAT TEXT)\n{query}"))
    }
}

fn trailing_word(text: &str) -> &str {
    let end = text.len();
    let start = text
        .char_indices()
        .rev()
        .find(|(_, c)| !(c.is_alphanumeric() || *c == '_' || *c == '.'))
        .map(|(i, c)| i + c.len_utf8())
        .unwrap_or(0);
    &text[start..end]
}

#[cfg(test)]
mod tests {
    use super::{ADMIN_INSIGHTS, prepare_query_plan};

    #[test]
    fn query_plan_does_not_execute_the_statement() {
        assert_eq!(
            prepare_query_plan(" SELECT * FROM users; "),
            Some("EXPLAIN (FORMAT TEXT)\nSELECT * FROM users".to_string())
        );
        assert_eq!(prepare_query_plan(" ; \n"), None);
    }

    #[test]
    fn admin_insights_cover_activity_locks_stats_and_roles() {
        assert_eq!(ADMIN_INSIGHTS.len(), 4);
        assert!(ADMIN_INSIGHTS[0].1.contains("pg_stat_activity"));
        assert!(ADMIN_INSIGHTS[1].1.contains("pg_blocking_pids"));
        assert!(ADMIN_INSIGHTS[2].1.contains("pg_stat_database"));
        assert!(ADMIN_INSIGHTS[3].1.contains("pg_roles"));
    }
}

fn replace_trailing_word(text: &mut String, completion: &str) {
    let start = text
        .char_indices()
        .rev()
        .find(|(_, c)| !(c.is_alphanumeric() || *c == '_' || *c == '.'))
        .map(|(i, c)| i + c.len_utf8())
        .unwrap_or(0);
    text.replace_range(start.., completion);
}

fn highlight_sql(text: &str, wrap_width: f32) -> LayoutJob {
    let mut job = LayoutJob::default();
    job.wrap.max_width = wrap_width;
    let mut i = 0;
    while i < text.len() {
        let rest = &text[i..];
        let (end, color) = if rest.starts_with("--") {
            (
                rest.find('\n').map(|n| i + n).unwrap_or(text.len()),
                Color32::from_rgb(115, 145, 115),
            )
        } else if rest.starts_with("'") || rest.starts_with('"') {
            let quote = rest.as_bytes()[0];
            let mut j = i + 1;
            while j < text.len() {
                let b = text.as_bytes()[j];
                j += 1;
                if b == quote {
                    if j < text.len() && text.as_bytes()[j] == quote {
                        j += 1;
                    } else {
                        break;
                    }
                }
            }
            (
                j,
                if quote == b'\'' {
                    Color32::from_rgb(215, 160, 110)
                } else {
                    Color32::from_rgb(155, 190, 220)
                },
            )
        } else {
            let ch = rest.chars().next().unwrap();
            if ch.is_ascii_alphabetic() || ch == '_' {
                let n = rest
                    .char_indices()
                    .skip(1)
                    .find(|(_, c)| !(c.is_ascii_alphanumeric() || *c == '_' || *c == '$'))
                    .map(|(n, _)| n)
                    .unwrap_or(rest.len());
                let token = &rest[..n];
                let color = if SQL_KEYWORDS.iter().any(|k| token.eq_ignore_ascii_case(k)) {
                    Color32::from_rgb(105, 170, 245)
                } else {
                    Color32::from_rgb(215, 215, 220)
                };
                (i + n, color)
            } else if ch.is_ascii_digit() {
                let n = rest
                    .char_indices()
                    .skip(1)
                    .find(|(_, c)| !(c.is_ascii_digit() || *c == '.' || *c == '_'))
                    .map(|(n, _)| n)
                    .unwrap_or(rest.len());
                (i + n, Color32::from_rgb(190, 155, 235))
            } else {
                (i + ch.len_utf8(), Color32::from_rgb(185, 190, 200))
            }
        };
        let end = end
            .max(i + text[i..].chars().next().unwrap().len_utf8())
            .min(text.len());
        let mut format = egui::TextFormat::default();
        format.font_id = egui::FontId::monospace(14.0);
        format.color = color;
        job.append(&text[i..end], 0.0, format);
        i = end;
    }
    job
}
