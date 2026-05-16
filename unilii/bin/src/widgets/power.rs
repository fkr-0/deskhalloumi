//! Power widget implementation for system controls

use super::{Widget, WidgetMessage};
use iced::widget::{button, column, text};
use iced::{Color, Element, Length};
use std::process::Command;

#[derive(Debug)]
pub struct Power {
    show_menu: bool,
    screensaver_enabled: bool,
}

impl Power {
    pub fn new() -> Self {
        Self {
            show_menu: false,
            screensaver_enabled: true,
        }
    }

    pub fn update_screensaver_status(&mut self) {
        // Check if screensaver is enabled using xset
        if let Ok(output) = Command::new("xset").args(["q"]).output() {
            let result = String::from_utf8_lossy(&output.stdout);
            self.screensaver_enabled = result.contains("Screen Saver: enable");
        }
    }

    pub fn toggle_screensaver(&mut self) {
        let new_state = if self.screensaver_enabled {
            "off"
        } else {
            "on"
        };
        if Command::new("xset").args(["s", new_state]).status().is_ok() {
            self.screensaver_enabled = !self.screensaver_enabled;
        }
    }

    pub fn standby(&self) {
        let _ = Command::new("sysstemctl").args(["suspend"]).status();
    }

    pub fn reboot(&self) {
        let _ = Command::new("sysstemctl").args(["reboot"]).status();
    }

    pub fn shutdown(&self) {
        let _ = Command::new("sysstemctl").args(["poweroff"]).status();
    }
}

impl Widget for Power {
    fn name(&self) -> &str {
        "power"
    }

    fn view(&self) -> Element<'_, WidgetMessage> {
        if self.show_menu {
            self.render_menu()
        } else {
            self.render_icon()
        }
    }

    fn update(&mut self, message: WidgetMessage) {
        if let WidgetMessage::Power(action) = message {
            match action.as_str() {
                "toggle_menu" => {
                    self.show_menu = !self.show_menu;
                    if self.show_menu {
                        self.update_screensaver_status();
                    }
                }
                "toggle_screensaver" => {
                    self.toggle_screensaver();
                }
                "standby" => {
                    self.standby();
                    self.show_menu = false;
                }
                "reboot" => {
                    self.reboot();
                    self.show_menu = false;
                }
                "shutdown" => {
                    self.shutdown();
                    self.show_menu = false;
                }
                _ => {}
            }
        }
    }

    fn update_interval(&self) -> Option<u64> {
        None
    }
}

impl Power {
    fn render_icon(&self) -> Element<'_, WidgetMessage> {
        let icon = "⏻";
        let label = icon.to_string();

        button(text(label).size(14).color(Color::WHITE))
            .padding([2, 8])
            .on_press(WidgetMessage::Power("toggle_menu".to_string()))
            .into()
    }

    fn render_menu(&self) -> Element<'static, WidgetMessage> {
        let mut menu_content = column![].spacing(4).padding(8);

        // Screensaver toggle
        let screensaver_text = if self.screensaver_enabled {
            "Disable Screensaver"
        } else {
            "Enable Screensaver"
        };
        menu_content = menu_content.push(
            button(text(screensaver_text).size(11).color(Color::WHITE))
                .padding([4, 8])
                .width(Length::Fill)
                .on_press(WidgetMessage::Power("toggle_screensaver".to_string())),
        );

        menu_content = menu_content.push(text("---").size(10).color(Color::WHITE));

        // System controls
        menu_content = menu_content.push(
            button(text("Standby").size(11).color(Color::WHITE))
                .padding([4, 8])
                .width(Length::Fill)
                .on_press(WidgetMessage::Power("standby".to_string())),
        );

        menu_content = menu_content.push(
            button(text("Reboot").size(11).color(Color::WHITE))
                .padding([4, 8])
                .width(Length::Fill)
                .on_press(WidgetMessage::Power("reboot".to_string())),
        );

        menu_content = menu_content.push(
            button(text("Shutdown").size(11).color(Color::WHITE))
                .padding([4, 8])
                .width(Length::Fill)
                .on_press(WidgetMessage::Power("shutdown".to_string())),
        );

        let icon_button = button(text("⏻").size(14).color(Color::WHITE))
            .padding([2, 8])
            .on_press(WidgetMessage::Power("toggle_menu".to_string()));

        column![icon_button, menu_content].spacing(4).into()
    }
}

impl Default for Power {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_power_widget_initialization() {
        let power = Power::new();
        assert_eq!(power.name(), "power");
        assert!(!power.show_menu);
        assert!(power.screensaver_enabled);
    }

    #[test]
    fn test_power_widget_default() {
        let power = Power::default();
        assert_eq!(power.name(), "power");
        assert!(power.screensaver_enabled);
    }

    #[test]
    fn test_power_widget_update_toggle_menu() {
        let mut power = Power::new();
        assert!(!power.show_menu);

        power.update(WidgetMessage::Power("toggle_menu".to_string()));
        assert!(power.show_menu);

        power.update(WidgetMessage::Power("toggle_menu".to_string()));
        assert!(!power.show_menu);
    }

    #[test]
    fn test_power_widget_update_screensaver() {
        let mut power = Power::new();
        power.update(WidgetMessage::Power("toggle_screensaver".to_string()));
        // Should attempt to toggle (actual system call may fail in tests)
    }

    #[test]
    fn test_power_widget_update_closes_menu_on_action() {
        let mut power = Power::new();
        power.show_menu = true;

        power.update(WidgetMessage::Power("standby".to_string()));
        assert!(!power.show_menu);

        power.show_menu = true;
        power.update(WidgetMessage::Power("reboot".to_string()));
        assert!(!power.show_menu);

        power.show_menu = true;
        power.update(WidgetMessage::Power("shutdown".to_string()));
        assert!(!power.show_menu);
    }

    #[test]
    fn test_power_widget_update_interval() {
        let power = Power::new();
        assert_eq!(power.update_interval(), None);
    }

    #[test]
    fn test_power_widget_render_icon() {
        let power = Power::new();
        let element = power.view();
        drop(element);
    }

    #[test]
    fn test_power_widget_render_menu() {
        let mut power = Power::new();
        power.show_menu = true;
        let element = power.view();
        drop(element);
    }

    #[test]
    fn test_power_toggle_screensaver() {
        let mut power = Power::new();
        power.toggle_screensaver();
        // Should attempt to toggle (actual system call may fail in tests)
    }

    #[test]
    fn test_power_standalone_commands_exist() {
        let power = Power::new();
        // These methods should exist and not panic
        power.standby();
        power.reboot();
        power.shutdown();
    }

    #[test]
    fn test_power_widget_invalid_action_no_panic() {
        let mut power = Power::new();
        let original_state = power.screensaver_enabled;

        power.update(WidgetMessage::Power("invalid_action".to_string()));
        // Should not panic and state should remain unchanged
        assert_eq!(power.screensaver_enabled, original_state);
    }
}
