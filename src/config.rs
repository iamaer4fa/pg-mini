use crate::models::AppConfig;
use std::fs;
use std::path::PathBuf;

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

    pub fn save(config: &AppConfig) -> Result<(), String> {
        let path = Self::config_path()
            .ok_or_else(|| "Could not locate the application config directory".to_string())?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| format!("Could not create config directory: {error}"))?;
        }
        let json = serde_json::to_string_pretty(config)
            .map_err(|error| format!("Could not encode application config: {error}"))?;
        fs::write(path, json).map_err(|error| format!("Could not save application config: {error}"))
    }
}
