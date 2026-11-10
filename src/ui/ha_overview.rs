use crate::ha::{
    HaCluster, HaEventSeverity, HaMonitoringConfig, HaNode, HaStatusChange, NodeRole,
    PrimaryStandbyStatus, ReplicationStatus, WalReceiverStatus,
};
use crate::models::ConnectionConfig;
use egui::{Color32, RichText, Ui};
use std::time::Duration;

pub fn render_ha_overview(
    ui: &mut Ui,
    clusters: &mut Vec<HaCluster>,
    saved_connections: &[ConnectionConfig],
    nodes: &[HaNode],
    is_busy: bool,
    monitoring: &mut HaMonitoringConfig,
    events: &std::collections::VecDeque<HaStatusChange>,
    new_cluster_name: &mut String,
    selected_connection: &mut Option<String>,
) -> (bool, bool) {
    let mut refresh = false;
    let mut config_changed = false;
    ui.horizontal(|ui| {
        ui.heading("High Availability Overview");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui
                .add_enabled(!is_busy, egui::Button::new("↻ Refresh"))
                .clicked()
            {
                refresh = true;
            }
        });
    });
    ui.label(
        RichText::new(
            "Group saved connections into clusters. Membership is verified only when PostgreSQL metadata supports it.",
        )
        .small()
        .color(Color32::GRAY),
    );
    ui.separator();

    ui.collapsing("Monitoring settings", |ui| {
        ui.horizontal(|ui| {
            ui.label("Refresh every");
            let before = monitoring.refresh_interval_seconds;
            ui.add(
                egui::DragValue::new(&mut monitoring.refresh_interval_seconds)
                    .range(5..=300)
                    .suffix(" s"),
            );
            config_changed |= before != monitoring.refresh_interval_seconds;
            ui.label("Mark stale after");
            let before = monitoring.stale_after_seconds;
            ui.add(
                egui::DragValue::new(&mut monitoring.stale_after_seconds)
                    .range(15..=3600)
                    .suffix(" s"),
            );
            config_changed |= before != monitoring.stale_after_seconds;
        });
        ui.horizontal(|ui| {
            let before = monitoring.alert_on_lag;
            ui.checkbox(
                &mut monitoring.alert_on_lag,
                "Alert when reported lag exceeds",
            );
            config_changed |= before != monitoring.alert_on_lag;
            let before = monitoring.lag_threshold_seconds;
            ui.add_enabled(
                monitoring.alert_on_lag,
                egui::DragValue::new(&mut monitoring.lag_threshold_seconds)
                    .range(0.1..=3600.0)
                    .speed(0.5)
                    .suffix(" s"),
            );
            config_changed |= before != monitoring.lag_threshold_seconds;
        });
        let before = monitoring.alert_on_status_check_failure;
        ui.checkbox(
            &mut monitoring.alert_on_status_check_failure,
            "Record an alert when an HA status check fails",
        );
        config_changed |= before != monitoring.alert_on_status_check_failure;
        monitoring.refresh_interval_seconds = monitoring.refresh_interval_seconds.clamp(5, 300);
        monitoring.stale_after_seconds = monitoring.stale_after_seconds.clamp(15, 3600);
        if !monitoring.lag_threshold_seconds.is_finite() {
            monitoring.lag_threshold_seconds = 10.0;
        }
        monitoring.lag_threshold_seconds = monitoring.lag_threshold_seconds.clamp(0.1, 3600.0);
    });

    ui.horizontal(|ui| {
        ui.label("New cluster:");
        ui.add(
            egui::TextEdit::singleline(new_cluster_name)
                .hint_text("Cluster name")
                .desired_width(180.0),
        );
        if ui
            .add_enabled(
                !new_cluster_name.trim().is_empty(),
                egui::Button::new("Create cluster"),
            )
            .clicked()
        {
            clusters.push(HaCluster {
                id: uuid::Uuid::new_v4().to_string(),
                name: new_cluster_name.trim().to_string(),
                connection_ids: Vec::new(),
            });
            new_cluster_name.clear();
            *selected_connection = None;
            config_changed = true;
        }
    });

    if clusters.is_empty() {
        ui.add_space(10.0);
        ui.label("Create a cluster, then associate saved server connections with it.");
        if saved_connections.is_empty() {
            ui.label("No saved connections are available.");
        }
    } else {
        egui::ScrollArea::vertical()
            .auto_shrink([false; 2])
            .show(ui, |ui| {
                let mut remove_cluster = None;
                let mut remove_node = None;
                for cluster in clusters.iter_mut() {
                    ui.add_space(10.0);
                    ui.horizontal(|ui| {
                        ui.heading(&cluster.name);
                        if ui.small_button("Remove cluster").clicked() {
                            remove_cluster = Some(cluster.id.clone());
                        }
                    });

                    let available: Vec<_> = saved_connections
                        .iter()
                        .filter(|connection| !cluster.connection_ids.contains(&connection.id))
                        .collect();
                    if !available.is_empty() {
                        if selected_connection
                            .as_ref()
                            .is_none_or(|selected| !available.iter().any(|c| &c.id == selected))
                        {
                            *selected_connection = Some(available[0].id.clone());
                        }
                        ui.horizontal(|ui| {
                            let selected_label = selected_connection
                                .as_ref()
                                .and_then(|id| available.iter().find(|c| &c.id == id))
                                .map(|connection| connection.name.as_str())
                                .unwrap_or("Select saved connection");
                            egui::ComboBox::from_id_salt(("ha_cluster_connection", &cluster.id))
                                .selected_text(selected_label)
                                .show_ui(ui, |ui| {
                                    for connection in &available {
                                        ui.selectable_value(
                                            selected_connection,
                                            Some(connection.id.clone()),
                                            &connection.name,
                                        );
                                    }
                                });
                            if ui.button("Add node").clicked() {
                                if let Some(connection_id) = selected_connection.clone() {
                                    if available
                                        .iter()
                                        .any(|connection| connection.id == connection_id)
                                    {
                                        cluster.connection_ids.push(connection_id);
                                        config_changed = true;
                                    }
                                }
                            }
                        });
                    } else {
                        ui.label(
                            RichText::new("All saved connections are already in this cluster.")
                                .small()
                                .color(Color32::GRAY),
                        );
                    }

                    if cluster.connection_ids.is_empty() {
                        ui.label("No nodes have been associated with this cluster.");
                    }
                    for connection_id in &cluster.connection_ids {
                        if let Some(node) = nodes
                            .iter()
                            .find(|node| &node.connection_id == connection_id)
                        {
                            render_node(ui, node, monitoring.stale_after_seconds);
                        } else if let Some(connection) = saved_connections
                            .iter()
                            .find(|connection| &connection.id == connection_id)
                        {
                            ui.label(format!(
                                "{} — connection profile unavailable",
                                connection.name
                            ));
                        }
                        ui.horizontal(|ui| {
                            if ui.small_button("Remove node").clicked() {
                                remove_node = Some((cluster.id.clone(), connection_id.clone()));
                            }
                        });
                    }

                    render_relationships(ui, cluster, nodes, monitoring.stale_after_seconds);
                    ui.separator();
                }

                if let Some(cluster_id) = remove_cluster {
                    clusters.retain(|cluster| cluster.id != cluster_id);
                    config_changed = true;
                }
                if let Some((cluster_id, connection_id)) = remove_node {
                    if let Some(cluster) = clusters.iter_mut().find(|c| c.id == cluster_id) {
                        cluster.connection_ids.retain(|id| id != &connection_id);
                        config_changed = true;
                    }
                }
            });
    }

    ui.label(
        RichText::new(
            "A saved profile that is not connected is shown as Not connected; pg-mini has not tested reachability. Topology is never inferred from hostnames alone.",
        )
        .small()
        .color(Color32::GRAY),
    );
    ui.separator();
    ui.label(RichText::new("Recent status changes").strong());
    if events.is_empty() {
        ui.label(RichText::new("No status changes recorded this session.").color(Color32::GRAY));
    } else {
        for event in events.iter().take(20) {
            let color = match event.severity {
                HaEventSeverity::Info => Color32::LIGHT_BLUE,
                HaEventSeverity::Warning => Color32::YELLOW,
                HaEventSeverity::Critical => Color32::LIGHT_RED,
            };
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(format!("{}s ago", event.occurred_at.elapsed().as_secs()))
                        .small()
                        .color(Color32::GRAY),
                );
                ui.label(RichText::new(&event.message).color(color));
            });
        }
    }
    ui.label(
        RichText::new(
            "pg-mini is a desktop monitor. These alerts and history are available only while the application is running and are not a substitute for server-side monitoring.",
        )
        .small()
        .color(Color32::GRAY),
    );
    (refresh, config_changed)
}

