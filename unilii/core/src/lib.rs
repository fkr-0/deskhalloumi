//! Core plugin API for unilii status bar modules.

pub mod config;

use async_trait::async_trait;
use iced::Element;

/// Result type for plugin operations.
pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

/// Configuration for a module instance.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct ModuleConfig {
    pub enabled: bool,
    pub position: ModulePosition,
    pub update_interval_ms: Option<u64>,
    pub theme_overrides: Option<ThemeOverrides>,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModulePosition {
    Left,
    Center,
    Right,
}

#[derive(Debug, Clone, serde::Deserialize)]
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
    async fn subscribe(
        &mut self,
    ) -> Result<Option<tokio::sync::mpsc::UnboundedReceiver<ModuleUpdate>>> {
        Ok(None)
    }

    /// Get current update interval (milliseconds).
    fn update_interval(&self) -> Option<u64> {
        None
    }
}

/// Registry for available modules.
pub trait ModuleRegistry {
    fn register(&mut self, name: &'static str, factory: ModuleFactory);
    fn create(&self, name: &str, config: &ModuleConfig) -> Result<Box<dyn Module>>;
}

pub type ModuleFactory = fn(&ModuleConfig) -> tokio::task::JoinHandle<Result<Box<dyn Module>>>;
