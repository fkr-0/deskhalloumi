//! Global keybinding daemon and parsing utilities.

use crate::Result;
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use tracing::{error, info, warn};

/// Command type for keybinding execution.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum CommandType {
    /// Execute command via shell (default)
    #[default]
    Shell,
    /// Internal bar action (e.g., toggle-module, reload-config)
    Bar,
    /// Tray menu action (e.g., open-menu, close-menu)
    Tray,
    /// Widget/action (future)
    Widget,
}

/// Global keybinding configuration.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct KeyBinding {
    pub name: String,
    pub keysym: String,
    pub command: String,
    #[serde(default = "default_command_type")]
    #[serde(rename = "type")]
    pub command_type: CommandType,
    /// Trigger on key release instead of press (for release-to-confirm)
    #[serde(default)]
    pub release: bool,
}

fn default_command_type() -> CommandType {
    CommandType::Shell
}

#[derive(Debug, Clone)]
struct ParsedBinding {
    binding: KeyBinding,
    /// Every group represents "one required key", where each entry in the
    /// group is accepted as an alternative (e.g. left or right modifier).
    required_groups: Vec<Vec<String>>,
}

/// Result of executing a keybinding.
#[derive(Debug, Clone)]
pub enum KeybindingResult {
    /// Successfully executed shell command
    ShellCommand(String),
    /// Internal bar action that should be handled by the application
    BarAction(String),
    /// Internal tray action that should be handled by the application
    TrayAction(String),
    /// Widget action (future)
    WidgetAction(String),
    /// Unknown or invalid action
    Unknown,
}

/// Keybinding manager using unilii-lib evdev keyboard streams.
pub struct KeybindingDaemon {
    bindings: Vec<ParsedBinding>,
    action_sender: Option<tokio::sync::mpsc::UnboundedSender<KeybindingResult>>,
}

impl KeybindingDaemon {
    pub fn new(bindings: Vec<KeyBinding>) -> Self {
        Self {
            bindings: Self::parse_bindings(bindings),
            action_sender: None,
        }
    }

    /// Set a sender for internal actions (bar/tray actions).
    /// This allows the daemon to communicate internal actions back to the application.
    pub fn set_action_sender(&mut self, sender: tokio::sync::mpsc::UnboundedSender<KeybindingResult>) {
        self.action_sender = Some(sender);
    }

    fn parse_bindings(bindings: Vec<KeyBinding>) -> Vec<ParsedBinding> {
        bindings
            .into_iter()
            .filter_map(|binding| match parse_binding(binding.clone()) {
                Ok(parsed) => Some(parsed),
                Err(message) => {
                    warn!(
                        "skipping invalid keybinding '{}' (keysym='{}'): {}",
                        binding.name, binding.keysym, message
                    );
                    None
                }
            })
            .collect()
    }

    pub async fn run(&self) -> Result<()> {
        let listener = match unilii_lib::input::listen_keyboard_events_experimental() {
            Ok(stream) => {
                info!("hotkeys: listener initialized using experimental tokio-udev path");
                Ok(stream)
            }
            Err(error) => {
                warn!(
                    "hotkeys: experimental listener unavailable, falling back to base evdev: {}",
                    error
                );
                unilii_lib::input::listen_keyboard_events()
            }
        }?;

        let mut stream = listener;
        let mut pressed_keys: HashSet<String> = HashSet::new();
        let mut already_triggered: HashSet<usize> = HashSet::new();
        // Track bindings that are ready for release-to-confirm
        let mut release_ready: HashSet<usize> = HashSet::new();

        while let Some(event) = stream.next().await {
            let key_name = format!("{:?}", event.code);

            match event.value {
                1 => {
                    // Key press
                    pressed_keys.insert(key_name);
                    self.check_bindings(&pressed_keys, &mut already_triggered, &mut release_ready)?;
                }
                0 => {
                    // Key release
                    pressed_keys.remove(&key_name);
                    // Execute release bindings and clear their ready state
                    self.execute_release_bindings(&mut release_ready)?;
                    already_triggered.retain(|index| self.matches_binding(*index, &pressed_keys));
                    release_ready.retain(|index| self.matches_binding(*index, &pressed_keys));
                }
                _ => {}
            }
        }

        Ok(())
    }

