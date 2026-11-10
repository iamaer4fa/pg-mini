use crate::models::ConnectionConfig;
use std::collections::{HashMap, HashSet};
use std::net::IpAddr;

pub const PROVISIONING_POSTGRES_MIN: i32 = 170000;
pub const PROVISIONING_POSTGRES_MAX: i32 = 190000;
pub const REPLICATION_ROLE_DEFAULT: &str = "pgmini_replicator";
pub const REPLICATION_SLOT_DEFAULT: &str = "pgmini_standby";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProvisioningPreflight {
    pub server_version_num: i32,
    pub server_version: String,
    pub is_in_recovery: bool,
    pub system_identifier: Option<String>,
    pub system_identifier_error: Option<String>,
    pub wal_level: String,
    pub max_wal_senders: i32,
    pub max_replication_slots: i32,
    pub listen_addresses: String,
    pub ssl_enabled: bool,
    pub is_superuser: bool,
    pub can_create_role: bool,
    pub can_replication: bool,
}

#[derive(Debug)]
pub struct HaProvisioningWizard {
    pub primary_connection_id: Option<String>,
    pub standby_connection_id: Option<String>,
    pub replication_role: String,
    pub allowed_cidr: String,
    pub use_physical_slot: bool,
    pub slot_name: String,
    pub preflights: HashMap<String, Result<ProvisioningPreflight, String>>,
    pub pending_preflights: HashSet<String>,
}

impl Default for HaProvisioningWizard {
    fn default() -> Self {
        Self {
            primary_connection_id: None,
            standby_connection_id: None,
            replication_role: REPLICATION_ROLE_DEFAULT.to_string(),
            allowed_cidr: String::new(),
            use_physical_slot: false,
            slot_name: REPLICATION_SLOT_DEFAULT.to_string(),
            preflights: HashMap::new(),
            pending_preflights: HashSet::new(),
        }
    }
}

impl HaProvisioningWizard {
    pub fn begin_preflight(&mut self, connection_id: String) {
        self.preflights.remove(&connection_id);
        self.pending_preflights.insert(connection_id);
    }

    pub fn finish_preflight(
        &mut self,
        connection_id: String,
        result: Result<ProvisioningPreflight, String>,
    ) {
        self.pending_preflights.remove(&connection_id);
        self.preflights.insert(connection_id, result);
    }
}

impl ProvisioningPreflight {
    pub fn supports_provisioning(&self) -> bool {
        (PROVISIONING_POSTGRES_MIN..PROVISIONING_POSTGRES_MAX).contains(&self.server_version_num)
    }

    pub fn version_label(&self) -> String {
        self.server_version
            .split_whitespace()
            .nth(1)
            .unwrap_or(&self.server_version)
            .to_string()
    }

