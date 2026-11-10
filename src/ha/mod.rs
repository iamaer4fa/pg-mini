use serde::{Deserialize, Serialize};
use std::time::Instant;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeRole {
    Primary,
    Standby,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PrimaryLsnFields {
    pub sent: &'static str,
    pub written: &'static str,
    pub flushed: &'static str,
    pub replayed: &'static str,
}

pub fn primary_lsn_fields(server_version_num: i32) -> PrimaryLsnFields {
    if server_version_num >= 100000 {
        PrimaryLsnFields {
            sent: "sent_lsn",
            written: "write_lsn",
            flushed: "flush_lsn",
            replayed: "replay_lsn",
        }
    } else {
        PrimaryLsnFields {
            sent: "sent_location",
            written: "write_location",
            flushed: "flush_location",
            replayed: "replay_location",
        }
    }
}

pub fn standby_replay_lsn_function(server_version_num: i32) -> Option<&'static str> {
    if server_version_num < 90600 {
        None
    } else if server_version_num >= 100000 {
        Some("pg_catalog.pg_last_wal_replay_lsn()")
    } else {
        Some("pg_catalog.pg_last_xlog_replay_location()")
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct HaMonitoringConfig {
    pub refresh_interval_seconds: u64,
    pub stale_after_seconds: u64,
    pub lag_threshold_seconds: f64,
    pub alert_on_lag: bool,
    pub alert_on_status_check_failure: bool,
}

impl Default for HaMonitoringConfig {
    fn default() -> Self {
        Self {
            refresh_interval_seconds: 15,
            stale_after_seconds: 45,
            lag_threshold_seconds: 10.0,
            alert_on_lag: false,
            alert_on_status_check_failure: true,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HaEventSeverity {
    Info,
    Warning,
    Critical,
}

#[derive(Debug, Clone)]
pub struct HaStatusChange {
    pub occurred_at: Instant,
    pub severity: HaEventSeverity,
    pub message: String,
}

impl NodeRole {
    pub fn label(self) -> &'static str {
        match self {
            Self::Primary => "Primary",
            Self::Standby => "Standby",
        }
    }

    pub fn from_recovery_state(is_in_recovery: bool) -> Self {
        if is_in_recovery {
            Self::Standby
        } else {
            Self::Primary
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct HaNodeStatus {
    pub role: NodeRole,
    pub replication: ReplicationStatus,
    pub system_identifier: Option<String>,
    pub system_identifier_error: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ReplicationStatus {
    Primary { standbys: Vec<PrimaryStandbyStatus> },
    Standby { receiver: Option<WalReceiverStatus> },
    Unknown { reason: String },
}

#[derive(Debug, Clone, PartialEq)]
pub struct PrimaryStandbyStatus {
    pub application_name: Option<String>,
    pub client_addr: Option<String>,
    pub state: Option<String>,
    pub sync_state: Option<String>,
    pub sent_lsn: Option<String>,
    pub write_lsn: Option<String>,
    pub flush_lsn: Option<String>,
    pub replay_lsn: Option<String>,
    pub write_lag_seconds: Option<f64>,
    pub flush_lag_seconds: Option<f64>,
    pub replay_lag_seconds: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WalReceiverStatus {
    pub status: Option<String>,
    pub sender_host: Option<String>,
    pub sender_port: Option<String>,
    pub slot_name: Option<String>,
    pub receive_start_lsn: Option<String>,
    pub written_lsn: Option<String>,
    pub flushed_lsn: Option<String>,
    pub latest_end_lsn: Option<String>,
    pub latest_end_time: Option<String>,
    pub replay_lsn: Option<String>,
    pub last_replay_at: Option<String>,
}

#[derive(Debug, Clone)]
pub struct HaNode {
    pub connection_id: String,
    pub name: String,
    pub host_port: String,
    pub database: String,
    pub configured_host: String,
    pub configured_port: u16,
    pub status: Option<HaNodeStatus>,
    pub error: Option<String>,
    pub last_updated: Option<std::time::Instant>,
    pub is_refreshing: bool,
    pub is_connected: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HaCluster {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub connection_ids: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::{HaCluster, NodeRole, PrimaryStandbyStatus, WalReceiverStatus};

    #[test]
    fn maps_postgres_recovery_state_to_node_role() {
        assert_eq!(NodeRole::from_recovery_state(false), NodeRole::Primary);
        assert_eq!(NodeRole::from_recovery_state(true), NodeRole::Standby);
    }

    #[test]
    fn missing_replication_values_remain_unknown() {
        let standby = PrimaryStandbyStatus {
            application_name: None,
            client_addr: None,
            state: None,
            sync_state: None,
            sent_lsn: None,
            write_lsn: None,
            flush_lsn: None,
            replay_lsn: None,
            write_lag_seconds: None,
            flush_lag_seconds: None,
            replay_lag_seconds: None,
        };
        assert!(standby.write_lag_seconds.is_none());

        let receiver = WalReceiverStatus {
            status: None,
            sender_host: None,
            sender_port: None,
            slot_name: None,
            receive_start_lsn: None,
            written_lsn: None,
            flushed_lsn: None,
            latest_end_lsn: None,
            latest_end_time: None,
            replay_lsn: None,
            last_replay_at: None,
        };
        assert!(receiver.replay_lsn.is_none());
    }

    #[test]
    fn legacy_app_config_without_clusters_deserializes() {
        let config: crate::models::AppConfig =
            serde_json::from_str(r#"{"saved_connections":[],"last_connection_id":null}"#).unwrap();
        assert!(config.ha_clusters.is_empty());
        assert_eq!(config.ha_monitoring.refresh_interval_seconds, 15);
        assert_eq!(config.ha_monitoring.stale_after_seconds, 45);

        let cluster = HaCluster {
            id: "cluster-1".to_string(),
            name: "production".to_string(),
            connection_ids: vec!["primary".to_string()],
        };
        assert_eq!(cluster.connection_ids, ["primary"]);
    }

    #[test]
    fn primary_lsn_field_names_follow_postgresql_major_version() {
        let legacy = super::primary_lsn_fields(90624);
        assert_eq!(legacy.sent, "sent_location");
        assert_eq!(legacy.written, "write_location");
        assert_eq!(legacy.flushed, "flush_location");
        assert_eq!(legacy.replayed, "replay_location");

        let modern = super::primary_lsn_fields(100000);
        assert_eq!(modern.sent, "sent_lsn");
        assert_eq!(modern.written, "write_lsn");
        assert_eq!(modern.flushed, "flush_lsn");
        assert_eq!(modern.replayed, "replay_lsn");
    }

    #[test]
    fn standby_replay_function_handles_unsupported_and_renamed_api() {
        assert_eq!(super::standby_replay_lsn_function(90525), None);
        assert_eq!(
            super::standby_replay_lsn_function(90624),
            Some("pg_catalog.pg_last_xlog_replay_location()")
        );
        assert_eq!(
            super::standby_replay_lsn_function(100000),
            Some("pg_catalog.pg_last_wal_replay_lsn()")
        );
    }
}
