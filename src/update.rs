use crate::models::{AppConfig, UpdateInfo};
use serde::{Deserialize, Serialize};
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
pub async fn check_for_updates(config: &AppConfig) -> Option<UpdateInfo> {
    // Only check if auto-update check is enabled
    if !config.check_for_updates {
        return None;
    }

    // Check if we should skip based on last check time (check at most once per day)
    if let Some(last_check) = &config.last_update_check {
        let now = chrono::Utc::now();
        let last_check_dt = chrono::DateTime::parse_from_rfc3339(last_check).ok()?;
        let elapsed = now.signed_duration_since(last_check_dt.with_timezone(&chrono::Utc));
        if elapsed < chrono::Duration::days(1) {
            return None;
        }
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
    let current_version = env!("CARGO_PKG_VERSION");
    let latest_version = release.tag_name.trim_start_matches('v');
    let current_version = current_version.trim_start_matches('v');

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

/// Background task to check for updates and store result in config
pub fn spawn_update_check(config: AppConfig) {
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

            if let Some(update_info) = runtime.block_on(check_for_updates(&config)) {
                let mut latest_config = crate::config::ConfigManager::load();
                latest_config.last_update_check = update_info.checked_at.clone();
                latest_config.update_info = Some(update_info);
                if let Err(error) = crate::config::ConfigManager::save(&latest_config) {
                    eprintln!("Failed to save update-check result: {error}");
                }
            }
        });

    if let Err(error) = result {
        eprintln!("Failed to start update check: {error}");
    }
}