    pub fn is_primary(&self) -> bool {
        !self.is_in_recovery
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplicationSetupPlan {
    pub primary_role_sql: String,
    pub set_password_command: String,
    pub primary_settings: String,
    pub pg_hba_rule: String,
    pub standby_command: String,
    pub slot_warning: Option<String>,
}

pub fn generate_setup_plan(
    primary: &ConnectionConfig,
    standby: &ConnectionConfig,
    primary_preflight: &ProvisioningPreflight,
    standby_preflight: &ProvisioningPreflight,
    replication_role: &str,
    allowed_cidr: &str,
    use_physical_slot: bool,
    slot_name: &str,
) -> Result<ReplicationSetupPlan, String> {
    if primary.id == standby.id {
        return Err("Choose two different saved connections.".to_string());
    }
    if same_endpoint(primary, standby) {
        return Err(
            "Primary and standby profiles must use different host and port values.".to_string(),
        );
    }
    if !primary_preflight.supports_provisioning() || !standby_preflight.supports_provisioning() {
        return Err("Provisioning is supported only on PostgreSQL 17 and 18.".to_string());
    }
    if primary_preflight.server_version_num / 10000 != standby_preflight.server_version_num / 10000
    {
        return Err("Physical replication requires the primary and standby to use the same PostgreSQL major version.".to_string());
    }
    if !primary_preflight.is_primary() {
        return Err(
            "The selected primary connection is currently in recovery; select a primary node."
                .to_string(),
        );
    }
    if !primary_preflight.ssl_enabled {
        return Err(
            "Enable TLS on the primary before generating the hostssl replication access rule."
                .to_string(),
        );
    }
    validate_role_name(replication_role)?;
    validate_cidr(allowed_cidr)?;
    if use_physical_slot {
        validate_slot_name(slot_name)?;
    }

    let (slot_setup, slot_warning) = if use_physical_slot {
        (
            format!("--create-slot --slot={slot_name} "),
            Some(
                "A disconnected standby can cause this slot to retain WAL indefinitely and fill primary storage. Monitor retained WAL and remove the slot only after confirming it is no longer needed."
                    .to_string(),
            ),
        )
    } else {
        (String::new(), None)
    };

    let primary_role_sql = format!("CREATE ROLE \"{replication_role}\" WITH LOGIN REPLICATION;");
    let set_password_command = format!("\\password {replication_role}");
    let mut settings = Vec::new();
    if primary_preflight.wal_level != "replica" && primary_preflight.wal_level != "logical" {
        settings.push(
            "Set wal_level = 'replica' (a PostgreSQL restart is required if this changes)."
                .to_string(),
        );
    }
    if primary_preflight.max_wal_senders < 3 {
        settings.push(format!(
            "Increase max_wal_senders from {} to at least 3 to allow streaming and base-backup connections; restart PostgreSQL after changing it.",
            primary_preflight.max_wal_senders
        ));
    }
    if !primary_preflight.is_superuser {
        settings.push(
            "Apply the CREATE ROLE statement as a PostgreSQL superuser; the REPLICATION role attribute requires superuser authority."
                .to_string(),
        );
    }
    if use_physical_slot && primary_preflight.max_replication_slots < 1 {
        settings.push(
            "Increase max_replication_slots to at least 1; restart PostgreSQL after changing it."
                .to_string(),
        );
    }
    if use_physical_slot {
        settings.push(format!(
            "Confirm that a replication slot is available; max_replication_slots is currently {}.",
            primary_preflight.max_replication_slots
        ));
    }
    settings.push(format!(
        "Current listen_addresses is '{}'. Ensure it includes the primary interface reachable by standby {}.",
        primary_preflight.listen_addresses, standby.host
    ));
    let standby_command = format!(
        "PGSSLMODE=verify-full pg_basebackup --host='{}' --port={} --username='{}' --pgdata='<STANDBY_DATA_DIRECTORY>' --write-recovery-conf --wal-method=stream {slot_setup}--progress\n\n\
         Store the replication password in the postgres operating-system account's .pgpass file with owner-only permissions; do not put it in this command or shell history.\n\
         Ensure the standby host trusts the primary's TLS certificate and the certificate matches the primary host name.\n\
         After reviewing the target path and taking any required backup, run the command on the standby host. Then start PostgreSQL using that host's documented service procedure.",
        shell_quote_value(&primary.host),
        primary.port,
        replication_role,
    );
    Ok(ReplicationSetupPlan {
        primary_role_sql,
        set_password_command,
        primary_settings: settings.join("\n"),
        pg_hba_rule: format!("hostssl replication {replication_role} {allowed_cidr} scram-sha-256"),
        standby_command,
        slot_warning,
    })
}

pub fn validate_role_name(name: &str) -> Result<(), String> {
    let valid = !name.is_empty()
        && name.len() <= 63
        && !name.starts_with("pg_")
        && name.bytes().enumerate().all(|(index, byte)| match byte {
            b'a'..=b'z' | b'A'..=b'Z' | b'_' => true,
            b'0'..=b'9' => index > 0,
            _ => false,
        });
    if valid {
        Ok(())
    } else {
        Err("Use a PostgreSQL role name of up to 63 ASCII letters, digits, or underscores; it must not start with pg_.".to_string())
    }
}

pub fn validate_slot_name(name: &str) -> Result<(), String> {
    let valid = !name.is_empty()
        && name.len() <= 63
        && !name.starts_with("pg_")
        && name
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_');
    if valid {
        Ok(())
    } else {
        Err("Use a slot name of up to 63 lowercase ASCII letters, digits, or underscores; it must not start with pg_.".to_string())
    }
}

pub fn validate_cidr(cidr: &str) -> Result<(), String> {
    let (address, prefix) = cidr
        .split_once('/')
        .ok_or_else(|| "Enter a network in CIDR notation, such as 192.0.2.10/32.".to_string())?;
    let address = address
        .parse::<IpAddr>()
        .map_err(|_| "Enter a valid IPv4 or IPv6 address and prefix.".to_string())?;
    let prefix = prefix
        .parse::<u8>()
        .map_err(|_| "Enter a valid CIDR prefix length.".to_string())?;
    let max_prefix = if address.is_ipv4() { 32 } else { 128 };
    if prefix > max_prefix {
        return Err(format!("CIDR prefix must be between 0 and {max_prefix}."));
    }
    if prefix == 0 {
        return Err(
            "A /0 network is too broad for replication access; enter a more specific CIDR."
                .to_string(),
        );
    }
    Ok(())
}

fn same_endpoint(left: &ConnectionConfig, right: &ConnectionConfig) -> bool {
    left.host.eq_ignore_ascii_case(&right.host) && left.port == right.port
}

fn shell_quote_value(value: &str) -> String {
    value.replace('\'', "'\\''")
}

#[cfg(test)]
mod tests {
    use super::{
        ProvisioningPreflight, generate_setup_plan, validate_cidr, validate_role_name,
        validate_slot_name,
    };
    use crate::models::ConnectionConfig;

    fn profile(id: &str, host: &str) -> ConnectionConfig {
        ConnectionConfig {
            id: id.to_string(),
            host: host.to_string(),
            ..ConnectionConfig::default()
        }
    }

    fn preflight(version: i32, recovery: bool) -> ProvisioningPreflight {
        ProvisioningPreflight {
            server_version_num: version,
            server_version: format!("PostgreSQL {}", version / 10000),
            is_in_recovery: recovery,
            system_identifier: None,
            system_identifier_error: None,
            wal_level: "replica".to_string(),
            max_wal_senders: 10,
            max_replication_slots: 10,
            listen_addresses: "*".to_string(),
            ssl_enabled: true,
            is_superuser: false,
            can_create_role: true,
            can_replication: false,
        }
    }

    #[test]
    fn provisioning_version_gate_accepts_only_postgres_17_and_18() {
        let mut preflight = ProvisioningPreflight {
            server_version_num: 170000,
            server_version: "PostgreSQL 17.2".to_string(),
            is_in_recovery: false,
            system_identifier: None,
            system_identifier_error: None,
            wal_level: "replica".to_string(),
            max_wal_senders: 10,
            max_replication_slots: 10,
            listen_addresses: "*".to_string(),
            ssl_enabled: true,
            is_superuser: true,
            can_create_role: true,
            can_replication: true,
        };
        assert!(preflight.supports_provisioning());
        preflight.server_version_num = 180000;
        assert!(preflight.supports_provisioning());
        preflight.server_version_num = 160000;
        assert!(!preflight.supports_provisioning());
        preflight.server_version_num = 190000;
        assert!(!preflight.supports_provisioning());
    }

    #[test]
    fn setup_plan_is_secret_free_and_slot_is_optional() {
        let mut primary = profile("primary", "db-primary.example");
        primary.password = "never-include-this-secret".to_string();
        let standby = profile("standby", "db-standby.example");
        let plan = generate_setup_plan(
            &primary,
            &standby,
            &preflight(170000, false),
            &preflight(170000, false),
            "replicator",
            "192.0.2.5/32",
            true,
            "standby_slot",
        )
        .unwrap();
        assert!(plan.primary_role_sql.contains("CREATE ROLE \"replicator\""));
        assert_eq!(plan.set_password_command, "\\password replicator");
        assert!(!plan.primary_role_sql.contains("password "));
        assert!(!plan.standby_command.contains(&primary.password));
        assert!(plan.standby_command.starts_with("PGSSLMODE=verify-full"));
        assert!(
            plan.standby_command
                .contains("--create-slot --slot=standby_slot")
        );
        assert!(plan.standby_command.contains("'<STANDBY_DATA_DIRECTORY>'"));
        assert!(plan.slot_warning.is_some());
        assert!(plan.pg_hba_rule.contains("192.0.2.5/32"));

        let no_slot = generate_setup_plan(
            &primary,
            &standby,
            &preflight(170000, false),
            &preflight(170000, false),
            "replicator",
            "192.0.2.5/32",
            false,
            "",
        )
        .unwrap();
        assert!(!no_slot.standby_command.contains("--create-slot"));
        assert!(no_slot.slot_warning.is_none());
    }

    #[test]
    fn setup_plan_rejects_same_profile_or_server_endpoint() {
        let primary = profile("one", "host-a");
        assert!(
            generate_setup_plan(
                &primary,
                &primary,
                &preflight(170000, false),
                &preflight(170000, false),
                "replicator",
                "192.0.2.5/32",
                false,
                "",
            )
            .is_err()
        );
        assert!(
            generate_setup_plan(
                &primary,
                &profile("two", "HOST-A"),
                &preflight(170000, false),
                &preflight(170000, false),
                "replicator",
                "192.0.2.5/32",
                false,
                "",
            )
            .is_err()
        );
    }

    #[test]
    fn role_slot_and_cidr_inputs_are_validated() {
        assert!(validate_role_name("replicator_1").is_ok());
        assert!(validate_role_name("pg_admin").is_err());
        assert!(validate_role_name("bad;role").is_err());
        assert!(validate_slot_name("standby_1").is_ok());
        assert!(validate_slot_name("UpperCase").is_err());
        assert!(validate_cidr("192.0.2.1/32").is_ok());
        assert!(validate_cidr("2001:db8::1/128").is_ok());
        assert!(validate_cidr("0.0.0.0/33").is_err());
        assert!(validate_cidr("0.0.0.0/0").is_err());
        assert!(validate_cidr("0.0.0.0").is_err());
    }

    #[test]
    fn setup_plan_requires_matching_versions_primary_role_and_tls() {
        let primary = profile("primary", "db-primary.example");
        let standby = profile("standby", "db-standby.example");
        let primary_preflight = preflight(170000, false);
        assert!(
            generate_setup_plan(
                &primary,
                &standby,
                &primary_preflight,
                &preflight(180000, true),
                "replicator",
                "192.0.2.5/32",
                false,
                "",
            )
            .is_err()
        );

        assert!(
            generate_setup_plan(
                &primary,
                &standby,
                &preflight(170000, true),
                &preflight(170000, true),
                "replicator",
                "192.0.2.5/32",
                false,
                "",
            )
            .is_err()
        );

        let mut no_tls = primary_preflight;
        no_tls.ssl_enabled = false;
        assert!(
            generate_setup_plan(
                &primary,
                &standby,
                &no_tls,
                &preflight(170000, true),
                "replicator",
                "192.0.2.5/32",
                false,
                "",
            )
            .is_err()
        );
    }
}
