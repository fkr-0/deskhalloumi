use std::time::Duration;

use deskhalloumi_core::{
    ModuleUpdate,
    runtime::{ModuleSubscription as RuntimeModuleSubscription, RuntimeSupervisor},
};

use super::{
    get_latest_module_update, has_module_updates, initialize_global_subscriptions,
    store_module_update,
};
use crate::module_loader::ModuleSubscription;

#[tokio::test]
async fn test_subscription_manager_initialization() {
    let supervisor = RuntimeSupervisor::start("subscription-test", 8);
    let subscriptions = vec![ModuleSubscription {
        name: "test_module".to_string(),
        subscription: RuntimeModuleSubscription::new(|updates| async move {
            let _ = updates.send(ModuleUpdate::Text("ready".to_string()));
        }),
    }];

    initialize_global_subscriptions(subscriptions, &supervisor.spawner()).unwrap();
    tokio::time::sleep(Duration::from_millis(10)).await;
    supervisor.shutdown(Duration::from_secs(1)).await.unwrap();
}

#[test]
fn test_arbitrary_module_update_storage() {
    store_module_update("tmux", ModuleUpdate::Text("pane:%17".to_string()));
    assert!(has_module_updates("tmux"));
    assert!(matches!(
        get_latest_module_update("tmux"),
        Some(ModuleUpdate::Text(text)) if text == "pane:%17"
    ));
}

#[tokio::test]
async fn test_module_update_storage() {
    let supervisor = RuntimeSupervisor::start("subscription-storage-test", 8);
    let subscriptions = vec![ModuleSubscription {
        name: "clock".to_string(),
        subscription: RuntimeModuleSubscription::new(|updates| async move {
            let _ = updates.send(ModuleUpdate::Text("12:34:56".to_string()));
        }),
    }];

    initialize_global_subscriptions(subscriptions, &supervisor.spawner()).unwrap();
    tokio::time::sleep(Duration::from_millis(20)).await;

    assert!(matches!(
        get_latest_module_update("clock"),
        Some(ModuleUpdate::Text(text)) if text == "12:34:56"
    ));
    supervisor.shutdown(Duration::from_secs(1)).await.unwrap();
}

#[test]
fn test_manual_module_update_storage() {
    store_module_update("clock", ModuleUpdate::Text("manual".to_string()));
    assert!(matches!(
        get_latest_module_update("clock"),
        Some(ModuleUpdate::Text(text)) if text == "manual"
    ));
}

#[test]
fn test_has_module_updates() {
    assert!(!has_module_updates("unknown_module"));
}

#[test]
fn test_get_latest_module_update_returns_none_for_unknown() {
    assert!(get_latest_module_update("unknown_module").is_none());
}

#[test]
fn test_module_update_types() {
    let text_update = ModuleUpdate::Text("test".to_string());
    let progress_update = ModuleUpdate::ProgressBar(0.5);
    let icon_update = ModuleUpdate::Icon("🔋".to_string());

    assert!(matches!(text_update, ModuleUpdate::Text(text) if text == "test"));
    assert!(matches!(progress_update, ModuleUpdate::ProgressBar(value) if value == 0.5));
    assert!(matches!(icon_update, ModuleUpdate::Icon(icon) if icon == "🔋"));
}
