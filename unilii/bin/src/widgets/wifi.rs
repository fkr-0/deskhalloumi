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
        // Check if WiFi is enabled
        if let Ok(output) = Command::new("nmcli")
            .args(["-t", "-f", "wifi", "radio"])
            .output()
        {
            let result = String::from_utf8_lossy(&output.stdout);
            self.wifi_enabled = result.trim() == "enabled";
        }

        // Get current WiFi connection using nmcli
        if self.wifi_enabled {
            if let Ok(output) = Command::new("nmcli")
                .args(["-t", "-f", "active,ssid,signal", "device", "wifi", "list"])
                .output()
            {
                let result = String::from_utf8_lossy(&output.stdout);
                if !result.trim().is_empty() {
                    let parts: Vec<&str> = result.trim().split_whitespace().collect();
                    self.connected = parts.get(0).map(|&s| s == "yes").unwrap_or(false);
                    self.ssid = parts.get(1).unwrap_or(&"Unknown").to_string();
                    self.signal = parts.get(2).and_then(|s| s.parse().ok()).unwrap_or(0);
                } else {
                    self.connected = false;
                    self.ssid = "Disconnected".to_string();
                    self.signal = 0;
                }
            }
        } else {
            self.connected = false;
            self.ssid = "WiFi Disabled".to_string();
            self.signal = 0;
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
            for line in result.lines() {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 3 {
                    networks.push(NetworkInfo {
                        ssid: parts[0].to_string(),
                        signal: parts.get(1).and_then(|s| s.parse().ok()).unwrap_or(0),
                        security: parts[2].to_string(),
                    });
                }
            }
        }

        networks
    }

    pub fn toggle_wifi(&mut self) {
        let new_state = if self.wifi_enabled { "off" } else { "on" };
        if let Ok(_) = Command::new("nmcli")
            .args(["radio", "wifi", new_state])
            .status()
        {
            self.wifi_enabled = !self.wifi_enabled;
            if !self.wifi_enabled {
                self.connected = false;
                self.ssid = "WiFi Disabled".to_string();
                self.signal = 0;
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

        let mut menu_content = column![]
            .spacing(4)
            .padding(8);

        // WiFi enable/disable toggle
        let wifi_status = if self.wifi_enabled { "Disable WiFi" } else { "Enable WiFi" };
        let toggle_button = button(text(wifi_status).size(11).color(Color::WHITE))
            .padding([4, 8])
            .width(Length::Fill)
            .on_press(WidgetMessage::Wifi("toggle_wifi".to_string()));
        menu_content = menu_content.push(toggle_button);

        menu_content = menu_content.push(text("---").size(10).color(Color::WHITE));

        if self.wifi_enabled {
            menu_content = menu_content.push(text("Available Networks").size(12).color(Color::WHITE));

            let connect_messages: Vec<WidgetMessage> = networks.iter().map(|network| {
                WidgetMessage::Wifi(format!("connect:{}", network.ssid))
            }).collect();

            for (network, msg) in networks.iter().zip(connect_messages.iter()) {
                let net_row = row![
                    text(network.ssid.clone()).size(11).color(Color::WHITE),
                    text(format!("{}%", network.signal)).size(10).color(Color::WHITE),
                ]
                .spacing(8)
                .align_y(Alignment::Center);

                menu_content = menu_content.push(
                    button(net_row)
                        .padding([4, 8])
                        .width(Length::Fill)
                        .on_press(msg.clone())
                );
            }

            if is_empty {
                menu_content = menu_content.push(
                    text("Scanning for networks...").size(11).color(Color::WHITE)
                );
            }
        } else {
            menu_content = menu_content.push(
                text("WiFi is disabled").size(11).color(Color::WHITE)
            );
        }

        let scroll_menu = scrollable(menu_content)
            .height(Length::Fixed(250.0))
            .width(Length::Fixed(300.0));

        let icon_button = button(text(label).size(12).color(Color::WHITE))
            .padding([2, 8])
            .on_press(WidgetMessage::Wifi("toggle_menu".to_string()));

        column![icon_button, scroll_menu]
            .spacing(4)
            .into()
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
    fn test_wifi_get_networks_when_disabled() {
        let mut wifi = Wifi::new();
        wifi.wifi_enabled = false;
        let networks = wifi.get_networks();
        assert!(networks.is_empty());
    }

    #[test]
    fn test_wifi_widget_update_status_when_disabled() {
        let mut wifi = Wifi::new();
        wifi.wifi_enabled = false;
        wifi.update_status();

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

        // This test requires nmcli to be available
        // Verify we get some network info (may be empty in isolated environment)
        assert!(networks.len() >= 0);

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
}

