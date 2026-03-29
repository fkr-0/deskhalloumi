//! Configuration loader for unilii status bar.

use crate::keys::KeyBinding;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use tracing::{info, warn};

/// Main configuration structure for unilii.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Config {
    /// Window configuration
    pub window: WindowConfig,
    /// Module configurations
    pub modules: Vec<ModuleConfigEntry>,
    /// Global keybinding daemon configuration
    #[serde(default)]
    pub keybindings: Vec<KeyBinding>,
}

/// Window configuration settings.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct WindowConfig {
    /// Window width in pixels
    pub width: u32,
    /// Window height in pixels
    pub height: u32,
    /// Window position x coordinate
    pub position_x: i32,
    /// Window position y coordinate
    pub position_y: i32,
    /// Background color (hex format)
    pub background_color: Option<String>,
    /// Text color (hex format)
    pub text_color: Option<String>,
}

/// Module configuration entry.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ModuleConfigEntry {
    /// Module name (e.g., "clock", "battery")
    pub name: String,
    /// Whether the module is enabled
    pub enabled: bool,
    /// Module position (left, center, right)
    pub position: String,
    /// Update interval in milliseconds
    pub update_interval_ms: Option<u64>,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            window: WindowConfig {
                width: 800,
                height: 24,
                position_x: 0,
                position_y: 0,
                background_color: Some("#1e1e1e".to_string()),
                text_color: Some("#ffffff".to_string()),
            },
            modules: vec![
                ModuleConfigEntry {
                    name: "clock".to_string(),
                    enabled: true,
                    position: "right".to_string(),
                    update_interval_ms: Some(1000),
                },
                ModuleConfigEntry {
                    name: "battery".to_string(),
                    enabled: true,
                    position: "right".to_string(),
                    update_interval_ms: Some(5000),
                },
            ],
            keybindings: Vec::new(),
        }
    }
}

impl Default for WindowConfig {
    fn default() -> Self {
        WindowConfig {
            width: 800,
            height: 24,
            position_x: 0,
            position_y: 0,
            background_color: Some("#1e1e1e".to_string()),
            text_color: Some("#ffffff".to_string()),
        }
    }
}

/// Get the default configuration directory path.
pub fn get_config_dir() -> Option<PathBuf> {
    directories::ProjectDirs::from("com", "unilii", "unilii")
        .map(|proj_dirs| proj_dirs.config_dir().to_path_buf())
}

/// Get the configuration file path.
pub fn get_config_path() -> Option<PathBuf> {
    get_config_dir().map(|dir| dir.join("unilii.toml"))
}

/// Create default configuration file at the standard location.
pub fn create_default_config() -> std::io::Result<PathBuf> {
    let config_path = get_config_path().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "Could not determine config directory",
        )
    })?;

    // Create config directory if it doesn't exist
    if let Some(parent) = config_path.parent() {
        fs::create_dir_all(parent)?;
    }

    // Write default config
    let default_config = Config::default();
    let toml_string = toml::to_string_pretty(&default_config)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;

    fs::write(&config_path, toml_string)?;
    info!("Created default config at: {:?}", config_path);

    Ok(config_path)
}

/// Load configuration from file, or create default if it doesn't exist.
pub fn load_config() -> Config {
    let config_path = match get_config_path() {
        Some(path) => path,
        None => {
            warn!("Could not determine config directory, using defaults");
            return Config::default();
        }
    };

    // Try to load existing config
    if config_path.exists() {
        match fs::read_to_string(&config_path) {
            Ok(contents) => match toml::from_str(&contents) {
                Ok(config) => {
                    info!("Loaded config from: {:?}", config_path);
                    return config;
                }
                Err(e) => {
                    warn!("Failed to parse config file: {}, using defaults", e);
                }
            },
            Err(e) => {
                warn!("Failed to read config file: {}, using defaults", e);
            }
        }
    }

    // Create default config if it doesn't exist or failed to load
    info!("Config file not found or invalid, creating default");
    match create_default_config() {
        Ok(_) => Config::default(),
        Err(e) => {
            warn!(
                "Failed to create default config: {}, using hardcoded defaults",
                e
            );
            Config::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = Config::default();
        assert_eq!(config.window.width, 800);
        assert_eq!(config.window.height, 24);
        assert_eq!(config.modules.len(), 2);
        assert!(config.keybindings.is_empty());
        assert_eq!(config.modules[0].name, "clock");
        assert_eq!(config.modules[1].name, "battery");
    }

    #[test]
    fn test_config_serialization() {
        let config = Config::default();
        let toml_string = toml::to_string_pretty(&config).unwrap();
        assert!(toml_string.contains("clock"));
        assert!(toml_string.contains("battery"));
    }
}