fn render_node(ui: &mut Ui, node: &HaNode, stale_after_seconds: u64) {
    ui.group(|ui| {
        ui.horizontal(|ui| {
            ui.label(RichText::new(&node.name).strong());
            ui.label(format!("{} / {}", node.host_port, node.database));
            if node.is_connected && node.error.is_none() {
                ui.label(RichText::new("Connected").color(Color32::LIGHT_GREEN));
            } else if node.is_connected {
                ui.label(RichText::new("Status check failed").color(Color32::YELLOW));
            } else {
                ui.label(RichText::new("Not connected").color(Color32::GRAY));
            }
            if let Some(updated) = node.last_updated {
                let age = updated.elapsed();
                if age >= Duration::from_secs(stale_after_seconds) {
                    ui.label(
                        RichText::new(format!("STALE — updated {}s ago", age.as_secs()))
                            .small()
                            .color(Color32::YELLOW),
                    );
                } else {
                    ui.label(
                        RichText::new(format!("Updated {}s ago", age.as_secs()))
                            .small()
                            .color(Color32::GRAY),
                    );
                }
            } else if node.is_refreshing {
                ui.label(RichText::new("Checking...").small().color(Color32::YELLOW));
            } else {
                ui.label(RichText::new("Not checked").small().color(Color32::GRAY));
            }
        });
        if let Some(error) = &node.error {
            ui.label(RichText::new(error).color(Color32::YELLOW));
        }
        match &node.status {
            Some(status) => {
                let role_color = match status.role {
                    NodeRole::Primary => Color32::from_rgb(100, 220, 130),
                    NodeRole::Standby => Color32::from_rgb(100, 180, 255),
                };
                ui.label(
                    RichText::new(status.role.label())
                        .color(role_color)
                        .strong(),
                );
                if let Some(identifier) = &status.system_identifier {
                    ui.label(format!("Cluster system identifier: {identifier}"));
                } else if let Some(error) = &status.system_identifier_error {
                    ui.label(
                        RichText::new(format!("Cluster identifier unknown: {error}"))
                            .small()
                            .color(Color32::YELLOW),
                    );
                }
                match &status.replication {
                    ReplicationStatus::Primary { standbys } => {
                        primary_standby_details(ui, standbys);
                    }
                    ReplicationStatus::Standby { receiver } => {
                        if let Some(receiver) = receiver {
                            wal_receiver_details(ui, receiver);
                        } else {
                            ui.label("WAL receiver: none reported");
                        }
                    }
                    ReplicationStatus::Unknown { reason } => {
                        ui.label(
                            RichText::new(format!("Replication status unknown: {reason}"))
                                .color(Color32::YELLOW),
                        );
                    }
                }
            }
            None if node.is_connected => {
                ui.label(RichText::new("Role unknown").color(Color32::YELLOW));
            }
            None => {
                ui.label(RichText::new("Role unknown").color(Color32::GRAY));
            }
        }
    });
}

