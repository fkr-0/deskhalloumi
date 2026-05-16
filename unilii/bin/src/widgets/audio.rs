//! Audio widget implementation for selecting input/output sources

use super::{Widget, WidgetMessage};
use iced::widget::{button, column, row, scrollable, text};
use iced::{Alignment, Color, Element, Length};
use std::process::Command;

#[derive(Debug)]
pub struct Audio {
    show_menu: bool,
    current_output: String,
    current_input: String,
    output_devices: Vec<AudioDevice>,
    input_devices: Vec<AudioDevice>,
}

#[derive(Debug, Clone)]
pub struct AudioDevice {
    pub name: String,
    pub description: String,
    pub is_active: bool,
}

impl Audio {
    pub fn new() -> Self {
        Self {
            show_menu: false,
            current_output: "Default".to_string(),
            current_input: "Default".to_string(),
            output_devices: Vec::new(),
            input_devices: Vec::new(),
        }
    }

    pub fn update_devices(&mut self) {
        // Get output devices using pactl
        if let Ok(output) = Command::new("pactl").args(["list", "sinks"]).output() {
            let result = String::from_utf8_lossy(&output.stdout);
            self.output_devices = self.parse_audio_devices(&result, "sink");
        }

        // Get input devices using pactl
        if let Ok(output) = Command::new("pactl").args(["list", "sources"]).output() {
            let result = String::from_utf8_lossy(&output.stdout);
            self.input_devices = self.parse_audio_devices(&result, "source");
        }

        // Update current devices
        if let Some(active) = self.output_devices.iter().find(|d| d.is_active) {
            self.current_output = active.name.clone();
        }
        if let Some(active) = self.input_devices.iter().find(|d| d.is_active) {
            self.current_input = active.name.clone();
        }
    }

    fn parse_audio_devices(&self, output: &str, device_type: &str) -> Vec<AudioDevice> {
        let mut devices = Vec::new();
        let mut current_name = String::new();
        let mut current_desc = String::new();
        let mut is_active = false;

        for line in output.lines() {
            let line = line.trim();

            if line.starts_with("Name:") {
                current_name = line.strip_prefix("Name: ").unwrap_or("").to_string();
            } else if line.starts_with("Description:") {
                current_desc = line.strip_prefix("Description: ").unwrap_or("").to_string();
            } else if line.starts_with("State:") && line.contains("RUNNING") {
                is_active = true;
            } else if line.is_empty() && !current_name.is_empty() {
                devices.push(AudioDevice {
                    name: current_name.clone(),
                    description: current_desc.clone(),
                    is_active,
                });
                current_name.clear();
                current_desc.clear();
                is_active = false;
            }
        }

        devices
    }

    pub fn set_default_output(&mut self, device_name: &str) {
        if let Ok(_) = Command::new("pactl")
            .args(["set-default-sink", device_name])
            .status()
        {
            self.current_output = device_name.to_string();
            self.update_devices();
        }
    }

    pub fn set_default_input(&mut self, device_name: &str) {
        if let Ok(_) = Command::new("pactl")
            .args(["set-default-source", device_name])
            .status()
        {
            self.current_input = device_name.to_string();
            self.update_devices();
        }
    }
}

impl Widget for Audio {
    fn name(&self) -> &str {
        "audio"
    }

    fn view(&self) -> Element<'_, WidgetMessage> {
        if self.show_menu {
            self.render_menu()
        } else {
            self.render_icon()
        }
    }

    fn update(&mut self, message: WidgetMessage) {
        if let WidgetMessage::Audio(action) = message {
            match action.as_str() {
                "toggle_menu" => {
                    self.show_menu = !self.show_menu;
                    if self.show_menu {
                        self.update_devices();
                    }
                }
                _ => {
                    // Handle device selection
                    if action.starts_with("set_output:") {
                        let device = action.strip_prefix("set_output:").unwrap();
                        self.set_default_output(device);
                    } else if action.starts_with("set_input:") {
                        let device = action.strip_prefix("set_input:").unwrap();
                        self.set_default_input(device);
                    }
                }
            }
        }
    }

    fn update_interval(&self) -> Option<u64> {
        None
    }
}

