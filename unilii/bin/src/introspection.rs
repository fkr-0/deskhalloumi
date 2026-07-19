//! Pure CLI-facing inventories derived from the same configuration and typed
//! action contracts used by the running bar.

use std::path::Path;

use serde::Serialize;

use crate::{
    cli::ActionKind,
    widgets::{audio, sysmonitor, wifi},
};
use deskhalloumi_core::{
    action_bus::{
        ActionBusRequest, ActionBusResponse, DesktopAction, default_action_bus_socket_path,
        send_action_request,
    },
    config::Config,
    menu::{MenuLifecycle, MenuModel, MenuSource},
    runtime::{ProviderContract, RuntimeMetricsSnapshot},
};

#[derive(Debug, Clone, Serialize)]
pub struct ProviderPolicyInfo {
    pub id: String,
    pub display_name: String,
    pub interval_ms: u128,
    pub timeout_ms: u128,
    pub stale_after_ms: u128,
    pub refresh_on_start: bool,
    pub shutdown_timeout_ms: u128,
    pub test_backend: String,
}

pub fn query_runtime_metrics(socket: Option<&Path>) -> Result<RuntimeMetricsSnapshot, String> {
    let request = ActionBusRequest::new(
        format!("metrics-cli-{}", std::process::id()),
        DesktopAction::Bar("runtime-metrics".to_string()),
    );
    let response = send_action_request(
        socket
            .map(Path::to_path_buf)
            .unwrap_or_else(default_action_bus_socket_path),
        &request,
    )?;
    if !response.ok {
        return Err(response.message);
    }
    let data = response
        .data
        .ok_or_else(|| "runtime metrics response did not contain structured data".to_string())?;
    serde_json::from_value(data)
        .map_err(|error| format!("invalid runtime metrics response: {error}"))
}

pub fn print_runtime_metrics(metrics: &RuntimeMetricsSnapshot, json: bool) -> Result<(), String> {
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(metrics)
                .map_err(|error| format!("failed to serialize runtime metrics: {error}"))?
        );
        return Ok(());
    }

    for (name, value) in [
        ("active_tasks", metrics.active_tasks as u64),
        ("tasks_started", metrics.tasks_started),
        ("tasks_completed", metrics.tasks_completed),
        ("tasks_cancelled", metrics.tasks_cancelled),
        ("tasks_panicked", metrics.tasks_panicked),
        ("actions_started", metrics.actions_started),
        ("actions_completed", metrics.actions_completed),
        ("actions_failed", metrics.actions_failed),
        ("action_timeouts", metrics.action_timeouts),
        ("action_duration_ms_total", metrics.action_duration_ms_total),
        ("action_duration_ms_max", metrics.action_duration_ms_max),
        ("truncated_outputs", metrics.truncated_outputs),
        ("truncated_bytes", metrics.truncated_bytes),
        (
            "provider_refreshes_started",
            metrics.provider_refreshes_started,
        ),
        (
            "provider_refreshes_completed",
            metrics.provider_refreshes_completed,
        ),
        (
            "provider_refreshes_coalesced",
            metrics.provider_refreshes_coalesced,
        ),
        (
            "provider_refreshes_saturated",
            metrics.provider_refreshes_saturated,
        ),
        ("updates_coalesced", metrics.updates_coalesced),
        ("updates_dropped", metrics.updates_dropped),
    ] {
        println!("{name}={value}");
    }
    Ok(())
}

