//! Video widget implementation for xrandr display management.

use super::{Widget, WidgetMessage};
use iced::widget::{button, column, container, row, scrollable, text};
use iced::{Alignment, Color, Element, Length};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const XRANDR_PRESETS_ENV: &str = "DESKHALLOUMI_XRANDR_PRESETS_YAML";
const LEGACY_XRANDR_PRESETS_ENV: &str = "UNILII_XRANDR_PRESETS_YAML";
const DEFAULT_PRESET_LOCATIONS: &[&str] = &[
    ".config/deskhalloumi/xrandr-presets.yml",
    ".config/deskhalloumi/xrandr-presets.yaml",
    ".config/unilii/xrandr-presets.yml",
    ".config/unilii/xrandr-presets.yaml",
];

#[derive(Debug)]
pub struct Video {
    show_menu: bool,
    current_preset: Option<String>,
    displays: Vec<DisplayInfo>,
    presets: BTreeMap<String, XrandrPreset>,
    preset_source: Option<PathBuf>,
    last_status: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DisplayInfo {
    pub name: String,
    pub connected: bool,
    pub primary: bool,
    pub mode: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct XrandrPreset {
    pub key: String,
    pub name: String,
    pub description: Option<String>,
    pub command: XrandrPresetCommand,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum XrandrPresetCommand {
    Shell(String),
    Args(Vec<String>),
}

#[derive(Debug, Deserialize)]
struct XrandrPresetFile {
    presets: Vec<XrandrPresetYaml>,
}

#[derive(Debug, Deserialize)]
struct XrandrPresetYaml {
    key: String,
    name: Option<String>,
    description: Option<String>,
    command: Option<String>,
    args: Option<Vec<String>>,
}

impl TryFrom<XrandrPresetYaml> for XrandrPreset {
    type Error = String;

    fn try_from(value: XrandrPresetYaml) -> Result<Self, Self::Error> {
        let command = match (value.command, value.args) {
            (Some(command), None) => XrandrPresetCommand::Shell(command),
            (None, Some(args)) if !args.is_empty() => XrandrPresetCommand::Args(args),
            (Some(_), Some(_)) => {
                return Err(format!(
                    "preset '{}' must define either 'command' or 'args', not both",
                    value.key
                ));
            }
            (None, None) => {
                return Err(format!(
                    "preset '{}' must define either 'command' or 'args'",
                    value.key
                ));
            }
            (None, Some(_)) => {
                return Err(format!(
                    "preset '{}' args list must not be empty",
                    value.key
                ));
            }
        };

        Ok(Self {
            name: value.name.clone().unwrap_or_else(|| value.key.clone()),
            key: value.key,
            description: value.description,
            command,
        })
    }
}

impl Video {
    pub fn new() -> Self {
        Self::with_preset_source(detect_default_preset_source())
    }

    pub fn with_preset_source(preset_source: Option<PathBuf>) -> Self {
        let preset_source = preset_source.map(expand_home_path);
        let mut video = Self {
            show_menu: false,
            current_preset: None,
            displays: Vec::new(),
            presets: BTreeMap::new(),
            preset_source,
            last_status: "Ready".to_string(),
        };
        video.refresh_state();
        video
    }

    pub fn refresh_state(&mut self) {
        match detect_displays() {
            Ok(displays) => {
                self.displays = displays;
                self.last_status =
                    format!("Detected {} display(s)", self.connected_display_count());
            }
            Err(err) => {
                self.displays.clear();
                self.last_status = err;
            }
        }

        match self.load_presets() {
            Ok(presets) => {
                self.presets = presets;
                if self.presets.is_empty() {
                    self.last_status = match &self.preset_source {
                        Some(path) => format!("No presets found in {}", path.display()),
                        None => "No preset file configured".to_string(),
                    };
                }
            }
            Err(err) => {
                self.presets.clear();
                self.last_status = err;
            }
        }
    }

    fn connected_display_count(&self) -> usize {
        self.displays
            .iter()
            .filter(|display| display.connected)
            .count()
    }

    fn display_summary(&self) -> String {
        let connected: Vec<_> = self
            .displays
            .iter()
            .filter(|display| display.connected)
            .map(|display| display.name.as_str())
            .collect();

        match connected.as_slice() {
            [] => "No displays".to_string(),
            [single] => (*single).to_string(),
            _ => format!("{} displays", connected.len()),
        }
    }

    fn preset_source_label(&self) -> String {
        self.preset_source
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| format!("set {} to a preset YAML file", XRANDR_PRESETS_ENV))
    }

    fn load_presets(&self) -> Result<BTreeMap<String, XrandrPreset>, String> {
        let Some(path) = &self.preset_source else {
            return Ok(BTreeMap::new());
        };

        let contents = fs::read_to_string(path)
            .map_err(|err| format!("Failed to read presets {}: {}", path.display(), err))?;
        let parsed: XrandrPresetFile = serde_yaml::from_str(&contents)
            .map_err(|err| format!("Failed to parse presets {}: {}", path.display(), err))?;

        let mut presets = BTreeMap::new();
        for preset in parsed.presets {
            let preset = XrandrPreset::try_from(preset)?;
            presets.insert(preset.key.clone(), preset);
        }

        Ok(presets)
    }

    pub fn compact_label(&self) -> String {
        format!("🖥 {}", self.display_summary())
    }

    pub fn displays(&self) -> &[DisplayInfo] {
        &self.displays
    }

    pub fn last_status(&self) -> &str {
        &self.last_status
    }

    pub fn preset_entries(&self) -> Vec<(String, String, Option<String>, String)> {
        self.presets
            .values()
            .map(|preset| {
                (
                    preset.key.clone(),
                    preset.name.clone(),
                    preset.description.clone(),
                    preset_command_as_shell(&preset.command),
                )
            })
            .collect()
    }

    pub fn apply_preset(&mut self, preset_key: &str) {
        let Some(preset) = self.presets.get(preset_key).cloned() else {
            self.last_status = format!("Unknown preset: {}", preset_key);
            return;
        };

        let status = match preset.command {
            XrandrPresetCommand::Shell(command) => {
                Command::new("sh").args(["-c", &command]).status()
            }
            XrandrPresetCommand::Args(args) => Command::new("xrandr").args(args).status(),
        };

        match status {
            Ok(exit) if exit.success() => {
                self.current_preset = Some(preset.key.clone());
                self.last_status = format!("Applied preset: {}", preset.name);
                if let Ok(displays) = detect_displays() {
                    self.displays = displays;
                }
            }
            Ok(exit) => {
                self.last_status = format!("Preset '{}' failed with {}", preset.name, exit);
            }
            Err(err) => {
                self.last_status = format!("Failed to execute '{}': {}", preset.name, err);
            }
        }
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
                    if self.show_menu {
                        self.refresh_state();
                    }
                }
                "refresh" => self.refresh_state(),
                _ if action.starts_with("preset:") => {
                    let preset = action.trim_start_matches("preset:");
                    self.apply_preset(preset);
                }
                _ => {}
            }
        }
    }

    fn update_interval(&self) -> Option<u64> {
        Some(15_000)
    }
}

