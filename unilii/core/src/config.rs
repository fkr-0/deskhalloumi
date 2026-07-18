//! Configuration loader for unilii status bar.

use crate::branding::{config_dir as deskhalloumi_config_dir, legacy_config_dir};
use crate::keys::KeyBinding;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use tracing::{info, warn};

/// Main configuration structure for unilii.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Config {
    /// Panel configurations (multiple panels supported)
    pub panels: Vec<PanelConfig>,
    /// Module configurations
    pub modules: Vec<ModuleConfigEntry>,
    /// Global keybinding daemon configuration
    #[serde(default)]
    pub keybindings: Vec<KeyBinding>,
    /// Menu-specific configuration (wifi, mount, calendar, etc.)
    #[serde(default)]
    pub menus: MenusConfig,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct MenusConfig {
    /// Shared presentation and interaction policy for every popup menu.
    #[serde(default)]
    pub ui: MenuUiConfig,
    #[serde(default)]
    pub wifi: WifiMenuConfig,
    #[serde(default)]
    pub mount: MountMenuConfig,
    #[serde(default)]
    pub calendar: CalendarMenuConfig,
    #[serde(default)]
    pub custom: CustomMenuConfig,
    /// Built-in menubar system menus (Wi-Fi, displays, stats, shortcuts, power/session).
    #[serde(default)]
    pub system: SystemMenuConfig,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct MenuUiConfig {
    /// Maximum number of rows shown before the menu body becomes scrollable.
    #[serde(default = "default_menu_max_visible_rows")]
    pub max_visible_rows: usize,
    /// Maximum title length before an ellipsis is appended.
    #[serde(default = "default_menu_max_label_chars")]
    pub max_label_chars: usize,
    /// Maximum secondary-text length before an ellipsis is appended.
    #[serde(default = "default_menu_max_subtitle_chars")]
    pub max_subtitle_chars: usize,
    /// Height of scrollable menu bodies in pixels.
    #[serde(default = "default_menu_scroll_height")]
    pub scroll_height: u16,
    #[serde(default = "default_true")]
    pub show_breadcrumbs: bool,
    #[serde(default = "default_true")]
    pub show_item_counts: bool,
    #[serde(default = "default_true")]
    pub show_keyboard_hints: bool,
    #[serde(default = "default_true")]
    pub show_all_favorites_controls: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct WifiMenuConfig {
    #[serde(default = "default_wifi_refresh_ms")]
    pub refresh_ms: u64,
    #[serde(default = "default_wifi_max_network_rows")]
    pub max_network_rows: usize,
    #[serde(default = "default_true")]
    pub show_known_networks: bool,
    #[serde(default = "default_true")]
    pub allow_forget: bool,
    #[serde(default = "default_wifi_settings_command")]
    pub settings_command: String,
    #[serde(default = "default_true")]
    pub scan_on_open: bool,
    #[serde(default = "default_wifi_connect_timeout_ms")]
    pub connect_timeout_ms: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SystemMenuConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Replace the old inline Wi-Fi/video/power/sysmonitor widgets with popup buttons.
    #[serde(default = "default_true")]
    pub replace_legacy_widgets: bool,
    #[serde(default = "default_system_menu_buttons")]
    pub buttons: Vec<SystemMenuButtonConfig>,
    #[serde(default = "default_system_sections")]
    pub sections: Vec<String>,
    /// Optional YAML file with named xrandr presets for the Displays menu.
    #[serde(default)]
    pub xrandr_presets_yaml: Option<String>,
    #[serde(default = "default_system_stats_command")]
    pub stats_command: String,
    #[serde(default = "default_system_lock_command")]
    pub lock_command: String,
    #[serde(default = "default_system_logout_command")]
    pub logout_command: String,
    #[serde(default = "default_system_suspend_command")]
    pub suspend_command: String,
    #[serde(default = "default_system_reboot_command")]
    pub reboot_command: String,
    #[serde(default = "default_system_poweroff_command")]
    pub poweroff_command: String,
    #[serde(default = "default_system_idle_status_command")]
    pub idle_status_command: String,
    #[serde(default = "default_system_idle_enable_command")]
    pub idle_enable_command: String,
    #[serde(default = "default_system_idle_disable_command")]
    pub idle_disable_command: String,
    #[serde(default = "default_true")]
    pub confirm_destructive: bool,
    #[serde(default = "default_system_command_timeout_ms")]
    pub command_timeout_ms: u64,
    #[serde(default = "default_system_shortcut_limit")]
    pub shortcut_limit: usize,
    #[serde(default)]
    pub extra_items: Vec<SystemMenuItemConfig>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SystemMenuButtonConfig {
    pub id: String,
    /// Root section to open. Use "root" for the combined menu.
    pub section: String,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SystemMenuItemConfig {
    pub id: String,
    pub title: String,
    pub command: String,
    #[serde(default)]
    pub shortcut: Option<String>,
    #[serde(default)]
    pub confirm: bool,
    #[serde(default = "default_system_extra_section")]
    pub section: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct MountMenuConfig {
    #[serde(default = "default_mount_refresh_ms")]
    pub refresh_ms: u64,
    #[serde(default = "default_true")]
    pub show_loop_devices: bool,
    #[serde(default = "default_mount_max_rows")]
    pub max_local_rows: usize,
    #[serde(default = "default_mount_profile_rows")]
    pub max_sshfs_rows: usize,
    #[serde(default = "default_mount_profile_rows")]
    pub max_loop_rows: usize,
    #[serde(default = "default_mount_profile_rows")]
    pub max_vcvolume_rows: usize,
    #[serde(default = "default_mount_disks_command")]
    pub disks_command: String,
    #[serde(default = "default_true")]
    pub show_device_details: bool,
    #[serde(default)]
    pub sshfs_profiles: Vec<SshfsProfileConfig>,
    #[serde(default)]
    pub vcvolume_profiles: Vec<VcvolumeProfileConfig>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SshfsProfileConfig {
    pub name: String,
    pub user: String,
    pub host: String,
    pub remote_path: String,
    pub mountpoint: String,
    #[serde(default)]
    pub options: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct VcvolumeProfileConfig {
    pub name: String,
    pub volume_path: String,
    pub mountpoint: String,
    #[serde(default = "default_vcvolume_template")]
    pub command_template: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CalendarMenuConfig {
    #[serde(default = "default_calendar_refresh_ms")]
    pub refresh_ms: u64,
    #[serde(default = "default_calendar_agenda_days")]
    pub agenda_days: u32,
    #[serde(default = "default_calendar_account_rows")]
    pub max_account_rows: usize,
    #[serde(default = "default_calendar_event_rows")]
    pub max_event_rows: usize,
    #[serde(default = "default_calendar_application_command")]
    pub application_command: String,
    #[serde(default = "default_true")]
    pub show_locations: bool,
    #[serde(default)]
    pub accounts: Vec<CalendarAccountConfig>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CalendarAccountConfig {
    pub id: String,
    pub base_url: String,
    pub principal_url: String,
    pub calendar_url: String,
    pub username: String,
    pub secret_ref: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CustomMenuConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_custom_max_rows")]
    pub max_rows: usize,
    #[serde(default = "default_true")]
    pub show_subtitles: bool,
    #[serde(default)]
    pub app_ids: Vec<String>,
    #[serde(default)]
    pub icon_name_patterns: Vec<String>,
    #[serde(default)]
    pub include: Vec<String>,
    #[serde(default)]
    pub sources: Vec<CustomMenuSourceConfig>,
    #[serde(default)]
    pub items: Vec<CustomMenuItemConfig>,
    #[serde(default = "default_custom_quickjump_alphabet")]
    pub quickjump_alphabet: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CustomMenuSourceConfig {
    #[serde(default)]
    pub enabled: bool,
    pub path: Option<String>,
    pub glob: Option<String>,
    #[serde(default)]
    pub priority: i32,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CustomMenuItemConfig {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub subtitle: Option<String>,
    #[serde(flatten)]
    pub action: CustomMenuActionConfig,
    #[serde(default)]
    pub icon: CustomMenuIconConfig,
    #[serde(default = "default_filter_fields")]
    pub filter_fields: Vec<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub working_dir: Option<String>,
    #[serde(default)]
    pub env: Vec<CustomMenuEnvVarConfig>,
    #[serde(default)]
    pub confirm: bool,
    #[serde(default)]
    pub visible_if: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum CustomMenuActionConfig {
    Shell {
        command: String,
    },
    Launcher {
        command: String,
        #[serde(default)]
        args: Vec<String>,
        #[serde(default)]
        desktop_id: Option<String>,
    },
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct CustomMenuIconConfig {
    #[serde(default)]
    pub theme_icon: Option<String>,
    #[serde(default)]
    pub svg_path: Option<String>,
    #[serde(default)]
    pub image_path: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CustomMenuEnvVarConfig {
    pub key: String,
    pub value: String,
}

impl Default for MenuUiConfig {
    fn default() -> Self {
        Self {
            max_visible_rows: default_menu_max_visible_rows(),
            max_label_chars: default_menu_max_label_chars(),
            max_subtitle_chars: default_menu_max_subtitle_chars(),
            scroll_height: default_menu_scroll_height(),
            show_breadcrumbs: true,
            show_item_counts: true,
            show_keyboard_hints: true,
            show_all_favorites_controls: true,
        }
    }
}

impl Default for WifiMenuConfig {
    fn default() -> Self {
        Self {
            refresh_ms: default_wifi_refresh_ms(),
            max_network_rows: default_wifi_max_network_rows(),
            show_known_networks: default_true(),
            allow_forget: default_true(),
            settings_command: default_wifi_settings_command(),
            scan_on_open: default_true(),
            connect_timeout_ms: default_wifi_connect_timeout_ms(),
        }
    }
}

impl Default for MountMenuConfig {
    fn default() -> Self {
        Self {
            refresh_ms: default_mount_refresh_ms(),
            show_loop_devices: true,
            max_local_rows: default_mount_max_rows(),
            max_sshfs_rows: default_mount_profile_rows(),
            max_loop_rows: default_mount_profile_rows(),
            max_vcvolume_rows: default_mount_profile_rows(),
            disks_command: default_mount_disks_command(),
            show_device_details: true,
            sshfs_profiles: Vec::new(),
            vcvolume_profiles: Vec::new(),
        }
    }
}

impl Default for CalendarMenuConfig {
    fn default() -> Self {
        Self {
            refresh_ms: default_calendar_refresh_ms(),
            agenda_days: default_calendar_agenda_days(),
            max_account_rows: default_calendar_account_rows(),
            max_event_rows: default_calendar_event_rows(),
            application_command: default_calendar_application_command(),
            show_locations: true,
            accounts: Vec::new(),
        }
    }
}

impl Default for SystemMenuConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            replace_legacy_widgets: true,
            buttons: default_system_menu_buttons(),
            sections: default_system_sections(),
            xrandr_presets_yaml: None,
            stats_command: default_system_stats_command(),
            lock_command: default_system_lock_command(),
            logout_command: default_system_logout_command(),
            suspend_command: default_system_suspend_command(),
            reboot_command: default_system_reboot_command(),
            poweroff_command: default_system_poweroff_command(),
            idle_status_command: default_system_idle_status_command(),
            idle_enable_command: default_system_idle_enable_command(),
            idle_disable_command: default_system_idle_disable_command(),
            confirm_destructive: true,
            command_timeout_ms: default_system_command_timeout_ms(),
            shortcut_limit: default_system_shortcut_limit(),
            extra_items: Vec::new(),
        }
    }
}

impl Default for CustomMenuConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            max_rows: default_custom_max_rows(),
            show_subtitles: true,
            app_ids: Vec::new(),
            icon_name_patterns: Vec::new(),
            include: Vec::new(),
            sources: Vec::new(),
            items: Vec::new(),
            quickjump_alphabet: default_custom_quickjump_alphabet(),
        }
    }
}

impl Default for CustomMenuSourceConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            path: None,
            glob: None,
            priority: 0,
        }
    }
}

fn default_menu_max_visible_rows() -> usize {
    12
}

fn default_menu_max_label_chars() -> usize {
    72
}

fn default_menu_max_subtitle_chars() -> usize {
    110
}

fn default_menu_scroll_height() -> u16 {
    320
}

fn default_true() -> bool {
    true
}

fn default_wifi_refresh_ms() -> u64 {
    4_000
}

fn default_wifi_connect_timeout_ms() -> u64 {
    20_000
}

fn default_wifi_max_network_rows() -> usize {
    20
}

fn default_wifi_settings_command() -> String {
    "nm-connection-editor".to_string()
}

fn default_mount_refresh_ms() -> u64 {
    5_000
}

fn default_mount_max_rows() -> usize {
    24
}

fn default_mount_profile_rows() -> usize {
    12
}

fn default_mount_disks_command() -> String {
    "gnome-disks".to_string()
}

fn default_vcvolume_template() -> String {
    "veracrypt --text {volume} {mountpoint}".to_string()
}

fn default_calendar_refresh_ms() -> u64 {
    300_000
}

fn default_calendar_agenda_days() -> u32 {
    7
}

fn default_custom_quickjump_alphabet() -> String {
    "asdfjkl;ghqwertyuiopzxcvbnm".to_string()
}

fn default_system_menu_buttons() -> Vec<SystemMenuButtonConfig> {
    vec![
        SystemMenuButtonConfig {
            id: "wifi".into(),
            section: "wifi".into(),
            label: None,
            enabled: true,
        },
        SystemMenuButtonConfig {
            id: "displays".into(),
            section: "displays".into(),
            label: Some("🖥".into()),
            enabled: true,
        },
        SystemMenuButtonConfig {
            id: "stats".into(),
            section: "stats".into(),
            label: None,
            enabled: true,
        },
        SystemMenuButtonConfig {
            id: "shortcuts".into(),
            section: "shortcuts".into(),
            label: Some("⌨".into()),
            enabled: true,
        },
        SystemMenuButtonConfig {
            id: "power".into(),
            section: "power".into(),
            label: Some("⏻".into()),
            enabled: true,
        },
    ]
}

fn default_calendar_account_rows() -> usize {
    8
}

fn default_calendar_event_rows() -> usize {
    24
}

fn default_calendar_application_command() -> String {
    "gnome-calendar".to_string()
}

fn default_custom_max_rows() -> usize {
    40
}

fn default_system_sections() -> Vec<String> {
    vec!["wifi", "displays", "stats", "shortcuts", "power", "extra"]
        .into_iter()
        .map(str::to_string)
        .collect()
}

fn default_system_stats_command() -> String {
    "x-terminal-emulator -e htop".to_string()
}
fn default_system_lock_command() -> String {
    "loginctl lock-session".to_string()
}
fn default_system_logout_command() -> String {
    "i3-msg exit".to_string()
}
fn default_system_suspend_command() -> String {
    "systemctl suspend".to_string()
}
fn default_system_reboot_command() -> String {
    "systemctl reboot".to_string()
}
fn default_system_poweroff_command() -> String {
    "systemctl poweroff".to_string()
}
fn default_system_idle_status_command() -> String {
    "xset q".to_string()
}
fn default_system_idle_enable_command() -> String {
    "xset s 600 600 +dpms dpms 0 0 900".to_string()
}
fn default_system_idle_disable_command() -> String {
    "xset s off -dpms".to_string()
}
fn default_system_command_timeout_ms() -> u64 {
    30_000
}
fn default_system_shortcut_limit() -> usize {
    40
}
fn default_system_extra_section() -> String {
    "extra".to_string()
}

fn default_filter_fields() -> Vec<String> {
    vec!["title".to_string()]
}

/// Panel configuration settings.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PanelConfig {
    /// Panel name (for identification)
    #[serde(default)]
    pub name: String,
    /// Panel width in pixels
    pub width: u32,
    /// Panel height in pixels
    pub height: u32,
    /// Panel position x coordinate
    pub position_x: i32,
    /// Panel position y coordinate
    pub position_y: i32,
    /// Background color (hex format)
    pub background_color: Option<String>,
    /// Text color (hex format)
    pub text_color: Option<String>,
}

/// Module configuration entry.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ModuleConfigEntry {
    /// Module name (e.g., "clock", "battery")
    pub name: String,
    /// Whether the module is enabled
    pub enabled: bool,
    /// Module position (left, center, right)
    pub position: String,
    /// Update interval in milliseconds
    pub update_interval_ms: Option<u64>,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            panels: vec![PanelConfig {
                name: "top_bar".to_string(),
                width: 800,
                height: 24,
                position_x: 0,
                position_y: 0,
                background_color: Some("#1e1e1e".to_string()),
                text_color: Some("#ffffff".to_string()),
            }],
            modules: vec![
                ModuleConfigEntry {
                    name: "clock".to_string(),
                    enabled: true,
                    position: "right".to_string(),
                    update_interval_ms: Some(1000),
                },
                ModuleConfigEntry {
                    name: "battery".to_string(),
                    enabled: true,
                    position: "right".to_string(),
                    update_interval_ms: Some(5000),
                },
            ],
            keybindings: Vec::new(),
            menus: MenusConfig::default(),
        }
    }
}

impl Default for PanelConfig {
    fn default() -> Self {
        PanelConfig {
            name: "default".to_string(),
            width: 800,
            height: 24,
            position_x: 0,
            position_y: 0,
            background_color: Some("#1e1e1e".to_string()),
            text_color: Some("#ffffff".to_string()),
        }
    }
}

/// Get the default configuration directory path.
pub fn get_config_dir() -> Option<PathBuf> {
    deskhalloumi_config_dir()
}

/// Get the configuration file path.
pub fn get_config_path() -> Option<PathBuf> {
    let current = get_config_dir()?.join("deskhalloumi.toml");
    if current.exists() {
        return Some(current);
    }
    let legacy = legacy_config_dir()?.join("unilii.toml");
    if legacy.exists() {
        Some(legacy)
    } else {
        Some(current)
    }
}

/// Create default configuration file at the standard location.
pub fn create_default_config() -> std::io::Result<PathBuf> {
    let config_path = get_config_dir()
        .map(|dir| dir.join("deskhalloumi.toml"))
        .ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "Could not determine config directory",
            )
        })?;

    // Create config directory if it doesn't exist
    if let Some(parent) = config_path.parent() {
        fs::create_dir_all(parent)?;
    }

    // Write default config
    let default_config = Config::default();
    let toml_string = toml::to_string_pretty(&default_config).map_err(std::io::Error::other)?;

    fs::write(&config_path, toml_string)?;
    info!("Created default config at: {:?}", config_path);

    Ok(config_path)
}

