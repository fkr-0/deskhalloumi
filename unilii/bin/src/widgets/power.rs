//! Power widget with asynchronous session actions.

use std::{ffi::OsString, time::Duration};

use deskhalloumi_core::runtime::{ActionCommand, ActionRunner};
use iced::widget::{button, column, text};
use iced::{Color, Element, Length};

use super::{Widget, WidgetMessage};

#[derive(Debug)]
pub struct Power {
    show_menu: bool,
    screensaver_enabled: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PowerSnapshot {
    pub screensaver_enabled: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PowerAction {
    ToggleScreensaver { enable: bool },
    Standby,
    Reboot,
    Shutdown,
}

impl Power {
    pub fn new() -> Self {
        Self {
            show_menu: false,
            screensaver_enabled: true,
        }
    }

    pub fn menu_is_open(&self) -> bool {
        self.show_menu
    }

    pub fn screensaver_enabled(&self) -> bool {
        self.screensaver_enabled
    }

    pub fn idle_sleep_enabled(&self) -> bool {
        self.screensaver_enabled
    }

    pub fn compact_label(&self) -> &'static str {
        "⏻"
    }

    pub fn apply_snapshot(&mut self, snapshot: PowerSnapshot) {
        self.screensaver_enabled = snapshot.screensaver_enabled;
    }

    pub fn requested_action(action: &str, screensaver_enabled: bool) -> Option<PowerAction> {
        match action {
            "toggle_screensaver" => Some(PowerAction::ToggleScreensaver {
                enable: !screensaver_enabled,
            }),
            "standby" => Some(PowerAction::Standby),
            "reboot" => Some(PowerAction::Reboot),
            "shutdown" => Some(PowerAction::Shutdown),
            _ => None,
        }
    }

    fn render_icon(&self) -> Element<'_, WidgetMessage> {
        button(text("⏻").size(14).color(Color::WHITE))
            .padding([2, 8])
            .on_press(WidgetMessage::Power("toggle_menu".to_string()))
            .into()
    }

    fn render_menu(&self) -> Element<'static, WidgetMessage> {
        let screensaver_text = if self.screensaver_enabled {
            "Disable Screensaver"
        } else {
            "Enable Screensaver"
        };
        let menu = column![
            button(text(screensaver_text).size(11).color(Color::WHITE))
                .padding([4, 8])
                .width(Length::Fill)
                .on_press(WidgetMessage::Power("toggle_screensaver".to_string())),
            button(text("Standby").size(11).color(Color::WHITE))
                .padding([4, 8])
                .width(Length::Fill)
                .on_press(WidgetMessage::Power("standby".to_string())),
            button(text("Reboot").size(11).color(Color::WHITE))
                .padding([4, 8])
                .width(Length::Fill)
                .on_press(WidgetMessage::Power("reboot".to_string())),
            button(text("Shutdown").size(11).color(Color::WHITE))
                .padding([4, 8])
                .width(Length::Fill)
                .on_press(WidgetMessage::Power("shutdown".to_string())),
        ]
        .spacing(4)
        .padding(8)
        .width(Length::Fixed(220.0));
        column![
            button(text("⏻").size(14).color(Color::WHITE))
                .padding([2, 8])
                .on_press(WidgetMessage::Power("toggle_menu".to_string())),
            menu,
        ]
        .spacing(4)
        .into()
    }
}

pub async fn read_power_snapshot(xset: String) -> Result<PowerSnapshot, String> {
    let outcome = ActionRunner::with_timeout("power-widget", "xset-query", Duration::from_secs(4))
        .run_command(ActionCommand::new(xset, vec![OsString::from("q")]))
        .await;
    if let Err(error) = outcome.result {
        return Err(if outcome.stderr.trim().is_empty() {
            error
        } else {
            outcome.stderr.trim().to_string()
        });
    }
    Ok(PowerSnapshot {
        screensaver_enabled: parse_xset_screensaver_enabled(&outcome.stdout).unwrap_or(true),
    })
}

