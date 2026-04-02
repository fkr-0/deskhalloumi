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