pub fn validate_menu_ui_config(config: &MenuUiConfig) -> Result<(), String> {
    if !(4..=100).contains(&config.max_visible_rows) {
        return Err("menus.ui.max_visible_rows must be between 4 and 100".to_string());
    }
    if !(16..=300).contains(&config.max_label_chars) {
        return Err("menus.ui.max_label_chars must be between 16 and 300".to_string());
    }
    if !(24..=500).contains(&config.max_subtitle_chars) {
        return Err("menus.ui.max_subtitle_chars must be between 24 and 500".to_string());
    }
    if !(120..=1200).contains(&config.scroll_height) {
        return Err("menus.ui.scroll_height must be between 120 and 1200".to_string());
    }
    Ok(())
}

pub fn validate_wifi_menu_config(config: &WifiMenuConfig) -> Result<(), String> {
    if !(250..=3_600_000).contains(&config.refresh_ms) {
        return Err("menus.wifi.refresh_ms must be between 250 and 3600000".to_string());
    }
    if !(1..=200).contains(&config.max_network_rows) {
        return Err("menus.wifi.max_network_rows must be between 1 and 200".to_string());
    }
    if !(100..=300_000).contains(&config.connect_timeout_ms) {
        return Err("menus.wifi.connect_timeout_ms must be between 100 and 300000".to_string());
    }
    Ok(())
}

