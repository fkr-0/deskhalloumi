//! Video widget implementation for xrandr presets

use super::{Widget, WidgetMessage};
use iced::widget::{button, column, scrollable, text};
use iced::{Color, Element, Length};
use std::collections::HashMap;
use std::process::Command;

#[derive(Debug)]
pub struct Video {
    show_menu: bool,
    current_mode: String,
    presets: HashMap<String, XrandrPreset>,
}

#[derive(Debug, Clone)]
pub struct XrandrPreset {
    pub name: String,
    pub command: String,
}

impl Video {
    pub fn new() -> Self {
        let mut presets = HashMap::new();

        // Common presets
        presets.insert(
            "internal".to_string(),
            XrandrPreset {
                name: "Internal Only".to_string(),
                command: "xrandr --output eDP-1 --auto --output HDMI-1 --off --output DP-1 --off".to_string(),
            },
        );
        presets.insert(
            "hdmi".to_string(),
            XrandrPreset {
                name: "HDMI Mirror".to_string(),
                command: "xrandr --output eDP-1 --auto --output HDMI-1 --auto --same-as eDP-1".to_string(),
            },
        );
        presets.insert(
            "extend_right".to_string(),
            XrandrPreset {
                name: "Extend Right".to_string(),
                command: "xrandr --output eDP-1 --auto --output HDMI-1 --auto --right-of eDP-1".to_string(),
            },
        );
        presets.insert(
            "extend_left".to_string(),
            XrandrPreset {
                name: "Extend Left".to_string(),
                command: "xrandr --output eDP-1 --auto --output HDMI-1 --auto --left-of eDP-1".to_string(),
            },
        );

        Self {
            show_menu: false,
            current_mode: "internal".to_string(),
            presets,
        }
    }

    pub fn apply_preset(&mut self, preset_key: &str) {
        if let Some(preset) = self.presets.get(preset_key) {
            if let Ok(_) = Command::new("sh")
                .args(["-c", &preset.command])
                .status()
            {
                self.current_mode = preset_key.to_string();
            }
        }
    }

    pub fn get_current_mode_name(&self) -> String {
        self.presets
            .get(&self.current_mode)
            .map(|p| p.name.clone())
            .unwrap_or_else(|| "Unknown".to_string())
    }
}

impl Widget for Video {
    fn name(&self) -> &str {
        "video"
    }

    fn view(&self) -> Element<'_, WidgetMessage> {
        if self.show_menu {
            self.render_menu()
        } else {
            self.render_icon()
        }
    }

    fn update(&mut self, message: WidgetMessage) {
        if let WidgetMessage::Video(action) = message {
            match action.as_str() {
                "toggle_menu" => {
                    self.show_menu = !self.show_menu;
                }
                _ => {
                    // Handle preset selection
                    if action.starts_with("preset:") {
                        let preset = action.strip_prefix("preset:").unwrap();
                        self.apply_preset(preset);
                    }
                }
            }
        }
    }

    fn update_interval(&self) -> Option<u64> {
        None
    }
}

impl Video {
    fn render_icon(&self) -> Element<'_, WidgetMessage> {
        let mode_name = self.get_current_mode_name();
        let label = format!("🖥️ {}", mode_name);

        button(text(label).size(12).color(Color::WHITE))
            .padding([2, 8])
            .on_press(WidgetMessage::Video("toggle_menu".to_string()))
            .into()
    }

    fn render_menu(&self) -> Element<'static, WidgetMessage> {
        let mut menu_content = column![].spacing(4).padding(8);

        menu_content = menu_content.push(
            text("Display Presets").size(12).color(Color::WHITE)
        );

        let mut preset_keys: Vec<_> = self.presets.keys().collect();
        preset_keys.sort();

        for key in preset_keys {
            if let Some(preset) = self.presets.get(key) {
                let is_active = self.current_mode == *key;
                let label = format!(
                    "{} {}",
                    if is_active { "✓" } else { " " },
                    preset.name
                );
                let msg = WidgetMessage::Video(format!("preset:{}", key));
                menu_content = menu_content.push(
                    button(text(label).size(11).color(Color::WHITE))
                        .padding([4, 8])
                        .width(Length::Fill)
                        .on_press(msg)
                );
            }
        }

        let scroll_menu = scrollable(menu_content)
            .height(Length::Fixed(200.0))
            .width(Length::Fixed(250.0));

        let mode_name = self.get_current_mode_name();
        let icon_button = button(text(format!("🖥️ {}", mode_name)).size(12).color(Color::WHITE))
            .padding([2, 8])
            .on_press(WidgetMessage::Video("toggle_menu".to_string()));

        column![icon_button, scroll_menu]
            .spacing(4)
            .into()
    }
}

impl Default for Video {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_video_widget_initialization() {
        let video = Video::new();
        assert_eq!(video.name(), "video");
        assert!(!video.show_menu);
        assert_eq!(video.current_mode, "internal");
        assert!(!video.presets.is_empty());
    }

    #[test]
    fn test_video_widget_default() {
        let video = Video::default();
        assert_eq!(video.name(), "video");
    }

    #[test]
    fn test_video_widget_update_toggle_menu() {
        let mut video = Video::new();
        assert!(!video.show_menu);

        video.update(WidgetMessage::Video("toggle_menu".to_string()));
        assert!(video.show_menu);

        video.update(WidgetMessage::Video("toggle_menu".to_string()));
        assert!(!video.show_menu);
    }

    #[test]
    fn test_video_widget_update_preset() {
        let mut video = Video::new();
        let original_mode = video.current_mode.clone();

        video.update(WidgetMessage::Video("preset:hdmi".to_string()));
        assert_eq!(video.current_mode, "hdmi");
        assert_ne!(video.current_mode, original_mode);
    }

    #[test]
    fn test_video_widget_update_interval() {
        let video = Video::new();
        assert_eq!(video.update_interval(), None);
    }

    #[test]
    fn test_video_widget_render_icon() {
        let video = Video::new();
        let element = video.view();
        drop(element);
    }

    #[test]
    fn test_video_widget_render_menu() {
        let mut video = Video::new();
        video.show_menu = true;
        let element = video.view();
        drop(element);
    }

    #[test]
    fn test_xrandr_preset_creation() {
        let preset = XrandrPreset {
            name: "Test Preset".to_string(),
            command: "xrandr --auto".to_string(),
        };

        assert_eq!(preset.name, "Test Preset");
        assert_eq!(preset.command, "xrandr --auto");
    }

    #[test]
    fn test_get_current_mode_name() {
        let video = Video::new();
        let mode_name = video.get_current_mode_name();
        assert!(mode_name.contains("Internal"));
    }

    #[test]
    fn test_presets_contain_common_modes() {
        let video = Video::new();
        assert!(video.presets.contains_key("internal"));
        assert!(video.presets.contains_key("hdmi"));
        assert!(video.presets.contains_key("extend_right"));
        assert!(video.presets.contains_key("extend_left"));
    }

    #[test]
    fn test_apply_preset_updates_current_mode() {
        let mut video = Video::new();
        video.apply_preset("hdmi");
        assert_eq!(video.current_mode, "hdmi");
    }

    #[test]
    fn test_apply_invalid_preset_no_change() {
        let mut video = Video::new();
        let original_mode = video.current_mode.clone();
        video.apply_preset("invalid_preset");
        assert_eq!(video.current_mode, original_mode);
    }
}
