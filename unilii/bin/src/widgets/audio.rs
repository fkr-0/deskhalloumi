//! Audio widget implementation for selecting input/output sources.

use std::{ffi::OsString, time::Duration};

use deskhalloumi_core::runtime::{
    ActionCommand, ActionRunner, ProviderContract, ProviderRefreshPolicy,
};
use iced::widget::{button, column, scrollable, text};
use iced::{Color, Element, Length};

use super::{Widget, WidgetMessage};

#[derive(Debug)]
pub struct Audio {
    show_menu: bool,
    current_output: String,
    current_input: String,
    output_devices: Vec<AudioDevice>,
    input_devices: Vec<AudioDevice>,
}

pub fn provider_contract() -> ProviderContract {
    ProviderContract::new(
        "audio",
        "Audio",
        ProviderRefreshPolicy {
            interval: Duration::from_secs(15),
            timeout: Duration::from_secs(3),
            stale_after: Duration::from_secs(45),
            refresh_on_start: true,
        },
        "TestProviderBackend<AudioSnapshot>",
    )
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AudioDevice {
    pub name: String,
    pub description: String,
    pub is_active: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AudioSnapshot {
    pub current_output: String,
    pub current_input: String,
    pub output_devices: Vec<AudioDevice>,
    pub input_devices: Vec<AudioDevice>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AudioSelectionAction {
    SetOutput(String),
    SetInput(String),
}

pub fn parse_audio_selection_action(action: &str) -> Option<AudioSelectionAction> {
    if let Some(device) = action.strip_prefix("set_output:") {
        return (!device.is_empty()).then(|| AudioSelectionAction::SetOutput(device.to_string()));
    }
    if let Some(device) = action.strip_prefix("set_input:") {
        return (!device.is_empty()).then(|| AudioSelectionAction::SetInput(device.to_string()));
    }
    None
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

    pub fn menu_is_open(&self) -> bool {
        self.show_menu
    }

    pub fn apply_snapshot(&mut self, snapshot: AudioSnapshot) {
        self.current_output = snapshot.current_output;
        self.current_input = snapshot.current_input;
        self.output_devices = snapshot.output_devices;
        self.input_devices = snapshot.input_devices;
    }

    fn parse_audio_devices(output: &str) -> Vec<AudioDevice> {
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
        if !current_name.is_empty() {
            devices.push(AudioDevice {
                name: current_name,
                description: current_desc,
                is_active,
            });
        }
        devices
    }

    fn render_icon(&self) -> Element<'_, WidgetMessage> {
        button(
            text(format!("🔊 {}", self.current_output))
                .size(12)
                .color(Color::WHITE),
        )
        .padding([2, 8])
        .on_press(WidgetMessage::Audio("toggle_menu".to_string()))
        .into()
    }

    fn render_menu(&self) -> Element<'static, WidgetMessage> {
        let mut menu_content = column![].spacing(4).padding(8);
        menu_content = menu_content.push(text("Output Devices").size(12).color(Color::WHITE));
        for device in &self.output_devices {
            let label = format!(
                "{} {}",
                if device.is_active { "✓" } else { " " },
                device.description
            );
            menu_content = menu_content.push(
                button(text(label).size(11).color(Color::WHITE))
                    .padding([4, 8])
                    .width(Length::Fill)
                    .on_press(WidgetMessage::Audio(format!("set_output:{}", device.name))),
            );
        }
        menu_content = menu_content.push(text("---").size(10).color(Color::WHITE));
        menu_content = menu_content.push(text("Input Devices").size(12).color(Color::WHITE));
        for device in &self.input_devices {
            let label = format!(
                "{} {}",
                if device.is_active { "✓" } else { " " },
                device.description
            );
            menu_content = menu_content.push(
                button(text(label).size(11).color(Color::WHITE))
                    .padding([4, 8])
                    .width(Length::Fill)
                    .on_press(WidgetMessage::Audio(format!("set_input:{}", device.name))),
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

pub async fn read_audio_snapshot(pactl: String) -> Result<AudioSnapshot, String> {
    let sinks = run_pactl(&pactl, "list-sinks", ["list", "sinks"]).await?;
    let sources = run_pactl(&pactl, "list-sources", ["list", "sources"]).await?;
    let output_devices = Audio::parse_audio_devices(&sinks);
    let input_devices = Audio::parse_audio_devices(&sources);
    let current_output = output_devices
        .iter()
        .find(|device| device.is_active)
        .map(|device| device.name.clone())
        .unwrap_or_else(|| "Default".to_string());
    let current_input = input_devices
        .iter()
        .find(|device| device.is_active)
        .map(|device| device.name.clone())
        .unwrap_or_else(|| "Default".to_string());
    Ok(AudioSnapshot {
        current_output,
        current_input,
        output_devices,
        input_devices,
    })
}

pub async fn apply_audio_selection(
    pactl: String,
    selection: AudioSelectionAction,
) -> Result<AudioSnapshot, String> {
    match selection {
        AudioSelectionAction::SetOutput(device) => {
            run_pactl(
                &pactl,
                "set-default-sink",
                ["set-default-sink", device.as_str()],
            )
            .await?;
        }
        AudioSelectionAction::SetInput(device) => {
            run_pactl(
                &pactl,
                "set-default-source",
                ["set-default-source", device.as_str()],
            )
            .await?;
        }
    }
    read_audio_snapshot(pactl).await
}

async fn run_pactl<const N: usize>(
    pactl: &str,
    action: &str,
    args: [&str; N],
) -> Result<String, String> {
    let outcome = ActionRunner::with_timeout("audio", action, Duration::from_secs(5))
        .with_output_limit(2 * 1024 * 1024)
        .run_command(ActionCommand::new(
            pactl,
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
            "pactl output exceeded limit ({} bytes)",
            outcome.stdout_bytes
        ));
    }
    Ok(outcome.stdout)
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
        if let WidgetMessage::Audio(action) = message
            && action == "toggle_menu"
        {
            self.show_menu = !self.show_menu;
        }
    }

    fn update_interval(&self) -> Option<u64> {
        None
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
    fn initializes_and_toggles_without_running_commands() {
        let mut audio = Audio::new();
        assert_eq!(audio.name(), "audio");
        assert!(!audio.menu_is_open());
        audio.update(WidgetMessage::Audio("toggle_menu".to_string()));
        assert!(audio.menu_is_open());
        audio.update(WidgetMessage::Audio("toggle_menu".to_string()));
        assert!(!audio.menu_is_open());
    }

    #[test]
    fn parses_audio_selection_actions() {
        assert_eq!(
            parse_audio_selection_action("set_output:alsa_output"),
            Some(AudioSelectionAction::SetOutput("alsa_output".to_string()))
        );
        assert_eq!(
            parse_audio_selection_action("set_input:alsa_input"),
            Some(AudioSelectionAction::SetInput("alsa_input".to_string()))
        );
        assert_eq!(parse_audio_selection_action("set_output:"), None);
        assert_eq!(parse_audio_selection_action("unknown:device"), None);
    }

    #[test]
    fn parses_audio_devices_and_applies_snapshot() {
        let devices = Audio::parse_audio_devices(
            "Name: sink-a\nDescription: Speakers\nState: RUNNING\n\nName: sink-b\nDescription: HDMI\nState: IDLE\n",
        );
        assert_eq!(devices.len(), 2);
        assert!(devices[0].is_active);
        let mut audio = Audio::new();
        audio.apply_snapshot(AudioSnapshot {
            current_output: "sink-a".to_string(),
            current_input: "source-a".to_string(),
            output_devices: devices,
            input_devices: Vec::new(),
        });
        assert_eq!(audio.current_output, "sink-a");
    }

    #[test]
    fn render_paths_and_interval_are_pure() {
        let mut audio = Audio::new();
        drop(audio.view());
        audio.show_menu = true;
        drop(audio.view());
        assert_eq!(audio.update_interval(), None);
    }

    #[test]
    fn lifecycle_contract_uses_fixture_backend() {
        let contract = provider_contract();
        assert_eq!(contract.id, "audio");
        assert!(contract.test_backend.contains("AudioSnapshot"));
    }
}
