use std::{sync::Arc, time::Duration};

use deskhalloumi_core::{
    ModuleUpdate,
    runtime::{
        ModuleSubscription as RuntimeModuleSubscription, ProviderContract, ProviderHealth,
        ProviderRefreshPolicy, RuntimeMetrics, RuntimeSupervisor,
    },
};

use crate::module_loader::ModuleSubscription;
use super::{initialize_module_subscriptions, snapshot_matches_active_provider};

fn contract(name: &str) -> ProviderContract {
    ProviderContract::new(
        name,
        name,
        ProviderRefreshPolicy::periodic(Duration::from_millis(10)),
        "TestProviderBackend<ModuleUpdate>",
    )
}

#[tokio::test]
async fn replacement_provider_rejects_queued_snapshot_from_old_instance() {
    let first_supervisor = RuntimeSupervisor::start("subscription-old", 4);
    let old = RuntimeModuleSubscription::with_contract(contract("clock"), |updates| async move {
        let _ = updates.send(ModuleUpdate::Text("old".to_string()));
    });
    let old_providers = initialize_module_subscriptions(
        vec![ModuleSubscription {
            name: "clock".to_string(),
            subscription: old,
        }],
        &first_supervisor.spawner(),
    )
    .unwrap();
    let mut old_receiver = old_providers.get("clock").unwrap().receiver.clone();
    let old_snapshot = old_receiver.changed().await.unwrap();

    let second_supervisor = RuntimeSupervisor::start("subscription-new", 4);
    let new = RuntimeModuleSubscription::with_contract(contract("clock"), |updates| async move {
        let _ = updates.send(ModuleUpdate::Text("new".to_string()));
    });
    let new_providers = initialize_module_subscriptions(
        vec![ModuleSubscription {
            name: "clock".to_string(),
            subscription: new,
        }],
        &second_supervisor.spawner(),
    )
    .unwrap();
    let active = new_providers.get("clock").unwrap();
    let mut new_receiver = active.receiver.clone();
    let new_snapshot = new_receiver.changed().await.unwrap();

    assert!(!snapshot_matches_active_provider(active, &old_snapshot));
    assert!(snapshot_matches_active_provider(active, &new_snapshot));

    first_supervisor.shutdown(Duration::from_secs(1)).await.unwrap();
    second_supervisor.shutdown(Duration::from_secs(1)).await.unwrap();
}

#[tokio::test]
async fn initialization_returns_typed_watch_receivers() {
    let supervisor = RuntimeSupervisor::start("subscription-test", 8);
    let provider = RuntimeModuleSubscription::with_contract(contract("clock"), |updates| async move {
        let _ = updates.send(ModuleUpdate::Text("ready".to_string()));
    });
    let providers = initialize_module_subscriptions(
        vec![ModuleSubscription {
            name: "clock".to_string(),
            subscription: provider,
        }],
        &supervisor.spawner(),
    )
    .unwrap();
    let mut receiver = providers.get("clock").unwrap().receiver.clone();
    let snapshot = receiver.changed().await.unwrap();
    assert_eq!(snapshot.health(), ProviderHealth::Fresh);
    assert!(matches!(
        snapshot.value(),
        Some(ModuleUpdate::Text(text)) if text == "ready"
    ));
    supervisor.shutdown(Duration::from_secs(1)).await.unwrap();
}

#[tokio::test]
async fn providers_are_independent_and_hardware_free() {
    let metrics = Arc::new(RuntimeMetrics::default());
    let supervisor = RuntimeSupervisor::with_metrics("subscription-storage-test", 8, metrics);
    let providers = ["clock", "battery", "tmux"]
        .into_iter()
        .map(|name| ModuleSubscription {
            name: name.to_string(),
            subscription: RuntimeModuleSubscription::with_contract(
                contract(name),
                move |updates| async move {
                    let _ = updates.send(ModuleUpdate::Text(format!("{name}-fixture")));
                },
            ),
        })
        .collect();
    let providers = initialize_module_subscriptions(providers, &supervisor.spawner()).unwrap();
    for name in ["clock", "battery", "tmux"] {
        let mut receiver = providers.get(name).unwrap().receiver.clone();
        let snapshot = receiver.changed().await.unwrap();
        assert!(matches!(
            snapshot.value(),
            Some(ModuleUpdate::Text(text)) if text.as_str() == format!("{name}-fixture")
        ));
    }
    supervisor.shutdown(Duration::from_secs(1)).await.unwrap();
}

#[test]
fn module_update_types_remain_renderer_neutral() {
    let text_update = ModuleUpdate::Text("test".to_string());
    let progress_update = ModuleUpdate::ProgressBar(0.5);
    let icon_update = ModuleUpdate::Icon("🔋".to_string());
    assert!(matches!(text_update, ModuleUpdate::Text(text) if text == "test"));
    assert!(matches!(progress_update, ModuleUpdate::ProgressBar(value) if value == 0.5));
    assert!(matches!(icon_update, ModuleUpdate::Icon(icon) if icon == "🔋"));
}
