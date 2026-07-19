#![allow(dead_code)]
// Auxiliary binaries import `menus/mod.rs` but only the main bar instantiates this model.

//! Configurable built-in menubar menu model.

use crate::enhanced_tray::{TrayMenuAction, TrayMenuItem, TrayWidgetType};
use deskhalloumi_core::action_history::{ActionHistory, ActionStatus};
use deskhalloumi_core::config::{SystemMenuButtonConfig, SystemMenuConfig};
use deskhalloumi_core::key_engine::KeyTrigger;
use deskhalloumi_core::keys::{CommandType, KeyBinding};
use deskhalloumi_core::menu::{MenuLifecycle, MenuModel, MenuSource};

pub const SYSTEM_MENU_APP_ID: &str = "unilii-system-menu";
pub const SYSTEM_MENU_KEY: &str = "unilii-system-menu";
const INTERNAL_PREFIX: &str = "unilii-system:";

#[derive(Debug, Clone, Default, PartialEq)]
pub struct SystemMenuSnapshot {
    pub wifi_enabled: bool,
    pub connected_ssid: Option<String>,
    pub wifi_label: String,
    pub display_label: String,
    pub displays: Vec<SystemDisplaySnapshot>,
    pub display_status: String,
    pub display_presets: Vec<SystemDisplayPreset>,
    pub stats_label: String,
    pub cpu_percent: Option<f32>,
    pub memory_percent: Option<f32>,
    pub load_average: [f32; 3],
    pub root_disk_percent: Option<u8>,
    pub uptime_label: String,
    pub idle_sleep_enabled: bool,
}

