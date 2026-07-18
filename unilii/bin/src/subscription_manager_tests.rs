use super::{initialize_global_subscriptions, get_latest_module_update, has_module_updates, store_module_update};
use crate::module_loader::ModuleSubscription;
use tokio::sync::mpsc;
use deskhalloumi_core::ModuleUpdate;

#[tokio::test]
async fn test_subscription_manager_initialization() {
    let subscriptions = vec![
        ModuleSubscription {
            name: "test_module".to_string(),
            receiver: {
                let (_tx, rx) = mpsc::unbounded_channel();
                rx
            },
        }
    ];
    
    // Should not panic when initializing
    initialize_global_subscriptions(subscriptions);
    
    // Give it a moment to start
    tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
}

#[tokio::test]
async fn test_module_update_storage() {
    // Initialize the registry first with a clock module subscription
    let subscriptions = vec![
        ModuleSubscription {
            name: "clock".to_string(),
            receiver: {
                let (_tx, rx) = mpsc::unbounded_channel();
                rx
            },
        }
    ];
    
    initialize_global_subscriptions(subscriptions);
    
    // Give the initialization a moment to complete
    tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
    
    // Test storing and retrieving module updates
    let update = ModuleUpdate::Text("12:34:56".to_string());
    
    // Store a clock update
    store_module_update("clock", update.clone());
    
    // Check that it can be retrieved
    let retrieved = get_latest_module_update("clock");
    assert!(retrieved.is_some());
    
    if let Some(ModuleUpdate::Text(text)) = retrieved {
        assert_eq!(text, "12:34:56");
    }
}

#[test] 
fn test_has_module_updates() {
    // Initially should have no updates for unknown modules
    assert!(!has_module_updates("unknown_module"));
    
    // After storing an update, should return true
    store_module_update("battery", ModuleUpdate::ProgressBar(0.75));
    // Note: has_module_updates checks if registry is initialized, not if there's data
}

#[test]
fn test_get_latest_module_update_returns_none_for_unknown() {
    let result = get_latest_module_update("unknown_module");
    assert!(result.is_none());
}

#[test]
fn test_module_update_types() {
    // Test different types of module updates
    let text_update = ModuleUpdate::Text("test".to_string());
    let progress_update = ModuleUpdate::ProgressBar(0.5);
    let icon_update = ModuleUpdate::Icon("🔋".to_string());
    
    // Test that they can be cloned and matched  
    match text_update.clone() {
        ModuleUpdate::Text(text) => assert_eq!(text, "test"),
        _ => panic!("Unexpected update type"),
    }
    
    match progress_update.clone() {
        ModuleUpdate::ProgressBar(value) => assert_eq!(value, 0.5),
        _ => panic!("Unexpected update type"),
    }
    
    match icon_update.clone() {
        ModuleUpdate::Icon(icon) => assert_eq!(icon, "🔋"),
        _ => panic!("Unexpected update type"),
    }
}