//! Global keybinding daemon and parsing utilities.

use crate::Result;
use crate::key_engine::{EngineBinding, KeyEngine, KeyEngineTraceReason, KeyTrigger};
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Instant;
use tracing::{debug, error, info, warn};

/// Command type for keybinding execution.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum CommandType {
    /// Execute command via shell (default)
    #[default]
    Shell,
    /// Internal bar action (e.g., toggle-module, reload-config)
    Bar,
    /// Internal tray menu action (e.g., open-menu, close-menu)
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
    /// Trigger mode for key execution semantics.
    #[serde(default)]
    pub trigger: KeyTrigger,
    /// Optional hold threshold (milliseconds) for mod-release semantics.
    #[serde(default)]
    pub hold_ms: Option<u64>,
    /// Optional cooldown between successful triggers.
    #[serde(default)]
    pub cooldown_ms: Option<u64>,
    /// Conflict resolution priority (higher wins).
    #[serde(default)]
    pub priority: u16,
    /// If true, suppress lower-priority matches in the same event.
    #[serde(default)]
    pub consume: bool,
}

fn default_command_type() -> CommandType {
    CommandType::Shell
}

#[derive(Debug, Clone)]
struct ParsedBinding {
    binding: KeyBinding,
    engine_binding: EngineBinding,
}

/// Structured tray actions that the bar can interpret directly.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TrayDaemonAction {
    OpenMenu,
    CloseMenu,
    ToggleMenu,
    ShowAggregated,
    ShowFavorites,
    FocusNext,
    FocusPrevious,
    ActivateSelected,
    OpenIndex(usize),
    RefreshStatus,
    Raw(String),
}