pub fn validate_mount_menu_config(config: &MountMenuConfig) -> Result<(), String> {
    if !(250..=3_600_000).contains(&config.refresh_ms) {
        return Err("menus.mount.refresh_ms must be between 250 and 3600000".to_string());
    }
    for (field, value) in [
        ("max_local_rows", config.max_local_rows),
        ("max_sshfs_rows", config.max_sshfs_rows),
        ("max_loop_rows", config.max_loop_rows),
        ("max_vcvolume_rows", config.max_vcvolume_rows),
    ] {
        if !(1..=200).contains(&value) {
            return Err(format!("menus.mount.{field} must be between 1 and 200"));
        }
    }

    let mut names = HashSet::new();
    for profile in &config.sshfs_profiles {
        if profile.name.trim().is_empty()
            || profile.user.trim().is_empty()
            || profile.host.trim().is_empty()
            || profile.remote_path.trim().is_empty()
            || profile.mountpoint.trim().is_empty()
        {
            return Err(
                "menus.mount.sshfs_profiles require name, user, host, remote_path, and mountpoint"
                    .to_string(),
            );
        }
        if !names.insert(profile.name.to_ascii_lowercase()) {
            return Err(format!(
                "menus.mount.sshfs_profiles contains duplicate name '{}'",
                profile.name
            ));
        }
        if profile
            .options
            .iter()
            .any(|option| option.trim().is_empty())
        {
            return Err(format!(
                "menus.mount SSHFS profile '{}' contains an empty option",
                profile.name
            ));
        }
    }

    let mut names = HashSet::new();
    for profile in &config.vcvolume_profiles {
        if profile.name.trim().is_empty()
            || profile.volume_path.trim().is_empty()
            || profile.mountpoint.trim().is_empty()
            || profile.command_template.trim().is_empty()
        {
            return Err(
                "menus.mount.vcvolume_profiles require name, volume_path, mountpoint, and command_template"
                    .to_string(),
            );
        }
        if !names.insert(profile.name.to_ascii_lowercase()) {
            return Err(format!(
                "menus.mount.vcvolume_profiles contains duplicate name '{}'",
                profile.name
            ));
        }
        if !profile.command_template.contains("{volume}")
            || !profile.command_template.contains("{mountpoint}")
        {
            return Err(format!(
                "menus.mount VCVolume profile '{}' command_template must contain {{volume}} and {{mountpoint}}",
                profile.name
            ));
        }
    }
    Ok(())
}

