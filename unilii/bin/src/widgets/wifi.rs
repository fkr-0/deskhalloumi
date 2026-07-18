//! WiFi widget implementation with menu

use super::{Widget, WidgetMessage};
use iced::widget::{button, column, row, scrollable, text};
use iced::{Alignment, Color, Element, Length};
use std::process::Command;

#[derive(Debug)]
pub struct Wifi {
    ssid: String,
    signal: u8,
    connected: bool,
    show_menu: bool,
    wifi_enabled: bool,
}

impl Wifi {
    pub fn new() -> Self {
        Self {
            ssid: "No WiFi".to_string(),
            signal: 0,
            connected: false,
            show_menu: false,
            wifi_enabled: true, // Assume enabled by default
        }
    }

    pub fn update_status(&mut self) {
        if let Ok(output) = Command::new("nmcli")
            .args(["-t", "-f", "wifi", "radio"])
            .output()
            && output.status.success()
        {
            let result = String::from_utf8_lossy(&output.stdout);
            self.wifi_enabled = result.trim().eq_ignore_ascii_case("enabled");
        }

        if !self.wifi_enabled {
            self.set_disabled_state();
            return;
        }

        if let Ok(output) = Command::new("nmcli")
            .args(["-t", "-f", "active,ssid,signal", "device", "wifi", "list"])
            .output()
            && output.status.success()
        {
            let result = String::from_utf8_lossy(&output.stdout);
            match parse_active_nmcli_network(&result) {
                Some((ssid, signal)) => {
                    self.connected = true;
                    self.ssid = ssid;
                    self.signal = signal;
                }
                None => {
                    self.connected = false;
                    self.ssid = "Disconnected".to_string();
                    self.signal = 0;
                }
            }
        }
    }

    pub fn get_networks(&self) -> Vec<NetworkInfo> {
        if !self.wifi_enabled {
            return Vec::new();
        }

        let mut networks = Vec::new();

        if let Ok(output) = Command::new("nmcli")
            .args(["-t", "-f", "ssid,signal,security", "device", "wifi", "list"])
            .output()
        {
            let result = String::from_utf8_lossy(&output.stdout);
            networks = parse_nmcli_networks(&result);
        }

        networks
    }

