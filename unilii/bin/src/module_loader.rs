//! Module loader for dynamically loading and initializing status bar modules.

use std::collections::HashMap;

use tracing::{info, warn};
use unilii_core::{
    DefaultModuleRegistry, ModuleConfig, ModuleRegistry, ModuleUpdate, Result, register_module,
};

/// Container for a loaded module with its update channel.
pub struct LoadedModule {
    /// The module instance.
    pub module: Box<dyn unilii_core::Module>,
}

/// Container for module subscription receiver channels.
pub struct ModuleSubscription {
    /// The module name.
    #[allow(dead_code)]
    pub name: String,
    /// The receiver for module updates.
    #[allow(dead_code)]
    pub receiver: tokio::sync::mpsc::UnboundedReceiver<ModuleUpdate>,
}

/// Registry manager that handles module registration and loading.
pub struct ModuleManager {
    registry: DefaultModuleRegistry,
}

impl ModuleManager {
    /// Create a new module manager with all available modules registered.
    pub fn new() -> Self {
        let mut registry = DefaultModuleRegistry::new();

        // Register available modules
        #[cfg(feature = "clock")]
        register_module!(registry, "clock", unilii_clock::Clock);

        #[cfg(feature = "battery")]
        register_module!(registry, "battery", unilii_battery::Battery);

        #[cfg(feature = "tmux")]
        register_module!(registry, "tmux", unilii_tmux::Tmux);

        Self { registry }
    }

    /// Load modules based on configuration with comprehensive error handling.
    pub async fn load_modules(
        &self,
        configs: HashMap<String, ModuleConfig>,
    ) -> Result<(HashMap<String, LoadedModule>, Vec<ModuleSubscription>)> {
        let mut modules = HashMap::new();
        let mut subscriptions = Vec::new();
        let mut load_errors = Vec::new();

        for (name, config) in configs.iter() {
            if !config.enabled {
                info!("Module '{}' is disabled, skipping", name);
                continue;
            }

            info!("Loading module: {}", name);

            // Attempt to load module with timeout and error recovery
            let load_result = tokio::time::timeout(
                std::time::Duration::from_secs(10), // 10 second timeout
                self.load_single_module(name, config),
            )
            .await;

            match load_result {
                Ok(Ok((module, subscription))) => {
                    modules.insert(name.clone(), LoadedModule { module });
                    if let Some(sub) = subscription {
                        subscriptions.push(sub);
                    }
                    info!("Module '{}' loaded successfully", name);
                }
                Ok(Err(e)) => {
                    let error_msg = format!("Failed to load module '{}': {}", name, e);
                    warn!("{}", error_msg);
                    load_errors.push(error_msg);
                }
                Err(_timeout) => {
                    let error_msg = format!("Module '{}' loading timed out after 10 seconds", name);
                    warn!("{}", error_msg);
                    load_errors.push(error_msg);
                }
            }
        }

        if modules.is_empty() && !configs.is_empty() {
            let enabled_count = configs.values().filter(|c| c.enabled).count();
            if enabled_count > 0 {
                return Err(format!(
                    "Failed to load any modules. {} modules were enabled but all failed to load. Errors: {}", 
                    enabled_count,
                    load_errors.join("; ")
                ).into());
            }
        }

        if !load_errors.is_empty() {
            info!(
                "Module loading completed with {} errors: {}",
                load_errors.len(),
                load_errors.join("; ")
            );
        }

        Ok((modules, subscriptions))
    }

    /// Load a single module with proper error handling.
    async fn load_single_module(
        &self,
        name: &str,
        config: &ModuleConfig,
    ) -> Result<(Box<dyn unilii_core::Module>, Option<ModuleSubscription>)> {
        // Create module with retry mechanism
        let mut module = self.create_module_with_retry(name, config, 3).await?;

        // Set up subscription with error handling
        let subscription = match module.subscribe().await {
            Ok(Some(rx)) => Some(ModuleSubscription {
                name: name.to_string(),
                receiver: rx,
            }),
            Ok(None) => {
                info!("Module '{}' does not provide subscriptions", name);
                None
            }
            Err(e) => {
                warn!(
                    "Module '{}' subscription setup failed: {}, continuing without subscription",
                    name, e
                );
                None
            }
        };

        Ok((module, subscription))
    }

    /// Create a module with retry mechanism for transient failures.
    async fn create_module_with_retry(
        &self,
        name: &str,
        config: &ModuleConfig,
        max_retries: usize,
    ) -> Result<Box<dyn unilii_core::Module>> {
        let mut last_error = None;

        for attempt in 0..max_retries {
            if attempt > 0 {
                // Exponential backoff: 100ms, 200ms, 400ms...
                let delay = std::time::Duration::from_millis(100 * (1 << attempt));
                info!(
                    "Module '{}' creation attempt {} failed, retrying in {:?}",
                    name, attempt, delay
                );
                tokio::time::sleep(delay).await;
            }

            match self.registry.create(name, config).await {
                Ok(module) => return Ok(module),
                Err(e) => {
                    last_error = Some(e);
                    warn!(
                        "Module '{}' creation attempt {} failed: {}",
                        name,
                        attempt + 1,
                        last_error.as_ref().unwrap()
                    );
                }
            }
        }

        Err(last_error.unwrap_or_else(|| "Unknown error".into()))
    }

    /// Get the default configuration for all available modules.
    #[allow(dead_code)]
    pub fn default_config(&self) -> HashMap<String, ModuleConfig> {
        let mut configs = HashMap::new();

        // Default configurations for each module
        if self.registry.has_module("clock") {
            configs.insert(
                "clock".to_string(),
                ModuleConfig {
                    enabled: true,
                    position: unilii_core::ModulePosition::Right,
                    update_interval_ms: Some(1000),
                    theme_overrides: None,
                },
            );
        }

        if self.registry.has_module("battery") {
            configs.insert(
                "battery".to_string(),
                ModuleConfig {
                    enabled: true,
                    position: unilii_core::ModulePosition::Right,
                    update_interval_ms: Some(5000),
                    theme_overrides: None,
                },
            );
        }

        configs
    }

    /// List all registered modules.
    #[allow(dead_code)]
    pub fn list_available_modules(&self) -> Vec<String> {
        self.registry.list_modules()
    }
}

#[cfg(test)]
mod tests {
    include!("module_loader_tests.rs");
}