    fn check_bindings(
        &self,
        pressed: &HashSet<String>,
        already_triggered: &mut HashSet<usize>,
        release_ready: &mut HashSet<usize>,
    ) -> Result<()> {
        for index in 0..self.bindings.len() {
            let matches = self.matches_binding(index, pressed);
            let binding = &self.bindings[index].binding;

            if binding.release {
                // Release-to-confirm: mark as ready if not already triggered
                if matches && !release_ready.contains(&index) {
                    release_ready.insert(index);
                }
            } else {
                // Normal: execute immediately if not already triggered
                if matches && !already_triggered.contains(&index) {
                    self.execute_binding(index)?;
                    already_triggered.insert(index);
                }
            }
        }
        Ok(())
    }

    fn execute_release_bindings(&self, release_ready: &mut HashSet<usize>) -> Result<()> {
        let bindings_to_execute: Vec<usize> = release_ready.iter().copied().collect();
        for index in bindings_to_execute {
            self.execute_binding(index)?;
            release_ready.remove(&index);
        }
        Ok(())
    }

    fn matches_binding(&self, index: usize, pressed: &HashSet<String>) -> bool {
        self.bindings[index]
            .required_groups
            .iter()
            .all(|alternatives| alternatives.iter().any(|key| pressed.contains(key)))
    }

    fn execute_binding(&self, index: usize) -> Result<()> {
        let binding = &self.bindings[index].binding;
        let command = binding.command.trim();
        if command.is_empty() {
            return Ok(());
        }

        match &binding.command_type {
            CommandType::Shell => {
                std::process::Command::new("sh")
                    .arg("-c")
                    .arg(command)
                    .spawn()
                    .map_err(|error| {
                        error!(
                            "hotkeys: failed to execute binding '{}' command='{}': {}",
                            binding.name, command, error
                        );
                        Box::new(error) as Box<dyn std::error::Error + Send + Sync>
                    })?;

                info!(
                    "hotkeys: executed shell binding '{}' keysym='{}'",
                    binding.name, binding.keysym
                );
            }
            CommandType::Bar | CommandType::Tray | CommandType::Widget => {
                let result = match &binding.command_type {
                    CommandType::Bar => KeybindingResult::BarAction(command.to_string()),
                    CommandType::Tray => KeybindingResult::TrayAction(command.to_string()),
                    CommandType::Widget => KeybindingResult::WidgetAction(command.to_string()),
                    _ => KeybindingResult::Unknown,
                };

                if let Some(sender) = &self.action_sender {
                    if sender.send(result).is_err() {
                        error!("hotkeys: failed to send internal action '{}'", command);
                    }
                }

                info!(
                    "hotkeys: executed internal binding '{}' type={:?} command='{}'",
                    binding.name, binding.command_type, command
                );
            }
        }

        Ok(())
    }
}

fn parse_binding(binding: KeyBinding) -> std::result::Result<ParsedBinding, String> {
    let groups = parse_keysym(&binding.keysym)?;
    if groups.is_empty() {
        return Err("no keys parsed".to_string());
    }

    Ok(ParsedBinding {
        binding,
        required_groups: groups,
    })
}

fn parse_keysym(keysym: &str) -> std::result::Result<Vec<Vec<String>>, String> {
    let mut groups = Vec::new();
    let mut seen = HashMap::<String, usize>::new();

    for token in keysym.split('+') {
        let alternatives = token_to_key_candidates(token)?;
        if alternatives.is_empty() {
            continue;
        }

        // De-duplicate logical keys while preserving order.
        let canonical = alternatives.join("|");
        if seen.contains_key(&canonical) {
            continue;
        }
        seen.insert(canonical, groups.len());
        groups.push(alternatives);
    }

    Ok(groups)
}