/// Structured bar actions that the bar can interpret directly.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BarDaemonAction {
    ReloadConfig,
    ToggleModule(String),
    FocusModule(String),
    Raw(String),
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyDryRunEvent {
    pub key: String,
    pub value: i32,
    pub at_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyDryRunStep {
    pub event: KeyDryRunEvent,
    pub triggered_binding_names: Vec<String>,
    pub trace_lines: Vec<String>,
}

/// Keybinding manager using unilii-lib evdev keyboard streams.
pub struct KeybindingDaemon {
    bindings: Vec<ParsedBinding>,
    engine: Mutex<KeyEngine>,
    action_sender: Option<tokio::sync::mpsc::UnboundedSender<KeybindingResult>>,
}

impl KeybindingDaemon {
    pub fn new(bindings: Vec<KeyBinding>) -> Self {
        let parsed_bindings = Self::parse_bindings(bindings);
        let engine_bindings = parsed_bindings
            .iter()
            .map(|parsed| parsed.engine_binding.clone())
            .collect();
        Self {
            bindings: parsed_bindings,
            engine: Mutex::new(KeyEngine::new(engine_bindings)),
            action_sender: None,
        }
    }

    /// Set a sender for internal actions (bar/tray actions).
    /// This allows the daemon to communicate internal actions back to the application.
    pub fn set_action_sender(
        &mut self,
        sender: tokio::sync::mpsc::UnboundedSender<KeybindingResult>,
    ) {
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
        while let Some(event) = stream.next().await {
            let key_name = format!("{:?}", event.code);

            let output = {
                let mut engine = self
                    .engine
                    .lock()
                    .map_err(|error| format!("failed to lock key engine: {}", error))?;
                engine.process_event(&key_name, event.value, Instant::now())
            };

            for trace in output.traces {
                let level = match trace.reason {
                    KeyEngineTraceReason::Matched => "matched",
                    KeyEngineTraceReason::Suppressed => "suppressed",
                    KeyEngineTraceReason::Invalidated => "invalidated",
                };
                debug!(
                    "hotkeys: engine trace binding='{}' index={} state={} detail={}",
                    trace.binding_name, trace.index, level, trace.detail
                );
            }

            for index in output.triggered {
                if let Err(error) = self.execute_binding(index) {
                    error!(
                        "hotkeys: binding execution failed index={} error={}",
                        index, error
                    );
                }
            }
        }

        Ok(())
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

pub fn dry_run_bindings(
    bindings: &[KeyBinding],
    events: &[KeyDryRunEvent],
) -> std::result::Result<Vec<KeyDryRunStep>, String> {
    let parsed_bindings = parse_bindings_strict(bindings)?;
    let engine_bindings = parsed_bindings
        .iter()
        .map(|parsed| parsed.engine_binding.clone())
        .collect::<Vec<_>>();
    let mut engine = KeyEngine::new(engine_bindings);
    let base = Instant::now();

    let mut steps = Vec::with_capacity(events.len());
    for event in events {
        let now = base + std::time::Duration::from_millis(event.at_ms);
        let output = engine.process_event(&event.key, event.value, now);
        let triggered_binding_names = output
            .triggered
            .iter()
            .map(|index| parsed_bindings[*index].binding.name.clone())
            .collect::<Vec<_>>();
        let trace_lines = output
            .traces
            .into_iter()
            .map(|trace| {
                let state = match trace.reason {
                    KeyEngineTraceReason::Matched => "matched",
                    KeyEngineTraceReason::Suppressed => "suppressed",
                    KeyEngineTraceReason::Invalidated => "invalidated",
                };
                format!(
                    "{} [{}] {} ({})",
                    trace.binding_name, trace.index, state, trace.detail
                )
            })
            .collect::<Vec<_>>();

        steps.push(KeyDryRunStep {
            event: event.clone(),
            triggered_binding_names,
            trace_lines,
        });
    }

    Ok(steps)
}

pub fn parse_tray_action(command: &str) -> TrayDaemonAction {
    let trimmed = command.trim();
    let lower = trimmed.to_ascii_lowercase();

    match lower.as_str() {
        "open-menu" | "menu:open" | "tray:open" => TrayDaemonAction::OpenMenu,
        "close-menu" | "menu:close" | "tray:close" => TrayDaemonAction::CloseMenu,
        "toggle-menu" | "menu:toggle" | "tray:toggle" => TrayDaemonAction::ToggleMenu,
        "show-aggregated" | "aggregated" | "tray:aggregated" => TrayDaemonAction::ShowAggregated,
        "show-favorites" | "favorites" | "tray:favorites" => TrayDaemonAction::ShowFavorites,
        "focus-next" | "next" | "tray:next" => TrayDaemonAction::FocusNext,
        "focus-previous" | "previous" | "prev" | "tray:previous" => TrayDaemonAction::FocusPrevious,
        "activate-selected" | "select" | "tray:activate" => TrayDaemonAction::ActivateSelected,
        "refresh-status" | "refresh" | "tray:refresh" => TrayDaemonAction::RefreshStatus,
        _ => {
            if let Some(index) = lower
                .strip_prefix("open-index:")
                .or_else(|| lower.strip_prefix("tray:index:"))
                .and_then(|value| value.parse::<usize>().ok())
            {
                TrayDaemonAction::OpenIndex(index)
            } else {
                TrayDaemonAction::Raw(trimmed.to_string())
            }
        }
    }
}

pub fn parse_bar_action(command: &str) -> BarDaemonAction {
    let trimmed = command.trim();
    let lower = trimmed.to_ascii_lowercase();

    if matches!(
        lower.as_str(),
        "reload-config" | "config:reload" | "bar:reload"
    ) {
        return BarDaemonAction::ReloadConfig;
    }

    if let Some(module) = trimmed
        .strip_prefix("toggle-module:")
        .or_else(|| trimmed.strip_prefix("bar:toggle:"))
    {
        return BarDaemonAction::ToggleModule(module.trim().to_string());
    }

    if let Some(module) = trimmed
        .strip_prefix("focus-module:")
        .or_else(|| trimmed.strip_prefix("bar:focus:"))
    {
        return BarDaemonAction::FocusModule(module.trim().to_string());
    }

    BarDaemonAction::Raw(trimmed.to_string())
}

fn parse_binding(binding: KeyBinding) -> std::result::Result<ParsedBinding, String> {
    let groups = parse_keysym(&binding.keysym)?;
    if groups.is_empty() {
        return Err("no keys parsed".to_string());
    }

    let trigger = if binding.release && matches!(binding.trigger, KeyTrigger::Press) {
        KeyTrigger::Release
    } else {
        binding.trigger.clone()
    };
    let trigger_keys = groups.last().cloned().unwrap_or_default();
    let engine_binding = EngineBinding::new(
        binding.name.clone(),
        groups.clone(),
        trigger,
        binding.priority,
        binding.consume,
        binding.hold_ms.unwrap_or(0),
        binding.cooldown_ms,
        trigger_keys,
    );

    Ok(ParsedBinding {
        binding,
        engine_binding,
    })
}

fn parse_bindings_strict(
    bindings: &[KeyBinding],
) -> std::result::Result<Vec<ParsedBinding>, String> {
    let mut parsed = Vec::with_capacity(bindings.len());
    for binding in bindings {
        parsed.push(
            parse_binding(binding.clone())
                .map_err(|error| format!("binding '{}' invalid: {}", binding.name, error))?,
        );
    }
    Ok(parsed)
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
    use super::{
        BarDaemonAction, TrayDaemonAction, parse_bar_action, parse_keysym, parse_tray_action,
        token_to_key_candidates,
    };

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

    #[test]
    fn parses_tray_commands_into_structured_actions() {
        assert_eq!(parse_tray_action("open-menu"), TrayDaemonAction::OpenMenu);
        assert_eq!(
            parse_tray_action("tray:index:3"),
            TrayDaemonAction::OpenIndex(3)
        );
        assert_eq!(
            parse_tray_action("favorites"),
            TrayDaemonAction::ShowFavorites
        );
    }

    #[test]
    fn preserves_unknown_tray_commands() {
        assert_eq!(
            parse_tray_action("custom:dispatch"),
            TrayDaemonAction::Raw("custom:dispatch".to_string())
        );
    }

    #[test]
    fn parses_bar_commands_into_structured_actions() {
        assert_eq!(
            parse_bar_action("reload-config"),
            BarDaemonAction::ReloadConfig
        );
        assert_eq!(
            parse_bar_action("toggle-module:clock"),
            BarDaemonAction::ToggleModule("clock".to_string())
        );
        assert_eq!(
            parse_bar_action("bar:focus:wifi"),
            BarDaemonAction::FocusModule("wifi".to_string())
        );
    }
}
