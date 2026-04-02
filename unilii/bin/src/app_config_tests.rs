use super::{AppConfig, load_app_config, save_app_config};
use unilii_core::ModulePosition;
use tempfile::NamedTempFile;

#[test]
fn test_default_app_config() {
    let config = AppConfig::default();
    
    // Should have default modules
    assert!(config.modules.contains_key("clock"));
    assert!(config.modules.contains_key("battery"));
    
    // Test clock module defaults
    let clock = &config.modules["clock"];
    assert!(clock.enabled);
    assert!(matches!(clock.position, ModulePosition::Right));
    assert_eq!(clock.update_interval_ms, Some(1000));
    
    // Test battery module defaults
    let battery = &config.modules["battery"];
    assert!(battery.enabled);
    assert!(matches!(battery.position, ModulePosition::Right));
    assert_eq!(battery.update_interval_ms, Some(5000));
    
    // Test application settings
    assert_eq!(config.app.refresh_rate_ms, 50);
    assert!(!config.app.verbose);
    assert_eq!(config.app.theme.font_size, Some(14));
}

#[test]
fn test_load_nonexistent_config_returns_default() {
    let config = load_app_config(Some("/nonexistent/path/config.toml"));
    let default_config = AppConfig::default();
    
    assert_eq!(config.modules.len(), default_config.modules.len());
    assert_eq!(config.app.refresh_rate_ms, default_config.app.refresh_rate_ms);
}

#[test]
fn test_save_and_load_config_roundtrip() {
    let mut config = AppConfig::default();
    config.app.verbose = true;
    config.app.refresh_rate_ms = 100;
    config.modules.get_mut("clock").unwrap().update_interval_ms = Some(2000);
    
    let temp_file = NamedTempFile::new().unwrap();
    let path = temp_file.path().to_str().unwrap();
    
    // Save config
    save_app_config(&config, path).expect("Failed to save config");
    
    // Load it back
    let loaded_config = load_app_config(Some(path));
    
    assert!(loaded_config.app.verbose);
    assert_eq!(loaded_config.app.refresh_rate_ms, 100);
    assert_eq!(loaded_config.modules["clock"].update_interval_ms, Some(2000));
}

#[test]
fn test_config_serialization_format() {
    let config = AppConfig::default();
    let serialized = toml::to_string(&config).unwrap();
    
    // Should contain expected sections and data
    assert!(serialized.contains("[app]"));
    assert!(serialized.contains("refresh_rate_ms"));
    assert!(serialized.contains("verbose"));
    
    // Should be able to deserialize back
    let deserialized: AppConfig = toml::from_str(&serialized).unwrap();
    assert_eq!(deserialized.modules.len(), config.modules.len());
    assert_eq!(deserialized.app.refresh_rate_ms, config.app.refresh_rate_ms);
}