pub async fn execute_power_action(
    xset: String,
    systemctl: String,
    action: PowerAction,
) -> Result<Option<PowerSnapshot>, String> {
    match action {
        PowerAction::ToggleScreensaver { enable } => {
            let command = if enable {
                format!("{} s 600 600 +dpms dpms 0 0 900", shell_quote(&xset))
            } else {
                format!("{} s off -dpms", shell_quote(&xset))
            };
            run_power_command(
                "sh",
                "toggle-screensaver",
                vec![OsString::from("-lc"), OsString::from(command)],
                Duration::from_secs(4),
            )
            .await?;
            Ok(Some(PowerSnapshot {
                screensaver_enabled: enable,
            }))
        }
        PowerAction::Standby => {
            run_power_command(
                &systemctl,
                "suspend",
                vec![OsString::from("suspend")],
                Duration::from_secs(8),
            )
            .await?;
            Ok(None)
        }
        PowerAction::Reboot => {
            run_power_command(
                &systemctl,
                "reboot",
                vec![OsString::from("reboot")],
                Duration::from_secs(8),
            )
            .await?;
            Ok(None)
        }
        PowerAction::Shutdown => {
            run_power_command(
                &systemctl,
                "poweroff",
                vec![OsString::from("poweroff")],
                Duration::from_secs(8),
            )
            .await?;
            Ok(None)
        }
    }
}

async fn run_power_command(
    program: &str,
    action: &str,
    args: Vec<OsString>,
    timeout: Duration,
) -> Result<(), String> {
    let outcome = ActionRunner::with_timeout("power-widget", action, timeout)
        .run_command(ActionCommand::new(program, args))
        .await;
    if let Err(error) = outcome.result {
        Err(if outcome.stderr.trim().is_empty() {
            error
        } else {
            outcome.stderr.trim().to_string()
        })
    } else {
        Ok(())
    }
}

fn parse_xset_screensaver_enabled(output: &str) -> Option<bool> {
    let timeout = output.lines().find_map(|line| {
        let rest = line.trim().strip_prefix("timeout:")?;
        rest.split_whitespace().next()?.parse::<u64>().ok()
    });
    let dpms = output.lines().find_map(|line| {
        let line = line.trim();
        if line.eq_ignore_ascii_case("DPMS is Enabled") {
            Some(true)
        } else if line.eq_ignore_ascii_case("DPMS is Disabled") {
            Some(false)
        } else {
            None
        }
    });
    match (timeout, dpms) {
        (Some(timeout), Some(dpms)) => Some(timeout > 0 || dpms),
        (Some(timeout), None) => Some(timeout > 0),
        (None, Some(dpms)) => Some(dpms),
        (None, None) => None,
    }
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
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
        if let WidgetMessage::Power(action) = message
            && action == "toggle_menu"
        {
            self.show_menu = !self.show_menu;
        }
    }

    fn update_interval(&self) -> Option<u64> {
        Some(30_000)
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
    fn pure_state_and_action_mapping() {
        let mut power = Power::new();
        power.update(WidgetMessage::Power("toggle_menu".to_string()));
        assert!(power.menu_is_open());
        assert_eq!(
            Power::requested_action("toggle_screensaver", true),
            Some(PowerAction::ToggleScreensaver { enable: false })
        );
        assert_eq!(
            Power::requested_action("standby", true),
            Some(PowerAction::Standby)
        );
        assert_eq!(power.update_interval(), Some(30_000));
    }

    #[test]
    fn parses_xset_enabled_and_disabled_states() {
        assert_eq!(
            parse_xset_screensaver_enabled("Screen Saver:\n  timeout:  600"),
            Some(true)
        );
        assert_eq!(
            parse_xset_screensaver_enabled("Screen Saver:\n  timeout:  0"),
            Some(false)
        );
        assert_eq!(parse_xset_screensaver_enabled("partial"), None);
    }

    #[test]
    fn snapshot_updates_state_and_render_paths_are_pure() {
        let mut power = Power::new();
        power.apply_snapshot(PowerSnapshot {
            screensaver_enabled: false,
        });
        assert!(!power.screensaver_enabled());
        drop(power.view());
        power.show_menu = true;
        drop(power.view());
    }
}