pub fn validate_calendar_menu_config(config: &CalendarMenuConfig) -> Result<(), String> {
    if !(1_000..=86_400_000).contains(&config.refresh_ms) {
        return Err("menus.calendar.refresh_ms must be between 1000 and 86400000".to_string());
    }
    if !(1..=365).contains(&config.agenda_days) {
        return Err("menus.calendar.agenda_days must be between 1 and 365".to_string());
    }
    for (field, value) in [
        ("max_account_rows", config.max_account_rows),
        ("max_event_rows", config.max_event_rows),
    ] {
        if !(1..=500).contains(&value) {
            return Err(format!("menus.calendar.{field} must be between 1 and 500"));
        }
    }
    let mut ids = HashSet::new();
    for account in &config.accounts {
        if account.id.trim().is_empty()
            || account.base_url.trim().is_empty()
            || account.principal_url.trim().is_empty()
            || account.calendar_url.trim().is_empty()
            || account.username.trim().is_empty()
            || account.secret_ref.trim().is_empty()
        {
            return Err(
                "menus.calendar.accounts require id, URLs, username, and secret_ref".to_string(),
            );
        }
        if !ids.insert(account.id.to_ascii_lowercase()) {
            return Err(format!(
                "menus.calendar.accounts contains duplicate id '{}'",
                account.id
            ));
        }
    }
    Ok(())
}

fn valid_custom_env_key(key: &str) -> bool {
    let mut chars = key.chars();
    matches!(chars.next(), Some(first) if first == '_' || first.is_ascii_alphabetic())
        && chars.all(|ch| ch == '_' || ch.is_ascii_alphanumeric())
}

