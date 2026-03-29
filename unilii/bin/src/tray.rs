use tokio::sync::mpsc::UnboundedSender;
use tokio::time::{sleep, Duration};
use tracing::warn;
use zbus::{zvariant::OwnedObjectPath, Connection, Proxy};

const WATCHER_SERVICE: &str = "org.kde.StatusNotifierWatcher";
const WATCHER_PATH: &str = "/StatusNotifierWatcher";
const WATCHER_INTERFACE: &str = "org.kde.StatusNotifierWatcher";
const ITEM_INTERFACE: &str = "org.kde.StatusNotifierItem";
const TRAY_HOST_NAME: &str = "org.freedesktop.StatusNotifierHost-unilii";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrayIcon {
    pub key: String,
    pub service: String,
    pub path: String,
    pub id: String,
    pub title: String,
    pub icon_name: Option<String>,
    pub status: String,
    pub has_menu: bool,
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
    pub networks: Vec<WifiNetwork>,
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
    if let Some(icon_name) = &icon.icon_name {
        if is_network_label(icon_name) {
            return true;
        }
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
        .arg("-lc")
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
        networks,
    })
}

pub async fn set_wifi_enabled(nmcli_path: String, enable: bool) -> Result<(), String> {
    let setting = if enable { "on" } else { "off" };
    run_nmcli(&nmcli_path, &["radio", "wifi", setting]).await?;
    Ok(())
}

pub fn icon_label_for(icon: &TrayIcon) -> String {
    if let Some(name) = &icon.icon_name {
        return icon_label_for_name(name);
    }
    "◉".to_string()
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
    } else {
        "◉".to_string()
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
    let icon_name = proxy.get_property("IconName").await.ok();
    let status: String = proxy
        .get_property("Status")
        .await
        .unwrap_or_else(|_| "Active".to_string());
    let menu = proxy.get_property::<OwnedObjectPath>("Menu").await.ok();

    Some(TrayIcon {
        key: format!("{service}{path}"),
        service,
        path,
        id,
        title,
        icon_name,
        status,
        has_menu: menu.is_some(),
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

#[cfg(test)]
mod tests {
    use super::{
        animate_progress, icon_label_for_name, parse_device_status, parse_wifi_networks,
        spawn_command, visible_menu_items,
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
}
