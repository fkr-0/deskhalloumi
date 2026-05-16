//! Widget trait and common types for unilii widgets

pub mod audio;
pub mod power;
pub mod sysmonitor;
pub mod video;
pub mod wifi;

use crate::app::Message;
use crate::module_loader::LoadedModule;
use iced::Element;
use std::collections::HashMap;
use std::fmt::Debug;

/// Common widget message type
#[derive(Debug, Clone)]
pub enum WidgetMessage {
    SysMonitor(String),
    Wifi(String),
    Tray(String),
    Audio(String),
    Video(String),
    Power(String),
}

/// Widget trait that all bar widgets must implement
pub trait Widget: Debug + Send + Sync {
    /// Get widget name
    fn name(&self) -> &str;

    /// Render widget
    fn view(&self) -> Element<'_, WidgetMessage>;

    /// Handle widget message
    fn update(&mut self, message: WidgetMessage);

    /// Get update interval in milliseconds (None for no updates)
    fn update_interval(&self) -> Option<u64> {
        None
    }
}

// Re-export widget implementations
pub use audio::Audio;
pub use power::Power;
pub use sysmonitor::SysMonitor;
pub use video::Video;
pub use wifi::Wifi;

/// Render modules as widgets in status bar
pub fn render_modules(modules: &HashMap<String, LoadedModule>) -> Vec<Element<'_, Message>> {
    let mut module_names: Vec<_> = modules.keys().collect();
    module_names.sort();

    let mut widgets = Vec::new();

    for name in module_names {
        if let Some(loaded) = modules.get(name) {
            let widget = loaded.module.view().map({
                let name = name.clone();
                move |update| Message::ModuleUpdate(name.clone(), update)
            });
            tracing::info!("Rendering module widget: {}", name);
            widgets.push(widget);
        }
    }

    tracing::info!("Total module widgets rendered: {}", widgets.len());
    widgets
}

/// Returns 0-based tray index if key is a digit 1-9 (Character("1") etc.)
pub fn key_char_digit(key: &str) -> Option<usize> {
    // iced Key::Character(SmolStr) formats as: Character("1")
    if let Some(inner) = key
        .strip_prefix("Character(\"")
        .and_then(|s| s.strip_suffix("\")"))
    {
        if inner.len() == 1 {
            if let Some(d) = inner.chars().next().and_then(|c| c.to_digit(10)) {
                if d >= 1 {
                    return Some(d as usize - 1);
                }
            }
        }
    }
    None
}