fn render_relationships(
    ui: &mut Ui,
    cluster: &HaCluster,
    nodes: &[HaNode],
    stale_after_seconds: u64,
) {
    ui.label(RichText::new("Topology").strong());
    let configured_members: Vec<_> = cluster
        .connection_ids
        .iter()
        .filter_map(|id| nodes.iter().find(|node| &node.connection_id == id))
        .collect();
    let members: Vec<_> = configured_members
        .iter()
        .copied()
        .filter(|node| {
            node.is_connected
                && node.error.is_none()
                && node.last_updated.is_some_and(|updated| {
                    updated.elapsed() < Duration::from_secs(stale_after_seconds)
                })
        })
        .collect();
    let primaries: Vec<_> = members
        .iter()
        .copied()
        .filter(|node| {
            node.status
                .as_ref()
                .is_some_and(|s| s.role == NodeRole::Primary)
        })
        .collect();
    let standbys: Vec<_> = members
        .iter()
        .copied()
        .filter(|node| {
            node.status
                .as_ref()
                .is_some_and(|s| s.role == NodeRole::Standby)
        })
        .collect();

    if standbys.is_empty() {
        let configured_standbys = configured_members.iter().any(|node| {
            node.status
                .as_ref()
                .is_some_and(|status| status.role == NodeRole::Standby)
        });
        if configured_standbys {
            ui.label(
                "Standby topology is stale or disconnected; current relationship is unverified.",
            );
        } else {
            ui.label("No connected standby is currently identified.");
        }
        return;
    }
    for standby in standbys {
        let receiver = standby.status.as_ref().and_then(|status| {
            if let ReplicationStatus::Standby { receiver } = &status.replication {
                receiver.as_ref()
            } else {
                None
            }
        });
        let Some(receiver) = receiver else {
            ui.label(format!(
                "{} — upstream relationship unknown (no WAL receiver metadata)",
                standby.name
            ));
            continue;
        };
        let upstream_host = receiver.sender_host.as_deref();
        let upstream_port = receiver
            .sender_port
            .as_deref()
            .and_then(|port| port.parse::<u16>().ok());
        let matches: Vec<_> = primaries
            .iter()
            .copied()
            .filter(|primary| {
                upstream_matches_primary(
                    upstream_host,
                    upstream_port,
                    &primary.configured_host,
                    primary.configured_port,
                )
            })
            .collect();

        match matches.as_slice() {
            [primary] => {
                let primary_id = primary
                    .status
                    .as_ref()
                    .and_then(|status| status.system_identifier.as_deref());
                let standby_id = standby
                    .status
                    .as_ref()
                    .and_then(|status| status.system_identifier.as_deref());
                if let (Some(primary_id), Some(standby_id)) = (primary_id, standby_id) {
                    if primary_id != standby_id {
                        ui.label(
                            RichText::new(format!(
                                "{} !→ {} — system identifiers differ; cluster membership conflict",
                                primary.name, standby.name
                            ))
                            .color(Color32::LIGHT_RED),
                        );
                        continue;
                    }
                }
                ui.label(
                    RichText::new(format!(
                        "{} → {} — upstream endpoint matches PostgreSQL receiver metadata{}",
                        primary.name,
                        standby.name,
                        if primary_id.is_some() && standby_id.is_some() {
                            " and system identifiers match"
                        } else {
                            "; system identifier comparison unavailable"
                        }
                    ))
                    .color(Color32::LIGHT_GREEN),
                );
            }
            [] => {
                let same_cluster = standby
                    .status
                    .as_ref()
                    .and_then(|status| status.system_identifier.as_deref())
                    .is_some_and(|standby_id| {
                        primaries.iter().any(|primary| {
                            primary
                                .status
                                .as_ref()
                                .and_then(|status| status.system_identifier.as_deref())
                                == Some(standby_id)
                        })
                    });
                ui.label(
                    RichText::new(if same_cluster {
                        format!(
                            "{} — system identifier matches a primary, but upstream endpoint is not verified",
                            standby.name
                        )
                    } else if primaries.is_empty() {
                        format!("{} — no connected primary available to verify upstream", standby.name)
                    } else {
                        format!(
                            "{} — upstream endpoint does not match a configured primary; relationship unverified",
                            standby.name
                        )
                    })
                    .color(Color32::YELLOW),
                );
            }
            _ => {
                ui.label(
                    RichText::new(format!(
                        "{} — upstream endpoint matches multiple configured primaries; relationship ambiguous",
                        standby.name
                    ))
                    .color(Color32::YELLOW),
                );
            }
        };
    }
}

