use crate::models::ConnectionConfig;
use serde::{Deserialize, Serialize};

const PROFILE_FORMAT: &str = "pg-mini-connection-profiles";
const PROFILE_FORMAT_VERSION: u32 = 1;

#[derive(Serialize, Deserialize)]
struct ProfileFile {
    format: String,
    version: u32,
    profiles: Vec<ConnectionConfig>,
}

pub fn export_profiles(
    profiles: &[ConnectionConfig],
    include_passwords: bool,
) -> Result<String, String> {
    if profiles.is_empty() {
        return Err("There are no saved profiles to export".to_string());
    }
    let mut profiles = profiles.to_vec();
    if !include_passwords {
        for profile in &mut profiles {
            profile.password.clear();
        }
    }
    serde_json::to_string_pretty(&ProfileFile {
        format: PROFILE_FORMAT.to_string(),
        version: PROFILE_FORMAT_VERSION,
        profiles,
    })
    .map_err(|error| format!("Could not encode connection profiles: {error}"))
}

pub fn import_profiles(json: &str) -> Result<Vec<ConnectionConfig>, String> {
    let value: serde_json::Value =
        serde_json::from_str(json).map_err(|error| format!("Invalid profile JSON: {error}"))?;

    let profiles = if value
        .get("format")
        .and_then(serde_json::Value::as_str)
        .is_some()
    {
        let file: ProfileFile = serde_json::from_value(value)
            .map_err(|error| format!("Invalid profile file: {error}"))?;
        if file.format != PROFILE_FORMAT {
            return Err("This file is not a pg-mini connection profile export".to_string());
        }
        if file.version != PROFILE_FORMAT_VERSION {
            return Err(format!(
                "Unsupported profile export version: {}",
                file.version
            ));
        }
        file.profiles
    } else if let Some(profiles) = value
        .get("saved_connections")
        .and_then(serde_json::Value::as_array)
    {
        serde_json::from_value(serde_json::Value::Array(profiles.clone()))
            .map_err(|error| format!("Invalid saved connections: {error}"))?
    } else if value.is_array() {
        serde_json::from_value(value)
            .map_err(|error| format!("Invalid connection profiles: {error}"))?
    } else {
        return Err(
            "Expected a pg-mini profile export or a JSON array of connection profiles".to_string(),
        );
    };

    if profiles.is_empty() {
        return Err("The selected file contains no connection profiles".to_string());
    }

    profiles
        .into_iter()
        .map(|mut profile: ConnectionConfig| {
            if profile.name.trim().is_empty()
                || profile.host.trim().is_empty()
                || profile.database.trim().is_empty()
                || profile.username.trim().is_empty()
                || profile.port == 0
            {
                return Err(format!(
                    "Profile '{}' must have a name, host, non-zero port, database, and username",
                    profile.name
                ));
            }
            profile.id = uuid::Uuid::new_v4().to_string();
            Ok(profile)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{export_profiles, import_profiles};
    use crate::models::{ConnectionConfig, TlsMode};

    #[test]
    fn exports_passwords_only_when_requested() {
        let mut profile = ConnectionConfig::default();
        profile.password = "sensitive".to_string();
        let without_password = export_profiles(&[profile.clone()], false).unwrap();
        assert!(!without_password.contains("sensitive"));
        assert!(
            import_profiles(&without_password).unwrap()[0]
                .password
                .is_empty()
        );
        let with_password = export_profiles(&[profile], true).unwrap();
        assert!(with_password.contains("sensitive"));
    }

    #[test]
    fn imports_profiles_with_new_ids_and_tls_settings() {
        let mut profile = ConnectionConfig::default();
        profile.tls_mode = TlsMode::Require;
        profile.tls_root_cert = "/etc/postgresql/ca.pem".to_string();
        let json = export_profiles(&[profile.clone()], true).unwrap();
        let imported = import_profiles(&json).unwrap();
        assert_eq!(imported.len(), 1);
        assert_ne!(imported[0].id, profile.id);
        assert_eq!(imported[0].tls_mode, TlsMode::Require);
        assert_eq!(imported[0].tls_root_cert, profile.tls_root_cert);
        assert_eq!(imported[0].password, profile.password);
    }

    #[test]
    fn imports_legacy_profiles_without_tls_fields() {
        let imported = import_profiles(
            r#"[{"id":"old","name":"legacy","host":"db.example","port":5432,"database":"app","username":"me","password":""}]"#,
        )
        .unwrap();
        assert_eq!(imported[0].tls_mode, TlsMode::Disable);
        assert!(imported[0].tls_root_cert.is_empty());
    }

    #[test]
    fn rejects_invalid_profiles() {
        let error = import_profiles(
            r#"[{"id":"bad","name":"invalid","host":"db.example","port":0,"database":"app","username":"me","password":""}]"#,
        )
        .unwrap_err();
        assert!(error.contains("non-zero port"));
    }
}
