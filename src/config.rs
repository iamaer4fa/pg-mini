use std::fs;
use std::path::PathBuf;
use crate::models::AppConfig;

pub struct ConfigManager;

impl ConfigManager {
    fn config_path() -> Option<PathBuf> {
        dirs::config_dir().map(|dir| dir.join("pg-mini").join("config.json"))
    }

    pub fn load() -> AppConfig {
        if let Some(path) = Self::config_path() {
            if path.exists() {
                if let Ok(content) = fs::read_to_string(&path) {
                    if let Ok(config) = serde_json::from_str::<AppConfig>(&content) {
                        return config;
                    }
                }
            }
        }
        AppConfig::default()
    }

    pub fn save(config: &AppConfig) {
        if let Some(path) = Self::config_path() {
            if let Some(parent) = path.parent() {
                let _ = fs::create_dir_all(parent);
            }
            if let Ok(json) = serde_json::to_string_pretty(config) {
                let _ = fs::write(path, json);
            }
        }
    }
}
