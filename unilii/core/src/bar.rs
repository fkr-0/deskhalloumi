//! Shared configuration and provider metadata for the `unilii-bar` binary.
//!
//! The first implementation deliberately keeps renderer-specific details out of
//! this module. Makepad, Iced, or a headless test runner can all consume the
//! same validated configuration and provider registry.

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::path::{Path, PathBuf};

const MAX_INTERVAL_MS: u64 = 86_400_000;
const MAX_TIMEOUT_MS: u64 = 3_600_000;
const MAX_BAR_HEIGHT: u16 = 256;
const MAX_FONT_SIZE: u16 = 96;

/// Result type for bar configuration operations.
pub type BarResult<T> = std::result::Result<T, BarConfigError>;

/// Actionable validation or parse error for bar configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BarConfigError {
    message: String,
}

impl BarConfigError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for BarConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for BarConfigError {}

/// Complete declarative bar configuration.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct BarConfig {
    pub bar: BarSettings,
    pub theme: BarTheme,
    pub layout: BarLayout,
    #[serde(rename = "module")]
    pub modules: Vec<BarModuleSpec>,
}

impl Default for BarConfig {
    fn default() -> Self {
        Self {
            bar: BarSettings::default(),
            theme: BarTheme::default(),
            layout: BarLayout {
                left: vec!["workspaces".to_string()],
                center: vec!["window_title".to_string()],
                right: vec![
                    "network".to_string(),
                    "vpn".to_string(),
                    "audio".to_string(),
                    "battery".to_string(),
                    "clock".to_string(),
                ],
            },
            modules: vec![
                BarModuleSpec::new("workspaces", "workspaces", Some(1000)),
                BarModuleSpec::new("window_title", "window_title", Some(1000)),
                BarModuleSpec::new("network", "network", Some(5000)),
                BarModuleSpec::new("vpn", "vpn", Some(5000)),
                BarModuleSpec::new("audio", "audio", Some(1000)),
                BarModuleSpec::new("battery", "battery", Some(10000)),
                BarModuleSpec::new("clock", "clock", Some(1000)),
            ],
        }
    }
}

/// Window/bar placement settings.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct BarSettings {
    pub height: u16,
    pub position: BarPosition,
    pub monitor: Option<String>,
    pub reserve_space: bool,
    pub hot_reload: bool,
}

impl Default for BarSettings {
    fn default() -> Self {
        Self {
            height: 28,
            position: BarPosition::Top,
            monitor: None,
            reserve_space: true,
            hot_reload: true,
        }
    }
}

/// Bar edge placement.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum BarPosition {
    #[default]
    Top,
    Bottom,
}

/// Module layout by visual zone.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(default)]
pub struct BarLayout {
    pub left: Vec<String>,
    pub center: Vec<String>,
    pub right: Vec<String>,
}

/// Theme tokens shared by bar renderers.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct BarTheme {
    pub background: String,
    pub foreground: String,
    pub accent: String,
    pub warning: String,
    pub critical: String,
    pub font_family: Option<String>,
    pub font_size: u16,
    pub padding: u16,
    pub margin: u16,
    pub border_size: u16,
}

impl Default for BarTheme {
    fn default() -> Self {
        Self {
            background: "#1e1e2e".to_string(),
            foreground: "#cdd6f4".to_string(),
            accent: "#89b4fa".to_string(),
            warning: "#f9e2af".to_string(),
            critical: "#f38ba8".to_string(),
            font_family: Some("monospace".to_string()),
            font_size: 13,
            padding: 8,
            margin: 0,
            border_size: 0,
        }
    }
}

/// Configurable module instance.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct BarModuleSpec {
    pub id: String,
    #[serde(rename = "type")]
    pub module_type: String,
    pub enabled: bool,
    pub interval_ms: Option<u64>,
    pub zone: Option<BarZone>,
    pub format: Option<String>,
    pub command: Option<String>,
    pub timeout_ms: Option<u64>,
    pub on_click_left: Option<BarAction>,
    pub on_click_middle: Option<BarAction>,
    pub on_click_right: Option<BarAction>,
    #[serde(flatten)]
    pub extra: HashMap<String, toml::Value>,
}

impl BarModuleSpec {
    pub fn new(
        id: impl Into<String>,
        module_type: impl Into<String>,
        interval_ms: Option<u64>,
    ) -> Self {
        Self {
            id: id.into(),
            module_type: module_type.into(),
            enabled: true,
            interval_ms,
            zone: None,
            format: None,
            command: None,
            timeout_ms: None,
            on_click_left: None,
            on_click_middle: None,
            on_click_right: None,
            extra: HashMap::new(),
        }
    }
}