fn primary_standby_details(ui: &mut Ui, standbys: &[PrimaryStandbyStatus]) {
    ui.label(format!(
        "Primary reports {} standby connection(s)",
        standbys.len()
    ));
    for standby in standbys {
        ui.label(format!(
            "{} / {} — state: {}, sync: {}",
            value_or_unknown(standby.application_name.as_deref()),
            value_or_unknown(standby.client_addr.as_deref()),
            value_or_unknown(standby.state.as_deref()),
            value_or_unknown(standby.sync_state.as_deref())
        ));
        detail_row(ui, "Sent LSN", standby.sent_lsn.as_deref());
        detail_row(ui, "Written LSN", standby.write_lsn.as_deref());
        detail_row(ui, "Flushed LSN", standby.flush_lsn.as_deref());
        detail_row(ui, "Replayed LSN", standby.replay_lsn.as_deref());
        let write_lag = format_lag(standby.write_lag_seconds);
        let flush_lag = format_lag(standby.flush_lag_seconds);
        let replay_lag = format_lag(standby.replay_lag_seconds);
        detail_row(ui, "Write lag", write_lag.as_deref());
        detail_row(ui, "Flush lag", flush_lag.as_deref());
        detail_row(ui, "Replay lag", replay_lag.as_deref());
    }
}

