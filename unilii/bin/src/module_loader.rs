//! Module loader for dynamically loading and initializing status bar modules.

use std::collections::HashMap;

use tracing::{info, warn};
use unilii_core::{Module, ModuleConfig, ModuleUpdate, Result};

/// Container for a loaded module with its update channel.
pub struct LoadedModule {
    /// The module instance.
    pub module: Box<dyn Module>,
    /// Channel sender for module updates.
    pub tx: tokio::sync::mpsc::UnboundedSender<ModuleUpdate>,
}

/// Load and initialize all configured modules.
///
/// This function creates module instances based on the default configuration
/// and sets up their update channels. Returns a HashMap mapping module names
/// to their loaded instances.
pub async fn load_modules() -> Result<HashMap<String, LoadedModule>> {
    let mut modules = HashMap::new();

    // Default module configurations
    let clock_config = ModuleConfig {
        enabled: true,
        position: unilii_core::ModulePosition::Right,
        update_interval_ms: Some(1000),
        theme_overrides: None,
    };

    let battery_config = ModuleConfig {
        enabled: true,
        position: unilii_core::ModulePosition::Right,
        update_interval_ms: Some(5000),
        theme_overrides: None,
    };

    // Load Clock module
    #[cfg(feature = "clock")]
    {
        info!("Loading Clock module");
        match create_clock_module(&clock_config).await {
            Ok((module, tx)) => {
                modules.insert("clock".to_string(), LoadedModule { module, tx });
                info!("Clock module loaded successfully");
            }
            Err(e) => {
                warn!("Failed to load Clock module: {}", e);
            }
        }
    }

    // Load Battery module
    #[cfg(feature = "battery")]
    {
        info!("Loading Battery module");
        match create_battery_module(&battery_config).await {
            Ok((module, tx)) => {
                modules.insert("battery".to_string(), LoadedModule { module, tx });
                info!("Battery module loaded successfully");
            }
            Err(e) => {
                warn!("Failed to load Battery module: {}", e);
            }
        }
    }

    Ok(modules)
}

#[cfg(feature = "clock")]
async fn create_clock_module(
    config: &ModuleConfig,
) -> Result<(Box<dyn Module>, tokio::sync::mpsc::UnboundedSender<ModuleUpdate>)> {
    let mut module = unilii_clock::Clock::new(config).await?;
    let mut rx = module.subscribe().await?.unwrap();

    let (tx, _rx_main) = tokio::sync::mpsc::unbounded_channel();

    // Forward module updates to main channel
    let tx_clone = tx.clone();
    tokio::spawn(async move {
        while let Some(update) = rx.recv().await {
            let _ = tx_clone.send(update);
        }
    });

    Ok((Box::new(module), tx))
}

#[cfg(feature = "battery")]
async fn create_battery_module(
    config: &ModuleConfig,
) -> Result<(Box<dyn Module>, tokio::sync::mpsc::UnboundedSender<ModuleUpdate>)> {
    let mut module = unilii_battery::Battery::new(config).await?;
    let mut rx = module.subscribe().await?.unwrap();

    let (tx, _rx_main) = tokio::sync::mpsc::unbounded_channel();

    // Forward module updates to main channel
    let tx_clone = tx.clone();
    tokio::spawn(async move {
        while let Some(update) = rx.recv().await {
            let _ = tx_clone.send(update);
        }
    });

    Ok((Box::new(module), tx))
}
