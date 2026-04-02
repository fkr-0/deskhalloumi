//! Subscription management for coordinating module updates with Iced subscriptions.

use std::sync::{Arc, Mutex};
use tracing::{error, info, warn};
use unilii_core::ModuleUpdate;

use crate::module_loader::ModuleSubscription;

/// Registry of active module update streams that Iced subscriptions can tap into.
#[allow(dead_code)]
struct ModuleUpdateRegistry {
    clock_updates: Option<Arc<Mutex<Option<ModuleUpdate>>>>,
    battery_updates: Option<Arc<Mutex<Option<ModuleUpdate>>>>,
}

#[allow(dead_code)]
static MODULE_REGISTRY: Mutex<ModuleUpdateRegistry> = Mutex::new(ModuleUpdateRegistry {
    clock_updates: None,
    battery_updates: None,
});

/// Initialize with module subscriptions and start update threads with error recovery.
#[allow(dead_code)]
pub fn initialize_global_subscriptions(module_subscriptions: Vec<ModuleSubscription>) {
    // Initialize the registry with error handling
    {
        let mut registry = match MODULE_REGISTRY.lock() {
            Ok(reg) => reg,
            Err(poisoned) => {
                error!("Module registry mutex was poisoned, recovering...");
                poisoned.into_inner()
            }
        };
        
        for sub in &module_subscriptions {
            match sub.name.as_str() {
                "clock" => {
                    registry.clock_updates = Some(Arc::new(Mutex::new(None)));
                }
                "battery" => {
                    registry.battery_updates = Some(Arc::new(Mutex::new(None)));
                }
                _ => {
                    warn!("Subscription for unknown module '{}' will be monitored but not stored", sub.name);
                }
            }
        }
    }

    // Start subscription monitoring with error recovery
    tokio::spawn(async move {
        info!("Starting global subscription monitor for {} modules", module_subscriptions.len());
        
        // Create tasks for each module subscription with error isolation
        let mut handles = Vec::new();
        
        for mut sub in module_subscriptions {
            let name = sub.name.clone();
            
            let handle = tokio::spawn(async move {
                info!("Starting resilient subscription handler for module: {}", name);
                
                while let Some(update) = sub.receiver.recv().await {
                    info!("Module '{}' update: {:?}", name, update);
                    
                    // Store the update with error handling
                    if let Err(e) = store_module_update_safe(&name, update) {
                        warn!("Failed to store update for module '{}': {}", name, e);
                    }
                }
                
                info!("Module '{}' subscription handler terminated", name);
            });
            
            handles.push(handle);
        }
        
        // Monitor subscription tasks 
        for handle in handles {
            match handle.await {
                Ok(_) => {
                    // Task completed normally
                    info!("Module subscription task completed normally");
                }
                Err(e) if e.is_panic() => {
                    error!("Module subscription task panicked: {}", e);
                }
                Err(e) => {
                    error!("Module subscription task failed: {}", e);
                }
            }
        }
        
        info!("All module subscription handlers have completed");
    });
}

/// Store a module update in the global registry with error handling.
#[allow(dead_code)]
fn store_module_update_safe(module_name: &str, update: ModuleUpdate) -> Result<(), String> {
    let registry = MODULE_REGISTRY.lock()
        .map_err(|e| format!("Failed to acquire registry lock: {}", e))?;
    
    match module_name {
        "clock" => {
            if let Some(ref storage) = registry.clock_updates {
                storage.lock()
                    .map_err(|e| format!("Failed to acquire clock storage lock: {}", e))?
                    .replace(update);
            } else {
                return Err("Clock storage not initialized".to_string());
            }
        }
        "battery" => {
            if let Some(ref storage) = registry.battery_updates {
                storage.lock()
                    .map_err(|e| format!("Failed to acquire battery storage lock: {}", e))?
                    .replace(update);
            } else {
                return Err("Battery storage not initialized".to_string());
            }
        }
        _ => {
            return Err(format!("Unknown module: {}", module_name));
        }
    }
    
    Ok(())
}

/// Store a module update in the global registry.
#[allow(dead_code)]
pub fn store_module_update(module_name: &str, update: ModuleUpdate) {
    let registry = MODULE_REGISTRY.lock().unwrap();
    
    match module_name {
        "clock" => {
            if let Some(ref storage) = registry.clock_updates
                && let Ok(mut stored) = storage.lock() {
                    *stored = Some(update);
                }
        }
        "battery" => {
            if let Some(ref storage) = registry.battery_updates
                && let Ok(mut stored) = storage.lock() {
                    *stored = Some(update);
                }
        }
        _ => {}
    }
}

/// Get the latest update for a module with error handling.
#[allow(dead_code)]
pub fn get_latest_module_update(module_name: &str) -> Option<ModuleUpdate> {
    let registry = match MODULE_REGISTRY.lock() {
        Ok(reg) => reg,
        Err(poisoned) => {
            warn!("Registry lock was poisoned, attempting recovery");
            poisoned.into_inner()
        }
    };
    
    let storage = match module_name {
        "clock" => registry.clock_updates.as_ref()?,
        "battery" => registry.battery_updates.as_ref()?,
        _ => {
            warn!("Attempted to get update for unknown module: {}", module_name);
            return None;
        }
    };
    
    match storage.lock() {
        Ok(stored) => stored.clone(),
        Err(poisoned) => {
            warn!("Module '{}' storage lock was poisoned, attempting recovery", module_name);
            poisoned.into_inner().clone()
        }
    }
}

/// Check if a module has any stored updates with error handling.
#[allow(dead_code)]
pub fn has_module_updates(module_name: &str) -> bool {
    let registry = match MODULE_REGISTRY.lock() {
        Ok(reg) => reg,
        Err(_) => return false,
    };
    
    match module_name {
        "clock" => registry.clock_updates.is_some(),
        "battery" => registry.battery_updates.is_some(),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    
    include!("subscription_manager_tests.rs");
}