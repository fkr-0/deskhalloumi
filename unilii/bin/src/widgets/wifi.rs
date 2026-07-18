//! WiFi widget with asynchronously refreshed NetworkManager state.

use std::{ffi::OsString, time::Duration};

use deskhalloumi_core::runtime::{ActionCommand, ActionRunner};
use iced::widget::{button, column, row, scrollable, text};
use iced::{Alignment, Color, Element, Length};

use super::{Widget, WidgetMessage};

#[derive(Debug)]
pub struct Wifi {
    ssid: String,
    signal: u8,
    connected: bool,
    show_menu: bool,
    wifi_enabled: bool,
    networks: Vec<NetworkInfo>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetworkInfo {
    pub ssid: String,
    pub signal: u8,
    pub security: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WifiSnapshot {
    pub ssid: String,
    pub signal: u8,
    pub connected: bool,
    pub wifi_enabled: bool,
    pub networks: Vec<NetworkInfo>,
}

impl Wifi {
    pub fn new() -> Self {
        Self {
            ssid: "No WiFi".to_string(),
            signal: 0,
            connected: false,
            show_menu: false,
            wifi_enabled: true,
            networks: Vec::new(),
        }
    }

    pub fn menu_is_open(&self) -> bool {
        self.show_menu
    }

    pub fn desired_enabled_state(&self) -> bool {
        !self.wifi_enabled
    }

    pub fn apply_snapshot(&mut self, snapshot: WifiSnapshot) {
        self.ssid = snapshot.ssid;
        self.signal = snapshot.signal;
        self.connected = snapshot.connected;
        self.wifi_enabled = snapshot.wifi_enabled;
        self.networks = snapshot.networks;
    }

    fn set_disabled_state(&mut self) {
        self.apply_snapshot(WifiSnapshot::disabled());
    }

    pub fn compact_label(&self) -> String {
        if !self.wifi_enabled {
            return "📡 off".to_string();
        }
        if self.connected {
            format!("📶 {}", compact_text(&self.ssid, 18))
        } else {
            "📡 --".to_string()
        }
    }

    pub fn wifi_enabled(&self) -> bool {
        self.wifi_enabled
    }

    pub fn connected_ssid(&self) -> Option<&str> {
        self.connected.then_some(self.ssid.as_str())
    }

    fn render_icon(&self) -> Element<'_, WidgetMessage> {
        let icon = if self.connected { "📶" } else { "📡" };
        button(
            text(format!("{} {}%", icon, self.signal))
                .size(12)
                .color(Color::WHITE),
        )
        .padding([2, 8])
        .on_press(WidgetMessage::Wifi("toggle_menu".to_string()))
        .into()
    }

    fn render_menu(&self) -> Element<'static, WidgetMessage> {
        let icon = if self.connected { "📶" } else { "📡" };
        let mut menu_content = column![].spacing(4).padding(8);
        menu_content = menu_content.push(
            button(
                text(if self.wifi_enabled {
                    "Disable WiFi"
                } else {
                    "Enable WiFi"
                })
                .size(11)
                .color(Color::WHITE),
            )
            .padding([4, 8])
            .width(Length::Fill)
            .on_press(WidgetMessage::Wifi("toggle_wifi".to_string())),
        );
        menu_content = menu_content.push(text("---").size(10).color(Color::WHITE));

        if self.wifi_enabled {
            menu_content =
                menu_content.push(text("Available Networks").size(12).color(Color::WHITE));
            for network in &self.networks {
                let network_row = row![
                    text(network.ssid.clone()).size(11).color(Color::WHITE),
                    text(format!("{}%", network.signal))
                        .size(10)
                        .color(Color::WHITE),
                ]
                .spacing(8)
                .align_y(Alignment::Center);
                menu_content = menu_content.push(
                    button(network_row)
                        .padding([4, 8])
                        .width(Length::Fill)
                        .on_press(WidgetMessage::Wifi(format!("connect:{}", network.ssid))),
                );
            }
            if self.networks.is_empty() {
                menu_content =
                    menu_content.push(text("No scan results yet").size(11).color(Color::WHITE));
            }
        } else {
            menu_content = menu_content.push(text("WiFi is disabled").size(11).color(Color::WHITE));
        }

        let scroll_menu = scrollable(menu_content)
            .height(Length::Fixed(250.0))
            .width(Length::Fixed(300.0));
        let icon_button = button(
            text(format!("{} {}%", icon, self.signal))
                .size(12)
                .color(Color::WHITE),
        )
        .padding([2, 8])
        .on_press(WidgetMessage::Wifi("toggle_menu".to_string()));
        column![icon_button, scroll_menu].spacing(4).into()
    }
}

