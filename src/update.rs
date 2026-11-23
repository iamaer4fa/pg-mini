use crate::models::{AppConfig, UpdateInfo};
use serde::{Deserialize, Serialize};
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct GitHubRelease {
    tag_name: String,
    name: String,
    body: String,
    html_url: String,
    published_at: String,
    prerelease: bool,
    draft: bool,
}

/// Checks for updates against GitHub releases
pub async fn check_for_updates(config: &AppConfig, force: bool) -> Option<UpdateInfo> {
    // Only check if auto-update check is enabled
    if !config.check_for_updates && !force {
        return None;
    }

    let current_version = env!("CARGO_PKG_VERSION").trim_start_matches('v');
    if should_skip_recent_check(config, current_version, chrono::Utc::now(), force) {
        return None;
    }

    // Fetch latest release from GitHub
    let client = reqwest::Client::new();
    let response = client
        .get("https://api.github.com/repos/iamaer4fa/pg-mini/releases/latest")
        .header("User-Agent", "pg-mini")
        .timeout(Duration::from_secs(10))
        .send()
        .await
        .ok()?;

    if !response.status().is_success() {
        return None;
    }

    let release: GitHubRelease = response.json().await.ok()?;

    // Parse versions (strip 'v' prefix if present)
    let latest_version = release.tag_name.trim_start_matches('v');

    // Compare versions
    let has_update = compare_versions(current_version, latest_version).is_lt();

    Some(UpdateInfo {
        current_version: current_version.to_string(),
        latest_version: latest_version.to_string(),
        has_update,
        release_notes: release.body,
        release_url: release.html_url,
        checked_at: Some(chrono::Utc::now().to_rfc3339()),
    })
}

fn should_skip_recent_check(
    config: &AppConfig,
    current_version: &str,
    now: chrono::DateTime<chrono::Utc>,
    force: bool,
) -> bool {
    if force {
        return false;
    }

    if config
        .update_info
        .as_ref()
        .is_some_and(|info| info.current_version != current_version)
    {
        return false;
    }

    config
        .last_update_check
        .as_ref()
        .and_then(|last_check| chrono::DateTime::parse_from_rfc3339(last_check).ok())
        .is_some_and(|last_check| {
            now.signed_duration_since(last_check.with_timezone(&chrono::Utc))
                < chrono::Duration::days(1)
        })
}

/// Compare two version strings (semver-like)
/// Returns Ordering::Less if a < b, Ordering::Equal if a == b, Ordering::Greater if a > b
fn compare_versions(a: &str, b: &str) -> std::cmp::Ordering {
    let a_parts: Vec<u32> = a
        .split('.')
        .filter_map(|s| s.parse::<u32>().ok())
        .collect();
    let b_parts: Vec<u32> = b
        .split('.')
        .filter_map(|s| s.parse::<u32>().ok())
        .collect();

    let max_len = a_parts.len().max(b_parts.len());
    for i in 0..max_len {
        let a_val = a_parts.get(i).copied().unwrap_or(0);
        let b_val = b_parts.get(i).copied().unwrap_or(0);
        match a_val.cmp(&b_val) {
            std::cmp::Ordering::Equal => continue,
            other => return other,
        }
    }
    std::cmp::Ordering::Equal
}

#[cfg(test)]
mod tests {
    use super::should_skip_recent_check;
    use crate::models::{AppConfig, UpdateInfo};

    #[test]
    fn stale_update_info_does_not_suppress_check_after_version_change() {
        let now = chrono::Utc::now();
        let mut config = AppConfig {
            last_update_check: Some(now.to_rfc3339()),
            ..AppConfig::default()
        };
        config.update_info = Some(UpdateInfo {
            current_version: "0.1.15".to_string(),
            ..UpdateInfo::default()
        });

        assert!(!should_skip_recent_check(&config, "0.1.16", now, false));

        config.update_info.as_mut().unwrap().current_version = "0.1.16".to_string();
        assert!(should_skip_recent_check(&config, "0.1.16", now, false));
        assert!(!should_skip_recent_check(&config, "0.1.16", now, true));
    }
}

/// Background task to check for updates and store result in config
pub fn spawn_update_check(config: AppConfig, force: bool) -> Receiver<Option<UpdateInfo>> {
    let (result_sender, result_receiver) = mpsc::channel();
    let result = std::thread::Builder::new()
        .name("pg-mini-update-check".to_string())
        .spawn(move || {
            let runtime = match tokio::runtime::Builder::new_multi_thread()
                .worker_threads(1)
                .enable_all()
                .build()
            {
                Ok(runtime) => runtime,
                Err(error) => {
                    eprintln!("Failed to create update-check Tokio runtime: {error}");
                    return;
                }
            };

            let Some(update_info) = runtime.block_on(check_for_updates(&config, force)) else {
                let _ = result_sender.send(None);
                return;
            };

            let mut latest_config = crate::config::ConfigManager::load();
            latest_config.last_update_check = update_info.checked_at.clone();
            latest_config.update_info = Some(update_info.clone());
            if let Err(error) = crate::config::ConfigManager::save(&latest_config) {
                eprintln!("Failed to save update-check result: {error}");
            }

            let _ = result_sender.send(Some(update_info));
        });

    if let Err(error) = result {
        eprintln!("Failed to start update check: {error}");
    }

    result_receiver
}
