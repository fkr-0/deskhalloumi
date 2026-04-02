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
}

impl Wifi {
    pub fn new() -> Self {
        Self {
            ssid: "No WiFi".to_string(),
            signal: 0,
            connected: false,
            show_menu: false,
        }
    }

    pub fn update_status(&mut self) {
        // Get current WiFi connection using nmcli
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
    }

    pub fn get_networks(&self) -> Vec<NetworkInfo> {
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

        let scroll_menu = scrollable(menu_content)
            .height(Length::Fixed(200.0))
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