impl WifiSnapshot {
    fn disabled() -> Self {
        Self {
            ssid: "WiFi Disabled".to_string(),
            signal: 0,
            connected: false,
            wifi_enabled: false,
            networks: Vec::new(),
        }
    }
}

pub async fn read_wifi_snapshot(nmcli: String) -> Result<WifiSnapshot, String> {
    let radio = run_nmcli(&nmcli, "radio", ["-t", "-f", "wifi", "radio"]).await?;
    let wifi_enabled = radio.trim().eq_ignore_ascii_case("enabled");
    if !wifi_enabled {
        return Ok(WifiSnapshot::disabled());
    }

    let active = run_nmcli(
        &nmcli,
        "active-network",
        ["-t", "-f", "active,ssid,signal", "device", "wifi", "list"],
    )
    .await?;
    let networks = run_nmcli(
        &nmcli,
        "network-scan",
        ["-t", "-f", "ssid,signal,security", "device", "wifi", "list"],
    )
    .await
    .map(|output| parse_nmcli_networks(&output))
    .unwrap_or_default();
    let active = parse_active_nmcli_network(&active);
    Ok(match active {
        Some((ssid, signal)) => WifiSnapshot {
            ssid,
            signal,
            connected: true,
            wifi_enabled: true,
            networks,
        },
        None => WifiSnapshot {
            ssid: "Disconnected".to_string(),
            signal: 0,
            connected: false,
            wifi_enabled: true,
            networks,
        },
    })
}

pub async fn set_wifi_enabled(nmcli: String, enabled: bool) -> Result<WifiSnapshot, String> {
    run_nmcli(
        &nmcli,
        "toggle-radio",
        ["radio", "wifi", if enabled { "on" } else { "off" }],
    )
    .await?;
    read_wifi_snapshot(nmcli).await
}

async fn run_nmcli<const N: usize>(
    nmcli: &str,
    action: &str,
    args: [&str; N],
) -> Result<String, String> {
    let outcome = ActionRunner::with_timeout("wifi-widget", action, Duration::from_secs(8))
        .with_output_limit(2 * 1024 * 1024)
        .run_command(ActionCommand::new(
            nmcli,
            args.into_iter().map(OsString::from).collect(),
        ))
        .await;
    if let Err(error) = outcome.result {
        return Err(if outcome.stderr.trim().is_empty() {
            error
        } else {
            outcome.stderr.trim().to_string()
        });
    }
    if outcome.stdout_truncated {
        return Err(format!(
            "nmcli output exceeded limit ({} bytes)",
            outcome.stdout_bytes
        ));
    }
    Ok(outcome.stdout)
}

fn compact_text(value: &str, max_chars: usize) -> String {
    let mut chars = value.chars();
    let prefix = chars.by_ref().take(max_chars).collect::<String>();
    if chars.next().is_some() {
        format!("{prefix}…")
    } else {
        prefix
    }
}

fn split_nmcli_terse_line(line: &str) -> Vec<String> {
    let mut fields = vec![String::new()];
    let mut escaped = false;
    for ch in line.chars() {
        if escaped {
            fields.last_mut().expect("one field").push(ch);
            escaped = false;
        } else if ch == '\\' {
            escaped = true;
        } else if ch == ':' {
            fields.push(String::new());
        } else {
            fields.last_mut().expect("one field").push(ch);
        }
    }
    if escaped {
        fields.last_mut().expect("one field").push('\\');
    }
    fields
}