fn valid_custom_visibility_condition(condition: &str) -> bool {
    let mut condition = condition.trim();
    while let Some(inner) = condition.strip_prefix("not:") {
        condition = inner.trim();
    }
    ["env:", "path:", "command:"].iter().any(|prefix| {
        condition
            .strip_prefix(prefix)
            .is_some_and(|value| !value.trim().is_empty())
    })
}

pub fn validate_custom_menu_config(config: &CustomMenuConfig) -> Result<(), String> {
    if !(1..=1_000).contains(&config.max_rows) {
        return Err("menus.custom.max_rows must be between 1 and 1000".to_string());
    }
    if config.quickjump_alphabet.is_empty()
        || config
            .quickjump_alphabet
            .chars()
            .any(|ch| ch.is_whitespace() || ch.is_control())
    {
        return Err(
            "menus.custom.quickjump_alphabet must contain visible non-whitespace characters"
                .to_string(),
        );
    }
    let unique_alphabet = config.quickjump_alphabet.chars().collect::<HashSet<_>>();
    if unique_alphabet.len() != config.quickjump_alphabet.chars().count() {
        return Err("menus.custom.quickjump_alphabet must not contain duplicates".to_string());
    }
    if config.include.iter().any(|spec| spec.trim().is_empty()) {
        return Err("menus.custom.include contains an empty path or glob".to_string());
    }
    for source in &config.sources {
        if source.enabled
            && source
                .path
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .is_none()
            && source
                .glob
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .is_none()
        {
            return Err("enabled menus.custom.sources entries require path or glob".to_string());
        }
    }

    let allowed_filter_fields = ["title", "subtitle", "id", "command", "tags"];
    let mut ids = HashSet::new();
    for item in &config.items {
        if item.id.trim().is_empty() || item.title.trim().is_empty() {
            return Err("menus.custom.items require non-empty id and title".to_string());
        }
        if !ids.insert(item.id.to_ascii_lowercase()) {
            return Err(format!(
                "menus.custom.items contains duplicate id '{}'",
                item.id
            ));
        }
        let command = match &item.action {
            CustomMenuActionConfig::Shell { command }
            | CustomMenuActionConfig::Launcher { command, .. } => command,
        };
        if command.trim().is_empty() {
            return Err(format!(
                "menus.custom item '{}' has an empty command",
                item.id
            ));
        }
        if let Some(directory) = &item.working_dir
            && directory.trim().is_empty()
        {
            return Err(format!(
                "menus.custom item '{}' has an empty working_dir",
                item.id
            ));
        }
        if let Some(condition) = &item.visible_if
            && !valid_custom_visibility_condition(condition)
        {
            return Err(format!(
                "menus.custom item '{}' has unsupported visible_if '{}'; use env:, path:, command:, or not:",
                item.id, condition
            ));
        }
        if item
            .filter_fields
            .iter()
            .any(|field| !allowed_filter_fields.contains(&field.as_str()))
        {
            return Err(format!(
                "menus.custom item '{}' contains an unsupported filter field",
                item.id
            ));
        }
        let icon_count = [
            item.icon.theme_icon.as_deref(),
            item.icon.svg_path.as_deref(),
            item.icon.image_path.as_deref(),
        ]
        .into_iter()
        .flatten()
        .filter(|value| !value.trim().is_empty())
        .count();
        if icon_count > 1 {
            return Err(format!(
                "menus.custom item '{}' must configure at most one icon source",
                item.id
            ));
        }
        let mut env_keys = HashSet::new();
        for variable in &item.env {
            if !valid_custom_env_key(&variable.key) {
                return Err(format!(
                    "menus.custom item '{}' has invalid environment key '{}'",
                    item.id, variable.key
                ));
            }
            if !env_keys.insert(variable.key.clone()) {
                return Err(format!(
                    "menus.custom item '{}' repeats environment key '{}'",
                    item.id, variable.key
                ));
            }
        }
    }
    Ok(())
}

pub fn validate_system_menu_config(config: &SystemMenuConfig) -> Result<(), String> {
    const SECTIONS: &[&str] = &[
        "root",
        "wifi",
        "displays",
        "stats",
        "shortcuts",
        "power",
        "extra",
    ];
    if config.command_timeout_ms < 100 || config.command_timeout_ms > 300_000 {
        return Err("menus.system.command_timeout_ms must be between 100 and 300000".to_string());
    }
    if config.shortcut_limit == 0 || config.shortcut_limit > 500 {
        return Err("menus.system.shortcut_limit must be between 1 and 500".to_string());
    }

    let mut section_ids = HashSet::new();
    for section in &config.sections {
        if !SECTIONS[1..].contains(&section.as_str()) {
            return Err(format!(
                "menus.system.sections contains unknown section '{section}'"
            ));
        }
        if !section_ids.insert(section.to_ascii_lowercase()) {
            return Err(format!(
                "menus.system.sections contains duplicate section '{section}'"
            ));
        }
    }

    let mut ids = HashSet::new();
    for button in &config.buttons {
        if button.id.trim().is_empty() {
            return Err("menus.system.buttons contains an empty id".to_string());
        }
        if !ids.insert(button.id.to_ascii_lowercase()) {
            return Err(format!(
                "menus.system.buttons contains duplicate id '{}'",
                button.id
            ));
        }
        if !SECTIONS.contains(&button.section.as_str()) {
            return Err(format!(
                "menus.system button '{}' references unknown section '{}'",
                button.id, button.section
            ));
        }
        if button.enabled
            && button.section != "root"
            && !section_ids.contains(&button.section.to_ascii_lowercase())
        {
            return Err(format!(
                "menus.system button '{}' opens section '{}' which is not listed in menus.system.sections",
                button.id, button.section
            ));
        }
    }

    let mut extra_ids = HashSet::new();
    for item in &config.extra_items {
        if item.id.trim().is_empty() || item.title.trim().is_empty() {
            return Err("menus.system.extra_items require non-empty id and title".to_string());
        }
        if !extra_ids.insert(item.id.to_ascii_lowercase()) {
            return Err(format!(
                "menus.system.extra_items contains duplicate id '{}'",
                item.id
            ));
        }
        if item.command.trim().is_empty() {
            return Err(format!(
                "menus.system extra item '{}' has an empty command",
                item.id
            ));
        }
        if item.section != "extra" {
            return Err(format!(
                "menus.system extra item '{}' uses unsupported section '{}'",
                item.id, item.section
            ));
        }
    }
    Ok(())
}