impl Default for BarModuleSpec {
    fn default() -> Self {
        Self::new("module", "script", Some(1000))
    }
}

/// Optional explicit zone for a module.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BarZone {
    Left,
    Center,
    Right,
}

/// Click action. A string means a shell command; the detailed shape is reserved
/// for future internal actions without breaking current TOML.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(untagged)]
pub enum BarAction {
    Command(String),
    Detailed {
        command: Option<String>,
        action: Option<String>,
    },
}

/// Provider metadata used by config generation and later discovery integration.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BarModuleProvider {
    pub module_type: String,
    pub description: String,
    pub default_interval_ms: Option<u64>,
    pub update_mode: BarUpdateMode,
    pub capabilities: Vec<String>,
}

/// How a provider refreshes values.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BarUpdateMode {
    Poll,
    Event,
    Manual,
}

/// Return the built-in provider metadata required by the feature plan.
pub fn built_in_bar_module_providers() -> Vec<BarModuleProvider> {
    vec![
        provider(
            "workspaces",
            "Workspace list and active workspace",
            Some(1000),
            BarUpdateMode::Poll,
            &["wm-backend"],
        ),
        provider(
            "window_title",
            "Focused window title",
            Some(1000),
            BarUpdateMode::Poll,
            &["wm-backend"],
        ),
        provider(
            "clock",
            "Formatted local or configured time",
            Some(1000),
            BarUpdateMode::Poll,
            &[],
        ),
        provider(
            "system",
            "CPU, memory, load, and temperature",
            Some(2000),
            BarUpdateMode::Poll,
            &["procfs"],
        ),
        provider(
            "network",
            "Network interface and connectivity status",
            Some(5000),
            BarUpdateMode::Poll,
            &["ip", "wifi-optional"],
        ),
        provider(
            "vpn",
            "VPN/tunnel interface status",
            Some(5000),
            BarUpdateMode::Poll,
            &["ip"],
        ),
        provider(
            "audio",
            "Volume and mute state",
            Some(1000),
            BarUpdateMode::Poll,
            &["pipewire-or-pulseaudio"],
        ),
        provider(
            "battery",
            "Battery percentage and charging state",
            Some(10000),
            BarUpdateMode::Poll,
            &["sysfs-power-supply"],
        ),
        provider(
            "script",
            "User supplied command output",
            Some(30000),
            BarUpdateMode::Poll,
            &["shell-command"],
        ),
        provider(
            "notifications",
            "Notification indicator/count hook",
            Some(5000),
            BarUpdateMode::Poll,
            &["configured-source"],
        ),
    ]
}

fn provider(
    module_type: &str,
    description: &str,
    default_interval_ms: Option<u64>,
    update_mode: BarUpdateMode,
    capabilities: &[&str],
) -> BarModuleProvider {
    BarModuleProvider {
        module_type: module_type.to_string(),
        description: description.to_string(),
        default_interval_ms,
        update_mode,
        capabilities: capabilities.iter().map(|cap| cap.to_string()).collect(),
    }
}

/// Parse and validate bar TOML.
pub fn parse_bar_config_str(input: &str) -> BarResult<BarConfig> {
    let config: BarConfig = toml::from_str(input)
        .map_err(|err| BarConfigError::new(format!("failed to parse bar config TOML: {err}")))?;
    validate_bar_config(&config)?;
    Ok(config)
}

/// Load and validate a bar config from disk.
pub fn load_bar_config(path: impl AsRef<Path>) -> BarResult<BarConfig> {
    let path = path.as_ref();
    let input = std::fs::read_to_string(path).map_err(|err| {
        BarConfigError::new(format!(
            "failed to read bar config '{}': {err}",
            path.display()
        ))
    })?;
    parse_bar_config_str(&input)
}

/// Generate a starter TOML config.
pub fn starter_bar_config_toml() -> &'static str {
    include_str!("../../../templates/bar.toml")
}

/// Return the default XDG-style config path for the bar.
pub fn default_bar_config_path() -> Option<PathBuf> {
    crate::branding::config_dir().map(|dir| dir.join("bar.toml"))
}

