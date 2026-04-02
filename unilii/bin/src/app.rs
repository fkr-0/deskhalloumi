//! Application state and message handling for unilii

use crate::{
    app_config::AppConfig,
    cli::RunOptions,
    enhanced_tray,
    module_loader::LoadedModule,
    tray,
};
use iced::{Element, Length, Task, Theme, window};
use std::collections::{BTreeMap, HashMap};
use tracing::{error, info};
use unilii_core::{config::Config, ModuleUpdate};

/// A single panel in a multi-panel setup
pub struct UniliiPanel {
    pub modules: HashMap<String, LoadedModule>,
    pub config: Config,
    pub app_config: AppConfig,
    pub panel_config_index: usize,
    pub shift_held: bool,
    pub tray_icons: Vec<tray::TrayIcon>,
    pub enhanced_tray: Option<enhanced_tray::EnhancedTrayState>,
    pub run_options: RunOptions,
}

/// Manager for multiple panels
pub struct UniliiPanelManager {
    pub panels: BTreeMap<window::Id, UniliiPanel>,
    pub panel_configs: Vec<Config>,
    pub next_panel_index: usize,
}

impl Default for UniliiPanelManager {
    fn default() -> Self {
        let (manager, _task) = Self::new();
        manager
    }
}

impl UniliiPanelManager {
    pub fn new() -> (Self, Task<Message>) {
        let manager = Self {
            panels: BTreeMap::new(),
            panel_configs: vec![Config::default()],
            next_panel_index: 0,
        };

        (manager, Task::done(Message::InitializePanels))
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::InitializePanels => {
                // Initialize panels from config
                info!("Initializing panels from config");
                Task::none()
            }
            Message::WindowOpened(id) => {
                // Create new panel for window
                info!("Window opened: {:?}", id);
                if let Some(panel_config) = self.panel_configs.get(self.next_panel_index) {
                    let panel = UniliiPanel {
                        modules: HashMap::new(),
                        config: panel_config.clone(),
                        app_config: AppConfig::default(),
                        panel_config_index: self.next_panel_index,
                        shift_held: false,
                        tray_icons: Vec::new(),
                        enhanced_tray: None,
                        run_options: RunOptions::default(),
                    };
                    self.panels.insert(id, panel);
                    self.next_panel_index += 1;
                }
                Task::none()
            }
            Message::WindowClosed(id) => {
                // Remove panel and exit if last panel
                info!("Window closed: {:?}", id);
                self.panels.remove(&id);
                if self.panels.is_empty() {
                    info!("Last panel closed, exiting");
                    std::process::exit(0);
                }
                Task::none()
            }
            _ => {
                // Route other messages to appropriate panel
                // For now, use first panel (single-panel mode)
                if let Some((_, panel)) = self.panels.iter_mut().next() {
                    panel.update_panel(message);
                }
                Task::none()
            }
        }
    }
}

impl UniliiPanel {
    pub fn update_panel(&mut self, message: Message) {
        // Handle panel-level messages
        match message {
            Message::ModuleUpdate(name, update) => {
                info!("Panel module update: {} -> {:?}", name, update);
                if let Some(loaded) = self.modules.get_mut(&name) {
                    if let Err(e) = loaded.module.update(update) {
                        error!("Failed to update module '{}': {}", name, e);
                    }
                }
            }
            Message::KeyboardInput { code, value } => {
                info!("Panel keyboard event: code={}, value={}", code, value);
                if code == "KEY_LEFTSHIFT" || code == "KEY_RIGHTSHIFT" {
                    self.shift_held = value != 0;
                    info!("Panel shift state changed: held={}", self.shift_held);
                }
            }
            Message::WindowKeyboardInput {
                key,
                pressed,
                is_shift,
            } => {
                if is_shift {
                    self.shift_held = pressed;
                }
                // Additional keyboard handling would go here
            }
            Message::TrayEvent(_event) => {
                // Handle tray events at panel level
            }
            _ => {
                // Other messages are handled at manager level
            }
        }
    }
}

/// Main application state (backwards compatibility)
pub struct UniliiBar {
    pub modules: HashMap<String, LoadedModule>,
    pub config: Config,
    pub app_config: AppConfig,
    pub shift_held: bool,
    pub tray_icons: Vec<tray::TrayIcon>,
    pub enhanced_tray: Option<enhanced_tray::EnhancedTrayState>,
    pub run_options: RunOptions,
}

/// Application messages
#[derive(Debug, Clone)]
pub enum Message {
    // Window management messages
    InitializePanels,
    WindowOpened(window::Id),
    WindowClosed(window::Id),
    
    // Panel messages
    ModuleUpdate(String, ModuleUpdate),
    KeyboardInput {
        code: String,
        value: i32,
    },
    WindowKeyboardInput {
        key: String,
        pressed: bool,
        is_shift: bool,
    },
    // Enhanced tray events
    EnhancedTrayEvent(enhanced_tray::TrayEvent),
    TrayIconPressed(String),
    TrayMenuTriggered(String, enhanced_tray::TrayMenuAction),
    TrayNavigateLeft,
    TrayNavigateRight,
    TrayShowAggregated,
    TrayShowFavorites,
    TrayToggleFavorite(String, String), // (app_id, item_id)
    TrayFilterUpdate(String),
    TrayNetworkSnapshot(String, Result<tray::NetworkSnapshot, String>),
    TrayNetworkRefresh(String),
    TrayNetworkToggle(String),
    TrayNetworkToggleDone(String, Result<(), String>),
    TraySpawnCommand(String, String),
    TraySpawnCommandDone(String, Result<(), String>),
    TrayAnimateTick,

    // Legacy tray events (keep for compatibility during transition)
    TrayEvent(tray::TrayEvent),
}
