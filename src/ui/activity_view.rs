use crate::activity::ActivitySnapshot;
use egui::{Color32, RichText, Ui};
use std::time::Instant;

const LONG_QUERY_SECONDS: f64 = 5.0;

fn is_long_running_query(state: &str, duration: Option<f64>) -> bool {
    state == "active" && duration.is_some_and(|duration| duration >= LONG_QUERY_SECONDS)
}

pub fn render_activity_view(
    ui: &mut Ui,
    snapshot: Option<&ActivitySnapshot>,
    is_busy: bool,
    updated_at: Option<Instant>,
) -> bool {
    let mut refresh = false;
    ui.horizontal(|ui| {
        ui.heading("Activity");
        ui.label(
            RichText::new("Sessions, waits, locks and storage usage")
                .small()
                .color(Color32::GRAY),
        );
        if let Some(updated_at) = updated_at {
            ui.label(
                RichText::new(format!("Updated {}s ago", updated_at.elapsed().as_secs()))
                    .small()
                    .color(Color32::GRAY),
            );
        } else {
            ui.label(
                RichText::new("Loading activity...")
                    .small()
                    .color(Color32::GRAY),
            );
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui
                .add_enabled(!is_busy, egui::Button::new("↻ Refresh"))
                .clicked()
            {
                refresh = true;
            }
        });
    });
    ui.separator();

    let Some(snapshot) = snapshot else {
        ui.centered_and_justified(|ui| {
            ui.label(if is_busy {
                "Loading server activity..."
            } else {
                "Activity data is not available yet."
            });
        });
        return refresh;
    };

    ui.horizontal_wrapped(|ui| {
        metric_card(ui, "Sessions", snapshot.sessions.len().to_string());
        metric_card(
            ui,
            "Long-running",
            snapshot
                .sessions
                .iter()
                .filter(|session| is_long_running_query(&session.state, session.duration_seconds))
                .count()
                .to_string(),
        );
        metric_card(ui, "Blocked", snapshot.locks.len().to_string());
        metric_card(ui, "Databases", snapshot.database_sizes.len().to_string());
        metric_card(ui, "Tables tracked", snapshot.table_sizes.len().to_string());
    });

    egui::ScrollArea::vertical()
        .auto_shrink([false; 2])
        .show(ui, |ui| {
            section_header(ui, "Long-running queries (5 seconds or more)");
            let long_queries: Vec<_> = snapshot
                .sessions
                .iter()
                .filter(|session| is_long_running_query(&session.state, session.duration_seconds))
                .collect();
            if long_queries.is_empty() {
                muted_row(ui, "No long-running queries detected.");
            } else {
                egui::Grid::new("activity_long_queries")
                    .striped(true)
                    .show(ui, |ui| {
                        for label in ["PID", "User / database", "Duration", "Query"] {
                            ui.label(RichText::new(label).strong());
                        }
                        ui.end_row();
                        for session in long_queries {
                            ui.label(session.pid.to_string());
                            ui.label(format!("{}/{}", session.username, session.database));
                            ui.label(format_duration(session.duration_seconds));
                            query_cell(ui, &session.query, true);
                            ui.end_row();
                        }
                    });
            }

            section_header(ui, "Active sessions");
            if snapshot.sessions.is_empty() {
                muted_row(ui, "No other sessions are visible to the connected role.");
            } else {
                egui::Grid::new("activity_sessions")
                    .striped(true)
                    .show(ui, |ui| {
                        for label in [
                            "PID",
                            "User",
                            "Database",
                            "State",
                            "Wait event",
                            "Duration",
                            "Application / client",
                            "Query",
                        ] {
                            ui.label(RichText::new(label).strong());
                        }
                        ui.end_row();
                        for session in &snapshot.sessions {
                            ui.label(session.pid.to_string());
                            ui.label(&session.username);
                            ui.label(&session.database);
                            ui.label(&session.state);
                            ui.label(&session.wait_event);
                            ui.label(format_duration(session.duration_seconds));
                            ui.label(format!("{} / {}", session.application, session.client_addr));
                            query_cell(ui, &session.query, false);
                            ui.end_row();
                        }
                    });
            }

            section_header(ui, "Blocked sessions and locks");
            if snapshot.locks.is_empty() {
                muted_row(ui, "No blocked sessions detected.");
            } else {
                egui::Grid::new("activity_locks")
                    .striped(true)
                    .show(ui, |ui| {
                        for label in [
                            "Blocked PID",
                            "Blocked user / database",
                            "Wait duration",
                            "Blocking PID / user",
                            "Blocked query",
                            "Blocking query",
                        ] {
                            ui.label(RichText::new(label).strong());
                        }
                        ui.end_row();
                        for lock in &snapshot.locks {
                            ui.label(lock.blocked_pid.to_string());
                            ui.label(format!("{}/{}", lock.blocked_user, lock.database));
                            ui.label(format_duration(lock.wait_seconds));
                            ui.label(format!("{}/{}", lock.blocking_pid, lock.blocking_user));
                            query_cell(ui, &lock.blocked_query, false);
                            query_cell(ui, &lock.blocking_query, false);
                            ui.end_row();
                        }
                    });
            }

            ui.columns(2, |columns| {
                section_header(&mut columns[0], "Largest databases");
                render_sizes(
                    &mut columns[0],
                    "activity_database_sizes",
                    &snapshot.database_sizes,
                );
                section_header(&mut columns[1], "Largest tables (including indexes)");
                render_sizes(
                    &mut columns[1],
                    "activity_table_sizes",
                    &snapshot.table_sizes,
                );
            });
        });

    refresh
}