/// Return candidate config paths in lookup order.
pub fn bar_config_candidate_paths_from_env(env: &HashMap<String, String>) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Some(explicit) = env
        .get("DESKHALLOUMI_BAR_CONFIG")
        .or_else(|| env.get("UNILII_BAR_CONFIG"))
        .filter(|value| !value.trim().is_empty())
    {
        paths.push(PathBuf::from(explicit));
    }
    if let Some(xdg_config_home) = env
        .get("XDG_CONFIG_HOME")
        .filter(|value| !value.trim().is_empty())
    {
        paths.push(
            PathBuf::from(xdg_config_home)
                .join("deskhalloumi")
                .join("bar.toml"),
        );
        paths.push(
            PathBuf::from(xdg_config_home)
                .join("unilii")
                .join("bar.toml"),
        );
        paths.push(
            PathBuf::from(xdg_config_home)
                .join("com")
                .join("unilii")
                .join("unilii")
                .join("bar.toml"),
        );
    }
    if let Some(home) = env.get("HOME").filter(|value| !value.trim().is_empty()) {
        paths.push(
            PathBuf::from(home)
                .join(".config")
                .join("deskhalloumi")
                .join("bar.toml"),
        );
        paths.push(
            PathBuf::from(home)
                .join(".config")
                .join("unilii")
                .join("bar.toml"),
        );
        paths.push(
            PathBuf::from(home)
                .join(".config")
                .join("com")
                .join("unilii")
                .join("unilii")
                .join("bar.toml"),
        );
    }
    paths
}

/// Find the first existing default config path from process environment.
pub fn find_default_bar_config_path() -> Option<PathBuf> {
    let env = std::env::vars().collect::<HashMap<_, _>>();
    bar_config_candidate_paths_from_env(&env)
        .into_iter()
        .find(|path| path.is_file())
        .or_else(|| default_bar_config_path().filter(|path| path.is_file()))
}

/// Load the discovered default config, or fall back to the built-in starter config.
pub fn load_default_or_starter_bar_config() -> BarResult<BarConfig> {
    match find_default_bar_config_path() {
        Some(path) => load_bar_config(path),
        None => parse_bar_config_str(starter_bar_config_toml()),
    }
}

/// Validate semantic constraints not covered by serde/TOML parsing.
pub fn validate_bar_config(config: &BarConfig) -> BarResult<()> {
    if config.bar.height == 0 || config.bar.height > MAX_BAR_HEIGHT {
        return Err(BarConfigError::new(format!(
            "bar.height must be between 1 and {MAX_BAR_HEIGHT}"
        )));
    }

    if config.theme.font_size == 0 || config.theme.font_size > MAX_FONT_SIZE {
        return Err(BarConfigError::new(format!(
            "theme.font_size must be between 1 and {MAX_FONT_SIZE}"
        )));
    }

    let supported: HashSet<String> = built_in_bar_module_providers()
        .into_iter()
        .map(|provider| provider.module_type)
        .collect();
    let mut ids = HashSet::new();

    for module in &config.modules {
        validate_module(module, &supported, &mut ids)?;
    }

    let known_ids: HashSet<&str> = config
        .modules
        .iter()
        .map(|module| module.id.as_str())
        .collect();
    validate_layout_refs("layout.left", &config.layout.left, &known_ids)?;
    validate_layout_refs("layout.center", &config.layout.center, &known_ids)?;
    validate_layout_refs("layout.right", &config.layout.right, &known_ids)?;

    Ok(())
}

fn validate_module(
    module: &BarModuleSpec,
    supported: &HashSet<String>,
    ids: &mut HashSet<String>,
) -> BarResult<()> {
    let id = module.id.trim();
    if id.is_empty() {
        return Err(BarConfigError::new("module id cannot be empty"));
    }
    if !ids.insert(id.to_string()) {
        return Err(BarConfigError::new(format!("duplicate module id '{id}'")));
    }

    let module_type = module.module_type.trim();
    if !supported.contains(module_type) {
        return Err(BarConfigError::new(format!(
            "module '{id}' has unknown type '{module_type}'"
        )));
    }

    if let Some(interval_ms) = module.interval_ms
        && (interval_ms == 0 || interval_ms > MAX_INTERVAL_MS)
    {
        return Err(BarConfigError::new(format!(
            "module '{id}' interval_ms must be between 1 and {MAX_INTERVAL_MS}"
        )));
    }

    if let Some(timeout_ms) = module.timeout_ms
        && (timeout_ms == 0 || timeout_ms > MAX_TIMEOUT_MS)
    {
        return Err(BarConfigError::new(format!(
            "module '{id}' timeout_ms must be between 1 and {MAX_TIMEOUT_MS}"
        )));
    }

    if module_type == "script" && module.command.as_deref().unwrap_or("").trim().is_empty() {
        return Err(BarConfigError::new(format!(
            "script module '{id}' requires command"
        )));
    }

    Ok(())
}