impl From<ProviderContract> for ProviderPolicyInfo {
    fn from(contract: ProviderContract) -> Self {
        Self {
            id: contract.id,
            display_name: contract.display_name,
            interval_ms: contract.refresh.interval.as_millis(),
            timeout_ms: contract.refresh.timeout.as_millis(),
            stale_after_ms: contract.refresh.stale_after.as_millis(),
            refresh_on_start: contract.refresh.refresh_on_start,
            shutdown_timeout_ms: contract.shutdown.graceful_timeout.as_millis(),
            test_backend: contract.test_backend,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ModuleInfo {
    pub id: String,
    pub enabled: bool,
    pub position: String,
    pub configured_interval_ms: Option<u64>,
    pub provider: Option<ProviderPolicyInfo>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ActionInfo {
    pub id: String,
    pub title: String,
    pub kind: String,
    pub payload: String,
    pub source: String,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct HotkeyInfo {
    pub index: usize,
    pub name: String,
    pub keysym: String,
    pub trigger: String,
    pub kind: String,
    pub command: String,
    pub enabled: bool,
}

pub fn provider_contracts() -> Vec<ProviderContract> {
    let mut contracts = vec![
        wifi::provider_contract(),
        audio::provider_contract(),
        sysmonitor::provider_contract(),
    ];
    #[cfg(feature = "clock")]
    contracts.push(deskhalloumi_clock::provider_contract());
    #[cfg(feature = "battery")]
    contracts.push(deskhalloumi_battery::provider_contract());
    #[cfg(feature = "tmux")]
    contracts.push(deskhalloumi_tmux::provider_contract());
    contracts
}

pub fn active_modules(config: &Config) -> Vec<ModuleInfo> {
    let contracts = provider_contracts();
    let mut modules = config
        .modules
        .iter()
        .map(|module| ModuleInfo {
            id: module.name.clone(),
            enabled: module.enabled,
            position: module.position.clone(),
            configured_interval_ms: module.update_interval_ms,
            provider: contracts
                .iter()
                .find(|contract| {
                    contract.id == module.name
                        || (contract.id == "network" && module.name == "wifi")
                        || (contract.id == "system" && module.name == "sysmonitor")
                })
                .cloned()
                .map(ProviderPolicyInfo::from),
        })
        .collect::<Vec<_>>();
    modules.sort_by(|left, right| left.id.cmp(&right.id));
    modules
}

pub fn menus(config: &Config) -> Vec<MenuModel> {
    let menu = |id: &str, title: &str, source: MenuSource, enabled: bool| {
        let mut model = MenuModel::new(id, title, source);
        model.lifecycle = if enabled {
            MenuLifecycle::Fresh
        } else {
            MenuLifecycle::Disabled {
                reason: "disabled by configuration".to_string(),
            }
        };
        model
    };

    vec![
        menu("tray", "Tray menus", MenuSource::Tray, true),
        menu("widget-network", "Network widget", MenuSource::Widget, true),
        menu("widget-audio", "Audio widget", MenuSource::Widget, true),
        menu(
            "widget-system",
            "System widget",
            MenuSource::Widget,
            config.menus.system.enabled,
        ),
        menu(
            "custom",
            "Custom launchers",
            MenuSource::Custom,
            config.menus.custom.enabled,
        ),
        menu("filter-tab", "Filter tab", MenuSource::FilterTab, true),
        menu(
            "system",
            "System actions",
            MenuSource::System,
            config.menus.system.enabled,
        ),
        menu("mount", "Mounts", MenuSource::System, true),
        menu(
            "calendar",
            "Calendar",
            MenuSource::System,
            !config.menus.calendar.accounts.is_empty(),
        ),
    ]
}

pub fn actions(config: &Config) -> Vec<ActionInfo> {
    let mut actions = vec![
        action(
            "tray:show-aggregated",
            "Show all tray actions",
            "tray",
            "show-aggregated",
            "tray",
            true,
        ),
        action(
            "tray:show-favorites",
            "Show tray favorites",
            "tray",
            "show-favorites",
            "tray",
            true,
        ),
        action(
            "bar:open-system",
            "Open system menu",
            "bar",
            "open-system-menu",
            "system",
            config.menus.system.enabled,
        ),
        action(
            "bar:runtime-metrics",
            "Query live runtime metrics",
            "bar",
            "runtime-metrics",
            "diagnostics",
            true,
        ),
        action(
            "widget:wifi-refresh",
            "Refresh Wi-Fi",
            "widget",
            "wifi:refresh",
            "network",
            true,
        ),
        action(
            "widget:audio-refresh",
            "Refresh audio",
            "widget",
            "audio:refresh",
            "audio",
            true,
        ),
    ];

    for (index, binding) in config.keybindings.iter().enumerate() {
        actions.push(action(
            &format!("hotkey:{index}"),
            &binding.name,
            &format!("{:?}", binding.command_type).to_ascii_lowercase(),
            &binding.command,
            "hotkey",
            true,
        ));
    }
    for item in &config.menus.system.extra_items {
        actions.push(action(
            &format!("system:extra:{}", item.id),
            &item.title,
            "bar",
            &format!("system:extra:{}", item.id),
            "system",
            !item.command.trim().is_empty(),
        ));
    }
    for item in &config.menus.custom.items {
        actions.push(action(
            &format!("custom:{}", item.id),
            &item.title,
            "menu",
            &format!("{:?}", item.action),
            "custom",
            true,
        ));
    }
    actions.sort_by(|left, right| left.id.cmp(&right.id));
    actions
}

fn action(
    id: &str,
    title: &str,
    kind: &str,
    payload: &str,
    source: &str,
    enabled: bool,
) -> ActionInfo {
    ActionInfo {
        id: id.to_string(),
        title: title.to_string(),
        kind: kind.to_string(),
        payload: payload.to_string(),
        source: source.to_string(),
        enabled,
    }
}

pub fn hotkeys(config: &Config) -> Vec<HotkeyInfo> {
    config
        .keybindings
        .iter()
        .enumerate()
        .map(|(index, binding)| HotkeyInfo {
            index,
            name: binding.name.clone(),
            keysym: binding.keysym.clone(),
            trigger: format!("{:?}", binding.trigger).to_ascii_lowercase(),
            kind: format!("{:?}", binding.command_type).to_ascii_lowercase(),
            command: binding.command.clone(),
            enabled: true,
        })
        .collect()
}

pub fn invoke_typed_action(
    kind: ActionKind,
    payload: String,
    socket: Option<&Path>,
) -> Result<ActionBusResponse, String> {
    let action = match kind {
        ActionKind::Bar => DesktopAction::Bar(payload),
        ActionKind::Tray => DesktopAction::Tray(payload),
        ActionKind::Widget => DesktopAction::Widget(payload),
    };
    let request = ActionBusRequest::new(format!("cli-{}", std::process::id()), action);
    send_action_request(
        socket
            .map(Path::to_path_buf)
            .unwrap_or_else(default_action_bus_socket_path),
        &request,
    )
}

pub fn print_records<T: Serialize>(records: &[T], json: bool) -> Result<(), String> {
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(records)
                .map_err(|error| format!("failed to serialize introspection output: {error}"))?
        );
    } else {
        for record in records {
            println!(
                "{}",
                serde_json::to_string(record)
                    .map_err(|error| format!("failed to serialize introspection row: {error}"))?
            );
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::net::UnixListener;
    use std::thread;

    #[test]
    fn catalog_uses_shared_provider_and_menu_contracts() {
        let config = Config::default();
        let modules = active_modules(&config);
        assert!(modules.iter().any(|module| module.id == "clock"));
        let menus = menus(&config);
        assert!(menus.iter().any(|menu| menu.id == "filter-tab"));
        assert!(menus.iter().any(|menu| menu.id == "system"));
        let actions = actions(&config);
        assert!(
            actions
                .iter()
                .any(|action| action.id == "tray:show-favorites")
        );
    }

    #[test]
    fn live_metrics_query_decodes_structured_action_bus_response() {
        let temp = tempfile::tempdir().unwrap();
        let socket = temp.path().join("action.sock");
        let listener = UnixListener::bind(&socket).unwrap();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut line = String::new();
            BufReader::new(stream.try_clone().unwrap())
                .read_line(&mut line)
                .unwrap();
            let request: ActionBusRequest = serde_json::from_str(line.trim()).unwrap();
            assert_eq!(
                request.action,
                DesktopAction::Bar("runtime-metrics".to_string())
            );
            let data = serde_json::to_value(RuntimeMetricsSnapshot {
                active_tasks: 7,
                ..RuntimeMetricsSnapshot::default()
            })
            .unwrap();
            writeln!(
                stream,
                "{}",
                serde_json::to_string(&ActionBusResponse::ok_with_data(
                    request.request_id,
                    "runtime metrics",
                    data,
                ))
                .unwrap()
            )
            .unwrap();
        });

        let metrics = query_runtime_metrics(Some(&socket)).unwrap();
        assert_eq!(metrics.active_tasks, 7);
        server.join().unwrap();
    }
}