fn metric_card(ui: &mut Ui, title: &str, value: String) {
    egui::Frame::group(ui.style())
        .inner_margin(egui::Margin::symmetric(12.0, 8.0))
        .show(ui, |ui| {
            ui.vertical(|ui| {
                ui.label(RichText::new(title).small().color(Color32::GRAY));
                ui.label(RichText::new(value).strong().size(18.0));
            });
        });
}

fn section_header(ui: &mut Ui, title: &str) {
    ui.add_space(12.0);
    ui.label(RichText::new(title).strong().size(15.0));
    ui.separator();
}

fn muted_row(ui: &mut Ui, text: &str) {
    ui.label(RichText::new(text).color(Color32::GRAY));
}

fn query_cell(ui: &mut Ui, query: &str, is_long_running: bool) {
    let preview = query.chars().take(180).collect::<String>();
    let text = if query.chars().count() > 180 {
        format!("{preview}…")
    } else {
        preview
    };
    let color = if is_long_running {
        Color32::LIGHT_RED
    } else {
        ui.visuals().text_color()
    };
    ui.label(RichText::new(text).monospace().color(color))
        .on_hover_text(query);
}

fn render_sizes(ui: &mut Ui, id: &str, sizes: &[crate::activity::SizeEntry]) {
    if sizes.is_empty() {
        muted_row(ui, "No size information available.");
        return;
    }
    egui::Grid::new(id).striped(true).show(ui, |ui| {
        for item in sizes {
            ui.label(&item.name);
            ui.label(
                RichText::new(format!("{} ({})", item.size, item.bytes))
                    .monospace()
                    .color(Color32::LIGHT_BLUE),
            );
            ui.end_row();
        }
    });
}

fn format_duration(seconds: Option<f64>) -> String {
    seconds
        .map(|seconds| {
            if seconds >= 60.0 {
                format!("{:.1} min", seconds / 60.0)
            } else {
                format!("{seconds:.1} s")
            }
        })
        .unwrap_or_else(|| "—".to_string())
}

#[cfg(test)]
mod tests {
    use super::is_long_running_query;

    #[test]
    fn long_running_filter_requires_active_queries_at_least_five_seconds() {
        assert!(is_long_running_query("active", Some(5.0)));
        assert!(!is_long_running_query("active", Some(4.99)));
        assert!(!is_long_running_query("idle", Some(50.0)));
        assert!(!is_long_running_query("active", None));
    }
}