fn validate_layout_refs(field: &str, refs: &[String], known_ids: &HashSet<&str>) -> BarResult<()> {
    let mut seen = HashSet::new();
    for module_id in refs {
        let module_id = module_id.trim();
        if module_id.is_empty() {
            return Err(BarConfigError::new(format!(
                "{field} contains an empty module id"
            )));
        }
        if !known_ids.contains(module_id) {
            return Err(BarConfigError::new(format!(
                "{field} references unknown module id '{module_id}'"
            )));
        }
        if !seen.insert(module_id.to_string()) {
            return Err(BarConfigError::new(format!(
                "{field} references module id '{module_id}' more than once"
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_err(input: &str) -> String {
        parse_bar_config_str(input)
            .unwrap_err()
            .message()
            .to_string()
    }

    #[test]
    fn starter_config_is_valid() {
        let config =
            parse_bar_config_str(starter_bar_config_toml()).expect("starter config parses");
        assert_eq!(config.bar.height, 28);
        assert_eq!(config.layout.left, vec!["workspaces"]);
        assert!(config.modules.iter().any(|module| module.id == "clock"));
    }

    #[test]
    fn default_config_is_valid() {
        let config = BarConfig::default();
        validate_bar_config(&config).expect("default config validates");
    }

    #[test]
    fn duplicate_module_ids_are_rejected() {
        let err = parse_err(
            r#"
            [[module]]
            id = "clock"
            type = "clock"

            [[module]]
            id = "clock"
            type = "battery"
            "#,
        );
        assert!(err.contains("duplicate module id 'clock'"));
    }

    #[test]
    fn unknown_module_types_are_rejected() {
        let err = parse_err(
            r#"
            [[module]]
            id = "mystery"
            type = "mystery"
            "#,
        );
        assert!(err.contains("unknown type 'mystery'"));
    }

    #[test]
    fn script_requires_command() {
        let err = parse_err(
            r#"
            [[module]]
            id = "custom"
            type = "script"
            "#,
        );
        assert!(err.contains("script module 'custom' requires command"));
    }

    #[test]
    fn invalid_interval_is_rejected() {
        let err = parse_err(
            r#"
            [[module]]
            id = "clock"
            type = "clock"
            interval_ms = 0
            "#,
        );
        assert!(err.contains("interval_ms must be between 1"));
    }

    #[test]
    fn bad_layout_reference_is_rejected() {
        let err = parse_err(
            r#"
            [layout]
            left = ["missing"]

            [[module]]
            id = "clock"
            type = "clock"
            "#,
        );
        assert!(err.contains("layout.left references unknown module id 'missing'"));
    }

    #[test]
    fn default_config_candidates_follow_env_order() {
        let mut env = HashMap::new();
        env.insert(
            "UNILII_BAR_CONFIG".to_string(),
            "/tmp/custom-bar.toml".to_string(),
        );
        env.insert("XDG_CONFIG_HOME".to_string(), "/tmp/xdg".to_string());
        env.insert("HOME".to_string(), "/home/example".to_string());
        let paths = bar_config_candidate_paths_from_env(&env);
        assert_eq!(paths[0], PathBuf::from("/tmp/custom-bar.toml"));
        assert!(paths.contains(&PathBuf::from("/tmp/xdg/unilii/bar.toml")));
        assert!(paths.contains(&PathBuf::from("/home/example/.config/unilii/bar.toml")));
    }

    #[test]
    fn load_default_or_starter_falls_back_to_valid_starter() {
        let config = load_default_or_starter_bar_config().expect("starter fallback validates");
        assert!(config.modules.iter().any(|module| module.id == "clock"));
    }

    #[test]
    fn provider_list_covers_planned_modules() {
        let providers = built_in_bar_module_providers();
        let types: HashSet<_> = providers
            .iter()
            .map(|provider| provider.module_type.as_str())
            .collect();
        for expected in [
            "workspaces",
            "window_title",
            "clock",
            "system",
            "network",
            "vpn",
            "audio",
            "battery",
            "script",
            "notifications",
        ] {
            assert!(types.contains(expected), "missing provider {expected}");
        }
    }
}