/// Load configuration from file, or create default if it doesn't exist.
pub fn load_config() -> Config {
    load_config_with_path(None)
}

/// Load configuration from a specific path, or create default if it doesn't exist.
pub fn load_config_with_path(config_path_override: Option<PathBuf>) -> Config {
    let config_path = config_path_override.or_else(get_config_path);

    let config_path = match config_path {
        Some(path) => path,
        None => {
            warn!("Could not determine config directory, using defaults");
            return Config::default();
        }
    };

    // Try to load existing config
    if config_path.exists() {
        match fs::read_to_string(&config_path) {
            Ok(contents) => match toml::from_str::<Config>(&contents) {
                Ok(mut config) => {
                    resolve_custom_menu_includes(&config_path, &mut config.menus.custom);
                    if let Err(error) = validate_menu_ui_config(&config.menus.ui) {
                        warn!(
                            "Invalid shared menu UI configuration: {}; using menu UI defaults",
                            error
                        );
                        config.menus.ui = MenuUiConfig::default();
                    }
                    if let Err(error) = validate_wifi_menu_config(&config.menus.wifi) {
                        warn!(
                            "Invalid Wi-Fi menu configuration: {}; using Wi-Fi menu defaults",
                            error
                        );
                        config.menus.wifi = WifiMenuConfig::default();
                    }
                    if let Err(error) = validate_mount_menu_config(&config.menus.mount) {
                        warn!(
                            "Invalid mount menu configuration: {}; using mount menu defaults",
                            error
                        );
                        config.menus.mount = MountMenuConfig::default();
                    }
                    if let Err(error) = validate_calendar_menu_config(&config.menus.calendar) {
                        warn!(
                            "Invalid calendar menu configuration: {}; using calendar menu defaults",
                            error
                        );
                        config.menus.calendar = CalendarMenuConfig::default();
                    }
                    if let Err(error) = validate_custom_menu_config(&config.menus.custom) {
                        warn!(
                            "Invalid custom menu configuration: {}; using custom menu defaults",
                            error
                        );
                        config.menus.custom = CustomMenuConfig::default();
                    }
                    if let Err(error) = validate_system_menu_config(&config.menus.system) {
                        warn!(
                            "Invalid system-menu configuration: {}; using system-menu defaults",
                            error
                        );
                        config.menus.system = SystemMenuConfig::default();
                    }
                    info!("Loaded config from: {:?}", config_path);
                    return config;
                }
                Err(e) => {
                    warn!("Failed to parse config file: {}, using defaults", e);
                }
            },
            Err(e) => {
                warn!("Failed to read config file: {}, using defaults", e);
            }
        }
    }

    // Create default config if it doesn't exist or failed to load
    info!("Config file not found or invalid, creating default");
    match create_default_config() {
        Ok(_) => Config::default(),
        Err(e) => {
            warn!(
                "Failed to create default config: {}, using hardcoded defaults",
                e
            );
            Config::default()
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
struct CustomMenuIncludeRoot {
    #[serde(default)]
    menus: CustomMenuIncludeMenus,
    #[serde(default)]
    custom: Option<CustomMenuConfig>,
    #[serde(default)]
    include: Vec<String>,
    #[serde(default)]
    sources: Vec<CustomMenuSourceConfig>,
    #[serde(default)]
    items: Vec<CustomMenuItemConfig>,
}

#[derive(Debug, Clone, Deserialize, Default)]
struct CustomMenuIncludeMenus {
    #[serde(default)]
    custom: Option<CustomMenuConfig>,
}

fn resolve_custom_menu_includes(config_path: &Path, custom: &mut CustomMenuConfig) {
    let mut visited = HashSet::new();
    let mut merged = Vec::new();
    let base_dir = config_path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));

    let includes = custom.include.clone();
    for include in includes {
        merged.extend(load_custom_menu_sources(
            &base_dir,
            &include,
            &mut visited,
            "include",
        ));
    }

    let mut sources = custom.sources.clone();
    sources.sort_by_key(|source| source.priority);
    for source in sources {
        if !source.enabled {
            continue;
        }
        if let Some(path) = source.path {
            merged.extend(load_custom_menu_sources(
                &base_dir,
                &path,
                &mut visited,
                "source.path",
            ));
        }
        if let Some(glob_pattern) = source.glob {
            merged.extend(load_custom_menu_sources(
                &base_dir,
                &glob_pattern,
                &mut visited,
                "source.glob",
            ));
        }
    }

    merged.extend(custom.items.clone());
    custom.items = dedup_custom_menu_items(merged);
}

fn load_custom_menu_sources(
    base_dir: &Path,
    spec: &str,
    visited: &mut HashSet<PathBuf>,
    scope: &str,
) -> Vec<CustomMenuItemConfig> {
    let mut items = Vec::new();
    let candidate = expand_custom_sources(base_dir, spec);

    for path in candidate {
        let canonical = path.canonicalize().unwrap_or(path.clone());
        if !visited.insert(canonical.clone()) {
            continue;
        }
        let content = match fs::read_to_string(&canonical) {
            Ok(content) => content,
            Err(error) => {
                warn!(
                    "Failed to read custom menu {} '{}': {}",
                    scope,
                    canonical.display(),
                    error
                );
                continue;
            }
        };
        let root = match toml::from_str::<CustomMenuIncludeRoot>(&content) {
            Ok(parsed) => parsed,
            Err(error) => {
                warn!(
                    "Failed to parse custom menu {} '{}': {}",
                    scope,
                    canonical.display(),
                    error
                );
                continue;
            }
        };
        let parsed_custom = root
            .menus
            .custom
            .or(root.custom)
            .unwrap_or(CustomMenuConfig {
                enabled: false,
                max_rows: default_custom_max_rows(),
                show_subtitles: true,
                app_ids: Vec::new(),
                icon_name_patterns: Vec::new(),
                include: root.include,
                sources: root.sources,
                items: root.items,
                quickjump_alphabet: default_custom_quickjump_alphabet(),
            });

        let nested_base = canonical
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| base_dir.to_path_buf());

        let nested_includes = parsed_custom.include.clone();
        for include in nested_includes {
            items.extend(load_custom_menu_sources(
                &nested_base,
                &include,
                visited,
                "nested include",
            ));
        }

        let mut nested_sources = parsed_custom.sources.clone();
        nested_sources.sort_by_key(|source| source.priority);
        for source in nested_sources {
            if !source.enabled {
                continue;
            }
            if let Some(path) = source.path {
                items.extend(load_custom_menu_sources(
                    &nested_base,
                    &path,
                    visited,
                    "nested source.path",
                ));
            }
            if let Some(glob_pattern) = source.glob {
                items.extend(load_custom_menu_sources(
                    &nested_base,
                    &glob_pattern,
                    visited,
                    "nested source.glob",
                ));
            }
        }

        items.extend(parsed_custom.items);
    }

    dedup_custom_menu_items(items)
}

