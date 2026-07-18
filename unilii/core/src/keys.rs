//! Global keybinding daemon and parsing utilities.

use crate::Result;
use crate::action_bus::{
    ActionBusRequest, DesktopAction, default_action_bus_socket_path, send_action_request,
};
use crate::key_engine::{EngineBinding, KeyEngine, KeyEngineTraceReason, KeyTrigger};
use crate::menu_process::{MenuProcessManager, acquire_process_instance, parse_menu_action};
use crate::x11_hotkeys::X11HotkeyListener;
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
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
    /// Managed external unilii menu action (show/hide/toggle).
    Menu,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum KeyBackend {
    #[default]
    Evdev,
    X11,
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
    /// Managed external menu action.
    MenuAction(String),
    /// Raw evdev event forwarded to an embedding application.
    RawKeyEvent { code: String, value: i32 },
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeybindingDaemonOptions {
    /// Input ownership backend. X11 uses selective passive grabs; evdev observes
    /// raw devices unless the explicitly unsafe whole-device grab is requested.
    pub backend: KeyBackend,
    /// Execute matching shell/internal bindings. Set to false for shadow mode.
    pub execute: bool,
    /// Request an exclusive evdev grab before listening.
    pub grab: bool,
    /// Raw evdev grabs suppress the whole keyboard because unilii does not yet
    /// re-inject unmatched events. This must be explicitly acknowledged.
    pub allow_unsafe_grab: bool,
    /// Ensure only one global unilii key listener owns the runtime at a time.
    pub singleton: bool,
}

impl Default for KeybindingDaemonOptions {
    fn default() -> Self {
        Self {
            backend: KeyBackend::Evdev,
            execute: true,
            grab: false,
            allow_unsafe_grab: false,
            singleton: true,
        }
    }
}

impl KeybindingDaemonOptions {
    pub fn shadow() -> Self {
        Self {
            backend: KeyBackend::Evdev,
            execute: false,
            grab: false,
            allow_unsafe_grab: false,
            singleton: true,
        }
    }

    pub fn active_grab() -> Self {
        Self {
            backend: KeyBackend::Evdev,
            execute: true,
            grab: true,
            allow_unsafe_grab: true,
            singleton: true,
        }
    }
}

/// Keybinding manager using unilii-lib evdev keyboard streams.
pub struct KeybindingDaemon {
    bindings: Vec<ParsedBinding>,
    engine: Mutex<KeyEngine>,
    action_sender: Option<tokio::sync::mpsc::UnboundedSender<KeybindingResult>>,
    options: KeybindingDaemonOptions,
    menu_manager: MenuProcessManager,
    action_bus_socket: PathBuf,
}

impl KeybindingDaemon {
    pub fn new(bindings: Vec<KeyBinding>) -> Self {
        Self::with_options(bindings, KeybindingDaemonOptions::default())
    }

    pub fn with_options(bindings: Vec<KeyBinding>, options: KeybindingDaemonOptions) -> Self {
        let parsed_bindings = Self::parse_bindings(bindings);
        let engine_bindings = parsed_bindings
            .iter()
            .map(|parsed| parsed.engine_binding.clone())
            .collect();
        Self {
            bindings: parsed_bindings,
            engine: Mutex::new(KeyEngine::new(engine_bindings)),
            action_sender: None,
            options,
            menu_manager: MenuProcessManager::default(),
            action_bus_socket: default_action_bus_socket_path(),
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

    /// Override the managed-menu runtime, primarily for integration tests.
    pub fn set_menu_manager(&mut self, menu_manager: MenuProcessManager) {
        self.menu_manager = menu_manager;
    }

    pub fn set_action_bus_socket(&mut self, path: PathBuf) {
        self.action_bus_socket = path;
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
        self.run_with_ready(None).await
    }

    /// Run the input worker and optionally report when the keyboard listener is ready.
    ///
    /// The standalone supervisor uses this handshake to make configuration reloads
    /// transactional: it only commits a new generation after device access succeeds.
    pub async fn run_with_ready(
        &self,
        mut ready: Option<tokio::sync::oneshot::Sender<std::result::Result<(), String>>>,
    ) -> Result<()> {
        let grab = self.options.grab;
        if self.options.backend == KeyBackend::X11 {
            return self.run_x11_with_ready(ready).await;
        }
        if grab && !self.options.allow_unsafe_grab {
            let message = "refusing unsafe raw evdev grab: grabbing a keyboard suppresses all keys, and unilii does not yet re-inject unmatched events. Use observe mode, or pass the explicit unsafe acknowledgement in the standalone daemon only.".to_string();
            if let Some(sender) = ready.take() {
                let _ = sender.send(Err(message.clone()));
            }
            return Err(message.into());
        }
        let _instance_guard = if self.options.singleton {
            match acquire_process_instance("hotkeyd") {
                Ok(guard) => Some(guard),
                Err(error) => {
                    let message = format!("global hotkey listener unavailable: {error}");
                    if let Some(sender) = ready.take() {
                        let _ = sender.send(Err(message.clone()));
                    }
                    return Err(message.into());
                }
            }
        } else {
            None
        };
        let listener_result =
            match deskhalloumi_lib::input::listen_keyboard_events_experimental_with_grab(grab) {
                Ok(stream) => {
                    info!(
                        "hotkeys: listener initialized with dynamic tokio-udev keyboard hot-plug (grab={}, execute={})",
                        grab, self.options.execute
                    );
                    Ok(stream)
                }
                Err(error) => {
                    warn!(
                        "hotkeys: dynamic udev listener unavailable, falling back to one-time evdev scan (grab={}): {}",
                        grab, error
                    );
                    deskhalloumi_lib::input::listen_keyboard_events_with_grab(grab)
                }
            };
        let listener = match listener_result {
            Ok(listener) => {
                if let Some(sender) = ready.take() {
                    let _ = sender.send(Ok(()));
                }
                listener
            }
            Err(error) => {
                let message = error.to_string();
                if let Some(sender) = ready.take() {
                    let _ = sender.send(Err(message.clone()));
                }
                return Err(error.into());
            }
        };

        let mut stream = listener;
        while let Some(event) = stream.next().await {
            let key_name = format!("{:?}", event.code);
            self.process_key_event(&key_name, event.value, Instant::now())?;
        }

        Ok(())
    }

    async fn run_x11_with_ready(
        &self,
        mut ready: Option<tokio::sync::oneshot::Sender<std::result::Result<(), String>>>,
    ) -> Result<()> {
        let _instance_guard = if self.options.singleton {
            match acquire_process_instance("hotkeyd") {
                Ok(guard) => Some(guard),
                Err(error) => {
                    let message = format!("global hotkey listener unavailable: {error}");
                    if let Some(sender) = ready.take() {
                        let _ = sender.send(Err(message.clone()));
                    }
                    return Err(message.into());
                }
            }
        } else {
            None
        };
        let bindings = self
            .bindings
            .iter()
            .map(|parsed| parsed.binding.clone())
            .collect::<Vec<_>>();
        let listener = match X11HotkeyListener::connect(&bindings) {
            Ok(listener) => listener,
            Err(error) => {
                if let Some(sender) = ready.take() {
                    let _ = sender.send(Err(error.clone()));
                }
                return Err(error.into());
            }
        };
        info!(
            "hotkeys: selective X11 backend initialized grabs={} execute={}",
            listener.diagnostics().len(),
            self.options.execute
        );
        if let Some(sender) = ready.take() {
            let _ = sender.send(Ok(()));
        }
        let mut events = listener.into_event_stream();
        while let Some(event) = events.recv().await {
            let event = event
                .map_err(|error| -> Box<dyn std::error::Error + Send + Sync> { error.into() })?;
            self.process_key_event(&event.code, event.value, Instant::now())?;
        }
        Err("X11 hotkey event stream ended unexpectedly".into())
    }

    fn process_key_event(&self, key_name: &str, value: i32, now: Instant) -> Result<()> {
        if let Some(sender) = &self.action_sender
            && sender
                .send(KeybindingResult::RawKeyEvent {
                    code: key_name.to_string(),
                    value,
                })
                .is_err()
        {
            debug!("hotkeys: embedding action receiver closed; raw key event not forwarded");
        }

        let output = {
            let mut engine = self
                .engine
                .lock()
                .map_err(|error| format!("failed to lock key engine: {}", error))?;
            engine.process_event(key_name, value, now)
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
        Ok(())
    }

    fn execute_binding(&self, index: usize) -> Result<()> {
        let binding = &self.bindings[index].binding;
        let command = binding.command.trim();
        if command.is_empty() {
            return Ok(());
        }

        if !self.options.execute {
            info!(
                "hotkeys: shadow match binding='{}' keysym='{}' type={:?} command='{}'",
                binding.name, binding.keysym, binding.command_type, command
            );
            return Ok(());
        }

        match &binding.command_type {
            CommandType::Shell => {
                let mut child = std::process::Command::new("sh")
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
                std::thread::spawn(move || {
                    let _ = child.wait();
                });

                info!(
                    "hotkeys: executed shell binding '{}' keysym='{}'",
                    binding.name, binding.keysym
                );
            }
            CommandType::Menu => {
                let action = parse_menu_action(command).map_err(|error| {
                    format!("invalid managed-menu binding '{}': {error}", binding.name)
                })?;
                let outcome = self.menu_manager.execute(&action).map_err(|error| {
                    format!("managed-menu binding '{}' failed: {error}", binding.name)
                })?;
                info!(
                    "hotkeys: executed managed-menu binding '{}' action='{}' outcome={:?}",
                    binding.name, command, outcome
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
                    sender.send(result).map_err(|_| {
                        format!("hotkeys: failed to send internal action '{}'", command)
                    })?;
                } else {
                    let action = match binding.command_type {
                        CommandType::Bar => DesktopAction::Bar(command.to_string()),
                        CommandType::Tray => DesktopAction::Tray(command.to_string()),
                        CommandType::Widget => DesktopAction::Widget(command.to_string()),
                        _ => unreachable!(),
                    };
                    let request = ActionBusRequest::new(
                        format!("{}-{}", std::process::id(), binding.name),
                        action,
                    );
                    let response = send_action_request(&self.action_bus_socket, &request)?;
                    if !response.ok {
                        return Err(format!(
                            "desktop action receiver rejected '{}': {}",
                            binding.name, response.message
                        )
                        .into());
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

/// Validate one binding using the same parser as the live daemon.
pub fn validate_binding(binding: &KeyBinding) -> std::result::Result<(), String> {
    parse_binding(binding.clone()).map(|_| ())
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
    #[test]
    fn unsafe_grab_reports_readiness_failure_before_device_access() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime.block_on(async {
            let options = super::KeybindingDaemonOptions {
                backend: super::KeyBackend::Evdev,
                execute: true,
                grab: true,
                allow_unsafe_grab: false,
                singleton: false,
            };
            let daemon = super::KeybindingDaemon::with_options(Vec::new(), options);
            let (sender, receiver) = tokio::sync::oneshot::channel();
            let result = daemon.run_with_ready(Some(sender)).await;
            assert!(result.is_err());
            let readiness = receiver.await.unwrap();
            assert!(readiness.unwrap_err().contains("unsafe raw evdev grab"));
        });
    }
    #[test]
    fn embedded_action_channel_receives_raw_key_events() {
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        let mut daemon = super::KeybindingDaemon::with_options(
            Vec::new(),
            super::KeybindingDaemonOptions {
                backend: super::KeyBackend::Evdev,
                execute: true,
                grab: false,
                allow_unsafe_grab: false,
                singleton: false,
            },
        );
        daemon.set_action_sender(sender);
        daemon
            .process_key_event("KEY_LEFTSHIFT", 1, std::time::Instant::now())
            .unwrap();
        match receiver.try_recv().unwrap() {
            super::KeybindingResult::RawKeyEvent { code, value } => {
                assert_eq!(code, "KEY_LEFTSHIFT");
                assert_eq!(value, 1);
            }
            other => panic!("expected raw key event, got {other:?}"),
        }
    }
}