    fn set_disabled_state(&mut self) {
        self.wifi_enabled = false;
        self.connected = false;
        self.ssid = "WiFi Disabled".to_string();
        self.signal = 0;
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

    pub fn toggle_wifi(&mut self) {
        let new_state = if self.wifi_enabled { "off" } else { "on" };
        if Command::new("nmcli")
            .args(["radio", "wifi", new_state])
            .status()
            .is_ok_and(|status| status.success())
        {
            self.wifi_enabled = !self.wifi_enabled;
            if !self.wifi_enabled {
                self.set_disabled_state();
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct NetworkInfo {
    pub ssid: String,
    pub signal: u8,
    pub security: String,
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
            match action.as_str() {
                "toggle_menu" => {
                    self.show_menu = !self.show_menu;
                }
                "toggle_wifi" => {
                    self.toggle_wifi();
                }
                "connect" => {
                    // Connection logic would be handled by the panel
                    self.show_menu = false;
                }
                _ => {}
            }
        }
    }

    fn update_interval(&self) -> Option<u64> {
        Some(5000)
    }
}

impl Wifi {
    fn render_icon(&self) -> Element<'_, WidgetMessage> {
        let icon = if self.connected { "📶" } else { "📡" };
        let label = format!("{} {}%", icon, self.signal);

        button(text(label).size(12).color(Color::WHITE))
            .padding([2, 8])
            .on_press(WidgetMessage::Wifi("toggle_menu".to_string()))
            .into()
    }

    fn render_menu(&self) -> Element<'static, WidgetMessage> {
        let networks = self.get_networks();
        let is_empty = networks.is_empty();
        let icon_str = if self.connected { "📶" } else { "📡" };
        let label = format!("{} {}%", icon_str, self.signal);

        let mut menu_content = column![].spacing(4).padding(8);

        // WiFi enable/disable toggle
        let wifi_status = if self.wifi_enabled {
            "Disable WiFi"
        } else {
            "Enable WiFi"
        };
        let toggle_button = button(text(wifi_status).size(11).color(Color::WHITE))
            .padding([4, 8])
            .width(Length::Fill)
            .on_press(WidgetMessage::Wifi("toggle_wifi".to_string()));
        menu_content = menu_content.push(toggle_button);

        menu_content = menu_content.push(text("---").size(10).color(Color::WHITE));

        if self.wifi_enabled {
            menu_content =
                menu_content.push(text("Available Networks").size(12).color(Color::WHITE));

            let connect_messages: Vec<WidgetMessage> = networks
                .iter()
                .map(|network| WidgetMessage::Wifi(format!("connect:{}", network.ssid)))
                .collect();

            for (network, msg) in networks.iter().zip(connect_messages.iter()) {
                let net_row = row![
                    text(network.ssid.clone()).size(11).color(Color::WHITE),
                    text(format!("{}%", network.signal))
                        .size(10)
                        .color(Color::WHITE),
                ]
                .spacing(8)
                .align_y(Alignment::Center);

                menu_content = menu_content.push(
                    button(net_row)
                        .padding([4, 8])
                        .width(Length::Fill)
                        .on_press(msg.clone()),
                );
            }

            if is_empty {
                menu_content = menu_content.push(
                    text("Scanning for networks...")
                        .size(11)
                        .color(Color::WHITE),
                );
            }
        } else {
            menu_content = menu_content.push(text("WiFi is disabled").size(11).color(Color::WHITE));
        }

        let scroll_menu = scrollable(menu_content)
            .height(Length::Fixed(250.0))
            .width(Length::Fixed(300.0));

        let icon_button = button(text(label).size(12).color(Color::WHITE))
            .padding([2, 8])
            .on_press(WidgetMessage::Wifi("toggle_menu".to_string()));

        column![icon_button, scroll_menu].spacing(4).into()
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
    fn test_wifi_widget_initialization() {
        let wifi = Wifi::new();
        assert_eq!(wifi.name(), "wifi");
        assert_eq!(wifi.ssid, "No WiFi");
        assert_eq!(wifi.signal, 0);
        assert!(!wifi.connected);
        assert!(!wifi.show_menu);
        assert!(wifi.wifi_enabled);
    }

    #[test]
    fn test_wifi_widget_default() {
        let wifi = Wifi::default();
        assert_eq!(wifi.name(), "wifi");
        assert!(wifi.wifi_enabled);
    }

    #[test]
    fn test_wifi_widget_update_toggle_menu() {
        let mut wifi = Wifi::new();
        assert!(!wifi.show_menu);

        // Toggle menu on
        wifi.update(WidgetMessage::Wifi("toggle_menu".to_string()));
        assert!(wifi.show_menu);

        // Toggle menu off
        wifi.update(WidgetMessage::Wifi("toggle_menu".to_string()));
        assert!(!wifi.show_menu);
    }

    #[test]
    fn test_wifi_widget_update_connect() {
        let mut wifi = Wifi::new();
        wifi.show_menu = true;

        wifi.update(WidgetMessage::Wifi("connect".to_string()));
        assert!(!wifi.show_menu);
    }

    #[test]
    fn test_wifi_widget_update_invalid_action() {
        let mut wifi = Wifi::new();
        let original_ssid = wifi.ssid.clone();

        wifi.update(WidgetMessage::Wifi("invalid_action".to_string()));
        assert_eq!(wifi.ssid, original_ssid);
    }

    #[test]
    fn test_wifi_widget_update_interval() {
        let wifi = Wifi::new();
        assert_eq!(wifi.update_interval(), Some(5000));
    }

    #[test]
    fn test_wifi_widget_render_icon() {
        let wifi = Wifi::new();
        let element = wifi.view();
        // Should not panic
        drop(element);
    }

    #[test]
    fn test_wifi_widget_render_menu() {
        let mut wifi = Wifi::new();
        wifi.show_menu = true;
        let element = wifi.view();
        // Should not panic
        drop(element);
    }

    #[test]
    fn test_network_info_creation() {
        let network = NetworkInfo {
            ssid: "TestNetwork".to_string(),
            signal: 85,
            security: "WPA2".to_string(),
        };

        assert_eq!(network.ssid, "TestNetwork");
        assert_eq!(network.signal, 85);
        assert_eq!(network.security, "WPA2");
    }

    #[test]
    fn test_parse_nmcli_networks_keeps_fields_and_filters_invalid_rows() {
        let networks = parse_nmcli_networks("Cafe Net:87:WPA2\nOpenWifi:42:\n:90:WPA3\nBroken\n");

        assert_eq!(networks.len(), 2);
        assert_eq!(networks[0].ssid, "Cafe Net");
        assert_eq!(networks[0].signal, 87);
        assert_eq!(networks[0].security, "WPA2");
        assert_eq!(networks[1].ssid, "OpenWifi");
        assert_eq!(networks[1].signal, 42);
        assert_eq!(networks[1].security, "Open");
    }

    #[test]
    fn test_wifi_get_networks_when_disabled() {
        let mut wifi = Wifi::new();
        wifi.wifi_enabled = false;
        let networks = wifi.get_networks();
        assert!(networks.is_empty());
    }

    #[test]
    fn disabled_state_clears_connection_details() {
        let mut wifi = Wifi::new();
        wifi.connected = true;
        wifi.ssid = "Previously connected".to_string();
        wifi.signal = 95;
        wifi.set_disabled_state();

        assert!(!wifi.wifi_enabled);
        assert!(!wifi.connected);
        assert_eq!(wifi.ssid, "WiFi Disabled");
        assert_eq!(wifi.signal, 0);
    }

    // Integration tests that require nmcli

    #[test]
    #[ignore]
    fn test_wifi_update_status_connected() {
        let mut wifi = Wifi::new();
        wifi.update_status();

        // This test requires nmcli to be available and a connection to exist
        // Mark as ignored to avoid failing in CI
        if wifi.connected {
            assert_ne!(wifi.ssid, "No WiFi");
            assert_ne!(wifi.ssid, "Disconnected");
        }
    }

    #[test]
    #[ignore]
    fn test_wifi_get_networks() {
        let wifi = Wifi::new();
        let networks = wifi.get_networks();

        // This test requires nmcli to be available.
        // In isolated environments the list may be empty, but any returned row must be meaningful.
        for network in networks {
            assert!(!network.ssid.is_empty());
            assert!(network.signal <= 100);
        }
    }

    #[test]
    #[ignore]
    fn test_wifi_toggle_wifi() {
        let mut wifi = Wifi::new();
        let original_enabled = wifi.wifi_enabled;

        // This test requires nmcli and proper permissions
        wifi.toggle_wifi();

        // After toggle, the state should flip
        assert_ne!(wifi.wifi_enabled, original_enabled);
    }

    #[test]
    fn terse_parser_preserves_spaces_colons_and_backslashes() {
        let fields = split_nmcli_terse_line(r"Cafe WiFi\:Guest:72:WPA2\\Enterprise");
        assert_eq!(fields, vec!["Cafe WiFi:Guest", "72", "WPA2\\Enterprise"]);
    }

    #[test]
    fn active_network_parser_finds_active_row_not_first_row() {
        let active = parse_active_nmcli_network("no:Cafe:88\nyes:Home\\:Office:67\nno:Other:50\n");
        assert_eq!(active, Some(("Home:Office".to_string(), 67)));
    }

    #[test]
    fn network_parser_handles_escaped_ssids() {
        let networks = parse_nmcli_networks("Home\\:Office:80:WPA2\nOpen Network:45:\n");
        assert_eq!(networks[0].ssid, "Home:Office");
        assert_eq!(networks[1].security, "Open");
    }

    #[test]
    fn compact_wifi_label_bounds_long_unicode_ssids() {
        assert_eq!(compact_text("short", 8), "short");
        assert_eq!(compact_text("café-network-very-long", 8), "café-net…");
    }
}