fn expand_custom_sources(base_dir: &Path, spec: &str) -> Vec<PathBuf> {
    let expanded = if spec.starts_with("~/") {
        if let Ok(home) = std::env::var("HOME") {
            PathBuf::from(home).join(spec.trim_start_matches("~/"))
        } else {
            base_dir.join(spec)
        }
    } else if Path::new(spec).is_absolute() {
        PathBuf::from(spec)
    } else {
        base_dir.join(spec)
    };

    let pattern = expanded.to_string_lossy().to_string();
    if pattern.contains('*') || pattern.contains('?') || pattern.contains('[') {
        let mut matches = glob::glob(&pattern)
            .ok()
            .into_iter()
            .flat_map(|entries| entries.filter_map(Result::ok))
            .collect::<Vec<_>>();
        matches.sort();
        matches
    } else {
        vec![expanded]
    }
}

fn dedup_custom_menu_items(items: Vec<CustomMenuItemConfig>) -> Vec<CustomMenuItemConfig> {
    let mut deduped = Vec::new();
    let mut seen = HashSet::new();
    for item in items.into_iter().rev() {
        if seen.insert(item.id.clone()) {
            deduped.push(item);
        }
    }
    deduped.reverse();
    deduped
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn test_default_config() {
        let config = Config::default();
        assert_eq!(config.panels.len(), 1);
        assert_eq!(config.panels[0].width, 800);
        assert_eq!(config.panels[0].height, 24);
        assert_eq!(config.modules.len(), 2);
        assert!(config.keybindings.is_empty());
        assert_eq!(config.menus.wifi.refresh_ms, 4_000);
        assert!(config.menus.wifi.show_known_networks);
        assert_eq!(config.menus.mount.refresh_ms, 5_000);
        assert!(config.menus.mount.show_loop_devices);
        assert_eq!(config.menus.calendar.refresh_ms, 300_000);
        assert_eq!(config.menus.calendar.agenda_days, 7);
        assert!(config.menus.calendar.accounts.is_empty());
        assert!(!config.menus.custom.enabled);
        assert!(config.menus.custom.items.is_empty());
        assert_eq!(config.modules[0].name, "clock");
        assert_eq!(config.modules[1].name, "battery");
    }

    #[test]
    fn test_config_serialization() {
        let config = Config::default();
        let toml_string = toml::to_string_pretty(&config).unwrap();
        assert!(toml_string.contains("clock"));
        assert!(toml_string.contains("battery"));
    }

    #[test]
    fn parses_custom_menu_inline_item() {
        let config_toml = r#"
panels = [{ name = "top", width = 800, height = 24, position_x = 0, position_y = 0 }]
modules = []

[menus.custom]
enabled = true

[[menus.custom.items]]
id = "xrandr.docked"
title = "Docked"
action = "shell"
command = "~/bin/xrandr-docked.sh"
filter_fields = ["title", "command"]
"#;

        let config: Config = toml::from_str(config_toml).expect("config must parse");
        assert!(config.menus.custom.enabled);
        assert_eq!(config.menus.custom.items.len(), 1);
        assert_eq!(config.menus.custom.items[0].id, "xrandr.docked");
    }

    #[test]
    fn resolves_custom_menu_includes_and_dedups_by_id() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let base = std::env::temp_dir().join(format!("unilii-config-test-{unique}"));
        std::fs::create_dir_all(&base).expect("create temp base");

        let included = base.join("custom-items.toml");
        std::fs::write(
            &included,
            r#"
[[items]]
id = "monitor.docked"
title = "Docked"
action = "shell"
command = "~/bin/xrandr-docked.sh"

[[items]]
id = "monitor.mirror"
title = "Mirror"
action = "shell"
command = "~/bin/xrandr-mirror.sh"
"#,
        )
        .expect("write include");

        let root = base.join("unilii.toml");
        std::fs::write(
            &root,
            format!(
                r#"
panels = [{{ name = "top", width = 800, height = 24, position_x = 0, position_y = 0 }}]
modules = []

[menus.custom]
enabled = true
include = ["{}"]

[[menus.custom.items]]
id = "monitor.mirror"
title = "Mirror override"
action = "shell"
command = "~/bin/xrandr-mirror-override.sh"
"#,
                included.display()
            ),
        )
        .expect("write root");

        let loaded = load_config_with_path(Some(root));
        assert_eq!(loaded.menus.custom.items.len(), 2);
        let mirror = loaded
            .menus
            .custom
            .items
            .iter()
            .find(|item| item.id == "monitor.mirror")
            .expect("mirror item");
        assert_eq!(mirror.title, "Mirror override");
    }

    #[test]
    fn default_system_menu_configuration_is_valid() {
        assert_eq!(validate_menu_ui_config(&MenuUiConfig::default()), Ok(()));
        assert_eq!(
            validate_wifi_menu_config(&WifiMenuConfig::default()),
            Ok(())
        );
        assert_eq!(
            validate_mount_menu_config(&MountMenuConfig::default()),
            Ok(())
        );
        assert_eq!(
            validate_calendar_menu_config(&CalendarMenuConfig::default()),
            Ok(())
        );
        assert_eq!(
            validate_custom_menu_config(&CustomMenuConfig::default()),
            Ok(())
        );
        assert_eq!(
            validate_system_menu_config(&SystemMenuConfig::default()),
            Ok(())
        );
    }

    #[test]
    fn domain_menu_validation_rejects_release_blocking_values() {
        let wifi = WifiMenuConfig {
            max_network_rows: 0,
            ..Default::default()
        };
        assert!(validate_wifi_menu_config(&wifi).is_err());

        let mut mount = MountMenuConfig::default();
        mount.vcvolume_profiles.push(VcvolumeProfileConfig {
            name: "vault".into(),
            volume_path: "/vault.hc".into(),
            mountpoint: "/mnt/vault".into(),
            command_template: "veracrypt {volume}".into(),
        });
        assert!(
            validate_mount_menu_config(&mount)
                .unwrap_err()
                .contains("{mountpoint}")
        );

        let calendar = CalendarMenuConfig {
            agenda_days: 0,
            ..Default::default()
        };
        assert!(validate_calendar_menu_config(&calendar).is_err());

        let mut custom = CustomMenuConfig {
            enabled: true,
            ..Default::default()
        };
        custom.items.push(CustomMenuItemConfig {
            id: "bad-env".into(),
            title: "Bad environment".into(),
            subtitle: None,
            action: CustomMenuActionConfig::Shell {
                command: "true".into(),
            },
            icon: CustomMenuIconConfig::default(),
            filter_fields: vec!["title".into()],
            tags: vec![],
            working_dir: None,
            env: vec![CustomMenuEnvVarConfig {
                key: "1INVALID".into(),
                value: "x".into(),
            }],
            confirm: false,
            visible_if: None,
        });
        assert!(
            validate_custom_menu_config(&custom)
                .unwrap_err()
                .contains("invalid environment key")
        );
    }

    #[test]
    fn invalid_menu_slice_falls_back_without_discarding_other_configuration() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let base = std::env::temp_dir().join(format!("unilii-menu-fallback-{unique}"));
        std::fs::create_dir_all(&base).expect("create temp base");
        let root = base.join("unilii.toml");
        std::fs::write(
            &root,
            r#"
panels = [{ name = "kept", width = 1234, height = 31, position_x = 0, position_y = 0 }]
modules = []

[menus.ui]
max_visible_rows = 0

[menus.wifi]
refresh_ms = 9000
max_network_rows = 18
show_known_networks = true
allow_forget = true
settings_command = "custom-network-editor"
scan_on_open = true
connect_timeout_ms = 15000
"#,
        )
        .expect("write config");

        let loaded = load_config_with_path(Some(root));
        assert_eq!(loaded.panels[0].name, "kept");
        assert_eq!(loaded.panels[0].width, 1234);
        assert_eq!(
            loaded.menus.ui.max_visible_rows,
            MenuUiConfig::default().max_visible_rows
        );
        assert_eq!(loaded.menus.wifi.refresh_ms, 9000);
        assert_eq!(loaded.menus.wifi.settings_command, "custom-network-editor");
    }

    #[test]
    fn system_menu_validation_rejects_duplicate_buttons_and_hidden_sections() {
        let mut duplicate = SystemMenuConfig::default();
        duplicate.buttons.push(duplicate.buttons[0].clone());
        assert!(
            validate_system_menu_config(&duplicate)
                .unwrap_err()
                .contains("duplicate id")
        );

        let mut hidden = SystemMenuConfig::default();
        hidden.sections.retain(|section| section != "wifi");
        assert!(
            validate_system_menu_config(&hidden)
                .unwrap_err()
                .contains("not listed")
        );
    }

    #[test]
    fn parses_configurable_system_menubar() {
        let config_toml = r#"
panels = [{ name = "top", width = 800, height = 24, position_x = 0, position_y = 0 }]
modules = []

[menus.system]
enabled = true
replace_legacy_widgets = true
sections = ["wifi", "stats", "power", "extra"]
shortcut_limit = 25
poweroff_command = "sessionctl poweroff"

[[menus.system.buttons]]
id = "combined"
section = "root"
label = "Menu"
enabled = true

[[menus.system.extra_items]]
id = "logs"
title = "System logs"
command = "x-terminal-emulator -e journalctl -f"
shortcut = "Super+Shift+L"
section = "extra"
"#;
        let config: Config = toml::from_str(config_toml).expect("system menu config must parse");
        assert_eq!(config.menus.system.buttons[0].section, "root");
        assert_eq!(config.menus.system.shortcut_limit, 25);
        assert_eq!(config.menus.system.extra_items[0].id, "logs");
        assert_eq!(validate_system_menu_config(&config.menus.system), Ok(()));
    }

    #[test]
    fn release_system_menubar_example_parses_and_validates() {
        let config: Config =
            toml::from_str(include_str!("../../../examples/system-menubar/unilii.toml"))
                .expect("release system-menubar example must parse");
        assert_eq!(validate_menu_ui_config(&config.menus.ui), Ok(()));
        assert_eq!(validate_wifi_menu_config(&config.menus.wifi), Ok(()));
        assert_eq!(validate_mount_menu_config(&config.menus.mount), Ok(()));
        assert_eq!(
            validate_calendar_menu_config(&config.menus.calendar),
            Ok(())
        );
        assert_eq!(validate_custom_menu_config(&config.menus.custom), Ok(()));
        assert_eq!(validate_system_menu_config(&config.menus.system), Ok(()));
        assert!(config.menus.system.enabled);
        assert!(config.menus.wifi.show_known_networks);
        assert_eq!(config.menus.mount.sshfs_profiles.len(), 1);
        assert_eq!(config.menus.custom.items.len(), 1);
        assert_eq!(config.keybindings.len(), 3);
    }
}