impl Video {
    fn render_icon(&self) -> Element<'_, WidgetMessage> {
        let label = format!("🖥 {}", self.display_summary());

        button(text(label).size(12).color(Color::WHITE))
            .padding([2, 8])
            .on_press(WidgetMessage::Video("toggle_menu".to_string()))
            .into()
    }

    fn render_menu(&self) -> Element<'static, WidgetMessage> {
        let icon_button = button(
            text(format!("🖥 {}", self.display_summary()))
                .size(12)
                .color(Color::WHITE),
        )
        .padding([2, 8])
        .on_press(WidgetMessage::Video("toggle_menu".to_string()));

        let mut content = column![
            row![
                text("Displays").size(12).color(Color::WHITE),
                container(
                    button(text("↻").size(12).color(Color::WHITE))
                        .padding([2, 6])
                        .on_press(WidgetMessage::Video("refresh".to_string()))
                )
                .width(Length::Fill)
                .align_x(iced::alignment::Horizontal::Right)
            ]
            .align_y(Alignment::Center)
        ]
        .spacing(6)
        .padding(8);

        if self.displays.is_empty() {
            content = content.push(
                text("No display data available")
                    .size(11)
                    .color(Color::from_rgb8(210, 210, 210)),
            );
        } else {
            for display in &self.displays {
                let state = if display.connected {
                    "connected"
                } else {
                    "disconnected"
                };
                let primary = if display.primary { " • primary" } else { "" };
                let mode = display
                    .mode
                    .as_ref()
                    .map(|mode| format!(" • {}", mode))
                    .unwrap_or_default();
                let line = format!("{} — {}{}{}", display.name, state, primary, mode);
                content = content.push(text(line).size(11).color(Color::from_rgb8(224, 224, 224)));
            }
        }

        content = content
            .push(text("Presets").size(12).color(Color::WHITE))
            .push(
                text(self.preset_source_label())
                    .size(10)
                    .color(Color::from_rgb8(180, 180, 180)),
            );

        if self.presets.is_empty() {
            content = content.push(
                text("No xrandr presets loaded")
                    .size(11)
                    .color(Color::from_rgb8(210, 210, 210)),
            );
        } else {
            for (key, preset) in &self.presets {
                let active = self.current_preset.as_deref() == Some(key.as_str());
                let label = if active {
                    format!("✓ {}", preset.name)
                } else {
                    preset.name.clone()
                };
                let mut button_col = column![text(label).size(11).color(Color::WHITE)].spacing(2);
                if let Some(description) = &preset.description {
                    button_col = button_col.push(
                        text(description.clone())
                            .size(10)
                            .color(Color::from_rgb8(180, 180, 180)),
                    );
                }
                content = content.push(
                    button(button_col)
                        .padding([4, 8])
                        .width(Length::Fill)
                        .on_press(WidgetMessage::Video(format!("preset:{}", key))),
                );
            }
        }

        content = content.push(
            text(self.last_status.clone())
                .size(10)
                .color(Color::from_rgb8(180, 220, 180)),
        );

        let scroll_menu = scrollable(content)
            .height(Length::Fixed(220.0))
            .width(Length::Fixed(320.0));

        column![icon_button, scroll_menu].spacing(4).into()
    }
}

