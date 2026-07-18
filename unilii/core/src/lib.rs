//! Core plugin API for unilii status bar modules.

pub mod action_bus;
pub mod bar;
pub mod bar_runtime;
pub mod branding;
pub mod config;
pub mod filter_tab;
pub mod hotkey_control;
pub mod i3_config;
pub mod i3_keybindings;
pub mod i3_vis;
pub mod key_engine;
pub mod key_import_sxhkd;
pub mod keys;
pub mod menu_process;
pub mod runtime;
pub mod x11_hotkeys;

use async_trait::async_trait;
use iced::Element;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

/// Result type for plugin operations.
pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

/// Configuration for a module instance.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ModuleConfig {
    pub enabled: bool,
    pub position: ModulePosition,
    pub update_interval_ms: Option<u64>,
    pub theme_overrides: Option<ThemeOverrides>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModulePosition {
    Left,
    Center,
    Right,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ThemeOverrides {
    pub bg_color: Option<String>,
    pub fg_color: Option<String>,
    pub font_size: Option<u16>,
}

/// State update from a module.
#[derive(Debug, Clone)]
pub enum ModuleUpdate {
    Text(String),
    ProgressBar(f32), // 0.0 to 1.0
    Icon(String),
    Custom(String), // JSON for complex widgets
}

/// Trait that all status bar modules must implement.
#[async_trait]
pub trait Module: Send + Sync {
    /// Create a new module instance from config.
    async fn new(config: &ModuleConfig) -> Result<Self>
    where
        Self: Sized;

    /// Returns the module's name (e.g., "clock", "battery").
    fn name(&self) -> &str;

    /// Returns the initial UI view.
    fn view(&self) -> Element<'_, ModuleUpdate>;

    /// Handle an update message from the UI.
    fn update(&mut self, message: ModuleUpdate) -> Result<()>;

    /// Subscribe to async events (called once at startup).
    /// Returns a stream of updates or None if not needed.
    async fn subscribe(&mut self) -> Result<Option<runtime::ModuleSubscription>> {
        Ok(None)
    }

    /// Get current update interval (milliseconds).
    fn update_interval(&self) -> Option<u64> {
        None
    }
}

/// Registry for available modules.
#[async_trait]
pub trait ModuleRegistry: Send + Sync {
    /// Register a module creator function with the given name.
    fn register(&mut self, name: &str, creator: ModuleCreator);

    /// Create a module instance by name with given config.
    async fn create(&self, name: &str, config: &ModuleConfig) -> Result<Box<dyn Module>>;

    /// List all registered module names.
    fn list_modules(&self) -> Vec<String>;

    /// Check if a module is registered.
    fn has_module(&self, name: &str) -> bool;
}

/// Function type for creating module instances.
pub type ModuleCreator = Arc<
    dyn Fn(
            &ModuleConfig,
        )
            -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Box<dyn Module>>> + Send>>
        + Send
        + Sync,
>;

/// Default implementation of ModuleRegistry.
pub struct DefaultModuleRegistry {
    creators: Arc<RwLock<HashMap<String, ModuleCreator>>>,
}

impl DefaultModuleRegistry {
    /// Create a new empty registry.
    pub fn new() -> Self {
        Self {
            creators: Arc::new(RwLock::new(HashMap::new())),
        }
    }
}

impl Default for DefaultModuleRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ModuleRegistry for DefaultModuleRegistry {
    fn register(&mut self, name: &str, creator: ModuleCreator) {
        if let Ok(mut creators) = self.creators.write() {
            creators.insert(name.to_string(), creator);
        }
    }

    async fn create(&self, name: &str, config: &ModuleConfig) -> Result<Box<dyn Module>> {
        let creator = {
            let creators = self
                .creators
                .read()
                .map_err(|e| format!("Failed to read registry: {}", e))?;
            creators
                .get(name)
                .cloned()
                .ok_or_else(|| format!("Module '{}' not found in registry", name))?
        };

        creator(config).await
    }

    fn list_modules(&self) -> Vec<String> {
        self.creators
            .read()
            .map(|creators| creators.keys().cloned().collect())
            .unwrap_or_default()
    }

    fn has_module(&self, name: &str) -> bool {
        self.creators
            .read()
            .map(|creators| creators.contains_key(name))
            .unwrap_or(false)
    }
}

/// Helper function to create a module creator from a module type.
pub fn create_module_creator<T>() -> ModuleCreator
where
    T: Module + 'static,
{
    Arc::new(|config: &ModuleConfig| {
        let config = config.clone();
        Box::pin(async move {
            let module = T::new(&config).await?;
            Ok(Box::new(module) as Box<dyn Module>)
        })
    })
}

/// Macro to simplify module registration.
#[macro_export]
macro_rules! register_module {
    ($registry:expr, $name:expr, $module_type:ty) => {
        $registry.register($name, $crate::create_module_creator::<$module_type>())
    };
}
