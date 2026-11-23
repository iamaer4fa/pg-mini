use crate::ha::{
    HaCluster, HaEventSeverity, HaMonitoringConfig, HaNode, HaStatusChange, NodeRole,
    PrimaryStandbyStatus, ReplicationStatus, WalReceiverStatus,
};
use crate::models::ConnectionConfig;
use egui::{Color32, RichText, Ui};
use std::time::Duration;
use crate::ha::{TopologyNode, TopologyNodeState};
use std::collections::HashMap;


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

    // 1. Allocate the canvas once primaries and standbys are known
    let rows = primaries.len().max(standbys.len()).max(1) as f32;
    let box_size = egui::vec2(190.0, 56.0);
    let height = (rows * 68.0 + 24.0).max(140.0);
    let desired_size = egui::vec2(ui.available_width().max(240.0), height);
    let (rect, _response) = ui.allocate_exact_size(desired_size, egui::Sense::hover());
    let painter = ui.painter_at(rect);

    painter.rect_filled(rect, 6.0, Color32::from_rgb(16, 20, 26));
    painter.rect_stroke(rect, 6.0, egui::Stroke::new(1.0, Color32::from_rgb(55, 65, 81)));

    let left_x = rect.left() + 16.0;
    let right_x = (rect.right() - box_size.x - 16.0).max(left_x);

    let mut primary_rects: HashMap<String, egui::Rect> = HashMap::new();
    let mut standby_rects: HashMap<String, egui::Rect> = HashMap::new();

    for (idx, primary) in primaries.iter().enumerate() {
        let node_rect = egui::Rect::from_min_size(
            egui::pos2(left_x, rect.top() + 14.0 + idx as f32 * 68.0),
            box_size,
        );
        primary_rects.insert(primary.connection_id.clone(), node_rect);
        let topo_node = TopologyNode {
            connection_id: primary.connection_id.clone(),
            label: primary.name.clone(),
            endpoint: primary.host_port.clone(),
            role_label: "Primary".to_string(),
            state: TopologyNodeState::PrimaryHealthy,
            system_identifier: primary
                .status
                .as_ref()
                .and_then(|s| s.system_identifier.clone()),
        };
        draw_node_box(&painter, node_rect, &topo_node, false);
    }

    for (idx, standby) in standbys.iter().enumerate() {
        let node_rect = egui::Rect::from_min_size(
            egui::pos2(right_x, rect.top() + 14.0 + idx as f32 * 68.0),
            box_size,
        );
        standby_rects.insert(standby.connection_id.clone(), node_rect);
        let topo_node = TopologyNode {
            connection_id: standby.connection_id.clone(),
            label: standby.name.clone(),
            endpoint: standby.host_port.clone(),
            role_label: "Standby".to_string(),
            state: TopologyNodeState::StandbyHealthy,
            system_identifier: standby
                .status
                .as_ref()
                .and_then(|s| s.system_identifier.clone()),
        };
        draw_node_box(&painter, node_rect, &topo_node, false);
    }

    for standby in &standbys {
        let Some(receiver) = standby.status.as_ref().and_then(|status| {
            if let ReplicationStatus::Standby { receiver } = &status.replication {
                receiver.as_ref()
            } else {
                None
            }
        }) else {
            continue;
        };

        let upstream_host = receiver.sender_host.as_deref();
        let upstream_port = receiver
            .sender_port
            .as_deref()
            .and_then(|port| port.parse::<u16>().ok());

        for primary in primaries.iter().filter(|primary| {
            upstream_matches_primary(
                upstream_host,
                upstream_port,
                &primary.configured_host,
                primary.configured_port,
            )
        }) {
            if let (Some(&from_rect), Some(&to_rect)) = (
                primary_rects.get(&primary.connection_id),
                standby_rects.get(&standby.connection_id),
            ) {
                let start = from_rect.right_center();
                let end = to_rect.left_center();
                let stroke = egui::Stroke::new(2.0, Color32::from_rgb(100, 220, 130));
                painter.line_segment([start, end], stroke);
                draw_arrow_head(&painter, start, end, stroke.color);

                let midpoint = egui::pos2((start.x + end.x) * 0.5, (start.y + end.y) * 0.5 - 10.0);
                let edge_label = format!(
                    "{} · {}",
                    receiver.status.as_deref().unwrap_or("unknown"),
                    receiver.slot_name.as_deref().unwrap_or("no slot"),
                );
                painter.text(
                    midpoint,
                    egui::Align2::CENTER_BOTTOM,
                    edge_label,
                    egui::FontId::proportional(11.0),
                    Color32::LIGHT_GREEN,
                );
            }
        }
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

fn draw_node_box(
    painter: &egui::Painter,
    rect: egui::Rect,
    node: &TopologyNode,
    is_selected: bool,
) {
    let accent = match node.state {
        TopologyNodeState::PrimaryHealthy => Color32::from_rgb(100, 220, 130),
        TopologyNodeState::StandbyHealthy => Color32::from_rgb(100, 180, 255),
        TopologyNodeState::Stale => Color32::YELLOW,
        TopologyNodeState::Error => Color32::LIGHT_RED,
        TopologyNodeState::Disconnected => Color32::GRAY,
    };
    let stroke_width: f32 = if is_selected { 2.5 } else { 1.25 };

    painter.rect_filled(rect, 6.0, Color32::from_rgb(24, 28, 34));
    painter.rect_stroke(rect, 6.0, egui::Stroke::new(stroke_width, accent));
    painter.text(
        rect.left_top() + egui::vec2(10.0, 8.0),
        egui::Align2::LEFT_TOP,
        &node.label,
        egui::FontId::proportional(14.0),
        Color32::WHITE,
    );
    painter.text(
        rect.right_top() + egui::vec2(-10.0, 9.0),
        egui::Align2::RIGHT_TOP,
        &node.role_label,
        egui::FontId::proportional(12.0),
        accent,
    );
    painter.text(
        rect.left_top() + egui::vec2(10.0, 28.0),
        egui::Align2::LEFT_TOP,
        &node.endpoint,
        egui::FontId::proportional(11.5),
        Color32::GRAY,
    );
}

fn draw_arrow_head(painter: &egui::Painter, start: egui::Pos2, end: egui::Pos2, color: Color32) {
    let dir = (end - start).normalized();
    if !dir.x.is_finite() || !dir.y.is_finite() {
        return;
    }
    let perp = egui::vec2(-dir.y, dir.x);
    let tip = end;
    let left = tip - dir * 10.0 + perp * 5.0;
    let right = tip - dir * 10.0 - perp * 5.0;
    painter.add(egui::Shape::convex_polygon(
        vec![tip, left, right],
        color,
        egui::Stroke::NONE,
    ));
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