impl Default for Video {
    fn default() -> Self {
        Self::new()
    }
}

fn detect_default_preset_source() -> Option<PathBuf> {
    if let Ok(explicit) =
        env::var(XRANDR_PRESETS_ENV).or_else(|_| env::var(LEGACY_XRANDR_PRESETS_ENV))
    {
        if !explicit.trim().is_empty() {
            return Some(PathBuf::from(explicit));
        }
    }

    let home = env::var("HOME").ok()?;
    DEFAULT_PRESET_LOCATIONS
        .iter()
        .map(|relative| Path::new(&home).join(relative))
        .find(|path| path.exists())
}

fn expand_home_path(path: PathBuf) -> PathBuf {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    expand_home_path_with(path, home.as_deref())
}

fn expand_home_path_with(path: PathBuf, home: Option<&Path>) -> PathBuf {
    if path == Path::new("~") {
        return home.map(Path::to_path_buf).unwrap_or(path);
    }
    if let Ok(rest) = path.strip_prefix("~")
        && let Some(home) = home
    {
        return home.join(rest);
    }
    path
}

fn preset_command_as_shell(command: &XrandrPresetCommand) -> String {
    match command {
        XrandrPresetCommand::Shell(command) => command.clone(),
        XrandrPresetCommand::Args(args) => format!(
            "xrandr {}",
            args.iter()
                .map(|arg| shell_quote(arg))
                .collect::<Vec<_>>()
                .join(" ")
        ),
    }
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn detect_displays() -> Result<Vec<DisplayInfo>, String> {
    let output = Command::new("xrandr")
        .arg("--query")
        .output()
        .map_err(|err| format!("Failed to execute xrandr: {}", err))?;

    if !output.status.success() {
        return Err(format!("xrandr --query failed with {}", output.status));
    }

    let stdout = String::from_utf8(output.stdout)
        .map_err(|err| format!("xrandr output was not valid UTF-8: {}", err))?;

    Ok(parse_xrandr_output(&stdout))
}

fn parse_xrandr_output(output: &str) -> Vec<DisplayInfo> {
    output.lines().filter_map(parse_xrandr_line).collect()
}

fn parse_xrandr_line(line: &str) -> Option<DisplayInfo> {
    let trimmed = line.trim();
    if !(trimmed.contains(" connected") || trimmed.contains(" disconnected")) {
        return None;
    }

    let mut parts = trimmed.split_whitespace();
    let name = parts.next()?.to_string();
    let connected_token = parts.next()?;
    let connected = connected_token == "connected";
    let primary = trimmed.contains(" primary ");
    let mode = trimmed.split_whitespace().find_map(|token| {
        let head = token.split('+').next().unwrap_or(token);
        if head.contains('x') && head.chars().any(|ch| ch.is_ascii_digit()) {
            Some(head.to_string())
        } else {
            None
        }
    });

    Some(DisplayInfo {
        name,
        connected,
        primary,
        mode,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_xrandr_output() {
        let sample = "Screen 0: minimum 8 x 8, current 4480 x 1440, maximum 32767 x 32767\neDP-1 connected primary 1920x1080+0+360 (normal left inverted right x axis y axis) 309mm x 174mm\nHDMI-1 connected 2560x1440+1920+0 (normal left inverted right x axis y axis) 698mm x 392mm\nDP-1 disconnected (normal left inverted right x axis y axis)\n";

        let displays = parse_xrandr_output(sample);
        assert_eq!(displays.len(), 3);
        assert_eq!(displays[0].name, "eDP-1");
        assert!(displays[0].connected);
        assert!(displays[0].primary);
        assert_eq!(displays[0].mode.as_deref(), Some("1920x1080"));
        assert_eq!(displays[2].name, "DP-1");
        assert!(!displays[2].connected);
    }

    #[test]
    fn test_yaml_presets_load() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("xrandr.yml");
        fs::write(
            &path,
            "presets:\n  - key: docked\n    name: Docked\n    description: External display only\n    args:\n      - --output\n      - eDP-1\n      - --off\n      - --output\n      - HDMI-1\n      - --auto\n  - key: mirror\n    command: xrandr --output eDP-1 --auto --output HDMI-1 --auto --same-as eDP-1\n",
        )
        .unwrap();

        let video = Video::with_preset_source(Some(path));
        assert!(video.presets.contains_key("docked"));
        assert!(video.presets.contains_key("mirror"));
        assert_eq!(
            video.presets["docked"].description.as_deref(),
            Some("External display only")
        );
    }

    #[test]
    fn test_video_widget_initialization() {
        let video = Video::with_preset_source(None);
        assert_eq!(video.name(), "video");
        assert!(!video.show_menu);
    }

    #[test]
    fn test_video_widget_update_toggle_menu() {
        let mut video = Video::with_preset_source(None);
        assert!(!video.show_menu);
        video.update(WidgetMessage::Video("toggle_menu".to_string()));
        assert!(video.show_menu);
    }

    #[test]
    fn test_invalid_preset_definition_rejected() {
        let invalid = XrandrPresetYaml {
            key: "broken".to_string(),
            name: None,
            description: None,
            command: Some("xrandr --auto".to_string()),
            args: Some(vec!["--auto".to_string()]),
        };
        assert!(XrandrPreset::try_from(invalid).is_err());
    }

    #[test]
    fn preset_source_expands_home_prefix() {
        assert_eq!(
            expand_home_path_with(
                PathBuf::from("~/.config/unilii/presets.yml"),
                Some(Path::new("/tmp/unilii-home")),
            ),
            PathBuf::from("/tmp/unilii-home/.config/unilii/presets.yml")
        );
        assert_eq!(
            expand_home_path_with(PathBuf::from("/absolute/presets.yml"), None),
            PathBuf::from("/absolute/presets.yml")
        );
    }
}
