//! Configuration loader for unilii status bar.

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
    #[serde(default)]
    pub wifi: WifiMenuConfig,
    #[serde(default)]
    pub mount: MountMenuConfig,
    #[serde(default)]
    pub calendar: CalendarMenuConfig,
    #[serde(default)]
    pub custom: CustomMenuConfig,
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
pub struct MountMenuConfig {
    #[serde(default = "default_mount_refresh_ms")]
    pub refresh_ms: u64,
    #[serde(default = "default_true")]
    pub show_loop_devices: bool,
    #[serde(default = "default_mount_max_rows")]
    pub max_local_rows: usize,
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
            accounts: Vec::new(),
        }
    }
}

impl Default for CustomMenuConfig {
    fn default() -> Self {
        Self {
            enabled: false,
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
    directories::ProjectDirs::from("com", "unilii", "unilii")
        .map(|proj_dirs| proj_dirs.config_dir().to_path_buf())
}

/// Get the configuration file path.
pub fn get_config_path() -> Option<PathBuf> {
    get_config_dir().map(|dir| dir.join("unilii.toml"))
}

/// Create default configuration file at the standard location.
pub fn create_default_config() -> std::io::Result<PathBuf> {
    let config_path = get_config_path().ok_or_else(|| {
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
    sources.sort_by(|left, right| left.priority.cmp(&right.priority));
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
        nested_sources.sort_by(|left, right| left.priority.cmp(&right.priority));
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
}