impl Audio {
    fn render_icon(&self) -> Element<'_, WidgetMessage> {
        let label = format!("🔊 {}", self.current_output);

        button(text(label).size(12).color(Color::WHITE))
            .padding([2, 8])
            .on_press(WidgetMessage::Audio("toggle_menu".to_string()))
            .into()
    }

    fn render_menu(&self) -> Element<'static, WidgetMessage> {
        let mut menu_content = column![].spacing(4).padding(8);

        // Output devices
        menu_content = menu_content.push(text("Output Devices").size(12).color(Color::WHITE));

        for device in &self.output_devices {
            let label = format!(
                "{} {}",
                if device.is_active { "✓" } else { " " },
                device.description
            );
            let msg = WidgetMessage::Audio(format!("set_output:{}", device.name));
            menu_content = menu_content.push(
                button(text(label).size(11).color(Color::WHITE))
                    .padding([4, 8])
                    .width(Length::Fill)
                    .on_press(msg),
            );
        }

        menu_content = menu_content.push(text("---").size(10).color(Color::WHITE));

        // Input devices
        menu_content = menu_content.push(text("Input Devices").size(12).color(Color::WHITE));

        for device in &self.input_devices {
            let label = format!(
                "{} {}",
                if device.is_active { "✓" } else { " " },
                device.description
            );
            let msg = WidgetMessage::Audio(format!("set_input:{}", device.name));
            menu_content = menu_content.push(
                button(text(label).size(11).color(Color::WHITE))
                    .padding([4, 8])
                    .width(Length::Fill)
                    .on_press(msg),
            );
        }

        let scroll_menu = scrollable(menu_content)
            .height(Length::Fixed(300.0))
            .width(Length::Fixed(300.0));

        let icon_button = button(
            text(format!("🔊 {}", self.current_output))
                .size(12)
                .color(Color::WHITE),
        )
        .padding([2, 8])
        .on_press(WidgetMessage::Audio("toggle_menu".to_string()));

        column![icon_button, scroll_menu].spacing(4).into()
    }
}

impl Default for Audio {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_audio_widget_initialization() {
        let audio = Audio::new();
        assert_eq!(audio.name(), "audio");
        assert!(!audio.show_menu);
        assert_eq!(audio.current_output, "Default");
        assert_eq!(audio.current_input, "Default");
    }

    #[test]
    fn test_audio_widget_default() {
        let audio = Audio::default();
        assert_eq!(audio.name(), "audio");
    }

    #[test]
    fn test_audio_widget_update_toggle_menu() {
        let mut audio = Audio::new();
        assert!(!audio.show_menu);

        audio.update(WidgetMessage::Audio("toggle_menu".to_string()));
        assert!(audio.show_menu);

        audio.update(WidgetMessage::Audio("toggle_menu".to_string()));
        assert!(!audio.show_menu);
    }

    #[test]
    fn test_audio_widget_update_interval() {
        let audio = Audio::new();
        assert_eq!(audio.update_interval(), None);
    }

    #[test]
    fn test_audio_widget_render_icon() {
        let audio = Audio::new();
        let element = audio.view();
        drop(element);
    }

    #[test]
    fn test_audio_widget_render_menu() {
        let mut audio = Audio::new();
        audio.show_menu = true;
        let element = audio.view();
        drop(element);
    }

    #[test]
    fn test_audio_device_creation() {
        let device = AudioDevice {
            name: "alsa_output.pci-0000_00_1f.3.analog-stereo".to_string(),
            description: "Built-in Audio Analog Stereo".to_string(),
            is_active: true,
        };

        assert!(device.name.contains("analog-stereo"));
        assert!(device.description.contains("Analog"));
        assert!(device.is_active);
    }

    #[test]
    fn test_parse_audio_devices_empty_output() {
        let audio = Audio::new();
        let devices = audio.parse_audio_devices("", "sink");
        assert!(devices.is_empty());
    }

    #[test]
    fn test_audio_update_devices_with_mock_data() {
        let mut audio = Audio::new();
        // This would normally parse pactl output, but we test the method exists
        audio.update_devices();
        // Should not panic even without pactl
    }
}