fn parse_active_nmcli_network(output: &str) -> Option<(String, u8)> {
    output.lines().find_map(|line| {
        let fields = split_nmcli_terse_line(line);
        if fields
            .first()
            .is_some_and(|active| active.eq_ignore_ascii_case("yes"))
        {
            let ssid = fields.get(1)?.trim();
            if ssid.is_empty() {
                return None;
            }
            let signal = fields
                .get(2)
                .and_then(|value| value.parse().ok())
                .unwrap_or(0);
            Some((ssid.to_string(), signal))
        } else {
            None
        }
    })
}

fn parse_nmcli_networks(output: &str) -> Vec<NetworkInfo> {
    output
        .lines()
        .filter_map(|line| {
            let fields = split_nmcli_terse_line(line);
            let ssid = fields.first()?.trim();
            let signal = fields.get(1)?.trim();
            let security = fields.get(2).map(String::as_str).unwrap_or_default().trim();
            if ssid.is_empty() || signal.is_empty() {
                return None;
            }
            Some(NetworkInfo {
                ssid: ssid.to_string(),
                signal: signal.parse().unwrap_or(0),
                security: if security.is_empty() {
                    "Open".to_string()
                } else {
                    security.to_string()
                },
            })
        })
        .collect()
}

impl Widget for Wifi {
    fn name(&self) -> &str {
        "wifi"
    }

    fn view(&self) -> Element<'_, WidgetMessage> {
        if self.show_menu {
            self.render_menu()
        } else {
            self.render_icon()
        }
    }

    fn update(&mut self, message: WidgetMessage) {
        if let WidgetMessage::Wifi(action) = message {
            if action == "toggle_menu" {
                self.show_menu = !self.show_menu;
            } else if action == "connect" || action.starts_with("connect:") {
                self.show_menu = false;
            }
        }
    }

    fn update_interval(&self) -> Option<u64> {
        Some(5000)
    }
}

impl Default for Wifi {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pure_widget_state_and_render_paths() {
        let mut wifi = Wifi::new();
        assert_eq!(wifi.name(), "wifi");
        wifi.update(WidgetMessage::Wifi("toggle_menu".to_string()));
        assert!(wifi.menu_is_open());
        drop(wifi.view());
        wifi.update(WidgetMessage::Wifi("connect:Home".to_string()));
        assert!(!wifi.menu_is_open());
        assert_eq!(wifi.update_interval(), Some(5000));
    }

    #[test]
    fn disabled_state_clears_connection_details() {
        let mut wifi = Wifi::new();
        wifi.apply_snapshot(WifiSnapshot {
            ssid: "Previously connected".to_string(),
            signal: 95,
            connected: true,
            wifi_enabled: true,
            networks: Vec::new(),
        });
        wifi.set_disabled_state();
        assert!(!wifi.wifi_enabled());
        assert_eq!(wifi.connected_ssid(), None);
        assert_eq!(wifi.compact_label(), "📡 off");
    }

    #[test]
    fn terse_parser_preserves_spaces_colons_and_backslashes() {
        let fields = split_nmcli_terse_line(r"Cafe WiFi\:Guest:72:WPA2\\Enterprise");
        assert_eq!(fields, vec!["Cafe WiFi:Guest", "72", "WPA2\\Enterprise"]);
    }

    #[test]
    fn active_and_scan_parsers_handle_escaped_ssids() {
        let active = parse_active_nmcli_network("no:Cafe:88\nyes:Home\\:Office:67\n");
        assert_eq!(active, Some(("Home:Office".to_string(), 67)));
        let networks = parse_nmcli_networks("Home\\:Office:80:WPA2\nOpen Network:45:\n");
        assert_eq!(networks.len(), 2);
        assert_eq!(networks[0].ssid, "Home:Office");
        assert_eq!(networks[1].security, "Open");
    }

    #[test]
    fn compact_wifi_label_bounds_unicode_ssids() {
        let mut wifi = Wifi::new();
        wifi.apply_snapshot(WifiSnapshot {
            ssid: "Funknetz-Überraschung-mit-sehr-langem-Namen".to_string(),
            signal: 80,
            connected: true,
            wifi_enabled: true,
            networks: Vec::new(),
        });
        assert!(wifi.compact_label().chars().count() <= 21);
    }
}