fn token_to_key_candidates(token: &str) -> std::result::Result<Vec<String>, String> {
    let raw = token.trim().to_ascii_uppercase().replace('-', "_");
    if raw.starts_with("KEY_") {
        return Ok(vec![raw]);
    }

    let normalized = normalize_token(token);
    if normalized.is_empty() {
        return Err("empty token".to_string());
    }

    let candidates = match normalized.as_str() {
        "SHIFT" => vec!["KEY_LEFTSHIFT".to_string(), "KEY_RIGHTSHIFT".to_string()],
        "CTRL" | "CONTROL" => vec!["KEY_LEFTCTRL".to_string(), "KEY_RIGHTCTRL".to_string()],
        "ALT" => vec!["KEY_LEFTALT".to_string(), "KEY_RIGHTALT".to_string()],
        "SUPER" | "META" | "WIN" | "WINDOWS" => {
            vec!["KEY_LEFTMETA".to_string(), "KEY_RIGHTMETA".to_string()]
        }
        "RETURN" | "ENTER" => vec!["KEY_ENTER".to_string()],
        "ESC" | "ESCAPE" => vec!["KEY_ESC".to_string()],
        "SPACE" => vec!["KEY_SPACE".to_string()],
        "TAB" => vec!["KEY_TAB".to_string()],
        "BACKSPACE" => vec!["KEY_BACKSPACE".to_string()],
        "DELETE" | "DEL" => vec!["KEY_DELETE".to_string()],
        "HOME" => vec!["KEY_HOME".to_string()],
        "END" => vec!["KEY_END".to_string()],
        "PAGEUP" => vec!["KEY_PAGEUP".to_string()],
        "PAGEDOWN" => vec!["KEY_PAGEDOWN".to_string()],
        "UP" => vec!["KEY_UP".to_string()],
        "DOWN" => vec!["KEY_DOWN".to_string()],
        "LEFT" => vec!["KEY_LEFT".to_string()],
        "RIGHT" => vec!["KEY_RIGHT".to_string()],
        _ if normalized.len() == 1 && normalized.chars().all(|c| c.is_ascii_alphabetic()) => {
            vec![format!("KEY_{}", normalized)]
        }
        _ if normalized.len() == 1 && normalized.chars().all(|c| c.is_ascii_digit()) => {
            vec![format!("KEY_{}", normalized)]
        }
        _ if normalized.starts_with('F')
            && normalized.len() <= 3
            && normalized[1..].chars().all(|c| c.is_ascii_digit()) =>
        {
            vec![format!("KEY_{}", normalized)]
        }
        _ => return Err(format!("unsupported key token '{}'", token.trim())),
    };

    Ok(candidates)
}

fn normalize_token(token: &str) -> String {
    token
        .chars()
        .filter(|c| !matches!(c, ' ' | '-' | '_'))
        .flat_map(|c| c.to_uppercase())
        .collect::<String>()
}

#[cfg(test)]
mod tests {
    use super::{parse_keysym, token_to_key_candidates};

    #[test]
    fn parses_modifiers_with_left_right_variants() {
        let parsed = parse_keysym("Super+Shift+q").expect("keysym should parse");
        assert_eq!(parsed.len(), 3);
        assert_eq!(parsed[0], vec!["KEY_LEFTMETA", "KEY_RIGHTMETA"]);
        assert_eq!(parsed[1], vec!["KEY_LEFTSHIFT", "KEY_RIGHTSHIFT"]);
        assert_eq!(parsed[2], vec!["KEY_Q"]);
    }

    #[test]
    fn parses_key_prefixed_tokens_without_changes() {
        let parsed = token_to_key_candidates("KEY_ENTER").expect("token should parse");
        assert_eq!(parsed, vec!["KEY_ENTER"]);
    }

    #[test]
    fn rejects_unknown_tokens() {
        let err = token_to_key_candidates("HyperMega").expect_err("token should fail");
        assert!(err.contains("unsupported key token"));
    }
}
