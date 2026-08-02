//! Application state and message handling for unilii

use crate::{
    bar_control::BarControlState,
    cli::RunOptions,
    enhanced_tray,
    module_loader::LoadedModule,
    provider_runtime::LegacyProviderRuntime,
    subscription_manager::ManagedModuleProvider,
    tray,
    widgets::{
        Audio, Power, SysMonitor, Video, WidgetMessage, Wifi, audio::AudioSnapshot,
        power::PowerSnapshot, video::VideoSnapshot, wifi::WifiSnapshot,
    },
};
use deskhalloumi_core::{
    ModuleUpdate,
    action_history::ActionHistory,
    config::Config,
    keys::KeybindingResult,
    quick_select::QuickSelectSession,
    runtime::{ProviderRefreshRegistry, ProviderSnapshot, RuntimeSupervisor, TaskSpawner},
};
use iced::window;
use std::path::PathBuf;
use std::{collections::HashMap, sync::Arc};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuQuickSelectAction {
    pub app_id: String,
    pub action: enhanced_tray::TrayMenuAction,
}

/// Main application state.
pub struct UniliiBar {
    pub main_window_id: Option<window::Id>,
    pub tray_window_id: Option<window::Id>,
    pub modules: HashMap<String, LoadedModule>,
    pub module_providers: HashMap<String, ManagedModuleProvider>,
    pub config_path: Option<PathBuf>,
    pub config: Config,
    pub bar_control: BarControlState,
    pub sysmonitor: SysMonitor,
    pub wifi: Wifi,
    pub audio: Audio,
    pub video: Video,
    pub power: Power,
    pub runtime_supervisor: Arc<RuntimeSupervisor>,
    pub runtime_spawner: TaskSpawner,
    pub provider_refreshes: ProviderRefreshRegistry,
    pub legacy_providers: LegacyProviderRuntime,
    pub system_menu: crate::menus::system::SystemMenuRuntime,
    pub action_history: ActionHistory,
    pub shift_held: bool,
    pub tray_icons: Vec<tray::TrayIcon>,
    pub enhanced_tray: Option<enhanced_tray::EnhancedTrayState>,
    pub tray_quick_select: Option<QuickSelectSession<MenuQuickSelectAction>>,
    pub run_options: RunOptions,
    /// True when the bar owns an embedded KeybindingDaemon action channel.
    pub keybinding_actions_enabled: bool,
}

/// Application messages
#[derive(Debug, Clone)]
pub enum Message {
    // Window management messages
    WindowOpened(window::Id),
    WindowClosed(window::Id),
    RuntimeShutdownComplete(Result<(), String>),

    // Panel messages
    ModuleUpdate(String, ModuleUpdate),
    ModuleProviderState(
        String,
        deskhalloumi_core::runtime::ProviderSnapshot<ModuleUpdate>,
    ),
    BarConfigReloaded {
        generation: u64,
        result: Box<Result<crate::bar_control::ReloadCandidate, String>>,
    },
    DismissBarStatus,
    WindowKeyboardInput {
        key: String,
        pressed: bool,
        is_shift: bool,
    },
    // Enhanced tray events
    TrayIconPressed(String),
    TrayMenuTriggered(String, enhanced_tray::TrayMenuAction),
    TrayNavigateLeft,
    TrayNavigateRight,
    TrayShowAggregated,
    TrayShowFavorites,
    TrayToggleFavorite(String, String), // (app_id, item_id)
    TrayFilterUpdate(String),
    TrayEnterSubmenu(String, Vec<String>),
    TrayExitSubmenu,
    TrayTextInputChanged(String, String), // item_id, value
    TrayNetworkSnapshot(String, Result<tray::NetworkSnapshot, String>),
    TrayNetworkRefresh(String),
    TrayNetworkToggle(String),
    TrayNetworkToggleDone(String, Result<(), String>),
    TrayMountSnapshot(
        String,
        Result<crate::menus::mount::MountMenuSnapshot, String>,
    ),
    TrayMountRefresh(String),
    TrayCalendarSnapshot(
        String,
        Result<crate::menus::calendar::CalendarMenuSnapshot, String>,
    ),
    TrayCalendarRefresh(String),
    TraySpawnCommand(String, String),
    TraySpawnCommandDone(String, Result<(), String>),
    TrayAnimateTick,
    TrayMenuFetched(String, Result<Vec<enhanced_tray::TrayMenuItem>, String>),
    /// Open one configured system-menu section (or "root").
    SystemMenuPressed(String),
    /// Completion of an asynchronous system-menu command.
    SystemActionDone {
        sequence: u64,
        action_id: String,
        duration_ms: u128,
        timed_out: bool,
        result: Result<String, String>,
    },

    // Legacy widget events
    LegacyWidget(WidgetMessage),
    LegacyWidgetTick(String),
    AudioProviderState(ProviderSnapshot<AudioSnapshot>),
    WifiProviderState(ProviderSnapshot<WifiSnapshot>),
    SystemProviderState(ProviderSnapshot<crate::widgets::sysmonitor::SystemStatsSnapshot>),
    VideoRefreshDone(Result<VideoSnapshot, String>),
    PowerRefreshDone(Result<PowerSnapshot, String>),
    PowerActionDone(Result<Option<PowerSnapshot>, String>),

    // Legacy tray events (keep for compatibility during transition)
    KeybindingAction(KeybindingResult),
    TrayEvent(tray::TrayEvent),
}
