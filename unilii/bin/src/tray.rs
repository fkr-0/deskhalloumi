// Temporarily mark this module with dead_code allowances since tray functionality
// is currently disabled for runtime debugging
#![allow(dead_code)]

use tokio::sync::mpsc::UnboundedSender;
use tokio::time::{Duration, sleep};
use tracing::warn;
use zbus::{Connection, Proxy, zvariant::OwnedObjectPath};

const WATCHER_SERVICE: &str = "org.kde.StatusNotifierWatcher";
const WATCHER_PATH: &str = "/StatusNotifierWatcher";
const WATCHER_INTERFACE: &str = "org.kde.StatusNotifierWatcher";
const ITEM_INTERFACE: &str = "org.kde.StatusNotifierItem";
const TRAY_HOST_NAME: &str = "org.freedesktop.StatusNotifierHost-unilii";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrayIconPixmap {
    pub width: i32,
    pub height: i32,
    pub data: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrayIcon {
    pub key: String,
    pub service: String,
    pub path: String,
    pub id: String,
    pub title: String,
    pub icon_name: Option<String>,
    pub icon_pixmap: Option<TrayIconPixmap>,
    pub status: String,
    pub has_menu: bool,
    pub menu_object_path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TrayMenuAction {
    Activate,
    ContextMenu,
    SecondaryActivate,
    SpawnCommand(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrayMenuItem {
    pub label: String,
    pub action: TrayMenuAction,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TrayEvent {
    Icons(Vec<TrayIcon>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WifiNetwork {
    pub ssid: String,
    pub signal: u8,
    pub security: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetworkSnapshot {
    pub interface: String,
    pub state: String,
    pub enabled: bool,
    pub connected_ssid: Option<String>,
    pub known_networks: Vec<KnownNetwork>,
    pub networks: Vec<WifiNetwork>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnownNetwork {
    pub name: String,
    pub autoconnect: bool,
}

pub async fn run_tray_watcher(output: UnboundedSender<TrayEvent>, poll_ms: u64) {
    loop {
        match Connection::session().await {
            Ok(connection) => {
                register_as_host(&connection).await;
                let mut previous_icons: Vec<TrayIcon> = Vec::new();

                loop {
                    let icons = read_tray_icons(&connection).await;
                    if icons != previous_icons {
                        if output.send(TrayEvent::Icons(icons.clone())).is_err() {
                            return;
                        }
                        previous_icons = icons;
                    }
                    sleep(Duration::from_millis(poll_ms.max(500))).await;
                }
            }
            Err(error) => {
                warn!("tray: failed to connect to DBus session bus: {error}");
                sleep(Duration::from_secs(5)).await;
            }
        }
    }
}

pub fn is_network_icon(icon: &TrayIcon) -> bool {
    if let Some(icon_name) = &icon.icon_name
        && is_network_label(icon_name)
    {
        return true;
    }
    is_network_label(&icon.id) || is_network_label(&icon.title)
}

pub fn build_menu_items(icon: &TrayIcon) -> Vec<TrayMenuItem> {
    let mut items = vec![TrayMenuItem {
        label: format!("Activate {}", icon.title),
        action: TrayMenuAction::Activate,
    }];

    if icon.has_menu {
        items.push(TrayMenuItem {
            label: "Open context menu".to_string(),
            action: TrayMenuAction::ContextMenu,
        });
    }

    items.push(TrayMenuItem {
        label: "Secondary action".to_string(),
        action: TrayMenuAction::SecondaryActivate,
    });

    if is_network_icon(icon) {
        items.push(TrayMenuItem {
            label: "Open Network Settings".to_string(),
            action: TrayMenuAction::SpawnCommand("nm-connection-editor".to_string()),
        });
    }

    items
}

pub async fn invoke_menu_action(icon: &TrayIcon, action: TrayMenuAction) {
    if let TrayMenuAction::SpawnCommand(command) = action {
        if let Err(error) = spawn_command(command).await {
            warn!("tray: command spawn failed: {error}");
        }
        return;
    }

    let connection = match Connection::session().await {
        Ok(connection) => connection,
        Err(error) => {
            warn!("tray: failed to open DBus session for action: {error}");
            return;
        }
    };

    let proxy = match Proxy::new(
        &connection,
        icon.service.as_str(),
        icon.path.as_str(),
        ITEM_INTERFACE,
    )
    .await
    {
        Ok(proxy) => proxy,
        Err(error) => {
            warn!(
                "tray: failed to create item proxy service={} path={}: {error}",
                icon.service, icon.path
            );
            return;
        }
    };

    let call_result = match action {
        TrayMenuAction::Activate => proxy.call_method("Activate", &(0i32, 0i32)).await,
        TrayMenuAction::ContextMenu => proxy.call_method("ContextMenu", &(0i32, 0i32)).await,
        TrayMenuAction::SecondaryActivate => {
            proxy.call_method("SecondaryActivate", &(0i32, 0i32)).await
        }
        TrayMenuAction::SpawnCommand(_) => return,
    };

    if let Err(error) = call_result {
        warn!(
            "tray: action call failed service={} path={} action={:?}: {error}",
            icon.service, icon.path, action
        );
    }
}

pub async fn spawn_command(command: String) -> Result<(), String> {
    let command = command.trim().to_string();
    if command.is_empty() {
        return Err("cannot spawn an empty command".to_string());
    }

    std::process::Command::new("sh")
        .arg("-c")
        .arg(&command)
        .spawn()
        .map_err(|error| format!("failed to spawn command '{command}': {error}"))?;

    Ok(())
}

pub async fn read_network_snapshot(
    nmcli_path: String,
    rescan: bool,
) -> Result<NetworkSnapshot, String> {
    let device_status = run_nmcli(
        &nmcli_path,
        &["-t", "-f", "DEVICE,TYPE,STATE", "device", "status"],
    )
    .await?;
    let (interface, state, enabled) = parse_device_status(&device_status)
        .ok_or_else(|| "no wifi interface found via nmcli".to_string())?;
    let connected_ssid = if enabled {
        let active = run_nmcli(&nmcli_path, &["-t", "-f", "ACTIVE,SSID", "device", "wifi"]).await?;
        parse_connected_ssid(&active)
    } else {
        None
    };
    let known_networks = if enabled {
        let known = run_nmcli(
            &nmcli_path,
            &["-t", "-f", "NAME,TYPE,AUTOCONNECT", "connection", "show"],
        )
        .await?;
        parse_known_networks(&known)
    } else {
        Vec::new()
    };

    let networks = if enabled {
        let mut args = vec!["-t", "-f", "SSID,SIGNAL,SECURITY", "device", "wifi", "list"];
        if rescan {
            args.extend(["--rescan", "yes"]);
        }
        let wifi_list = run_nmcli(&nmcli_path, &args).await?;
        parse_wifi_networks(&wifi_list)
    } else {
        Vec::new()
    };

    Ok(NetworkSnapshot {
        interface,
        state,
        enabled,
        connected_ssid,
        known_networks,
        networks,
    })
}

pub async fn set_wifi_enabled(nmcli_path: String, enable: bool) -> Result<(), String> {
    let setting = if enable { "on" } else { "off" };
    run_nmcli(&nmcli_path, &["radio", "wifi", setting]).await?;
    Ok(())
}

pub fn icon_label_for(icon: &TrayIcon) -> String {
    if is_network_label(&icon.id) || is_network_label(&icon.title) {
        return "📶".to_string();
    }

    icon.icon_name
        .as_deref()
        .and_then(initials_label)
        .or_else(|| initials_label(&icon.id))
        .or_else(|| initials_label(icon.title.as_str()))
        .or_else(|| icon.service.rsplit('.').next().and_then(initials_label))
        .unwrap_or_else(|| "◉".to_string())
}

pub fn icon_label_for_name(icon_name: &str) -> String {
    let lower = icon_name.to_ascii_lowercase();
    if lower.contains("network") || lower.contains("wifi") || lower.contains("wireless") {
        "📶".to_string()
    } else if lower.contains("volume") || lower.contains("audio") || lower.contains("sound") {
        "🔊".to_string()
    } else if lower.contains("battery") {
        "🔋".to_string()
    } else if lower.contains("bluetooth") {
        "🅱".to_string()
    } else if lower.contains("mail") {
        "✉".to_string()
    } else if lower.contains("sync") || lower.contains("cloud") {
        "☁".to_string()
    } else if lower.contains("chat") || lower.contains("message") || lower.contains("im") {
        "💬".to_string()
    } else if lower.contains("calendar") || lower.contains("alarm") || lower.contains("clock") {
        "🕒".to_string()
    } else {
        initials_label(icon_name).unwrap_or_else(|| "◉".to_string())
    }
}

fn initials_label(input: &str) -> Option<String> {
    let compact: String = input
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric() || ch.is_whitespace())
        .collect();

    let mut initials = compact
        .split_whitespace()
        .filter_map(|part| part.chars().next())
        .filter(|ch| ch.is_ascii_alphanumeric())
        .take(2)
        .collect::<String>()
        .to_uppercase();

    if initials.is_empty() {
        initials = compact
            .chars()
            .filter(|ch| ch.is_ascii_alphanumeric())
            .take(2)
            .collect::<String>()
            .to_uppercase();
    }

    if initials.is_empty() {
        None
    } else {
        Some(initials)
    }
}

pub fn visible_menu_items(total_items: usize, progress: f32) -> usize {
    let clamped = progress.clamp(0.0, 1.0);
    (total_items as f32 * clamped).ceil() as usize
}

pub fn animate_progress(current: f32, target: f32, step: f32) -> f32 {
    let current = current.clamp(0.0, 1.0);
    let target = target.clamp(0.0, 1.0);
    if (current - target).abs() <= step {
        target
    } else if current < target {
        (current + step).clamp(0.0, 1.0)
    } else {
        (current - step).clamp(0.0, 1.0)
    }
}

async fn register_as_host(connection: &Connection) {
    if let Err(error) = connection.request_name(TRAY_HOST_NAME).await {
        warn!("tray: failed requesting host name: {error}");
        return;
    }

    let watcher_proxy =
        match Proxy::new(connection, WATCHER_SERVICE, WATCHER_PATH, WATCHER_INTERFACE).await {
            Ok(proxy) => proxy,
            Err(error) => {
                warn!("tray: watcher proxy unavailable: {error}");
                return;
            }
        };

    if let Err(error) = watcher_proxy
        .call_method("RegisterStatusNotifierHost", &(TRAY_HOST_NAME))
        .await
    {
        warn!("tray: host registration failed: {error}");
    }
}

async fn read_tray_icons(connection: &Connection) -> Vec<TrayIcon> {
    let watcher_proxy =
        match Proxy::new(connection, WATCHER_SERVICE, WATCHER_PATH, WATCHER_INTERFACE).await {
            Ok(proxy) => proxy,
            Err(_) => return Vec::new(),
        };

    let registered: Vec<String> = match watcher_proxy
        .get_property("RegisteredStatusNotifierItems")
        .await
    {
        Ok(items) => items,
        Err(_) => return Vec::new(),
    };

    let mut icons = Vec::new();
    for identifier in registered {
        if let Some(icon) = read_tray_icon(connection, &identifier).await {
            icons.push(icon);
        }
    }

    icons.sort_by(|a, b| a.title.cmp(&b.title));
    icons
}

async fn read_tray_icon(connection: &Connection, identifier: &str) -> Option<TrayIcon> {
    fn pick_best_pixmap(candidates: Vec<(i32, i32, Vec<u8>)>) -> Option<TrayIconPixmap> {
        candidates
            .into_iter()
            .filter(|(w, h, data)| *w > 0 && *h > 0 && !data.is_empty())
            .max_by_key(|(w, h, _)| (*w as i64) * (*h as i64))
            .map(|(width, height, data)| TrayIconPixmap {
                width,
                height,
                data,
            })
    }
    let (service, path) = parse_identifier(identifier);
    let proxy = Proxy::new(connection, service.as_str(), path.as_str(), ITEM_INTERFACE)
        .await
        .ok()?;

    let id: String = proxy
        .get_property("Id")
        .await
        .unwrap_or_else(|_| service.clone());
    let title: String = proxy
        .get_property("Title")
        .await
        .unwrap_or_else(|_| id.clone());
    let status: String = proxy
        .get_property("Status")
        .await
        .unwrap_or_else(|_| "Active".to_string());
    let primary_icon_name = proxy.get_property("IconName").await.ok();
    let attention_icon_name = proxy.get_property("AttentionIconName").await.ok();
    let primary_icon_pixmap: Vec<(i32, i32, Vec<u8>)> =
        proxy.get_property("IconPixmap").await.unwrap_or_default();
    let attention_icon_pixmap: Vec<(i32, i32, Vec<u8>)> = proxy
        .get_property("AttentionIconPixmap")
        .await
        .unwrap_or_default();
    let icon_name = if status.eq_ignore_ascii_case("NeedsAttention") {
        attention_icon_name.clone().or(primary_icon_name.clone())
    } else {
        primary_icon_name.clone().or(attention_icon_name.clone())
    };
    let icon_pixmap = if status.eq_ignore_ascii_case("NeedsAttention") {
        pick_best_pixmap(attention_icon_pixmap).or_else(|| pick_best_pixmap(primary_icon_pixmap))
    } else {
        pick_best_pixmap(primary_icon_pixmap).or_else(|| pick_best_pixmap(attention_icon_pixmap))
    };
    let menu = proxy.get_property::<OwnedObjectPath>("Menu").await.ok();

    Some(TrayIcon {
        key: format!("{service}{path}"),
        service,
        path,
        id,
        title,
        icon_name,
        icon_pixmap,
        status,
        has_menu: menu.is_some(),
        menu_object_path: menu.map(|p| p.as_str().to_string()),
    })
}

fn parse_identifier(identifier: &str) -> (String, String) {
    if let Some((service, object_path)) = identifier.split_once('/') {
        (service.to_string(), format!("/{object_path}"))
    } else {
        (identifier.to_string(), "/StatusNotifierItem".to_string())
    }
}

fn is_network_label(label: &str) -> bool {
    let lower = label.to_ascii_lowercase();
    lower.contains("network") || lower.contains("wifi") || lower.contains("wireless")
}

async fn run_nmcli(nmcli_path: &str, args: &[&str]) -> Result<String, String> {
    let nmcli = nmcli_path.to_string();
    let args_vec: Vec<String> = args.iter().map(|value| value.to_string()).collect();
    let output = tokio::task::spawn_blocking(move || {
        std::process::Command::new(nmcli).args(args_vec).output()
    })
    .await
    .map_err(|error| format!("failed to join nmcli task: {error}"))?
    .map_err(|error| format!("failed to execute nmcli: {error}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(if stderr.is_empty() {
            format!("nmcli failed with status {}", output.status)
        } else {
            format!("nmcli failed: {stderr}")
        });
    }

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

fn parse_device_status(output: &str) -> Option<(String, String, bool)> {
    output.lines().find_map(|line| {
        let mut parts = line.splitn(3, ':');
        let device = parts.next()?.trim();
        let iface_type = parts.next()?.trim();
        let state = parts.next()?.trim();
        if iface_type != "wifi" {
            return None;
        }

        let enabled = !matches!(state, "unavailable" | "unmanaged");
        Some((device.to_string(), state.to_string(), enabled))
    })
}

fn parse_wifi_networks(output: &str) -> Vec<WifiNetwork> {
    let mut seen = std::collections::HashSet::new();
    let mut networks = Vec::new();

    for line in output.lines() {
        let mut parts = line.splitn(3, ':');
        let ssid = parts.next().unwrap_or_default().trim();
        let signal = parts
            .next()
            .and_then(|value| value.trim().parse::<u8>().ok())
            .unwrap_or(0);
        let security_raw = parts.next().unwrap_or_default().trim();
        let security = if security_raw.is_empty() {
            "Open".to_string()
        } else {
            security_raw.to_string()
        };

        if ssid.is_empty() || !seen.insert(ssid.to_string()) {
            continue;
        }

        networks.push(WifiNetwork {
            ssid: ssid.to_string(),
            signal,
            security,
        });
    }

    networks.sort_by(|a, b| b.signal.cmp(&a.signal).then(a.ssid.cmp(&b.ssid)));
    networks
}

fn parse_connected_ssid(output: &str) -> Option<String> {
    for line in output.lines() {
        let mut parts = line.splitn(2, ':');
        let active = parts.next().unwrap_or_default().trim();
        let ssid = parts.next().unwrap_or_default().trim();
        if active.eq_ignore_ascii_case("yes") && !ssid.is_empty() {
            return Some(ssid.to_string());
        }
    }
    None
}

fn parse_known_networks(output: &str) -> Vec<KnownNetwork> {
    let mut known = Vec::new();
    let mut seen = std::collections::HashSet::new();

    for line in output.lines() {
        let mut parts = line.splitn(3, ':');
        let name = parts.next().unwrap_or_default().trim();
        let kind = parts.next().unwrap_or_default().trim();
        let autoconnect_raw = parts.next().unwrap_or_default().trim();
        if name.is_empty() || kind != "802-11-wireless" {
            continue;
        }
        if !seen.insert(name.to_string()) {
            continue;
        }
        let autoconnect = matches!(autoconnect_raw, "yes" | "true" | "on" | "1");
        known.push(KnownNetwork {
            name: name.to_string(),
            autoconnect,
        });
    }

    known.sort_by(|left, right| {
        right
            .autoconnect
            .cmp(&left.autoconnect)
            .then(left.name.cmp(&right.name))
    });
    known
}

#[cfg(test)]
mod tests {
    use super::{
        animate_progress, icon_label_for_name, parse_connected_ssid, parse_device_status,
        parse_known_networks, parse_wifi_networks, spawn_command, visible_menu_items,
    };

    #[test]
    fn icon_label_uses_known_icon_keywords() {
        assert_eq!(icon_label_for_name("network-wireless-signal-good"), "📶");
        assert_eq!(icon_label_for_name("audio-volume-high"), "🔊");
        assert_eq!(icon_label_for_name("unknown-icon"), "◉");
    }

    #[test]
    fn visible_menu_items_reveals_incrementally() {
        assert_eq!(visible_menu_items(5, 0.0), 0);
        assert_eq!(visible_menu_items(5, 0.2), 1);
        assert_eq!(visible_menu_items(5, 0.5), 3);
        assert_eq!(visible_menu_items(5, 1.0), 5);
    }

    #[test]
    fn animate_progress_moves_towards_target_and_clamps() {
        assert_eq!(animate_progress(0.0, 1.0, 0.15), 0.15);
        assert_eq!(animate_progress(0.95, 1.0, 0.15), 1.0);
        assert_eq!(animate_progress(0.4, 0.0, 0.2), 0.2);
        assert_eq!(animate_progress(0.1, 0.0, 0.2), 0.0);
    }

    #[test]
    fn parses_nmcli_device_status_for_wifi_interface() {
        let input = "wlp2s0:wifi:connected\nlo:loopback:unmanaged\n";
        let parsed = parse_device_status(input).expect("wifi interface should parse");
        assert_eq!(parsed.0, "wlp2s0");
        assert_eq!(parsed.1, "connected");
        assert!(parsed.2);
    }

    #[test]
    fn parses_nmcli_wifi_list_with_signal_and_security() {
        let input = "CafeNet:78:WPA2\nOpenWifi:45:\n";
        let networks = parse_wifi_networks(input);
        assert_eq!(networks.len(), 2);
        assert_eq!(networks[0].ssid, "CafeNet");
        assert_eq!(networks[0].signal, 78);
        assert_eq!(networks[0].security, "WPA2");
        assert_eq!(networks[1].ssid, "OpenWifi");
        assert_eq!(networks[1].security, "Open");
    }

    #[tokio::test]
    async fn rejects_empty_spawn_command() {
        let result = spawn_command("   ".to_string()).await;
        assert!(result.is_err());
    }

    #[test]
    fn icon_label_for_battery_icons() {
        assert_eq!(icon_label_for_name("battery-full"), "🔋");
        assert_eq!(icon_label_for_name("battery-low"), "🔋");
    }

    #[test]
    fn icon_label_for_audio_icons() {
        assert_eq!(icon_label_for_name("audio-volume-muted"), "🔊");
        assert_eq!(icon_label_for_name("audio-input-microphone"), "🔊");
    }

    #[test]
    fn icon_label_for_mail_icons() {
        assert_eq!(icon_label_for_name("mail-unread"), "✉");
        assert_eq!(icon_label_for_name("email-new"), "✉");
    }

    #[test]
    fn icon_label_for_bluetooth_icons() {
        assert_eq!(icon_label_for_name("bluetooth-active"), "🅱");
        assert_eq!(icon_label_for_name("bluetooth-disabled"), "🅱");
    }

    #[test]
    fn icon_label_for_cloud_and_chat_icons() {
        assert_eq!(icon_label_for_name("cloud-syncing"), "☁");
        assert_eq!(icon_label_for_name("chat-unread"), "💬");
        assert_eq!(icon_label_for_name("calendar-today"), "🕒");
    }

    #[test]
    fn visible_menu_items_clamps_to_valid_range() {
        assert_eq!(visible_menu_items(10, -0.5), 0);
        assert_eq!(visible_menu_items(10, 1.5), 10);
        assert_eq!(visible_menu_items(0, 0.5), 0);
    }

    #[test]
    fn animate_progress_handles_edge_cases() {
        assert_eq!(animate_progress(0.0, 0.0, 0.1), 0.0);
        assert_eq!(animate_progress(1.0, 1.0, 0.1), 1.0);
        assert_eq!(animate_progress(0.5, 0.5, 0.1), 0.5);
    }

    #[test]
    fn parse_device_status_handles_no_wifi_interface() {
        let input = "lo:loopback:unmanaged\neth0:ethernet:connected\n";
        let parsed = parse_device_status(input);
        assert!(parsed.is_none());
    }

    #[test]
    fn parse_wifi_networks_removes_duplicates() {
        let input = "TestNet:50:WPA2\nTestNet:50:WPA2\nOtherNet:75:WEP\n";
        let networks = parse_wifi_networks(input);
        assert_eq!(networks.len(), 2);
        assert_eq!(networks[0].ssid, "OtherNet");
        assert_eq!(networks[1].ssid, "TestNet");
    }

    #[test]
    fn parse_wifi_networks_sorts_by_signal_then_ssid() {
        let input = "NetA:50:WPA2\nNetB:75:WPA2\nNetC:75:Open\nNetD:25:Open\n";
        let networks = parse_wifi_networks(input);
        assert_eq!(networks.len(), 4);
        assert_eq!(networks[0].ssid, "NetB");
        assert_eq!(networks[1].ssid, "NetC");
        assert_eq!(networks[2].ssid, "NetA");
        assert_eq!(networks[3].ssid, "NetD");
    }

    #[test]
    fn parse_wifi_networks_handles_empty_ssid() {
        let input = ":75:WPA2\nValidNet:50:Open\n";
        let networks = parse_wifi_networks(input);
        assert_eq!(networks.len(), 1);
        assert_eq!(networks[0].ssid, "ValidNet");
    }

    #[test]
    fn parse_connected_ssid_extracts_yes_row() {
        let input = "no:Cafe\nyes:Home\n";
        assert_eq!(parse_connected_ssid(input).as_deref(), Some("Home"));
    }

    #[test]
    fn parse_known_networks_filters_wifi_and_sorts() {
        let input = "Home:802-11-wireless:yes\nVPN:vpn:yes\nCafe:802-11-wireless:no\n";
        let known = parse_known_networks(input);
        assert_eq!(known.len(), 2);
        assert_eq!(known[0].name, "Home");
        assert!(known[0].autoconnect);
    }
}
