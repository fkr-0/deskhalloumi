use super::ModuleManager;
use deskhalloumi_core::{ModuleConfig, ModulePosition, ModuleRegistry, DefaultModuleRegistry};
use std::collections::HashMap;

#[tokio::test]
async fn test_module_manager_creation() {
    let manager = ModuleManager::new();
    let available = manager.list_available_modules();
    
    // Should have registered modules based on enabled features
    #[cfg(feature = "clock")]
    assert!(available.contains(&"clock".to_string()));
    
    #[cfg(feature = "battery")]
    assert!(available.contains(&"battery".to_string()));
}

#[tokio::test]
async fn test_module_loading_with_disabled_module() {
    let manager = ModuleManager::new();
    let mut configs = HashMap::new();
    
    // Add a disabled clock module
    configs.insert("clock".to_string(), ModuleConfig {
        enabled: false,
        position: ModulePosition::Left,
        update_interval_ms: Some(1000),
        theme_overrides: None,
    });
    
    let (modules, subscriptions) = manager.load_modules(configs).await.unwrap();
    
    // Should not load disabled modules
    assert!(!modules.contains_key("clock"));
    assert!(subscriptions.iter().find(|s| s.name == "clock").is_none());
}

#[tokio::test]
async fn test_module_loading_with_enabled_modules() {
    let manager = ModuleManager::new();
    let configs = manager.default_config();
    
    let (modules, subscriptions) = manager.load_modules(configs).await.unwrap();
    
    // Should load enabled modules
    #[cfg(feature = "clock")]
    {
        assert!(modules.contains_key("clock"));
        assert!(subscriptions.iter().any(|s| s.name == "clock"));
    }
    
    #[cfg(feature = "battery")]
    {
        assert!(modules.contains_key("battery"));
        assert!(subscriptions.iter().any(|s| s.name == "battery"));
    }
}

#[tokio::test]
async fn test_default_module_registry() {
    let registry = DefaultModuleRegistry::new();
    
    // Test empty registry
    assert_eq!(registry.list_modules().len(), 0);
    assert!(!registry.has_module("test"));
    
    // Test registry creation attempt with non-existent module
    let config = ModuleConfig {
        enabled: true,
        position: ModulePosition::Center,
        update_interval_ms: Some(500),
        theme_overrides: None,
    };
    
    let result = registry.create("nonexistent", &config).await;
    assert!(result.is_err());
}

#[test]
fn test_module_config_serialization() {
    let config = ModuleConfig {
        enabled: true,
        position: ModulePosition::Right,
        update_interval_ms: Some(1500),
        theme_overrides: None,
    };
    
    // Test serialization
    let serialized = serde_json::to_string(&config).unwrap();
    assert!(serialized.contains("\"enabled\":true"));
    assert!(serialized.contains("\"right\""));
    assert!(serialized.contains("1500"));
    
    // Test deserialization
    let deserialized: ModuleConfig = serde_json::from_str(&serialized).unwrap();
    assert_eq!(deserialized.enabled, config.enabled);
    assert!(matches!(deserialized.position, ModulePosition::Right));
    assert_eq!(deserialized.update_interval_ms, config.update_interval_ms);
}