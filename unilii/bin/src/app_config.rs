//! Configuration structure for modules and application settings.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use unilii_core::{ModuleConfig, ModulePosition, ThemeOverrides};

/// Main application configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    /// Module configurations.
    pub modules: HashMap<String, ModuleConfig>,
    
    /// Application-level settings.
    pub app: ApplicationSettings,
}

/// Application-level settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApplicationSettings {
    /// Refresh rate in milliseconds for the main UI loop.
    pub refresh_rate_ms: u64,
    
    /// Enable verbose logging.
    pub verbose: bool,
    
    /// Theme settings.
    pub theme: ThemeSettings,
}

/// Theme settings for the application.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThemeSettings {
    /// Application background color.
    pub background_color: Option<String>,
    
    /// Default text color.
    pub text_color: Option<String>,
    
    /// Default font size.
    pub font_size: Option<u16>,
}

impl Default for AppConfig {
    fn default() -> Self {
        let mut modules = HashMap::new();
        
        // Default clock module configuration
        modules.insert("clock".to_string(), ModuleConfig {
            enabled: true,
            position: ModulePosition::Right,
            update_interval_ms: Some(1000),
            theme_overrides: None,
        });
        
        // Default battery module configuration
        modules.insert("battery".to_string(), ModuleConfig {
            enabled: true,
            position: ModulePosition::Right,
            update_interval_ms: Some(5000),
            theme_overrides: Some(ThemeOverrides {
                bg_color: None,
                fg_color: None,
                font_size: Some(12),
            }),
        });
        
        Self {
            modules,
            app: ApplicationSettings::default(),
        }
    }
}

impl Default for ApplicationSettings {
    fn default() -> Self {
        Self {
            refresh_rate_ms: 50, // 20 FPS
            verbose: false,
            theme: ThemeSettings::default(),
        }
    }
}

impl Default for ThemeSettings {
    fn default() -> Self {
        Self {
            background_color: Some("#2c3e50".to_string()),
            text_color: Some("#ecf0f1".to_string()),
            font_size: Some(14),
        }
    }
}

/// Load configuration from file or return default with enhanced error handling.
pub fn load_app_config(path: Option<&str>) -> AppConfig {
    if let Some(path) = path {
        match std::fs::read_to_string(path) {
            Ok(contents) => {
                match toml::from_str(&contents) {
                    Ok(config) => {
                        // Validate configuration before returning
                        if let Err(validation_error) = validate_app_config(&config) {
                            eprintln!("Configuration validation failed for '{}': {}", path, validation_error);
                            eprintln!("Falling back to default configuration");
                            AppConfig::default()
                        } else {
                            config
                        }
                    }
                    Err(e) => {
                        eprintln!("Failed to parse config file '{}': {}", path, e);
                        eprintln!("Please check the TOML syntax. Falling back to default configuration");
                        AppConfig::default()
                    }
                }
            }
            Err(e) => {
                eprintln!("Failed to read config file '{}': {}", path, e);
                eprintln!("Falling back to default configuration");
                AppConfig::default()
            }
        }
    } else {
        AppConfig::default()
    }
}

/// Validate the application configuration for common issues.
fn validate_app_config(config: &AppConfig) -> Result<(), String> {
    // Check for reasonable refresh rate
    if config.app.refresh_rate_ms == 0 {
        return Err("Refresh rate cannot be zero".to_string());
    }
    
    if config.app.refresh_rate_ms > 10000 {
        return Err("Refresh rate too high (max 10000ms)".to_string());
    }
    
    // Validate module configurations
    for (name, module_config) in &config.modules {
        if let Some(interval) = module_config.update_interval_ms {
            if interval == 0 {
                return Err(format!("Module '{}' has zero update interval", name));
            }
            if interval > 3600000 { // 1 hour max
                return Err(format!("Module '{}' update interval too high (max 1 hour)", name));
            }
        }
    }
    
    Ok(())
}

/// Save configuration to file.
#[allow(dead_code)]
pub fn save_app_config(config: &AppConfig, path: &str) -> Result<(), Box<dyn std::error::Error>> {
    let contents = toml::to_string_pretty(config)?;
    std::fs::write(path, contents)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    include!("app_config_tests.rs");
}