fn wal_receiver_details(ui: &mut Ui, receiver: &WalReceiverStatus) {
    ui.label(format!(
        "WAL receiver: {}",
        value_or_unknown(receiver.status.as_deref())
    ));
    detail_row(ui, "Sender", receiver.sender_host.as_deref());
    detail_row(ui, "Sender port", receiver.sender_port.as_deref());
    detail_row(ui, "Replication slot", receiver.slot_name.as_deref());
    detail_row(
        ui,
        "Latest received LSN",
        receiver.latest_end_lsn.as_deref(),
    );
    detail_row(
        ui,
        "Latest received at",
        receiver.latest_end_time.as_deref(),
    );
    detail_row(ui, "Replayed LSN", receiver.replay_lsn.as_deref());
    detail_row(
        ui,
        "Last replayed transaction",
        receiver.last_replay_at.as_deref(),
    );
}

fn detail_row(ui: &mut Ui, label: &str, value: Option<&str>) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(label).small().color(Color32::GRAY));
        ui.label(value_or_unknown(value));
    });
}

fn value_or_unknown(value: Option<&str>) -> &str {
    value.filter(|value| !value.is_empty()).unwrap_or("Unknown")
}

fn format_lag(seconds: Option<f64>) -> Option<String> {
    seconds.map(|seconds| format!("{seconds:.3} s"))
}

fn upstream_matches_primary(
    upstream_host: Option<&str>,
    upstream_port: Option<u16>,
    primary_host: &str,
    primary_port: u16,
) -> bool {
    upstream_host.is_some_and(|host| {
        host.trim().eq_ignore_ascii_case(primary_host.trim()) && upstream_port == Some(primary_port)
    })
}

#[cfg(test)]
mod tests {
    use super::upstream_matches_primary;

    #[test]
    fn topology_requires_upstream_metadata_to_match_profile_endpoint() {
        assert!(upstream_matches_primary(
            Some("db-primary.internal"),
            Some(5432),
            "DB-PRIMARY.INTERNAL",
            5432
        ));
        assert!(!upstream_matches_primary(
            Some("db-primary.internal"),
            Some(5432),
            "10.0.0.12",
            5432
        ));
        assert!(!upstream_matches_primary(
            Some("db-primary.internal"),
            Some(5433),
            "db-primary.internal",
            5432
        ));
        assert!(!upstream_matches_primary(
            None,
            Some(5432),
            "db-primary.internal",
            5432
        ));
    }
}
