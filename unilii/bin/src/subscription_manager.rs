//! Subscription management for coordinating module updates with Iced subscriptions.

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use deskhalloumi_core::{ModuleUpdate, runtime::TaskSpawner};
use tracing::{error, info, warn};

use crate::module_loader::ModuleSubscription;

type UpdateStorage = Arc<Mutex<Option<ModuleUpdate>>>;

/// Registry of latest module values that Iced subscriptions can poll without
/// owning the producer workers themselves.
#[derive(Default)]
struct ModuleUpdateRegistry {
    updates: HashMap<String, UpdateStorage>,
}

static MODULE_REGISTRY: std::sync::LazyLock<Mutex<ModuleUpdateRegistry>> =
    std::sync::LazyLock::new(|| Mutex::new(ModuleUpdateRegistry::default()));

/// Initialize module subscriptions under the process runtime supervisor.
pub fn initialize_global_subscriptions(
    module_subscriptions: Vec<ModuleSubscription>,
    spawner: &TaskSpawner,
) -> Result<(), String> {
    {
        let mut registry = registry_guard();
        for subscription in &module_subscriptions {
            registry
                .updates
                .entry(subscription.name.clone())
                .or_insert_with(|| Arc::new(Mutex::new(None)));
        }
    }

    info!(
        modules = module_subscriptions.len(),
        "registering module subscriptions with runtime supervisor"
    );

    for mut module in module_subscriptions {
        let producer_name = format!("module:{}:producer", module.name);
        let producer_token = spawner.cancellation_token();
        let producer = module
            .subscription
            .take_worker()
            .ok_or_else(|| format!("module '{}' subscription has no producer", module.name))?;
        spawner
            .try_spawn(producer_name, async move {
                tokio::select! {
                    _ = producer_token.cancelled() => {}
                    _ = producer => {}
                }
            })
            .map_err(|error| format!("failed to supervise module producer: {error}"))?;

        let consumer_name = format!("module:{}:consumer", module.name);
        let module_name = module.name.clone();
        let consumer_token = spawner.cancellation_token();
        spawner
            .try_spawn(consumer_name, async move {
                loop {
                    let update = tokio::select! {
                        _ = consumer_token.cancelled() => break,
                        update = module.subscription.recv() => update,
                    };
                    let Some(update) = update else {
                        break;
                    };
                    if let Err(error) = store_module_update_safe(&module_name, update) {
                        warn!(module = %module_name, %error, "failed to store module update");
                    }
                }
                info!(module = %module_name, "module subscription consumer stopped");
            })
            .map_err(|error| format!("failed to supervise module consumer: {error}"))?;
    }

    Ok(())
}

fn registry_guard() -> std::sync::MutexGuard<'static, ModuleUpdateRegistry> {
    match MODULE_REGISTRY.lock() {
        Ok(registry) => registry,
        Err(poisoned) => {
            error!("module registry mutex was poisoned, recovering");
            poisoned.into_inner()
        }
    }
}

fn storage_for(module_name: &str) -> UpdateStorage {
    registry_guard()
        .updates
        .entry(module_name.to_string())
        .or_insert_with(|| Arc::new(Mutex::new(None)))
        .clone()
}

fn store_module_update_safe(module_name: &str, update: ModuleUpdate) -> Result<(), String> {
    storage_for(module_name)
        .lock()
        .map_err(|error| format!("failed to lock '{module_name}' update storage: {error}"))?
        .replace(update);
    Ok(())
}

#[cfg(test)]
pub fn store_module_update(module_name: &str, update: ModuleUpdate) {
    if let Err(error) = store_module_update_safe(module_name, update) {
        warn!(module = module_name, %error, "failed to store module update");
    }
}

pub fn get_latest_module_update(module_name: &str) -> Option<ModuleUpdate> {
    let storage = registry_guard().updates.get(module_name)?.clone();
    match storage.lock() {
        Ok(stored) => stored.clone(),
        Err(poisoned) => {
            warn!(
                module = module_name,
                "module update storage was poisoned, recovering"
            );
            poisoned.into_inner().clone()
        }
    }
}

pub fn has_module_updates(module_name: &str) -> bool {
    registry_guard().updates.contains_key(module_name)
}

#[cfg(test)]
mod tests {
    include!("subscription_manager_tests.rs");
}
