#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeRole {
    Primary,
    Standby,
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

        let cluster = HaCluster {
            id: "cluster-1".to_string(),
            name: "production".to_string(),
            connection_ids: vec!["primary".to_string()],
        };
        assert_eq!(cluster.connection_ids, ["primary"]);
    }
}
use serde::{Deserialize, Serialize};