fn action_history_items(history: &ActionHistory) -> Vec<TrayMenuItem> {
    history
        .recent(8)
        .into_iter()
        .map(|record| {
            let status = match record.status {
                ActionStatus::Running => "running",
                ActionStatus::Succeeded => "ok",
                ActionStatus::Failed => "failed",
                ActionStatus::TimedOut => "timeout",
                ActionStatus::Cancelled => "cancelled",
            };
            let duration = record
                .duration_ms
                .map(|duration| format!(" · {duration}ms"))
                .unwrap_or_default();
            let detail = record
                .detail
                .as_deref()
                .filter(|detail| !detail.trim().is_empty())
                .map(|detail| format!(" — {detail}"))
                .unwrap_or_default();
            label_item(
                &format!("history-{}", record.sequence),
                &format!("{} · {status}{duration}{detail}", record.title),
            )
        })
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SystemDisplaySnapshot {
    pub name: String,
    pub mode: Option<String>,
    pub primary: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SystemDisplayPreset {
    pub key: String,
    pub name: String,
    pub description: Option<String>,
    pub command: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingSystemAction {
    pub id: String,
    pub title: String,
    pub command: String,
    pub return_section: String,
}

#[derive(Debug, Clone, Default)]
pub struct SystemMenuRuntime {
    pub pending_confirmation: Option<PendingSystemAction>,
    pub busy_action: Option<String>,
    pub last_status: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SystemInternalAction {
    OpenWifi,
    ToggleWifi,
    RefreshWifi,
    RefreshDisplays,
    ApplyDisplayPreset(String),
    RefreshStats,
    RunConfigured(String),
    Shortcut(usize),
    Extra(String),
    Confirm(String),
    ConfirmExecute,
    ConfirmCancel,
}

pub fn parse_internal_action(command: &str) -> Option<SystemInternalAction> {
    let raw = command.strip_prefix(INTERNAL_PREFIX)?;
    match raw {
        "wifi:open" => Some(SystemInternalAction::OpenWifi),
        "wifi:toggle" => Some(SystemInternalAction::ToggleWifi),
        "wifi:refresh" => Some(SystemInternalAction::RefreshWifi),
        "displays:refresh" => Some(SystemInternalAction::RefreshDisplays),
        "stats:refresh" => Some(SystemInternalAction::RefreshStats),
        "confirm:execute" => Some(SystemInternalAction::ConfirmExecute),
        "confirm:cancel" => Some(SystemInternalAction::ConfirmCancel),
        _ => raw
            .strip_prefix("display-preset:")
            .map(|v| SystemInternalAction::ApplyDisplayPreset(v.to_string()))
            .or_else(|| {
                raw.strip_prefix("run:")
                    .map(|v| SystemInternalAction::RunConfigured(v.to_string()))
            })
            .or_else(|| {
                raw.strip_prefix("shortcut:")
                    .and_then(|v| v.parse::<usize>().ok())
                    .map(SystemInternalAction::Shortcut)
            })
            .or_else(|| {
                raw.strip_prefix("extra:")
                    .map(|v| SystemInternalAction::Extra(v.to_string()))
            })
            .or_else(|| {
                raw.strip_prefix("confirm:")
                    .map(|v| SystemInternalAction::Confirm(v.to_string()))
            }),
    }
}

pub fn internal_command(action: &str) -> String {
    format!("{INTERNAL_PREFIX}{action}")
}

pub fn button_label(button: &SystemMenuButtonConfig, snapshot: &SystemMenuSnapshot) -> String {
    if let Some(label) = &button.label {
        return label.clone();
    }
    match button.id.as_str() {
        "wifi" => snapshot.wifi_label.clone(),
        "displays" => snapshot.display_label.clone(),
        "stats" => snapshot.stats_label.clone(),
        "power" => "⏻".to_string(),
        "shortcuts" => "⌨".to_string(),
        _ => "☰".to_string(),
    }
}

pub fn build_system_menu(
    config: &SystemMenuConfig,
    snapshot: &SystemMenuSnapshot,
    keybindings: &[KeyBinding],
    runtime: &SystemMenuRuntime,
    history: &ActionHistory,
) -> Vec<TrayMenuItem> {
    if let Some(pending) = &runtime.pending_confirmation {
        return confirmation_items(pending);
    }
    let mut items = Vec::new();
    for section in &config.sections {
        let item = match section.as_str() {
            "wifi" => Some(submenu("wifi", "Wi-Fi", wifi_items(config, snapshot), "📡")),
            "displays" => Some(submenu(
                "displays",
                "Displays",
                display_items(snapshot),
                "🖥",
            )),
            "stats" => Some(submenu(
                "stats",
                "System statistics",
                stats_items(config, snapshot),
                "📊",
            )),
            "shortcuts" => Some(submenu(
                "shortcuts",
                "Shortcut table",
                shortcut_items(config, keybindings),
                "⌨",
            )),
            "power" => Some(submenu(
                "power",
                "Session and power",
                power_items(config, snapshot.idle_sleep_enabled),
                "⏻",
            )),
            "extra" if !config.extra_items.is_empty() => Some(submenu(
                "extra",
                "Additional actions",
                extra_items(config),
                "⋯",
            )),
            _ => None,
        };
        if let Some(item) = item {
            items.push(item);
        }
    }
    if history.records().next().is_some() {
        items.push(submenu(
            "action-history",
            "Recent actions",
            action_history_items(history),
            "◷",
        ));
    }
    if let Some(status) = &runtime.last_status {
        items.push(separator("status-separator"));
        items.push(label_item("status", status));
    }
    if let Some(action) = &runtime.busy_action {
        items.push(label_item("busy", &format!("Working: {action}…")));
    }
    items
}

pub fn build_system_menu_model(
    config: &SystemMenuConfig,
    snapshot: &SystemMenuSnapshot,
    keybindings: &[KeyBinding],
    runtime: &SystemMenuRuntime,
    history: &ActionHistory,
) -> MenuModel {
    if !config.enabled {
        return MenuModel::disabled(
            SYSTEM_MENU_APP_ID,
            "System actions",
            MenuSource::System,
            "disabled by configuration",
        );
    }
    let items = build_system_menu(config, snapshot, keybindings, runtime, history);
    let mut model = MenuModel::with_items(
        SYSTEM_MENU_APP_ID,
        "System actions",
        MenuSource::System,
        0,
        items,
    );
    if let Some(action_id) = &runtime.busy_action {
        model.lifecycle = MenuLifecycle::Busy {
            action_id: action_id.clone(),
        };
    }
    model
}

fn wifi_items(_config: &SystemMenuConfig, snapshot: &SystemMenuSnapshot) -> Vec<TrayMenuItem> {
    let status = match snapshot.connected_ssid.as_deref() {
        Some(ssid) => format!("Connected to {ssid}"),
        None if snapshot.wifi_enabled => "Wi-Fi enabled; not connected".to_string(),
        None => "Wi-Fi disabled".to_string(),
    };
    vec![
        label_item("wifi-status", &status),
        action_item(
            "wifi-open",
            "Available and known networks",
            internal_command("wifi:open"),
            None,
            true,
            false,
        ),
        action_item(
            "wifi-toggle",
            if snapshot.wifi_enabled {
                "Disable Wi-Fi"
            } else {
                "Enable Wi-Fi"
            },
            internal_command("wifi:toggle"),
            None,
            true,
            false,
        ),
        action_item(
            "wifi-refresh",
            "Rescan networks",
            internal_command("wifi:refresh"),
            Some("R".to_string()),
            true,
            false,
        ),
        action_item(
            "wifi-settings",
            "Network settings",
            internal_command("run:wifi-settings"),
            None,
            true,
            false,
        ),
    ]
}

fn display_items(snapshot: &SystemMenuSnapshot) -> Vec<TrayMenuItem> {
    let mut items = Vec::new();
    if snapshot.displays.is_empty() {
        items.push(label_item("display-empty", "No display data available"));
    } else {
        for display in &snapshot.displays {
            let mut details = display
                .mode
                .clone()
                .unwrap_or_else(|| "no mode".to_string());
            if display.primary {
                details.push_str(" · primary");
            }
            items.push(label_item(
                &format!("display-{}", display.name),
                &format!("{} — {}", display.name, details),
            ));
        }
    }
    items.push(separator("display-separator"));
    items.push(action_item(
        "display-refresh",
        "Refresh display state",
        internal_command("displays:refresh"),
        Some("R".to_string()),
        true,
        false,
    ));
    for preset in &snapshot.display_presets {
        let title = preset
            .description
            .as_ref()
            .map(|description| format!("{} — {description}", preset.name))
            .unwrap_or_else(|| preset.name.clone());
        items.push(action_item(
            &format!("display-preset-{}", preset.key),
            &title,
            internal_command(&format!("display-preset:{}", preset.key)),
            None,
            true,
            false,
        ));
    }
    items.push(label_item("display-status", &snapshot.display_status));
    items
}

fn stats_items(config: &SystemMenuConfig, snapshot: &SystemMenuSnapshot) -> Vec<TrayMenuItem> {
    let percent = |value: Option<f32>| {
        value
            .map(|value| format!("{value:.1}%"))
            .unwrap_or_else(|| "--".to_string())
    };
    let mut items = vec![
        label_item(
            "stats-cpu",
            &format!("CPU usage: {}", percent(snapshot.cpu_percent)),
        ),
        label_item(
            "stats-memory",
            &format!("Memory usage: {}", percent(snapshot.memory_percent)),
        ),
        label_item(
            "stats-load",
            &format!(
                "Load average: {:.2}  {:.2}  {:.2}",
                snapshot.load_average[0], snapshot.load_average[1], snapshot.load_average[2]
            ),
        ),
        label_item(
            "stats-disk",
            &format!(
                "Root filesystem: {}",
                snapshot
                    .root_disk_percent
                    .map(|value| format!("{value}% used"))
                    .unwrap_or_else(|| "--".to_string())
            ),
        ),
        label_item(
            "stats-uptime",
            &format!("Uptime: {}", snapshot.uptime_label),
        ),
        separator("stats-separator"),
        action_item(
            "stats-refresh",
            "Refresh statistics",
            internal_command("stats:refresh"),
            Some("R".to_string()),
            true,
            false,
        ),
    ];
    if !config.stats_command.trim().is_empty() {
        items.push(action_item(
            "stats-open",
            "Open system monitor",
            internal_command("run:stats"),
            None,
            true,
            false,
        ));
    }
    items
}

fn shortcut_items(config: &SystemMenuConfig, bindings: &[KeyBinding]) -> Vec<TrayMenuItem> {
    let mut indexed = bindings.iter().enumerate().collect::<Vec<_>>();
    indexed.sort_by(|(_, left), (_, right)| {
        left.keysym
            .cmp(&right.keysym)
            .then(left.name.cmp(&right.name))
    });
    indexed
        .into_iter()
        .take(config.shortcut_limit)
        .map(|(index, binding)| {
            let action_type = match binding.command_type {
                CommandType::Shell => "shell",
                CommandType::Menu => "menu",
                CommandType::Tray => "tray",
                CommandType::Bar => "bar",
                CommandType::Widget => "widget",
            };
            let trigger = match binding.trigger {
                KeyTrigger::Press => String::new(),
                KeyTrigger::Release => " · release".to_string(),
                KeyTrigger::Modrelease => " · modifier release".to_string(),
                KeyTrigger::Repeat => " · repeat".to_string(),
            };
            action_item(
                &format!("shortcut-{index}"),
                &format!("{} · {action_type}", binding.name),
                internal_command(&format!("shortcut:{index}")),
                Some(format!("{}{trigger}", binding.keysym)),
                !matches!(binding.command_type, CommandType::Widget),
                false,
            )
        })
        .collect()
}

fn power_items(config: &SystemMenuConfig, idle_sleep_enabled: bool) -> Vec<TrayMenuItem> {
    let configured = |command: &str| !command.trim().is_empty();
    vec![
        action_item(
            "idle-toggle",
            if idle_sleep_enabled {
                "Disable sleep/display blanking after inactivity"
            } else {
                "Enable sleep/display blanking after inactivity"
            },
            internal_command("run:idle-toggle"),
            None,
            configured(if idle_sleep_enabled {
                &config.idle_disable_command
            } else {
                &config.idle_enable_command
            }),
            false,
        ),
        separator("power-separator-a"),
        action_item(
            "lock",
            "Lock session",
            internal_command("run:lock"),
            None,
            configured(&config.lock_command),
            false,
        ),
        action_item(
            "suspend",
            "Suspend",
            internal_command("run:suspend"),
            None,
            configured(&config.suspend_command),
            false,
        ),
        action_item(
            "logout",
            "Log out of the session",
            internal_command(if config.confirm_destructive {
                "confirm:logout"
            } else {
                "run:logout"
            }),
            None,
            configured(&config.logout_command),
            config.confirm_destructive,
        ),
        separator("power-separator-b"),
        action_item(
            "reboot",
            "Restart computer",
            internal_command(if config.confirm_destructive {
                "confirm:reboot"
            } else {
                "run:reboot"
            }),
            None,
            configured(&config.reboot_command),
            config.confirm_destructive,
        ),
        action_item(
            "poweroff",
            "Shut down computer",
            internal_command(if config.confirm_destructive {
                "confirm:poweroff"
            } else {
                "run:poweroff"
            }),
            None,
            configured(&config.poweroff_command),
            config.confirm_destructive,
        ),
    ]
}

fn extra_items(config: &SystemMenuConfig) -> Vec<TrayMenuItem> {
    config
        .extra_items
        .iter()
        .filter(|item| item.section == "extra")
        .map(|item| {
            let action = if item.confirm {
                format!("confirm:extra:{}", item.id)
            } else {
                format!("extra:{}", item.id)
            };
            action_item(
                &format!("extra-{}", item.id),
                &item.title,
                internal_command(&action),
                item.shortcut.clone(),
                !item.command.trim().is_empty(),
                item.confirm,
            )
        })
        .collect()
}

fn confirmation_items(pending: &PendingSystemAction) -> Vec<TrayMenuItem> {
    vec![
        label_item("confirm-title", &format!("Confirm: {}", pending.title)),
        label_item(
            "confirm-warning",
            "This action may end the current session or stop the machine.",
        ),
        separator("confirm-separator"),
        action_item(
            "confirm-execute",
            "Confirm action",
            internal_command("confirm:execute"),
            Some("Enter".to_string()),
            true,
            true,
        ),
        action_item(
            "confirm-cancel",
            "Cancel",
            internal_command("confirm:cancel"),
            Some("Esc".to_string()),
            true,
            false,
        ),
    ]
}

fn submenu(id: &str, title: &str, items: Vec<TrayMenuItem>, icon: &str) -> TrayMenuItem {
    TrayMenuItem {
        id: id.to_string(),
        label: title.to_string(),
        action: TrayMenuAction::NavigateToSubmenu {
            item_id: id.to_string(),
            submenu_path: vec![id.to_string()],
        },
        icon: Some(icon.to_string()),
        submenu: items,
        enabled: true,
        visible: true,
        checkable: false,
        checked: false,
        shortcut: None,
        is_separator: false,
        app_id: SYSTEM_MENU_APP_ID.to_string(),
        full_path: title.to_string(),
        widget_type: TrayWidgetType::SubmenuButton,
        default_value: None,
        placeholder: None,
    }
}

fn action_item(
    id: &str,
    title: &str,
    command: String,
    shortcut: Option<String>,
    enabled: bool,
    dangerous: bool,
) -> TrayMenuItem {
    TrayMenuItem {
        id: id.to_string(),
        label: if dangerous {
            format!("⚠ {title}")
        } else {
            title.to_string()
        },
        action: TrayMenuAction::SpawnCommand(command),
        icon: None,
        submenu: Vec::new(),
        enabled,
        visible: true,
        checkable: false,
        checked: false,
        shortcut,
        is_separator: false,
        app_id: SYSTEM_MENU_APP_ID.to_string(),
        full_path: title.to_string(),
        widget_type: TrayWidgetType::Button,
        default_value: None,
        placeholder: None,
    }
}

fn label_item(id: &str, title: &str) -> TrayMenuItem {
    action_item(id, title, String::new(), None, false, false)
}

fn separator(id: &str) -> TrayMenuItem {
    TrayMenuItem {
        id: id.to_string(),
        label: String::new(),
        action: TrayMenuAction::Activate,
        icon: None,
        submenu: Vec::new(),
        enabled: false,
        visible: true,
        checkable: false,
        checked: false,
        shortcut: None,
        is_separator: true,
        app_id: SYSTEM_MENU_APP_ID.to_string(),
        full_path: String::new(),
        widget_type: TrayWidgetType::Separator,
        default_value: None,
        placeholder: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use deskhalloumi_core::key_engine::KeyTrigger;

    #[test]
    fn parser_recognizes_internal_actions() {
        assert_eq!(
            parse_internal_action("unilii-system:wifi:open"),
            Some(SystemInternalAction::OpenWifi)
        );
        assert_eq!(
            parse_internal_action("unilii-system:shortcut:3"),
            Some(SystemInternalAction::Shortcut(3))
        );
        assert_eq!(parse_internal_action("echo no"), None);
    }

    #[test]
    fn shortcut_table_is_clickable() {
        let binding = KeyBinding {
            name: "Open menu".into(),
            keysym: "Super+i".into(),
            command: "toggle:i3-vis".into(),
            command_type: CommandType::Menu,
            release: false,
            trigger: KeyTrigger::Press,
            hold_ms: None,
            cooldown_ms: None,
            priority: 1,
            consume: true,
        };
        let items = shortcut_items(&SystemMenuConfig::default(), &[binding]);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].shortcut.as_deref(), Some("Super+i"));
        assert_eq!(items[0].label, "Open menu · menu");
        assert!(items[0].enabled);
    }

    #[test]
    fn destructive_actions_require_confirmation_by_default() {
        let items = power_items(&SystemMenuConfig::default(), true);
        let shutdown = items.iter().find(|item| item.id == "poweroff").unwrap();
        assert_eq!(
            shutdown.action,
            TrayMenuAction::SpawnCommand(internal_command("confirm:poweroff"))
        );
    }

    #[test]
    fn system_menu_surfaces_action_failure_history() {
        let mut history = ActionHistory::new(4);
        history.record(
            "network-restart",
            "Restart network",
            "system-menu",
            ActionStatus::Failed,
            std::time::Duration::from_millis(12),
            Some("permission denied".to_string()),
        );
        let items = build_system_menu(
            &SystemMenuConfig::default(),
            &SystemMenuSnapshot::default(),
            &[],
            &SystemMenuRuntime::default(),
            &history,
        );
        let history = items
            .iter()
            .find(|item| item.id == "action-history")
            .expect("history submenu");
        assert!(history.submenu[0].label.contains("permission denied"));
        assert!(history.submenu[0].label.contains("12ms"));
    }
}
