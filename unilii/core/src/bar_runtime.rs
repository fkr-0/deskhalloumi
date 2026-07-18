//! Headless, testable runtime primitives for `unilii-bar` modules.
//!
//! This module is intentionally renderer-neutral. It gives the future Makepad
//! app a stable module graph, render model, and built-in providers before any
//! graphical windowing code is introduced.

use crate::bar::{
    BarAction, BarConfig, BarLayout, BarModuleSpec, BarResult, parse_bar_config_str,
    validate_bar_config,
};
use std::collections::HashMap;
use std::fs::File;
use std::io::Read;
use std::process::{Command, Stdio};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use wait_timeout::ChildExt;

/// Common visual/semantic state for a rendered module.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BarModuleState {
    Ok,
    Warning,
    Critical,
    Active,
    Visible,
    Urgent,
    Empty,
    Muted,
    Charging,
    Disconnected,
    Unavailable,
    Stale,
}

/// Renderer-neutral output for one module.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BarModuleViewModel {
    pub id: String,
    pub module_type: String,
    pub label: String,
    pub tooltip: Option<String>,
    pub state: BarModuleState,
    pub visible: bool,
    pub last_error: Option<String>,
}

/// Renderer-neutral output grouped by configured visual zone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BarRenderModel {
    pub left: Vec<BarModuleViewModel>,
    pub center: Vec<BarModuleViewModel>,
    pub right: Vec<BarModuleViewModel>,
}

/// Mouse button used to trigger a configured module action.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BarClickButton {
    Left,
    Middle,
    Right,
}

/// Result category for action dispatch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BarActionStatus {
    Success,
    Failed,
    TimedOut,
    Unsupported,
}

/// Renderer-neutral output from a click/action dispatch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BarActionResult {
    pub module_id: String,
    pub button: BarClickButton,
    pub status: BarActionStatus,
    pub stdout: String,
    pub stderr: String,
    pub exit_code: Option<i32>,
    pub message: Option<String>,
}

/// Result of checking a watched config file for hot reload.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BarReloadStatus {
    Reloaded,
    Unchanged,
    Failed,
}

#[derive(Debug, Clone, Default)]
struct BarModuleActions {
    left: Option<BarAction>,
    middle: Option<BarAction>,
    right: Option<BarAction>,
}

impl BarModuleViewModel {
    pub fn ok(id: &str, module_type: &str, label: impl Into<String>) -> Self {
        Self {
            id: id.to_string(),
            module_type: module_type.to_string(),
            label: label.into(),
            tooltip: None,
            state: BarModuleState::Ok,
            visible: true,
            last_error: None,
        }
    }

    pub fn unavailable(id: &str, module_type: &str, message: impl Into<String>) -> Self {
        let message = message.into();
        Self {
            id: id.to_string(),
            module_type: module_type.to_string(),
            label: "unavailable".to_string(),
            tooltip: Some(message.clone()),
            state: BarModuleState::Unavailable,
            visible: true,
            last_error: Some(message),
        }
    }
}

/// A renderer-neutral bar module.
pub trait BarModule: Send {
    fn id(&self) -> &str;
    fn module_type(&self) -> &str;
    fn update(&mut self, ctx: &BarRuntimeContext) -> BarModuleViewModel;
}

/// Runtime inputs that make tests deterministic and later allow dependency injection.
#[derive(Debug, Clone)]
pub struct BarRuntimeContext {
    pub now: SystemTime,
    pub env: HashMap<String, String>,
}

impl Default for BarRuntimeContext {
    fn default() -> Self {
        Self {
            now: SystemTime::now(),
            env: std::env::vars().collect(),
        }
    }
}

impl BarRuntimeContext {
    pub fn at_unix_timestamp(seconds: u64) -> Self {
        Self {
            now: UNIX_EPOCH + Duration::from_secs(seconds),
            env: HashMap::new(),
        }
    }
}

/// Compiled module graph for a bar config.
pub struct BarModuleGraph {
    modules: Vec<Box<dyn BarModule>>,
    layout: BarLayout,
    actions: HashMap<String, BarModuleActions>,
    intervals: HashMap<String, Duration>,
    workspace_switch_templates: HashMap<String, String>,
    last_updates: HashMap<String, SystemTime>,
    cached_models: HashMap<String, BarModuleViewModel>,
}

impl BarModuleGraph {
    pub fn from_config(config: &BarConfig) -> BarResult<Self> {
        validate_bar_config(config)?;
        let enabled_specs = config
            .modules
            .iter()
            .filter(|spec| spec.enabled)
            .collect::<Vec<_>>();
        let modules = enabled_specs
            .iter()
            .map(|spec| build_module(spec))
            .collect::<Vec<_>>();
        let intervals = enabled_specs
            .iter()
            .map(|spec| {
                (
                    spec.id.clone(),
                    Duration::from_millis(spec.interval_ms.unwrap_or(1000)),
                )
            })
            .collect::<HashMap<_, _>>();
        let workspace_switch_templates = enabled_specs
            .iter()
            .filter(|spec| spec.module_type == "workspaces")
            .filter_map(|spec| {
                string_extra(spec, "switch_command_template")
                    .or_else(|| {
                        backend_extra(spec)
                            .as_deref()
                            .and_then(default_switch_template_for_backend)
                    })
                    .map(|template| (spec.id.clone(), template))
            })
            .collect::<HashMap<_, _>>();
        let actions = config
            .modules
            .iter()
            .filter(|spec| spec.enabled)
            .map(|spec| {
                (
                    spec.id.clone(),
                    BarModuleActions {
                        left: spec.on_click_left.clone(),
                        middle: spec.on_click_middle.clone(),
                        right: spec.on_click_right.clone(),
                    },
                )
            })
            .collect::<HashMap<_, _>>();
        Ok(Self {
            modules,
            layout: config.layout.clone(),
            actions,
            intervals,
            workspace_switch_templates,
            last_updates: HashMap::new(),
            cached_models: HashMap::new(),
        })
    }

    pub fn len(&self) -> usize {
        self.modules.len()
    }

    pub fn is_empty(&self) -> bool {
        self.modules.is_empty()
    }

    pub fn update_all(&mut self, ctx: &BarRuntimeContext) -> Vec<BarModuleViewModel> {
        let mut models = Vec::new();
        for module in &mut self.modules {
            let model = module.update(ctx);
            self.last_updates.insert(model.id.clone(), ctx.now);
            self.cached_models.insert(model.id.clone(), model.clone());
            models.push(model);
        }
        models
    }

    pub fn update_due_modules(&mut self, ctx: &BarRuntimeContext) -> Vec<BarModuleViewModel> {
        let mut models = Vec::new();
        let due_ids = self
            .modules
            .iter()
            .filter_map(|module| {
                let id = module.id().to_string();
                if self.is_module_due(&id, ctx.now) {
                    Some(id)
                } else {
                    None
                }
            })
            .collect::<std::collections::HashSet<_>>();
        for module in &mut self.modules {
            let id = module.id().to_string();
            if due_ids.contains(&id) {
                let model = module.update(ctx);
                self.last_updates.insert(id, ctx.now);
                self.cached_models.insert(model.id.clone(), model.clone());
                models.push(model);
            }
        }
        models
    }

    pub fn update_render_model(&mut self, ctx: &BarRuntimeContext) -> BarRenderModel {
        let models = self.update_all(ctx);
        render_model_from_layout(&self.layout, models)
    }

    pub fn update_due_render_model(&mut self, ctx: &BarRuntimeContext) -> BarRenderModel {
        self.update_due_modules(ctx);
        self.cached_render_model()
    }

    pub fn cached_render_model(&self) -> BarRenderModel {
        render_model_from_layout(
            &self.layout,
            self.cached_models.values().cloned().collect::<Vec<_>>(),
        )
    }

    pub fn next_due_in(&self, now: SystemTime) -> Option<Duration> {
        self.modules
            .iter()
            .map(|module| {
                let id = module.id();
                let interval = self
                    .intervals
                    .get(id)
                    .copied()
                    .unwrap_or_else(|| Duration::from_millis(1000));
                match self.last_updates.get(id) {
                    None => Duration::ZERO,
                    Some(last_update) => match now.duration_since(*last_update) {
                        Ok(elapsed) if elapsed >= interval => Duration::ZERO,
                        Ok(elapsed) => interval - elapsed,
                        Err(_) => interval,
                    },
                }
            })
            .min()
    }

    fn is_module_due(&self, id: &str, now: SystemTime) -> bool {
        let interval = self
            .intervals
            .get(id)
            .copied()
            .unwrap_or_else(|| Duration::from_millis(1000));
        match self.last_updates.get(id) {
            None => true,
            Some(last_update) => now
                .duration_since(*last_update)
                .map(|elapsed| elapsed >= interval)
                .unwrap_or(false),
        }
    }

    pub fn workspace_switch_command(
        &self,
        module_id: &str,
        workspace_name: &str,
    ) -> Option<String> {
        let template = self.workspace_switch_templates.get(module_id)?;
        Some(render_workspace_switch_command(template, workspace_name))
    }

    pub fn dispatch_workspace_switch(
        &self,
        module_id: &str,
        workspace_name: &str,
        ctx: &BarRuntimeContext,
    ) -> Option<BarActionResult> {
        let command = self.workspace_switch_command(module_id, workspace_name)?;
        let action = BarAction::Command(command);
        Some(dispatch_bar_action(
            module_id,
            BarClickButton::Left,
            &action,
            ctx,
        ))
    }

    pub fn dispatch_action(
        &self,
        module_id: &str,
        button: BarClickButton,
        ctx: &BarRuntimeContext,
    ) -> Option<BarActionResult> {
        let action = match button {
            BarClickButton::Left => self.actions.get(module_id)?.left.as_ref(),
            BarClickButton::Middle => self.actions.get(module_id)?.middle.as_ref(),
            BarClickButton::Right => self.actions.get(module_id)?.right.as_ref(),
        }?;
        Some(dispatch_bar_action(module_id, button, action, ctx))
    }
}

/// Runtime state that can hot-reload config while preserving the last valid graph.
pub struct BarRuntimeState {
    config: BarConfig,
    graph: BarModuleGraph,
    last_reload_error: Option<String>,
    config_path: Option<std::path::PathBuf>,
    config_modified: Option<SystemTime>,
}

impl BarRuntimeState {
    pub fn from_config(config: BarConfig) -> BarResult<Self> {
        let graph = BarModuleGraph::from_config(&config)?;
        Ok(Self {
            config,
            graph,
            last_reload_error: None,
            config_path: None,
            config_modified: None,
        })
    }

    pub fn from_config_file(path: impl AsRef<std::path::Path>) -> BarResult<Self> {
        let path = path.as_ref().to_path_buf();
        let input = std::fs::read_to_string(&path).map_err(|error| {
            crate::bar::BarConfigError::new(format!(
                "failed to read bar config '{}': {error}",
                path.display()
            ))
        })?;
        let config = parse_bar_config_str(&input)?;
        let mut state = Self::from_config(config)?;
        state.config_modified = file_modified_time(&path);
        state.config_path = Some(path);
        Ok(state)
    }

    pub fn config(&self) -> &BarConfig {
        &self.config
    }

    pub fn graph_mut(&mut self) -> &mut BarModuleGraph {
        &mut self.graph
    }

    pub fn last_reload_error(&self) -> Option<&str> {
        self.last_reload_error.as_deref()
    }

    pub fn config_path(&self) -> Option<&std::path::Path> {
        self.config_path.as_deref()
    }

    pub fn config_modified(&self) -> Option<SystemTime> {
        self.config_modified
    }

    pub fn reload_from_str(&mut self, input: &str) -> BarResult<()> {
        match parse_bar_config_str(input) {
            Ok(config) => {
                let graph = BarModuleGraph::from_config(&config)?;
                self.config = config;
                self.graph = graph;
                self.last_reload_error = None;
                Ok(())
            }
            Err(error) => {
                self.last_reload_error = Some(error.to_string());
                Err(error)
            }
        }
    }

    pub fn reload_from_file_if_changed(&mut self) -> BarResult<BarReloadStatus> {
        let Some(path) = self.config_path.clone() else {
            return Ok(BarReloadStatus::Unchanged);
        };
        let modified = file_modified_time(&path);
        if modified.is_some() && modified == self.config_modified {
            return Ok(BarReloadStatus::Unchanged);
        }
        let input = match std::fs::read_to_string(&path) {
            Ok(input) => input,
            Err(error) => {
                let message = format!("failed to read bar config '{}': {error}", path.display());
                self.last_reload_error = Some(message.clone());
                return Err(crate::bar::BarConfigError::new(message));
            }
        };
        match self.reload_from_str(&input) {
            Ok(()) => {
                self.config_modified = modified;
                Ok(BarReloadStatus::Reloaded)
            }
            Err(error) => {
                self.last_reload_error = Some(error.to_string());
                Ok(BarReloadStatus::Failed)
            }
        }
    }
}

fn file_modified_time(path: &std::path::Path) -> Option<SystemTime> {
    std::fs::metadata(path)
        .and_then(|metadata| metadata.modified())
        .ok()
}

fn render_model_from_layout(layout: &BarLayout, models: Vec<BarModuleViewModel>) -> BarRenderModel {
    let mut by_id = models
        .into_iter()
        .map(|model| (model.id.clone(), model))
        .collect::<HashMap<_, _>>();
    BarRenderModel {
        left: take_zone_models(&mut by_id, &layout.left),
        center: take_zone_models(&mut by_id, &layout.center),
        right: take_zone_models(&mut by_id, &layout.right),
    }
}

fn take_zone_models(
    by_id: &mut HashMap<String, BarModuleViewModel>,
    ids: &[String],
) -> Vec<BarModuleViewModel> {
    ids.iter()
        .filter_map(|id| by_id.remove(id))
        .filter(|model| model.visible)
        .collect()
}

fn render_workspace_switch_command(template: &str, workspace_name: &str) -> String {
    let quoted = shell_single_quote(workspace_name);
    template
        .replace("{workspace}", workspace_name)
        .replace("{workspace_shell}", &quoted)
}

fn shell_single_quote(input: &str) -> String {
    format!("'{}'", input.replace('\'', "'\\''"))
}

fn dispatch_bar_action(
    module_id: &str,
    button: BarClickButton,
    action: &BarAction,
    ctx: &BarRuntimeContext,
) -> BarActionResult {
    let Some(command) = action_command(action) else {
        return BarActionResult {
            module_id: module_id.to_string(),
            button,
            status: BarActionStatus::Unsupported,
            stdout: String::new(),
            stderr: String::new(),
            exit_code: None,
            message: Some("internal bar actions are not implemented yet".to_string()),
        };
    };
    if let Some(blocked) = test_side_effect_guard(module_id, button, command, ctx) {
        return blocked;
    }

    match run_script_command(
        command,
        Duration::from_millis(DEFAULT_SCRIPT_TIMEOUT_MS),
        DEFAULT_SCRIPT_OUTPUT_LIMIT,
        &ctx.env,
    ) {
        Ok(output) if output.timed_out => BarActionResult {
            module_id: module_id.to_string(),
            button,
            status: BarActionStatus::TimedOut,
            stdout: output.stdout,
            stderr: output.stderr,
            exit_code: None,
            message: Some("action command timed out".to_string()),
        },
        Ok(output) if output.exit_code == Some(0) => BarActionResult {
            module_id: module_id.to_string(),
            button,
            status: BarActionStatus::Success,
            stdout: trim_script_output(&output.stdout, DEFAULT_SCRIPT_OUTPUT_LIMIT),
            stderr: trim_script_output(&output.stderr, DEFAULT_SCRIPT_OUTPUT_LIMIT),
            exit_code: output.exit_code,
            message: None,
        },
        Ok(output) => BarActionResult {
            module_id: module_id.to_string(),
            button,
            status: BarActionStatus::Failed,
            stdout: trim_script_output(&output.stdout, DEFAULT_SCRIPT_OUTPUT_LIMIT),
            stderr: trim_script_output(&output.stderr, DEFAULT_SCRIPT_OUTPUT_LIMIT),
            exit_code: output.exit_code,
            message: Some(format!(
                "action exited with {}",
                output.exit_code.unwrap_or(-1)
            )),
        },
        Err(error) => BarActionResult {
            module_id: module_id.to_string(),
            button,
            status: BarActionStatus::Failed,
            stdout: String::new(),
            stderr: String::new(),
            exit_code: None,
            message: Some(error),
        },
    }
}

fn test_side_effect_guard(
    module_id: &str,
    button: BarClickButton,
    command: &str,
    ctx: &BarRuntimeContext,
) -> Option<BarActionResult> {
    if !is_test_command_guard_active() {
        return None;
    }
    if ctx
        .env
        .get("UNILII_BAR_ALLOW_SIDE_EFFECT_COMMANDS_IN_TESTS")
        .is_some_and(|value| matches!(value.as_str(), "1" | "true" | "yes" | "on"))
    {
        return None;
    }
    if !is_side_effectful_test_command(command) {
        return None;
    }
    Some(BarActionResult {
        module_id: module_id.to_string(),
        button,
        status: BarActionStatus::Unsupported,
        stdout: String::new(),
        stderr: String::new(),
        exit_code: None,
        message: Some(format!(
            "blocked side-effectful command in test: {}",
            command.trim()
        )),
    })
}

fn is_test_command_guard_active() -> bool {
    cfg!(test)
        || std::env::args().any(|arg| {
            arg == "--nocapture"
                || arg == "--ignored"
                || arg == "--include-ignored"
                || arg == "--test-threads"
                || arg.starts_with("--test-threads=")
        })
        || std::env::current_exe()
            .ok()
            .and_then(|path| path.parent().map(|parent| parent.ends_with("deps")))
            .unwrap_or(false)
}

fn guard_script_command_for_test_build(
    command: &str,
    env: &HashMap<String, String>,
) -> Result<(), String> {
    if !is_test_command_guard_active() {
        return Ok(());
    }
    if env
        .get("UNILII_BAR_ALLOW_SIDE_EFFECT_COMMANDS_IN_TESTS")
        .is_some_and(|value| matches!(value.as_str(), "1" | "true" | "yes" | "on"))
    {
        return Ok(());
    }
    if is_side_effectful_test_command(command) {
        return Err(format!("blocked command in test: {}", command.trim()));
    }
    Ok(())
}

fn is_side_effectful_test_command(command: &str) -> bool {
    let first = command
        .split_whitespace()
        .next()
        .unwrap_or("")
        .rsplit('/')
        .next()
        .unwrap_or("");
    matches!(
        first,
        "i3-msg"
            | "swaymsg"
            | "nmcli"
            | "pactl"
            | "wpctl"
            | "systemctl"
            | "loginctl"
            | "notify-send"
            | "unilii-test-side-effect"
    )
}

fn action_command(action: &BarAction) -> Option<&str> {
    match action {
        BarAction::Command(command) => {
            Some(command.as_str()).filter(|value| !value.trim().is_empty())
        }
        BarAction::Detailed { command, .. } => {
            command.as_deref().filter(|value| !value.trim().is_empty())
        }
    }
}

fn build_module(spec: &BarModuleSpec) -> Box<dyn BarModule> {
    match spec.module_type.as_str() {
        "clock" => Box::new(ClockModule::new(spec)),
        "script" => Box::new(ScriptModule::new(spec)),
        "system" => Box::new(SystemModule::new(spec)),
        "network" => Box::new(NetworkModule::new(spec)),
        "vpn" => Box::new(VpnModule::new(spec)),
        "audio" => Box::new(AudioModule::new(spec)),
        "battery" => Box::new(BatteryModule::new(spec)),
        "workspaces" => Box::new(WorkspacesModule::new(spec)),
        "window_title" => Box::new(WindowTitleModule::new(spec)),
        "notifications" => Box::new(NotificationsModule::new(spec)),
        _ => Box::new(StaticProbeModule::new(
            spec,
            "unknown",
            "unknown module type",
        )),
    }
}

struct ClockModule {
    id: String,
    module_type: String,
    format: String,
}

impl ClockModule {
    fn new(spec: &BarModuleSpec) -> Self {
        Self {
            id: spec.id.clone(),
            module_type: spec.module_type.clone(),
            format: spec
                .format
                .clone()
                .unwrap_or_else(|| "%Y-%m-%d %H:%M:%S".to_string()),
        }
    }
}

impl BarModule for ClockModule {
    fn id(&self) -> &str {
        &self.id
    }

    fn module_type(&self) -> &str {
        &self.module_type
    }

    fn update(&mut self, ctx: &BarRuntimeContext) -> BarModuleViewModel {
        let datetime: chrono::DateTime<chrono::Local> = ctx.now.into();
        BarModuleViewModel::ok(
            &self.id,
            &self.module_type,
            datetime.format(&self.format).to_string(),
        )
    }
}

const DEFAULT_SCRIPT_TIMEOUT_MS: u64 = 30_000;
const DEFAULT_SCRIPT_OUTPUT_LIMIT: usize = 4096;

#[derive(Debug, Clone, PartialEq, Eq)]
struct ScriptRunOutput {
    stdout: String,
    stderr: String,
    exit_code: Option<i32>,
    timed_out: bool,
}

struct ScriptModule {
    id: String,
    module_type: String,
    command: String,
    timeout_ms: u64,
    output_limit: usize,
    format: String,
    last_good_label: Option<String>,
}

impl ScriptModule {
    fn new(spec: &BarModuleSpec) -> Self {
        Self {
            id: spec.id.clone(),
            module_type: spec.module_type.clone(),
            command: spec.command.clone().unwrap_or_default(),
            timeout_ms: spec.timeout_ms.unwrap_or(DEFAULT_SCRIPT_TIMEOUT_MS),
            output_limit: spec
                .extra
                .get("max_output_bytes")
                .and_then(toml::Value::as_integer)
                .and_then(|value| usize::try_from(value).ok())
                .filter(|value| *value > 0)
                .unwrap_or(DEFAULT_SCRIPT_OUTPUT_LIMIT),
            format: spec
                .format
                .clone()
                .unwrap_or_else(|| "{output}".to_string()),
            last_good_label: None,
        }
    }

    fn render_output(&self, run: &ScriptRunOutput) -> String {
        let output = trim_script_output(&run.stdout, self.output_limit);
        let stderr = trim_script_output(&run.stderr, self.output_limit);
        self.format
            .replace("{output}", &output)
            .replace("{stdout}", &output)
            .replace("{stderr}", &stderr)
            .replace(
                "{exit_code}",
                &run.exit_code
                    .map(|code| code.to_string())
                    .unwrap_or_else(|| "timeout".to_string()),
            )
    }
}

impl BarModule for ScriptModule {
    fn id(&self) -> &str {
        &self.id
    }

    fn module_type(&self) -> &str {
        &self.module_type
    }

    fn update(&mut self, ctx: &BarRuntimeContext) -> BarModuleViewModel {
        match run_script_command(
            &self.command,
            Duration::from_millis(self.timeout_ms),
            self.output_limit,
            &ctx.env,
        ) {
            Ok(run) if run.timed_out => {
                let label = self
                    .last_good_label
                    .clone()
                    .unwrap_or_else(|| "script timeout".to_string());
                BarModuleViewModel {
                    id: self.id.clone(),
                    module_type: self.module_type.clone(),
                    label,
                    tooltip: Some(format!(
                        "script timed out after {}ms: {}",
                        self.timeout_ms, self.command
                    )),
                    state: BarModuleState::Critical,
                    visible: true,
                    last_error: Some("script timed out".to_string()),
                }
            }
            Ok(run) if run.exit_code == Some(0) => {
                let label = self.render_output(&run);
                self.last_good_label = Some(label.clone());
                BarModuleViewModel::ok(&self.id, &self.module_type, label)
            }
            Ok(run) => {
                let label = self.render_output(&run);
                BarModuleViewModel {
                    id: self.id.clone(),
                    module_type: self.module_type.clone(),
                    label: if label.is_empty() {
                        format!("exit {}", run.exit_code.unwrap_or(-1))
                    } else {
                        label
                    },
                    tooltip: Some(trim_script_output(&run.stderr, self.output_limit)),
                    state: BarModuleState::Warning,
                    visible: true,
                    last_error: Some(format!(
                        "script exited with {}",
                        run.exit_code.unwrap_or(-1)
                    )),
                }
            }
            Err(err) => BarModuleViewModel::unavailable(&self.id, &self.module_type, err),
        }
    }
}

fn run_script_command(
    command: &str,
    timeout: Duration,
    output_limit: usize,
    env: &HashMap<String, String>,
) -> Result<ScriptRunOutput, String> {
    guard_script_command_for_test_build(command, env)?;

    let unique = format!(
        "unilii-bar-script-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    );
    let stdout_path = std::env::temp_dir().join(format!("{unique}.stdout"));
    let stderr_path = std::env::temp_dir().join(format!("{unique}.stderr"));
    let stdout_file = File::create(&stdout_path)
        .map_err(|err| format!("failed to create script stdout capture: {err}"))?;
    let stderr_file = File::create(&stderr_path)
        .map_err(|err| format!("failed to create script stderr capture: {err}"))?;

    let mut child = Command::new("sh")
        .arg("-c")
        .arg(command)
        .envs(env)
        .stdin(Stdio::null())
        .stdout(Stdio::from(stdout_file))
        .stderr(Stdio::from(stderr_file))
        .spawn()
        .map_err(|err| format!("failed to spawn script command: {err}"))?;

    let status = match child
        .wait_timeout(timeout)
        .map_err(|err| format!("failed while waiting for script command: {err}"))?
    {
        Some(status) => status,
        None => {
            let _ = child.kill();
            let _ = child.wait();
            let stdout = read_limited_file(&stdout_path, output_limit);
            let stderr = read_limited_file(&stderr_path, output_limit);
            cleanup_capture_files(&stdout_path, &stderr_path);
            return Ok(ScriptRunOutput {
                stdout,
                stderr,
                exit_code: None,
                timed_out: true,
            });
        }
    };

    let stdout = read_limited_file(&stdout_path, output_limit);
    let stderr = read_limited_file(&stderr_path, output_limit);
    cleanup_capture_files(&stdout_path, &stderr_path);
    Ok(ScriptRunOutput {
        stdout,
        stderr,
        exit_code: status.code(),
        timed_out: false,
    })
}

fn read_limited_file(path: &std::path::Path, limit: usize) -> String {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(_) => return String::new(),
    };
    let mut buffer = Vec::new();
    let max_bytes = u64::try_from(limit).unwrap_or(u64::MAX);
    if file.take(max_bytes).read_to_end(&mut buffer).is_err() {
        return String::new();
    }
    String::from_utf8_lossy(&buffer).into_owned()
}

fn cleanup_capture_files(stdout_path: &std::path::Path, stderr_path: &std::path::Path) {
    let _ = std::fs::remove_file(stdout_path);
    let _ = std::fs::remove_file(stderr_path);
}

fn trim_script_output(output: &str, limit: usize) -> String {
    let trimmed = output.trim();
    if trimmed.len() <= limit {
        return trimmed.to_string();
    }
    let mut end = 0;
    for (idx, _) in trimmed.char_indices() {
        if idx > limit {
            break;
        }
        end = idx;
    }
    format!("{}…", &trimmed[..end])
}

#[derive(Debug, Clone, PartialEq)]
struct SystemSnapshot {
    load1: Option<String>,
    memory_used_percent: Option<u8>,
}

struct SystemModule {
    id: String,
    module_type: String,
    format: String,
    memory_warning_percent: Option<u8>,
    memory_critical_percent: Option<u8>,
}

impl SystemModule {
    fn new(spec: &BarModuleSpec) -> Self {
        Self {
            id: spec.id.clone(),
            module_type: spec.module_type.clone(),
            format: spec
                .format
                .clone()
                .unwrap_or_else(|| "load {load1} mem {memory_used_percent}%".to_string()),
            memory_warning_percent: percent_extra(spec, "memory_warning_percent"),
            memory_critical_percent: percent_extra(spec, "memory_critical_percent"),
        }
    }

    fn render_snapshot(&self, snapshot: &SystemSnapshot) -> String {
        self.format
            .replace("{load1}", snapshot.load1.as_deref().unwrap_or("?"))
            .replace(
                "{memory_used_percent}",
                &snapshot
                    .memory_used_percent
                    .map(|value| value.to_string())
                    .unwrap_or_else(|| "?".to_string()),
            )
    }

    fn state_for_snapshot(&self, snapshot: &SystemSnapshot) -> BarModuleState {
        let Some(memory_used_percent) = snapshot.memory_used_percent else {
            return BarModuleState::Stale;
        };
        if self
            .memory_critical_percent
            .is_some_and(|threshold| memory_used_percent >= threshold)
        {
            BarModuleState::Critical
        } else if self
            .memory_warning_percent
            .is_some_and(|threshold| memory_used_percent >= threshold)
        {
            BarModuleState::Warning
        } else {
            BarModuleState::Ok
        }
    }
}

impl BarModule for SystemModule {
    fn id(&self) -> &str {
        &self.id
    }

    fn module_type(&self) -> &str {
        &self.module_type
    }

    fn update(&mut self, ctx: &BarRuntimeContext) -> BarModuleViewModel {
        let snapshot = read_system_snapshot(ctx);
        let label = self.render_snapshot(&snapshot);
        let state = self.state_for_snapshot(&snapshot);
        BarModuleViewModel {
            id: self.id.clone(),
            module_type: self.module_type.clone(),
            label,
            tooltip: Some(format!(
                "load={:?} memory={:?}%",
                snapshot.load1, snapshot.memory_used_percent
            )),
            state,
            visible: true,
            last_error: if state == BarModuleState::Stale {
                Some("system data partially unavailable".to_string())
            } else {
                None
            },
        }
    }
}

fn percent_extra(spec: &BarModuleSpec, key: &str) -> Option<u8> {
    spec.extra
        .get(key)
        .and_then(toml::Value::as_integer)
        .and_then(|value| u8::try_from(value).ok())
        .filter(|value| *value <= 100)
}

fn read_system_snapshot(ctx: &BarRuntimeContext) -> SystemSnapshot {
    let proc_root = ctx
        .env
        .get("UNILII_BAR_PROC_ROOT")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from("/proc"));
    SystemSnapshot {
        load1: read_load1(&proc_root.join("loadavg")),
        memory_used_percent: read_memory_used_percent(&proc_root.join("meminfo")),
    }
}

fn read_load1(path: &std::path::Path) -> Option<String> {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|contents| contents.split_whitespace().next().map(str::to_string))
}

fn read_memory_used_percent(path: &std::path::Path) -> Option<u8> {
    let contents = std::fs::read_to_string(path).ok()?;
    let mut total_kb = None;
    let mut available_kb = None;
    for line in contents.lines() {
        if let Some(value) = parse_meminfo_kb(line, "MemTotal:") {
            total_kb = Some(value);
        } else if let Some(value) = parse_meminfo_kb(line, "MemAvailable:") {
            available_kb = Some(value);
        }
    }
    let total_kb: u64 = total_kb?;
    let available_kb: u64 = available_kb?;
    if total_kb == 0 || available_kb > total_kb {
        return None;
    }
    let used = total_kb - available_kb;
    Some(((used * 100) / total_kb).min(100) as u8)
}

fn parse_meminfo_kb(line: &str, key: &str) -> Option<u64> {
    let rest = line.strip_prefix(key)?.trim();
    rest.split_whitespace().next()?.parse().ok()
}

fn string_extra(spec: &BarModuleSpec, key: &str) -> Option<String> {
    spec.extra
        .get(key)
        .and_then(toml::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn backend_extra(spec: &BarModuleSpec) -> Option<String> {
    string_extra(spec, "backend").map(|backend| backend.to_ascii_lowercase())
}

fn default_workspaces_command_for_backend(backend: &str) -> Option<String> {
    match backend {
        "i3" => Some("i3-msg -t get_workspaces".to_string()),
        "sway" => Some("swaymsg -t get_workspaces".to_string()),
        _ => None,
    }
}

fn default_tree_command_for_backend(backend: &str) -> Option<String> {
    match backend {
        "i3" => Some("i3-msg -t get_tree".to_string()),
        "sway" => Some("swaymsg -t get_tree".to_string()),
        _ => None,
    }
}

fn default_switch_template_for_backend(backend: &str) -> Option<String> {
    match backend {
        "i3" => Some("i3-msg workspace -- {workspace_shell}".to_string()),
        "sway" => Some("swaymsg workspace -- {workspace_shell}".to_string()),
        _ => None,
    }
}

fn string_list_extra(spec: &BarModuleSpec, key: &str) -> Vec<String> {
    match spec.extra.get(key) {
        Some(toml::Value::Array(values)) => values
            .iter()
            .filter_map(toml::Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned)
            .collect(),
        Some(toml::Value::String(value)) => value
            .split(',')
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned)
            .collect(),
        _ => Vec::new(),
    }
}

fn runtime_root(ctx: &BarRuntimeContext, env_key: &str, default: &str) -> std::path::PathBuf {
    ctx.env
        .get(env_key)
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from(default))
}

fn read_trimmed(path: &std::path::Path) -> Option<String> {
    std::fs::read_to_string(path)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct NetworkSnapshot {
    interface: String,
    state: String,
    address: Option<String>,
    ip_address: Option<String>,
    ssid: Option<String>,
}

struct NetworkModule {
    id: String,
    module_type: String,
    format: String,
    interface: Option<String>,
    status_file: Option<String>,
    ip_command: Option<String>,
    ssid_command: Option<String>,
}

impl NetworkModule {
    fn new(spec: &BarModuleSpec) -> Self {
        Self {
            id: spec.id.clone(),
            module_type: spec.module_type.clone(),
            format: spec
                .format
                .clone()
                .unwrap_or_else(|| "{interface} {state}".to_string()),
            interface: string_extra(spec, "interface"),
            status_file: string_extra(spec, "status_file"),
            ip_command: string_extra(spec, "ip_command"),
            ssid_command: string_extra(spec, "ssid_command"),
        }
    }

    fn render_snapshot(&self, snapshot: &NetworkSnapshot) -> String {
        self.format
            .replace("{interface}", &snapshot.interface)
            .replace("{state}", &snapshot.state)
            .replace("{address}", snapshot.address.as_deref().unwrap_or(""))
            .replace("{ip}", snapshot.ip_address.as_deref().unwrap_or(""))
            .replace("{ssid}", snapshot.ssid.as_deref().unwrap_or(""))
    }
}

impl BarModule for NetworkModule {
    fn id(&self) -> &str {
        &self.id
    }

    fn module_type(&self) -> &str {
        &self.module_type
    }

    fn update(&mut self, ctx: &BarRuntimeContext) -> BarModuleViewModel {
        let root = runtime_root(ctx, "UNILII_BAR_SYS_CLASS_NET", "/sys/class/net");
        match read_network_snapshot(
            ctx,
            &root,
            self.interface.as_deref(),
            self.status_file.as_deref(),
            self.ip_command.as_deref(),
            self.ssid_command.as_deref(),
        ) {
            Some(snapshot) => {
                let state = if snapshot.state == "up" {
                    BarModuleState::Ok
                } else {
                    BarModuleState::Disconnected
                };
                BarModuleViewModel {
                    id: self.id.clone(),
                    module_type: self.module_type.clone(),
                    label: self.render_snapshot(&snapshot),
                    tooltip: Some(format!(
                        "interface={} state={} address={} ip={} ssid={}",
                        snapshot.interface,
                        snapshot.state,
                        snapshot.address.as_deref().unwrap_or("unknown"),
                        snapshot.ip_address.as_deref().unwrap_or("unknown"),
                        snapshot.ssid.as_deref().unwrap_or("unknown")
                    )),
                    state,
                    visible: true,
                    last_error: None,
                }
            }
            None => BarModuleViewModel::unavailable(
                &self.id,
                &self.module_type,
                "no usable network interface found",
            ),
        }
    }
}

fn read_network_snapshot(
    ctx: &BarRuntimeContext,
    root: &std::path::Path,
    preferred: Option<&str>,
    status_file: Option<&str>,
    ip_command: Option<&str>,
    ssid_command: Option<&str>,
) -> Option<NetworkSnapshot> {
    let interface = preferred
        .map(ToOwned::to_owned)
        .or_else(|| first_interface(root, |name| name != "lo"))?;
    let dir = root.join(&interface);
    let state = read_trimmed(&dir.join("operstate")).unwrap_or_else(|| "unknown".to_string());
    let address = read_trimmed(&dir.join("address"));
    let status_values = read_network_status_values(ctx, status_file);
    let ip_address = read_trimmed(&dir.join("ipv4"))
        .or_else(|| read_trimmed(&dir.join("ip_address")))
        .or_else(|| network_status_value(&status_values, "ip"))
        .or_else(|| ctx.env.get("UNILII_BAR_NETWORK_IP").cloned())
        .or_else(|| read_network_command_value(ctx, ip_command, "UNILII_BAR_NETWORK_IP_COMMAND"));
    let ssid = read_trimmed(&dir.join("ssid"))
        .or_else(|| network_status_value(&status_values, "ssid"))
        .or_else(|| ctx.env.get("UNILII_BAR_NETWORK_SSID").cloned())
        .or_else(|| {
            read_network_command_value(ctx, ssid_command, "UNILII_BAR_NETWORK_SSID_COMMAND")
        });
    Some(NetworkSnapshot {
        interface,
        state,
        address,
        ip_address,
        ssid,
    })
}

fn read_network_status_values(
    ctx: &BarRuntimeContext,
    status_file: Option<&str>,
) -> HashMap<String, String> {
    if let Some(input) = ctx.env.get("UNILII_BAR_NETWORK_STATUS") {
        return parse_network_status_values(input);
    }
    let status_file = status_file.or_else(|| {
        ctx.env
            .get("UNILII_BAR_NETWORK_STATUS_FILE")
            .map(String::as_str)
    });
    status_file
        .and_then(|path| std::fs::read_to_string(path).ok())
        .map(|input| parse_network_status_values(&input))
        .unwrap_or_default()
}

fn parse_network_status_values(input: &str) -> HashMap<String, String> {
    input
        .split(|ch: char| ch.is_whitespace() || ch == ',' || ch == ';')
        .filter_map(|token| token.split_once('='))
        .map(|(key, value)| (key.trim().to_ascii_lowercase(), value.trim().to_string()))
        .filter(|(key, value)| !key.is_empty() && !value.is_empty())
        .collect()
}

fn network_status_value(values: &HashMap<String, String>, key: &str) -> Option<String> {
    values.get(key).cloned().filter(|value| !value.is_empty())
}

fn read_network_command_value(
    ctx: &BarRuntimeContext,
    configured_command: Option<&str>,
    env_key: &str,
) -> Option<String> {
    let command = configured_command.or_else(|| ctx.env.get(env_key).map(String::as_str))?;
    let output = run_script_command(
        command,
        Duration::from_millis(DEFAULT_SCRIPT_TIMEOUT_MS),
        DEFAULT_SCRIPT_OUTPUT_LIMIT,
        &ctx.env,
    )
    .ok()?;
    if output.timed_out || output.exit_code != Some(0) {
        return None;
    }
    output
        .stdout
        .lines()
        .next()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn first_interface(root: &std::path::Path, predicate: impl Fn(&str) -> bool) -> Option<String> {
    let entries = std::fs::read_dir(root).ok()?;
    let mut names = entries
        .filter_map(Result::ok)
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|name| predicate(name))
        .collect::<Vec<_>>();
    names.sort();
    names.into_iter().next()
}

struct VpnModule {
    id: String,
    module_type: String,
    format: String,
    interfaces: Vec<String>,
}

impl VpnModule {
    fn new(spec: &BarModuleSpec) -> Self {
        Self {
            id: spec.id.clone(),
            module_type: spec.module_type.clone(),
            format: spec
                .format
                .clone()
                .unwrap_or_else(|| "vpn {interface}".to_string()),
            interfaces: string_list_extra(spec, "interfaces"),
        }
    }
}

impl BarModule for VpnModule {
    fn id(&self) -> &str {
        &self.id
    }

    fn module_type(&self) -> &str {
        &self.module_type
    }

    fn update(&mut self, ctx: &BarRuntimeContext) -> BarModuleViewModel {
        let root = runtime_root(ctx, "UNILII_BAR_SYS_CLASS_NET", "/sys/class/net");
        match read_vpn_interface(&root, &self.interfaces) {
            Some(snapshot) if snapshot.state == "up" || snapshot.state == "unknown" => {
                BarModuleViewModel {
                    id: self.id.clone(),
                    module_type: self.module_type.clone(),
                    label: self
                        .format
                        .replace("{interface}", &snapshot.interface)
                        .replace("{state}", &snapshot.state),
                    tooltip: Some(format!(
                        "vpn interface {} is {}",
                        snapshot.interface, snapshot.state
                    )),
                    state: BarModuleState::Ok,
                    visible: true,
                    last_error: None,
                }
            }
            Some(snapshot) => BarModuleViewModel {
                id: self.id.clone(),
                module_type: self.module_type.clone(),
                label: "vpn down".to_string(),
                tooltip: Some(format!(
                    "vpn interface {} is {}",
                    snapshot.interface, snapshot.state
                )),
                state: BarModuleState::Disconnected,
                visible: true,
                last_error: None,
            },
            None => BarModuleViewModel {
                id: self.id.clone(),
                module_type: self.module_type.clone(),
                label: "vpn down".to_string(),
                tooltip: Some("no vpn/tunnel interface found".to_string()),
                state: BarModuleState::Disconnected,
                visible: true,
                last_error: None,
            },
        }
    }
}

fn read_vpn_interface(root: &std::path::Path, configured: &[String]) -> Option<NetworkSnapshot> {
    let name = if configured.is_empty() {
        first_interface(root, is_vpn_interface_name)
    } else {
        configured
            .iter()
            .find(|name| root.join(name.as_str()).exists())
            .cloned()
    }?;
    read_network_snapshot(
        &BarRuntimeContext::default(),
        root,
        Some(&name),
        None,
        None,
        None,
    )
}

fn is_vpn_interface_name(name: &str) -> bool {
    name.starts_with("tun")
        || name.starts_with("tap")
        || name.starts_with("wg")
        || name.starts_with("ppp")
        || name.starts_with("tailscale")
        || name.starts_with("zt")
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct AudioSnapshot {
    volume_percent: u8,
    muted: bool,
    device: Option<String>,
}

struct AudioModule {
    id: String,
    module_type: String,
    format: String,
}

impl AudioModule {
    fn new(spec: &BarModuleSpec) -> Self {
        Self {
            id: spec.id.clone(),
            module_type: spec.module_type.clone(),
            format: spec
                .format
                .clone()
                .unwrap_or_else(|| "vol {volume}%".to_string()),
        }
    }
}

impl BarModule for AudioModule {
    fn id(&self) -> &str {
        &self.id
    }

    fn module_type(&self) -> &str {
        &self.module_type
    }

    fn update(&mut self, ctx: &BarRuntimeContext) -> BarModuleViewModel {
        match read_audio_snapshot(ctx) {
            Some(snapshot) => BarModuleViewModel {
                id: self.id.clone(),
                module_type: self.module_type.clone(),
                label: self
                    .format
                    .replace("{volume}", &snapshot.volume_percent.to_string())
                    .replace("{muted}", if snapshot.muted { "muted" } else { "unmuted" })
                    .replace("{device}", snapshot.device.as_deref().unwrap_or("")),
                tooltip: Some(format!(
                    "volume={} muted={} device={}",
                    snapshot.volume_percent,
                    snapshot.muted,
                    snapshot.device.as_deref().unwrap_or("unknown")
                )),
                state: if snapshot.muted {
                    BarModuleState::Muted
                } else {
                    BarModuleState::Ok
                },
                visible: true,
                last_error: None,
            },
            None => BarModuleViewModel::unavailable(
                &self.id,
                &self.module_type,
                "audio status unavailable; set UNILII_BAR_AUDIO_STATUS or install a supported backend",
            ),
        }
    }
}

fn read_audio_snapshot(ctx: &BarRuntimeContext) -> Option<AudioSnapshot> {
    ctx.env
        .get("UNILII_BAR_AUDIO_STATUS")
        .and_then(|status| parse_audio_status(status))
}

fn parse_audio_status(input: &str) -> Option<AudioSnapshot> {
    let lower = input.to_ascii_lowercase();
    let muted =
        lower.contains("muted") || lower.contains("mute=true") || lower.contains("muted=true");
    let mut volume_percent = None;
    for token in input.split(|ch: char| ch.is_whitespace() || ch == ',' || ch == ';') {
        let token = token.trim_matches(|ch: char| ch == '[' || ch == ']' || ch == '(' || ch == ')');
        if let Some(value) = token.strip_suffix('%') {
            volume_percent = value.parse::<u8>().ok();
            if volume_percent.is_some() {
                break;
            }
        }
        if let Some(value) = token
            .strip_prefix("volume=")
            .or_else(|| token.strip_prefix("vol="))
        {
            volume_percent = value.trim_end_matches('%').parse::<u8>().ok();
            if volume_percent.is_some() {
                break;
            }
        }
        if let Some(value) = token.strip_prefix("Volume:") {
            volume_percent = parse_audio_volume_value(value);
            if volume_percent.is_some() {
                break;
            }
        }
    }
    if volume_percent.is_none() {
        for token in input.split_whitespace() {
            if let Some(value) = token.strip_prefix("0.") {
                let parsed = format!("0.{value}").parse::<f32>().ok()?;
                volume_percent = Some((parsed * 100.0).round().clamp(0.0, 100.0) as u8);
                break;
            }
        }
    }
    Some(AudioSnapshot {
        volume_percent: volume_percent?,
        muted,
        device: key_value(input, "device").or_else(|| key_value(input, "sink")),
    })
}

fn parse_audio_volume_value(value: &str) -> Option<u8> {
    if let Some(percent) = value.trim().strip_suffix('%') {
        return percent.parse::<u8>().ok();
    }
    let parsed = value.trim().parse::<f32>().ok()?;
    Some((parsed * 100.0).round().clamp(0.0, 100.0) as u8)
}

fn key_value(input: &str, key: &str) -> Option<String> {
    input
        .split(|ch: char| ch.is_whitespace() || ch == ',' || ch == ';')
        .find_map(|token| token.strip_prefix(&format!("{key}=")))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct BatterySnapshot {
    name: String,
    percentage: u8,
    status: String,
}

struct BatteryModule {
    id: String,
    module_type: String,
    format: String,
    warning_percent: u8,
    critical_percent: u8,
}

impl BatteryModule {
    fn new(spec: &BarModuleSpec) -> Self {
        Self {
            id: spec.id.clone(),
            module_type: spec.module_type.clone(),
            format: spec
                .format
                .clone()
                .unwrap_or_else(|| "{percentage}% {state}".to_string()),
            warning_percent: percent_extra(spec, "warning_percent").unwrap_or(20),
            critical_percent: percent_extra(spec, "critical_percent").unwrap_or(10),
        }
    }
}

impl BarModule for BatteryModule {
    fn id(&self) -> &str {
        &self.id
    }

    fn module_type(&self) -> &str {
        &self.module_type
    }

    fn update(&mut self, ctx: &BarRuntimeContext) -> BarModuleViewModel {
        let root = runtime_root(
            ctx,
            "UNILII_BAR_POWER_SUPPLY_ROOT",
            "/sys/class/power_supply",
        );
        match read_battery_snapshot(&root) {
            Some(snapshot) => {
                let lower_status = snapshot.status.to_ascii_lowercase();
                let state = if lower_status == "charging" || lower_status == "full" {
                    BarModuleState::Charging
                } else if snapshot.percentage <= self.critical_percent {
                    BarModuleState::Critical
                } else if snapshot.percentage <= self.warning_percent {
                    BarModuleState::Warning
                } else {
                    BarModuleState::Ok
                };
                BarModuleViewModel {
                    id: self.id.clone(),
                    module_type: self.module_type.clone(),
                    label: self
                        .format
                        .replace("{percentage}", &snapshot.percentage.to_string())
                        .replace("{state}", &snapshot.status)
                        .replace("{name}", &snapshot.name),
                    tooltip: Some(format!(
                        "battery={} percentage={} status={}",
                        snapshot.name, snapshot.percentage, snapshot.status
                    )),
                    state,
                    visible: true,
                    last_error: None,
                }
            }
            None => BarModuleViewModel::unavailable(
                &self.id,
                &self.module_type,
                "no battery power_supply found",
            ),
        }
    }
}

fn read_battery_snapshot(root: &std::path::Path) -> Option<BatterySnapshot> {
    let entries = std::fs::read_dir(root).ok()?;
    let mut names = entries
        .filter_map(Result::ok)
        .filter_map(|entry| entry.file_name().into_string().ok())
        .collect::<Vec<_>>();
    names.sort();
    for name in names {
        let dir = root.join(&name);
        let power_type = read_trimmed(&dir.join("type"));
        if power_type.as_deref() != Some("Battery") && !name.starts_with("BAT") {
            continue;
        }
        let percentage = read_trimmed(&dir.join("capacity"))?.parse::<u8>().ok()?;
        let status = read_trimmed(&dir.join("status")).unwrap_or_else(|| "Unknown".to_string());
        return Some(BatterySnapshot {
            name,
            percentage: percentage.min(100),
            status,
        });
    }
    None
}

struct NotificationsModule {
    id: String,
    module_type: String,
    format: String,
    source_file: Option<String>,
}

impl NotificationsModule {
    fn new(spec: &BarModuleSpec) -> Self {
        Self {
            id: spec.id.clone(),
            module_type: spec.module_type.clone(),
            format: spec
                .format
                .clone()
                .unwrap_or_else(|| "notify {count}".to_string()),
            source_file: string_extra(spec, "source_file"),
        }
    }
}

impl BarModule for NotificationsModule {
    fn id(&self) -> &str {
        &self.id
    }

    fn module_type(&self) -> &str {
        &self.module_type
    }

    fn update(&mut self, ctx: &BarRuntimeContext) -> BarModuleViewModel {
        let count = read_notification_count(ctx, self.source_file.as_deref());
        match count {
            Some(count) => BarModuleViewModel {
                id: self.id.clone(),
                module_type: self.module_type.clone(),
                label: self
                    .format
                    .replace("{count}", &count.to_string())
                    .replace("{state}", if count == 0 { "empty" } else { "active" }),
                tooltip: Some(format!("notification count={count}")),
                state: if count == 0 {
                    BarModuleState::Empty
                } else {
                    BarModuleState::Active
                },
                visible: true,
                last_error: None,
            },
            None => BarModuleViewModel::unavailable(
                &self.id,
                &self.module_type,
                "notification count unavailable; set UNILII_BAR_NOTIFICATION_COUNT or source_file",
            ),
        }
    }
}

fn read_notification_count(ctx: &BarRuntimeContext, source_file: Option<&str>) -> Option<u32> {
    if let Some(value) = ctx.env.get("UNILII_BAR_NOTIFICATION_COUNT") {
        return parse_notification_count(value);
    }
    let source_file = source_file.or_else(|| {
        ctx.env
            .get("UNILII_BAR_NOTIFICATION_FILE")
            .map(String::as_str)
    })?;
    let contents = std::fs::read_to_string(source_file).ok()?;
    parse_notification_count(&contents)
}

fn parse_notification_count(input: &str) -> Option<u32> {
    let trimmed = input.trim();
    if let Ok(count) = trimmed.parse::<u32>() {
        return Some(count);
    }
    for token in trimmed.split(|ch: char| ch.is_whitespace() || ch == ',' || ch == ';') {
        if let Some(value) = token.strip_prefix("count=") {
            return value.parse::<u32>().ok();
        }
    }
    None
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct WorkspaceSnapshot {
    name: String,
    active: bool,
    urgent: bool,
    visible: bool,
}

struct WorkspacesModule {
    id: String,
    module_type: String,
    format_active: String,
    format_inactive: String,
    separator: String,
    command: Option<String>,
}

impl WorkspacesModule {
    fn new(spec: &BarModuleSpec) -> Self {
        Self {
            id: spec.id.clone(),
            module_type: spec.module_type.clone(),
            format_active: string_extra(spec, "format_active")
                .unwrap_or_else(|| "[{name}]".to_string()),
            format_inactive: string_extra(spec, "format_inactive")
                .unwrap_or_else(|| "{name}".to_string()),
            separator: string_extra(spec, "separator").unwrap_or_else(|| " ".to_string()),
            command: spec.command.clone().or_else(|| {
                string_extra(spec, "workspaces_command").or_else(|| {
                    backend_extra(spec)
                        .as_deref()
                        .and_then(default_workspaces_command_for_backend)
                })
            }),
        }
    }
}

impl BarModule for WorkspacesModule {
    fn id(&self) -> &str {
        &self.id
    }

    fn module_type(&self) -> &str {
        &self.module_type
    }

    fn update(&mut self, ctx: &BarRuntimeContext) -> BarModuleViewModel {
        match read_workspaces(ctx, self.command.as_deref()) {
            Some(workspaces) if !workspaces.is_empty() => {
                let mut state = BarModuleState::Ok;
                let labels = workspaces
                    .iter()
                    .map(|workspace| {
                        if workspace.urgent {
                            state = BarModuleState::Urgent;
                        } else if workspace.active && state != BarModuleState::Urgent {
                            state = BarModuleState::Active;
                        } else if workspace.visible && state == BarModuleState::Ok {
                            state = BarModuleState::Visible;
                        }
                        let format = if workspace.active {
                            &self.format_active
                        } else {
                            &self.format_inactive
                        };
                        format
                            .replace("{name}", &workspace.name)
                            .replace("{state}", workspace_state_name(workspace))
                    })
                    .collect::<Vec<_>>();
                BarModuleViewModel {
                    id: self.id.clone(),
                    module_type: self.module_type.clone(),
                    label: labels.join(&self.separator),
                    tooltip: Some(format!("{} workspaces", workspaces.len())),
                    state,
                    visible: true,
                    last_error: None,
                }
            }
            _ => BarModuleViewModel::unavailable(
                &self.id,
                &self.module_type,
                "workspace snapshot unavailable; set UNILII_BAR_WORKSPACES",
            ),
        }
    }
}

fn workspace_state_name(workspace: &WorkspaceSnapshot) -> &'static str {
    if workspace.urgent {
        "urgent"
    } else if workspace.active {
        "active"
    } else if workspace.visible {
        "visible"
    } else {
        "inactive"
    }
}

fn read_workspaces(
    ctx: &BarRuntimeContext,
    command: Option<&str>,
) -> Option<Vec<WorkspaceSnapshot>> {
    if let Some(input) = ctx.env.get("UNILII_BAR_WORKSPACES") {
        return parse_workspace_snapshot(input);
    }
    if let Some(input) = ctx.env.get("UNILII_BAR_I3_WORKSPACES_JSON") {
        return parse_i3_workspaces_json(input);
    }
    if let Some(path) = ctx.env.get("UNILII_BAR_I3_WORKSPACES_FILE") {
        return std::fs::read_to_string(path)
            .ok()
            .and_then(|input| parse_i3_workspaces_json(&input));
    }
    command
        .and_then(|command| run_wm_json_command(command, ctx))
        .and_then(|input| parse_i3_workspaces_json(&input))
}

fn parse_workspace_snapshot(input: &str) -> Option<Vec<WorkspaceSnapshot>> {
    let mut result = Vec::new();
    for raw in input.split(',') {
        let raw = raw.trim();
        if raw.is_empty() {
            continue;
        }
        let mut parts = raw.split(':');
        let name = parts.next()?.trim();
        if name.is_empty() {
            continue;
        }
        let flags = parts.next().unwrap_or("");
        result.push(WorkspaceSnapshot {
            name: name.to_string(),
            active: flags.contains('A') || flags.contains('*'),
            urgent: flags.contains('U') || flags.contains('!'),
            visible: flags.contains('V'),
        });
    }
    if result.is_empty() {
        None
    } else {
        Some(result)
    }
}

fn parse_i3_workspaces_json(input: &str) -> Option<Vec<WorkspaceSnapshot>> {
    let value: serde_json::Value = serde_json::from_str(input).ok()?;
    let array = value.as_array()?;
    let mut result = Vec::new();
    for item in array {
        let name = item.get("name").and_then(serde_json::Value::as_str)?;
        if name.trim().is_empty() {
            continue;
        }
        result.push(WorkspaceSnapshot {
            name: name.to_string(),
            active: item
                .get("focused")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false),
            urgent: item
                .get("urgent")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false),
            visible: item
                .get("visible")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false),
        });
    }
    if result.is_empty() {
        None
    } else {
        Some(result)
    }
}

struct WindowTitleModule {
    id: String,
    module_type: String,
    format: String,
    empty_label: String,
    max_len: Option<usize>,
    command: Option<String>,
}

impl WindowTitleModule {
    fn new(spec: &BarModuleSpec) -> Self {
        Self {
            id: spec.id.clone(),
            module_type: spec.module_type.clone(),
            format: spec.format.clone().unwrap_or_else(|| "{title}".to_string()),
            empty_label: string_extra(spec, "empty_label").unwrap_or_default(),
            max_len: spec
                .extra
                .get("max_len")
                .and_then(toml::Value::as_integer)
                .and_then(|value| usize::try_from(value).ok())
                .filter(|value| *value > 0),
            command: spec.command.clone().or_else(|| {
                string_extra(spec, "tree_command").or_else(|| {
                    backend_extra(spec)
                        .as_deref()
                        .and_then(default_tree_command_for_backend)
                })
            }),
        }
    }
}

impl BarModule for WindowTitleModule {
    fn id(&self) -> &str {
        &self.id
    }

    fn module_type(&self) -> &str {
        &self.module_type
    }

    fn update(&mut self, ctx: &BarRuntimeContext) -> BarModuleViewModel {
        let title = read_window_title(ctx, self.command.as_deref())
            .map(|value| truncate_chars(value.trim(), self.max_len))
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| self.empty_label.clone());
        BarModuleViewModel {
            id: self.id.clone(),
            module_type: self.module_type.clone(),
            label: self.format.replace("{title}", &title),
            tooltip: if title.is_empty() {
                None
            } else {
                Some(title.clone())
            },
            state: if title.is_empty() {
                BarModuleState::Empty
            } else {
                BarModuleState::Ok
            },
            visible: true,
            last_error: None,
        }
    }
}

fn read_window_title(ctx: &BarRuntimeContext, command: Option<&str>) -> Option<String> {
    if let Some(title) = ctx.env.get("UNILII_BAR_WINDOW_TITLE") {
        return Some(title.clone());
    }
    if let Some(input) = ctx.env.get("UNILII_BAR_I3_TREE_JSON") {
        return parse_i3_tree_focused_title(input);
    }
    if let Some(path) = ctx.env.get("UNILII_BAR_I3_TREE_FILE") {
        return std::fs::read_to_string(path)
            .ok()
            .and_then(|input| parse_i3_tree_focused_title(&input));
    }
    command
        .and_then(|command| run_wm_json_command(command, ctx))
        .and_then(|input| parse_i3_tree_focused_title(&input))
}

fn run_wm_json_command(command: &str, ctx: &BarRuntimeContext) -> Option<String> {
    let output = run_script_command(
        command,
        Duration::from_millis(DEFAULT_SCRIPT_TIMEOUT_MS),
        DEFAULT_SCRIPT_OUTPUT_LIMIT,
        &ctx.env,
    )
    .ok()?;
    if output.timed_out || output.exit_code != Some(0) {
        return None;
    }
    Some(output.stdout)
}

fn parse_i3_tree_focused_title(input: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(input).ok()?;
    focused_title_from_i3_node(&value)
}

fn focused_title_from_i3_node(node: &serde_json::Value) -> Option<String> {
    if node
        .get("focused")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false)
    {
        return node
            .get("window_properties")
            .and_then(|properties| properties.get("title"))
            .and_then(serde_json::Value::as_str)
            .or_else(|| node.get("name").and_then(serde_json::Value::as_str))
            .map(ToOwned::to_owned);
    }
    for key in ["nodes", "floating_nodes"] {
        if let Some(children) = node.get(key).and_then(serde_json::Value::as_array) {
            for child in children {
                if let Some(title) = focused_title_from_i3_node(child) {
                    return Some(title);
                }
            }
        }
    }
    None
}

fn truncate_chars(input: &str, max_len: Option<usize>) -> String {
    let Some(max_len) = max_len else {
        return input.to_string();
    };
    if input.chars().count() <= max_len {
        return input.to_string();
    }
    let mut output = input
        .chars()
        .take(max_len.saturating_sub(1))
        .collect::<String>();
    output.push('…');
    output
}

struct StaticProbeModule {
    id: String,
    module_type: String,
    label: String,
    unavailable_message: String,
}

impl StaticProbeModule {
    fn new(spec: &BarModuleSpec, label: &str, unavailable_message: &str) -> Self {
        Self {
            id: spec.id.clone(),
            module_type: spec.module_type.clone(),
            label: label.to_string(),
            unavailable_message: unavailable_message.to_string(),
        }
    }
}

impl BarModule for StaticProbeModule {
    fn id(&self) -> &str {
        &self.id
    }

    fn module_type(&self) -> &str {
        &self.module_type
    }

    fn update(&mut self, _ctx: &BarRuntimeContext) -> BarModuleViewModel {
        let mut model = BarModuleViewModel::unavailable(
            &self.id,
            &self.module_type,
            self.unavailable_message.clone(),
        );
        model.label = self.label.clone();
        model
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bar::{parse_bar_config_str, starter_bar_config_toml};

    #[test]
    fn builds_graph_from_starter_config() {
        let config = parse_bar_config_str(starter_bar_config_toml()).unwrap();
        let graph = BarModuleGraph::from_config(&config).unwrap();
        assert_eq!(graph.len(), 8);
    }

    #[test]
    fn update_all_returns_one_model_per_enabled_module() {
        let config = parse_bar_config_str(starter_bar_config_toml()).unwrap();
        let mut graph = BarModuleGraph::from_config(&config).unwrap();
        let models = graph.update_all(&BarRuntimeContext::at_unix_timestamp(0));
        assert_eq!(models.len(), 8);
        assert!(models.iter().any(|model| model.id == "clock"));
    }

    #[test]
    fn disabled_modules_are_not_loaded() {
        let config = parse_bar_config_str(
            r#"
            [layout]
            left = []
            center = []
            right = []

            [[module]]
            id = "clock"
            type = "clock"
            enabled = false
            "#,
        )
        .unwrap();
        let graph = BarModuleGraph::from_config(&config).unwrap();
        assert!(graph.is_empty());
    }

    #[test]
    fn script_module_executes_command_and_trims_output() {
        let config = parse_bar_config_str(
            r#"
            [layout]
            left = ["script"]
            center = []
            right = []

            [[module]]
            id = "script"
            type = "script"
            command = "printf 'hello\n'"
            timeout_ms = 1000
            max_output_bytes = 64
            "#,
        )
        .unwrap();
        let mut graph = BarModuleGraph::from_config(&config).unwrap();
        let models = graph.update_all(&BarRuntimeContext::at_unix_timestamp(0));
        assert_eq!(models[0].state, BarModuleState::Ok);
        assert_eq!(models[0].label, "hello");
    }

    #[test]
    fn script_module_reports_non_zero_exit() {
        let config = parse_bar_config_str(
            r#"
            [layout]
            left = ["script"]
            center = []
            right = []

            [[module]]
            id = "script"
            type = "script"
            command = "printf fail >&2; exit 7"
            timeout_ms = 1000
            format = "code={exit_code} err={stderr}"
            "#,
        )
        .unwrap();
        let mut graph = BarModuleGraph::from_config(&config).unwrap();
        let models = graph.update_all(&BarRuntimeContext::at_unix_timestamp(0));
        assert_eq!(models[0].state, BarModuleState::Warning);
        assert_eq!(models[0].label, "code=7 err=fail");
        assert!(models[0].last_error.as_deref().unwrap().contains("7"));
    }

    #[test]
    fn script_module_times_out_without_blocking_until_command_finishes() {
        let started = std::time::Instant::now();
        let config = parse_bar_config_str(
            r#"
            [layout]
            left = ["script"]
            center = []
            right = []

            [[module]]
            id = "script"
            type = "script"
            command = "sleep 2; echo late"
            timeout_ms = 50
            "#,
        )
        .unwrap();
        let mut graph = BarModuleGraph::from_config(&config).unwrap();
        let models = graph.update_all(&BarRuntimeContext::at_unix_timestamp(0));
        assert!(started.elapsed() < Duration::from_secs(1));
        assert_eq!(models[0].state, BarModuleState::Critical);
        assert!(
            models[0]
                .last_error
                .as_deref()
                .unwrap()
                .contains("timed out")
        );
    }
    #[test]
    fn system_module_reads_fixture_load_and_memory() {
        let proc_root = write_proc_fixture(
            "system-ok",
            "0.42 0.12 0.01 1/100 42\n",
            "MemTotal: 1000 kB\nMemAvailable: 750 kB\n",
        );
        let config = parse_bar_config_str(
            r#"
            [layout]
            left = ["system"]
            center = []
            right = []

            [[module]]
            id = "system"
            type = "system"
            format = "L={load1} M={memory_used_percent}%"
            memory_warning_percent = 80
            memory_critical_percent = 90
            "#,
        )
        .unwrap();
        let mut graph = BarModuleGraph::from_config(&config).unwrap();
        let mut ctx = BarRuntimeContext::at_unix_timestamp(0);
        ctx.env.insert(
            "UNILII_BAR_PROC_ROOT".to_string(),
            proc_root.display().to_string(),
        );
        let models = graph.update_all(&ctx);
        assert_eq!(models[0].label, "L=0.42 M=25%");
        assert_eq!(models[0].state, BarModuleState::Ok);
    }

    #[test]
    fn system_module_applies_memory_threshold_states() {
        let proc_root = write_proc_fixture(
            "system-critical",
            "1.00 0.50 0.25 1/100 42\n",
            "MemTotal: 1000 kB\nMemAvailable: 50 kB\n",
        );
        let config = parse_bar_config_str(
            r#"
            [layout]
            left = ["system"]
            center = []
            right = []

            [[module]]
            id = "system"
            type = "system"
            memory_warning_percent = 80
            memory_critical_percent = 90
            "#,
        )
        .unwrap();
        let mut graph = BarModuleGraph::from_config(&config).unwrap();
        let mut ctx = BarRuntimeContext::at_unix_timestamp(0);
        ctx.env.insert(
            "UNILII_BAR_PROC_ROOT".to_string(),
            proc_root.display().to_string(),
        );
        let models = graph.update_all(&ctx);
        assert_eq!(models[0].state, BarModuleState::Critical);
        assert!(models[0].label.contains("95%"));
    }

    #[test]
    fn system_module_reports_stale_when_fixture_is_incomplete() {
        let proc_root = write_proc_fixture(
            "system-stale",
            "2.00 1.00 0.50 1/100 42\n",
            "MemTotal: 1000 kB\n",
        );
        let config = parse_bar_config_str(
            r#"
            [layout]
            left = ["system"]
            center = []
            right = []

            [[module]]
            id = "system"
            type = "system"
            "#,
        )
        .unwrap();
        let mut graph = BarModuleGraph::from_config(&config).unwrap();
        let mut ctx = BarRuntimeContext::at_unix_timestamp(0);
        ctx.env.insert(
            "UNILII_BAR_PROC_ROOT".to_string(),
            proc_root.display().to_string(),
        );
        let models = graph.update_all(&ctx);
        assert_eq!(models[0].state, BarModuleState::Stale);
        assert!(models[0].last_error.is_some());
    }

    fn write_proc_fixture(name: &str, loadavg: &str, meminfo: &str) -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!("unilii-bar-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("loadavg"), loadavg).unwrap();
        std::fs::write(root.join("meminfo"), meminfo).unwrap();
        root
    }
    #[test]
    fn network_module_reads_fixture_interface_state() {
        let net_root = write_net_fixture(&[("eth0", "up", "aa:bb:cc:dd:ee:ff")]);
        let config = parse_bar_config_str(
            r#"
            [layout]
            left = ["network"]
            center = []
            right = []

            [[module]]
            id = "network"
            type = "network"
            format = "{interface}:{state}:{address}"
            "#,
        )
        .unwrap();
        let mut graph = BarModuleGraph::from_config(&config).unwrap();
        let mut ctx = BarRuntimeContext::at_unix_timestamp(0);
        ctx.env.insert(
            "UNILII_BAR_SYS_CLASS_NET".to_string(),
            net_root.display().to_string(),
        );
        let models = graph.update_all(&ctx);
        assert_eq!(models[0].label, "eth0:up:aa:bb:cc:dd:ee:ff");
        assert_eq!(models[0].state, BarModuleState::Ok);
    }

    #[test]
    fn network_module_marks_down_interface_disconnected() {
        let net_root = write_net_fixture(&[("wlan0", "down", "11:22:33:44:55:66")]);
        let config = parse_bar_config_str(
            r#"
            [layout]
            left = ["network"]
            center = []
            right = []

            [[module]]
            id = "network"
            type = "network"
            interface = "wlan0"
            "#,
        )
        .unwrap();
        let mut graph = BarModuleGraph::from_config(&config).unwrap();
        let mut ctx = BarRuntimeContext::at_unix_timestamp(0);
        ctx.env.insert(
            "UNILII_BAR_SYS_CLASS_NET".to_string(),
            net_root.display().to_string(),
        );
        let models = graph.update_all(&ctx);
        assert_eq!(models[0].state, BarModuleState::Disconnected);
        assert!(models[0].label.contains("wlan0"));
    }

    #[test]
    fn vpn_module_detects_common_tunnel_interfaces() {
        let net_root = write_net_fixture(&[("eth0", "up", "aa"), ("wg0", "up", "bb")]);
        let config = parse_bar_config_str(
            r#"
            [layout]
            left = ["vpn"]
            center = []
            right = []

            [[module]]
            id = "vpn"
            type = "vpn"
            format = "vpn:{interface}:{state}"
            "#,
        )
        .unwrap();
        let mut graph = BarModuleGraph::from_config(&config).unwrap();
        let mut ctx = BarRuntimeContext::at_unix_timestamp(0);
        ctx.env.insert(
            "UNILII_BAR_SYS_CLASS_NET".to_string(),
            net_root.display().to_string(),
        );
        let models = graph.update_all(&ctx);
        assert_eq!(models[0].label, "vpn:wg0:up");
        assert_eq!(models[0].state, BarModuleState::Ok);
    }

    #[test]
    fn vpn_module_reports_disconnected_when_no_tunnel_exists() {
        let net_root = write_net_fixture(&[("eth0", "up", "aa")]);
        let config = parse_bar_config_str(
            r#"
            [layout]
            left = ["vpn"]
            center = []
            right = []

            [[module]]
            id = "vpn"
            type = "vpn"
            "#,
        )
        .unwrap();
        let mut graph = BarModuleGraph::from_config(&config).unwrap();
        let mut ctx = BarRuntimeContext::at_unix_timestamp(0);
        ctx.env.insert(
            "UNILII_BAR_SYS_CLASS_NET".to_string(),
            net_root.display().to_string(),
        );
        let models = graph.update_all(&ctx);
        assert_eq!(models[0].state, BarModuleState::Disconnected);
        assert_eq!(models[0].label, "vpn down");
    }

    #[test]
    fn audio_module_parses_fixture_status_and_muted_state() {
        let config = parse_bar_config_str(
            r#"
            [layout]
            left = ["audio"]
            center = []
            right = []

            [[module]]
            id = "audio"
            type = "audio"
            format = "{device}:{volume}:{muted}"
            "#,
        )
        .unwrap();
        let mut graph = BarModuleGraph::from_config(&config).unwrap();
        let mut ctx = BarRuntimeContext::at_unix_timestamp(0);
        ctx.env.insert(
            "UNILII_BAR_AUDIO_STATUS".to_string(),
            "device=sink0 volume=42% muted".to_string(),
        );
        let models = graph.update_all(&ctx);
        assert_eq!(models[0].label, "sink0:42:muted");
        assert_eq!(models[0].state, BarModuleState::Muted);
    }

    #[test]
    fn battery_module_reads_sysfs_fixture_and_thresholds() {
        let power_root = write_power_fixture("BAT0", "15", "Discharging");
        let config = parse_bar_config_str(
            r#"
            [layout]
            left = ["battery"]
            center = []
            right = []

            [[module]]
            id = "battery"
            type = "battery"
            warning_percent = 20
            critical_percent = 10
            format = "{name}:{percentage}:{state}"
            "#,
        )
        .unwrap();
        let mut graph = BarModuleGraph::from_config(&config).unwrap();
        let mut ctx = BarRuntimeContext::at_unix_timestamp(0);
        ctx.env.insert(
            "UNILII_BAR_POWER_SUPPLY_ROOT".to_string(),
            power_root.display().to_string(),
        );
        let models = graph.update_all(&ctx);
        assert_eq!(models[0].label, "BAT0:15:Discharging");
        assert_eq!(models[0].state, BarModuleState::Warning);
    }

    #[test]
    fn battery_module_marks_charging_even_below_threshold() {
        let power_root = write_power_fixture("BAT0", "5", "Charging");
        let config = parse_bar_config_str(
            r#"
            [layout]
            left = ["battery"]
            center = []
            right = []

            [[module]]
            id = "battery"
            type = "battery"
            warning_percent = 20
            critical_percent = 10
            "#,
        )
        .unwrap();
        let mut graph = BarModuleGraph::from_config(&config).unwrap();
        let mut ctx = BarRuntimeContext::at_unix_timestamp(0);
        ctx.env.insert(
            "UNILII_BAR_POWER_SUPPLY_ROOT".to_string(),
            power_root.display().to_string(),
        );
        let models = graph.update_all(&ctx);
        assert_eq!(models[0].state, BarModuleState::Charging);
        assert!(models[0].label.contains("5%"));
    }

    fn write_net_fixture(entries: &[(&str, &str, &str)]) -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!(
            "unilii-bar-net-{}-{}-{}",
            std::process::id(),
            entries
                .iter()
                .map(|(name, state, address)| format!("{name}-{state}-{address}"))
                .collect::<Vec<_>>()
                .join("_"),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        for (name, state, address) in entries {
            let dir = root.join(name);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("operstate"), state).unwrap();
            std::fs::write(dir.join("address"), address).unwrap();
        }
        root
    }

    #[test]
    fn network_module_renders_fixture_ip_address() {
        let net_root =
            write_net_ip_fixture(&[("eth0", "up", "aa:bb:cc:dd:ee:ff", "192.0.2.10/24")]);
        let config = parse_bar_config_str(
            r#"
            [layout]
            left = ["network"]
            center = []
            right = []

            [[module]]
            id = "network"
            type = "network"
            interface = "eth0"
            format = "{interface}:{state}:{ip}"
            "#,
        )
        .unwrap();
        let mut graph = BarModuleGraph::from_config(&config).unwrap();
        let mut ctx = BarRuntimeContext::at_unix_timestamp(0);
        ctx.env.insert(
            "UNILII_BAR_SYS_CLASS_NET".to_string(),
            net_root.display().to_string(),
        );
        let models = graph.update_all(&ctx);
        assert_eq!(models[0].label, "eth0:up:192.0.2.10/24");
        assert_eq!(models[0].state, BarModuleState::Ok);
        assert!(
            models[0]
                .tooltip
                .as_deref()
                .unwrap()
                .contains("ip=192.0.2.10/24")
        );
    }
    fn write_net_ip_fixture(entries: &[(&str, &str, &str, &str)]) -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!(
            "unilii-bar-net-ip-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        for (name, state, address, ip) in entries {
            let dir = root.join(name);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("operstate"), state).unwrap();
            std::fs::write(dir.join("address"), address).unwrap();
            std::fs::write(dir.join("ipv4"), ip).unwrap();
        }
        root
    }

    #[test]
    fn network_module_renders_ssid_from_fixture_file() {
        let net_root = write_net_ip_ssid_fixture(&[(
            "wlan0",
            "up",
            "aa:bb:cc:dd:ee:ff",
            "198.51.100.7/24",
            "CafeNet",
        )]);
        let config = parse_bar_config_str(
            r#"
            [layout]
            left = ["network"]
            center = []
            right = []

            [[module]]
            id = "network"
            type = "network"
            interface = "wlan0"
            format = "{interface}:{state}:{ip}:{ssid}"
            "#,
        )
        .unwrap();
        let mut graph = BarModuleGraph::from_config(&config).unwrap();
        let mut ctx = BarRuntimeContext::at_unix_timestamp(0);
        ctx.env.insert(
            "UNILII_BAR_SYS_CLASS_NET".to_string(),
            net_root.display().to_string(),
        );
        let models = graph.update_all(&ctx);
        assert_eq!(models[0].label, "wlan0:up:198.51.100.7/24:CafeNet");
        assert!(
            models[0]
                .tooltip
                .as_deref()
                .unwrap()
                .contains("ssid=CafeNet")
        );
    }

    #[test]
    fn network_module_reads_ip_and_ssid_from_status_env() {
        let net_root = write_net_fixture(&[("eth0", "up", "aa:bb:cc:dd:ee:ff")]);
        let config = parse_bar_config_str(
            r#"
            [layout]
            left = ["network"]
            center = []
            right = []

            [[module]]
            id = "network"
            type = "network"
            interface = "eth0"
            format = "{ip}:{ssid}"
            "#,
        )
        .unwrap();
        let mut graph = BarModuleGraph::from_config(&config).unwrap();
        let mut ctx = BarRuntimeContext::at_unix_timestamp(0);
        ctx.env.insert(
            "UNILII_BAR_SYS_CLASS_NET".to_string(),
            net_root.display().to_string(),
        );
        ctx.env.insert(
            "UNILII_BAR_NETWORK_STATUS".to_string(),
            "ip=203.0.113.9/24 ssid=OfficeNet".to_string(),
        );
        let models = graph.update_all(&ctx);
        assert_eq!(models[0].label, "203.0.113.9/24:OfficeNet");
    }

    #[test]
    fn network_module_reads_ip_and_ssid_from_status_file() {
        let net_root = write_net_fixture(&[("eth0", "up", "aa:bb:cc:dd:ee:ff")]);
        let status_file = std::env::temp_dir().join(format!(
            "unilii-bar-network-status-{}-{}.txt",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        std::fs::write(&status_file, "ip=203.0.113.10/24;ssid=LabNet").unwrap();
        let config = parse_bar_config_str(&format!(
            r#"
            [layout]
            left = ["network"]
            center = []
            right = []

            [[module]]
            id = "network"
            type = "network"
            interface = "eth0"
            status_file = "{}"
            format = "{{ip}}:{{ssid}}"
            "#,
            status_file.display()
        ))
        .unwrap();
        let mut graph = BarModuleGraph::from_config(&config).unwrap();
        let mut ctx = BarRuntimeContext::at_unix_timestamp(0);
        ctx.env.insert(
            "UNILII_BAR_SYS_CLASS_NET".to_string(),
            net_root.display().to_string(),
        );
        let models = graph.update_all(&ctx);
        assert_eq!(models[0].label, "203.0.113.10/24:LabNet");
    }

    #[test]
    fn network_module_reads_ip_and_ssid_from_commands() {
        let net_root = write_net_fixture(&[("eth0", "up", "aa:bb:cc:dd:ee:ff")]);
        let config = parse_bar_config_str(
            r#"
            [layout]
            left = ["network"]
            center = []
            right = []

            [[module]]
            id = "network"
            type = "network"
            interface = "eth0"
            ip_command = "printf 192.0.2.55/24"
            ssid_command = "printf CommandNet"
            format = "{ip}:{ssid}"
            "#,
        )
        .unwrap();
        let mut graph = BarModuleGraph::from_config(&config).unwrap();
        let mut ctx = BarRuntimeContext::at_unix_timestamp(0);
        ctx.env.insert(
            "UNILII_BAR_SYS_CLASS_NET".to_string(),
            net_root.display().to_string(),
        );
        let models = graph.update_all(&ctx);
        assert_eq!(models[0].label, "192.0.2.55/24:CommandNet");
    }

    fn write_net_ip_ssid_fixture(entries: &[(&str, &str, &str, &str, &str)]) -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!(
            "unilii-bar-net-ip-ssid-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        for (name, state, address, ip, ssid) in entries {
            let dir = root.join(name);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("operstate"), state).unwrap();
            std::fs::write(dir.join("address"), address).unwrap();
            std::fs::write(dir.join("ipv4"), ip).unwrap();
            std::fs::write(dir.join("ssid"), ssid).unwrap();
        }
        root
    }
    fn write_power_fixture(name: &str, capacity: &str, status: &str) -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!(
            "unilii-bar-power-{}-{}-{}-{}-{}",
            std::process::id(),
            name,
            capacity,
            status,
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        let _ = std::fs::remove_dir_all(&root);
        let dir = root.join(name);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("type"), "Battery").unwrap();
        std::fs::write(dir.join("capacity"), capacity).unwrap();
        std::fs::write(dir.join("status"), status).unwrap();
        root
    }
    #[test]
    fn notifications_module_reads_count_from_env() {
        let config = parse_bar_config_str(
            r#"
            [layout]
            left = ["notifications"]
            center = []
            right = []

            [[module]]
            id = "notifications"
            type = "notifications"
            format = "n={count}:{state}"
            "#,
        )
        .unwrap();
        let mut graph = BarModuleGraph::from_config(&config).unwrap();
        let mut ctx = BarRuntimeContext::at_unix_timestamp(0);
        ctx.env
            .insert("UNILII_BAR_NOTIFICATION_COUNT".to_string(), "3".to_string());
        let models = graph.update_all(&ctx);
        assert_eq!(models[0].label, "n=3:active");
        assert_eq!(models[0].state, BarModuleState::Active);
    }

    #[test]
    fn notifications_module_reads_count_from_file_and_empty_state() {
        let path = std::env::temp_dir().join(format!(
            "unilii-bar-notify-{}-{}.txt",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        std::fs::write(&path, "count=0\n").unwrap();
        let config = parse_bar_config_str(&format!(
            r#"
            [layout]
            left = ["notifications"]
            center = []
            right = []

            [[module]]
            id = "notifications"
            type = "notifications"
            source_file = "{}"
            "#,
            path.display()
        ))
        .unwrap();
        let mut graph = BarModuleGraph::from_config(&config).unwrap();
        let models = graph.update_all(&BarRuntimeContext::at_unix_timestamp(0));
        assert_eq!(models[0].state, BarModuleState::Empty);
        assert_eq!(models[0].label, "notify 0");
    }
    #[test]
    fn workspaces_module_renders_active_visible_and_urgent_states() {
        let config = parse_bar_config_str(
            r#"
            [layout]
            left = ["workspaces"]
            center = []
            right = []

            [[module]]
            id = "workspaces"
            type = "workspaces"
            format_active = "<{name}>"
            format_inactive = "{name}:{state}"
            separator = "|"
            "#,
        )
        .unwrap();
        let mut graph = BarModuleGraph::from_config(&config).unwrap();
        let mut ctx = BarRuntimeContext::at_unix_timestamp(0);
        ctx.env.insert(
            "UNILII_BAR_WORKSPACES".to_string(),
            "1:A,2:V,3:U".to_string(),
        );
        let models = graph.update_all(&ctx);
        assert_eq!(models[0].label, "<1>|2:visible|3:urgent");
        assert_eq!(models[0].state, BarModuleState::Urgent);
    }

    #[test]
    fn workspaces_module_reports_unavailable_without_snapshot() {
        let config = parse_bar_config_str(
            r#"
            [layout]
            left = ["workspaces"]
            center = []
            right = []

            [[module]]
            id = "workspaces"
            type = "workspaces"
            "#,
        )
        .unwrap();
        let mut graph = BarModuleGraph::from_config(&config).unwrap();
        let models = graph.update_all(&BarRuntimeContext::at_unix_timestamp(0));
        assert_eq!(models[0].state, BarModuleState::Unavailable);
    }

    #[test]
    fn window_title_module_uses_env_title_and_truncates() {
        let config = parse_bar_config_str(
            r#"
            [layout]
            left = ["window_title"]
            center = []
            right = []

            [[module]]
            id = "window_title"
            type = "window_title"
            format = "title={title}"
            max_len = 6
            "#,
        )
        .unwrap();
        let mut graph = BarModuleGraph::from_config(&config).unwrap();
        let mut ctx = BarRuntimeContext::at_unix_timestamp(0);
        ctx.env.insert(
            "UNILII_BAR_WINDOW_TITLE".to_string(),
            "abcdefghi".to_string(),
        );
        let models = graph.update_all(&ctx);
        assert_eq!(models[0].label, "title=abcde…");
        assert_eq!(models[0].state, BarModuleState::Ok);
    }

    #[test]
    fn window_title_module_is_empty_without_title() {
        let config = parse_bar_config_str(
            r#"
            [layout]
            left = ["window_title"]
            center = []
            right = []

            [[module]]
            id = "window_title"
            type = "window_title"
            "#,
        )
        .unwrap();
        let mut graph = BarModuleGraph::from_config(&config).unwrap();
        let models = graph.update_all(&BarRuntimeContext::at_unix_timestamp(0));
        assert_eq!(models[0].state, BarModuleState::Empty);
        assert_eq!(models[0].label, "");
    }
    #[test]
    fn render_model_groups_modules_by_configured_layout() {
        let config = parse_bar_config_str(
            r#"
            [layout]
            left = ["workspaces"]
            center = ["window_title"]
            right = ["clock"]

            [[module]]
            id = "workspaces"
            type = "workspaces"

            [[module]]
            id = "window_title"
            type = "window_title"

            [[module]]
            id = "clock"
            type = "clock"
            format = "%H:%M"
            "#,
        )
        .unwrap();
        let mut graph = BarModuleGraph::from_config(&config).unwrap();
        let mut ctx = BarRuntimeContext::at_unix_timestamp(0);
        ctx.env
            .insert("UNILII_BAR_WORKSPACES".to_string(), "1:A".to_string());
        ctx.env
            .insert("UNILII_BAR_WINDOW_TITLE".to_string(), "Editor".to_string());
        let model = graph.update_render_model(&ctx);
        assert_eq!(model.left[0].id, "workspaces");
        assert_eq!(model.center[0].label, "Editor");
        assert_eq!(model.right[0].id, "clock");
    }

    #[test]
    fn action_dispatch_executes_configured_command() {
        let config = parse_bar_config_str(
            r#"
            [layout]
            left = ["clock"]
            center = []
            right = []

            [[module]]
            id = "clock"
            type = "clock"
            on_click_left = "printf action-ok"
            "#,
        )
        .unwrap();
        let graph = BarModuleGraph::from_config(&config).unwrap();
        let result = graph
            .dispatch_action(
                "clock",
                BarClickButton::Left,
                &BarRuntimeContext::at_unix_timestamp(0),
            )
            .unwrap();
        assert_eq!(result.status, BarActionStatus::Success);
        assert_eq!(result.stdout, "action-ok");
        assert_eq!(result.exit_code, Some(0));
    }

    #[test]
    fn action_dispatch_reports_non_zero_exit() {
        let config = parse_bar_config_str(
            r#"
            [layout]
            left = ["clock"]
            center = []
            right = []

            [[module]]
            id = "clock"
            type = "clock"
            on_click_right = "printf nope >&2; exit 9"
            "#,
        )
        .unwrap();
        let graph = BarModuleGraph::from_config(&config).unwrap();
        let result = graph
            .dispatch_action(
                "clock",
                BarClickButton::Right,
                &BarRuntimeContext::at_unix_timestamp(0),
            )
            .unwrap();
        assert_eq!(result.status, BarActionStatus::Failed);
        assert_eq!(result.stderr, "nope");
        assert_eq!(result.exit_code, Some(9));
    }

    #[test]
    fn action_dispatch_returns_none_for_unconfigured_button() {
        let config = parse_bar_config_str(
            r#"
            [layout]
            left = ["clock"]
            center = []
            right = []

            [[module]]
            id = "clock"
            type = "clock"
            on_click_left = "true"
            "#,
        )
        .unwrap();
        let graph = BarModuleGraph::from_config(&config).unwrap();
        assert!(
            graph
                .dispatch_action(
                    "clock",
                    BarClickButton::Middle,
                    &BarRuntimeContext::at_unix_timestamp(0)
                )
                .is_none()
        );
    }

    #[test]
    fn runtime_reload_keeps_previous_config_when_new_config_is_invalid() {
        let original = parse_bar_config_str(
            r#"
            [layout]
            left = ["clock"]
            center = []
            right = []

            [[module]]
            id = "clock"
            type = "clock"
            "#,
        )
        .unwrap();
        let mut state = BarRuntimeState::from_config(original).unwrap();
        let err = state
            .reload_from_str(
                r#"
                [layout]
                left = ["missing"]

                [[module]]
                id = "clock"
                type = "clock"
                "#,
            )
            .unwrap_err();
        assert!(
            err.to_string()
                .contains("layout.left references unknown module id")
        );
        assert!(state.last_reload_error().unwrap().contains("layout.left"));
        assert_eq!(state.config().modules[0].id, "clock");
    }

    #[test]
    fn runtime_reload_replaces_graph_on_valid_config() {
        let original = parse_bar_config_str(
            r#"
            [layout]
            left = ["clock"]
            center = []
            right = []

            [[module]]
            id = "clock"
            type = "clock"
            "#,
        )
        .unwrap();
        let mut state = BarRuntimeState::from_config(original).unwrap();
        state
            .reload_from_str(
                r#"
                [layout]
                left = ["network"]
                center = []
                right = []

                [[module]]
                id = "network"
                type = "network"
                "#,
            )
            .unwrap();
        assert!(state.last_reload_error().is_none());
        assert_eq!(state.config().modules[0].id, "network");
    }
    #[test]
    fn scheduler_updates_only_due_modules_and_reuses_cache() {
        let config = parse_bar_config_str(
            r#"
            [layout]
            left = ["fast", "slow"]
            center = []
            right = []

            [[module]]
            id = "fast"
            type = "clock"
            interval_ms = 1000
            format = "%S"

            [[module]]
            id = "slow"
            type = "clock"
            interval_ms = 10000
            format = "%S"
            "#,
        )
        .unwrap();
        let mut graph = BarModuleGraph::from_config(&config).unwrap();
        let first = graph.update_due_render_model(&BarRuntimeContext::at_unix_timestamp(0));
        assert_eq!(first.left.len(), 2);
        assert_eq!(first.left[0].label, "00");
        assert_eq!(first.left[1].label, "00");

        let second = graph.update_due_render_model(&BarRuntimeContext::at_unix_timestamp(2));
        assert_eq!(second.left.len(), 2);
        assert_eq!(second.left[0].label, "02");
        assert_eq!(second.left[1].label, "00");
    }

    #[test]
    fn scheduler_next_due_in_tracks_cached_update_times() {
        let config = parse_bar_config_str(
            r#"
            [layout]
            left = ["fast", "slow"]
            center = []
            right = []

            [[module]]
            id = "fast"
            type = "clock"
            interval_ms = 1000

            [[module]]
            id = "slow"
            type = "clock"
            interval_ms = 5000
            "#,
        )
        .unwrap();
        let mut graph = BarModuleGraph::from_config(&config).unwrap();
        assert_eq!(graph.next_due_in(UNIX_EPOCH), Some(Duration::ZERO));
        graph.update_due_modules(&BarRuntimeContext::at_unix_timestamp(10));
        assert_eq!(
            graph.next_due_in(UNIX_EPOCH + Duration::from_secs(10)),
            Some(Duration::from_secs(1))
        );
        assert_eq!(
            graph.next_due_in(UNIX_EPOCH + Duration::from_secs(11)),
            Some(Duration::ZERO)
        );
    }

    #[test]
    fn scheduler_cached_render_model_is_empty_before_first_tick() {
        let config = parse_bar_config_str(
            r#"
            [layout]
            left = ["clock"]
            center = []
            right = []

            [[module]]
            id = "clock"
            type = "clock"
            "#,
        )
        .unwrap();
        let graph = BarModuleGraph::from_config(&config).unwrap();
        let model = graph.cached_render_model();
        assert!(model.left.is_empty());
        assert!(model.center.is_empty());
        assert!(model.right.is_empty());
    }
    #[test]
    fn hot_reload_from_file_reports_unchanged_when_mtime_is_same() {
        let path = write_bar_config_fixture(
            "unchanged",
            r#"
            [layout]
            left = ["clock"]
            center = []
            right = []

            [[module]]
            id = "clock"
            type = "clock"
            "#,
        );
        let mut state = BarRuntimeState::from_config_file(&path).unwrap();
        assert_eq!(state.config_path(), Some(path.as_path()));
        assert_eq!(
            state.reload_from_file_if_changed().unwrap(),
            BarReloadStatus::Unchanged
        );
    }

    #[test]
    fn hot_reload_from_file_replaces_valid_changed_config() {
        let path = write_bar_config_fixture(
            "valid-reload",
            r#"
            [layout]
            left = ["clock"]
            center = []
            right = []

            [[module]]
            id = "clock"
            type = "clock"
            "#,
        );
        let mut state = BarRuntimeState::from_config_file(&path).unwrap();
        force_file_mtime_tick();
        std::fs::write(
            &path,
            r#"
            [layout]
            left = ["network"]
            center = []
            right = []

            [[module]]
            id = "network"
            type = "network"
            "#,
        )
        .unwrap();
        assert_eq!(
            state.reload_from_file_if_changed().unwrap(),
            BarReloadStatus::Reloaded
        );
        assert_eq!(state.config().modules[0].id, "network");
        assert!(state.last_reload_error().is_none());
    }

    #[test]
    fn hot_reload_from_file_preserves_previous_graph_on_invalid_changed_config() {
        let path = write_bar_config_fixture(
            "invalid-reload",
            r#"
            [layout]
            left = ["clock"]
            center = []
            right = []

            [[module]]
            id = "clock"
            type = "clock"
            "#,
        );
        let mut state = BarRuntimeState::from_config_file(&path).unwrap();
        force_file_mtime_tick();
        std::fs::write(
            &path,
            r#"
            [layout]
            left = ["missing"]
            center = []
            right = []

            [[module]]
            id = "clock"
            type = "clock"
            "#,
        )
        .unwrap();
        assert_eq!(
            state.reload_from_file_if_changed().unwrap(),
            BarReloadStatus::Failed
        );
        assert_eq!(state.config().modules[0].id, "clock");
        assert!(state.last_reload_error().unwrap().contains("layout.left"));
    }

    fn write_bar_config_fixture(name: &str, contents: &str) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!(
            "unilii-bar-config-{name}-{}-{}.toml",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        std::fs::write(&path, contents).unwrap();
        path
    }

    fn force_file_mtime_tick() {
        std::thread::sleep(Duration::from_millis(20));
    }
    #[test]
    fn workspaces_module_parses_i3_workspace_json() {
        let config = parse_bar_config_str(
            r#"
            [layout]
            left = ["workspaces"]
            center = []
            right = []

            [[module]]
            id = "workspaces"
            type = "workspaces"
            format_active = "[{name}:{state}]"
            format_inactive = "{name}:{state}"
            separator = ","
            "#,
        )
        .unwrap();
        let mut graph = BarModuleGraph::from_config(&config).unwrap();
        let mut ctx = BarRuntimeContext::at_unix_timestamp(0);
        ctx.env.insert(
            "UNILII_BAR_I3_WORKSPACES_JSON".to_string(),
            r#"[
                {"name":"1","focused":true,"visible":true,"urgent":false},
                {"name":"2","focused":false,"visible":false,"urgent":true}
            ]"#
            .to_string(),
        );
        let models = graph.update_all(&ctx);
        assert_eq!(models[0].label, "[1:active],2:urgent");
        assert_eq!(models[0].state, BarModuleState::Urgent);
    }

    #[test]
    fn workspaces_module_reads_i3_workspace_json_from_file() {
        let path = write_text_fixture(
            "i3-workspaces",
            r#"[
                {"name":"web","focused":false,"visible":true,"urgent":false},
                {"name":"term","focused":true,"visible":true,"urgent":false}
            ]"#,
        );
        let config = parse_bar_config_str(
            r#"
            [layout]
            left = ["workspaces"]
            center = []
            right = []

            [[module]]
            id = "workspaces"
            type = "workspaces"
            separator = " "
            "#,
        )
        .unwrap();
        let mut graph = BarModuleGraph::from_config(&config).unwrap();
        let mut ctx = BarRuntimeContext::at_unix_timestamp(0);
        ctx.env.insert(
            "UNILII_BAR_I3_WORKSPACES_FILE".to_string(),
            path.display().to_string(),
        );
        let models = graph.update_all(&ctx);
        assert_eq!(models[0].label, "web [term]");
        assert_eq!(models[0].state, BarModuleState::Active);
    }

    #[test]
    fn window_title_module_extracts_focused_title_from_i3_tree_json() {
        let config = parse_bar_config_str(
            r#"
            [layout]
            left = ["window_title"]
            center = []
            right = []

            [[module]]
            id = "window_title"
            type = "window_title"
            format = "{title}"
            "#,
        )
        .unwrap();
        let mut graph = BarModuleGraph::from_config(&config).unwrap();
        let mut ctx = BarRuntimeContext::at_unix_timestamp(0);
        ctx.env.insert(
            "UNILII_BAR_I3_TREE_JSON".to_string(),
            r#"{
                "name": "root",
                "focused": false,
                "nodes": [
                    {"name":"workspace","focused":false,"nodes":[
                        {"name":"fallback-name","focused":true,"window_properties":{"title":"Focused Editor"}}
                    ]}
                ],
                "floating_nodes": []
            }"#
            .to_string(),
        );
        let models = graph.update_all(&ctx);
        assert_eq!(models[0].label, "Focused Editor");
        assert_eq!(models[0].state, BarModuleState::Ok);
    }

    #[test]
    fn window_title_module_reads_i3_tree_json_from_file() {
        let path = write_text_fixture(
            "i3-tree",
            r#"{
                "name": "root",
                "focused": false,
                "nodes": [],
                "floating_nodes": [
                    {"name":"Floating Dialog","focused":true}
                ]
            }"#,
        );
        let config = parse_bar_config_str(
            r#"
            [layout]
            left = ["window_title"]
            center = []
            right = []

            [[module]]
            id = "window_title"
            type = "window_title"
            "#,
        )
        .unwrap();
        let mut graph = BarModuleGraph::from_config(&config).unwrap();
        let mut ctx = BarRuntimeContext::at_unix_timestamp(0);
        ctx.env.insert(
            "UNILII_BAR_I3_TREE_FILE".to_string(),
            path.display().to_string(),
        );
        let models = graph.update_all(&ctx);
        assert_eq!(models[0].label, "Floating Dialog");
    }

    fn write_text_fixture(name: &str, contents: &str) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!(
            "unilii-bar-{name}-{}-{}.txt",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        std::fs::write(&path, contents).unwrap();
        path
    }

    #[test]
    fn workspaces_module_reads_i3_workspace_json_from_command() {
        let config = parse_bar_config_str(
            r#"
            [layout]
            left = ["workspaces"]
            center = []
            right = []

            [[module]]
            id = "workspaces"
            type = "workspaces"
            command = "printf '[{\"name\":\"cmd\",\"focused\":true,\"visible\":true,\"urgent\":false}]'"
            "#,
        )
        .unwrap();
        let mut graph = BarModuleGraph::from_config(&config).unwrap();
        let models = graph.update_all(&BarRuntimeContext::at_unix_timestamp(0));
        assert_eq!(models[0].label, "[cmd]");
        assert_eq!(models[0].state, BarModuleState::Active);
    }

    #[test]
    fn window_title_module_reads_i3_tree_json_from_command() {
        let config = parse_bar_config_str(
            r#"
            [layout]
            left = ["window_title"]
            center = []
            right = []

            [[module]]
            id = "window_title"
            type = "window_title"
            command = "printf '{\"focused\":true,\"window_properties\":{\"title\":\"Command Title\"}}'"
            "#,
        )
        .unwrap();
        let mut graph = BarModuleGraph::from_config(&config).unwrap();
        let models = graph.update_all(&BarRuntimeContext::at_unix_timestamp(0));
        assert_eq!(models[0].label, "Command Title");
        assert_eq!(models[0].state, BarModuleState::Ok);
    }

    #[test]
    fn workspace_switch_dispatch_uses_shell_quoted_placeholder() {
        let config = parse_bar_config_str(
            r#"
            [layout]
            left = ["workspaces"]
            center = []
            right = []

            [[module]]
            id = "workspaces"
            type = "workspaces"
            switch_command_template = "printf switched:%s {workspace_shell}"
            "#,
        )
        .unwrap();
        let graph = BarModuleGraph::from_config(&config).unwrap();
        let result = graph
            .dispatch_workspace_switch(
                "workspaces",
                "3:dev tools",
                &BarRuntimeContext::at_unix_timestamp(0),
            )
            .unwrap();
        assert_eq!(result.status, BarActionStatus::Success);
        assert_eq!(result.stdout, "switched:3:dev tools");
    }

    #[test]
    fn workspace_switch_dispatch_quotes_single_quotes() {
        let config = parse_bar_config_str(
            r#"
            [layout]
            left = ["workspaces"]
            center = []
            right = []

            [[module]]
            id = "workspaces"
            type = "workspaces"
            switch_command_template = "printf %s {workspace_shell}"
            "#,
        )
        .unwrap();
        let graph = BarModuleGraph::from_config(&config).unwrap();
        let result = graph
            .dispatch_workspace_switch(
                "workspaces",
                "dev's space",
                &BarRuntimeContext::at_unix_timestamp(0),
            )
            .unwrap();
        assert_eq!(result.status, BarActionStatus::Success);
        assert_eq!(result.stdout, "dev's space");
    }

    #[test]
    fn workspace_switch_dispatch_returns_none_without_template() {
        let config = parse_bar_config_str(
            r#"
            [layout]
            left = ["workspaces"]
            center = []
            right = []

            [[module]]
            id = "workspaces"
            type = "workspaces"
            "#,
        )
        .unwrap();
        let graph = BarModuleGraph::from_config(&config).unwrap();
        assert!(
            graph
                .dispatch_workspace_switch(
                    "workspaces",
                    "1",
                    &BarRuntimeContext::at_unix_timestamp(0),
                )
                .is_none()
        );
    }

    #[test]
    fn backend_presets_resolve_i3_and_sway_commands() {
        assert_eq!(
            default_workspaces_command_for_backend("i3").as_deref(),
            Some("i3-msg -t get_workspaces")
        );
        assert_eq!(
            default_tree_command_for_backend("sway").as_deref(),
            Some("swaymsg -t get_tree")
        );
        assert_eq!(
            default_switch_template_for_backend("i3").as_deref(),
            Some("i3-msg workspace -- {workspace_shell}")
        );
        assert!(default_workspaces_command_for_backend("unknown").is_none());
    }

    #[test]
    fn backend_preset_provides_workspace_switch_template_without_executing_wm() {
        let config = parse_bar_config_str(
            r#"
            [layout]
            left = ["workspaces"]
            center = []
            right = []

            [[module]]
            id = "workspaces"
            type = "workspaces"
            backend = "i3"
            "#,
        )
        .unwrap();
        let graph = BarModuleGraph::from_config(&config).unwrap();
        let command = graph
            .workspace_switch_command("workspaces", "1:web")
            .expect("backend preset should render a switch command");
        assert_eq!(command, "i3-msg workspace -- '1:web'");
    }

    #[test]
    fn explicit_switch_template_overrides_backend_preset() {
        let config = parse_bar_config_str(
            r#"
            [layout]
            left = ["workspaces"]
            center = []
            right = []

            [[module]]
            id = "workspaces"
            type = "workspaces"
            backend = "sway"
            switch_command_template = "printf override:%s {workspace_shell}"
            "#,
        )
        .unwrap();
        let graph = BarModuleGraph::from_config(&config).unwrap();
        let result = graph
            .dispatch_workspace_switch(
                "workspaces",
                "2:term",
                &BarRuntimeContext::at_unix_timestamp(0),
            )
            .unwrap();
        assert_eq!(result.status, BarActionStatus::Success);
        assert_eq!(result.stdout, "override:2:term");
    }

    #[test]
    fn side_effect_guard_blocks_i3_msg_dispatch_in_tests() {
        let config = parse_bar_config_str(
            r#"
            [layout]
            left = ["workspaces"]
            center = []
            right = []

            [[module]]
            id = "workspaces"
            type = "workspaces"
            switch_command_template = "i3-msg workspace -- {workspace_shell}"
            "#,
        )
        .unwrap();
        let graph = BarModuleGraph::from_config(&config).unwrap();
        let result = graph
            .dispatch_workspace_switch(
                "workspaces",
                "999:test-guard",
                &BarRuntimeContext::at_unix_timestamp(0),
            )
            .unwrap();
        assert_eq!(result.status, BarActionStatus::Unsupported);
        assert!(
            result
                .message
                .as_deref()
                .unwrap()
                .contains("blocked side-effectful command in test")
        );
    }

    #[test]
    fn side_effect_guard_blocks_common_session_mutators_in_tests() {
        for command in [
            "swaymsg workspace -- '1'",
            "nmcli radio wifi off",
            "pactl set-sink-mute @DEFAULT_SINK@ toggle",
            "wpctl set-mute @DEFAULT_AUDIO_SINK@ toggle",
            "systemctl suspend",
            "loginctl lock-session",
            "notify-send test",
        ] {
            assert!(is_side_effectful_test_command(command), "{command}");
        }
        assert!(!is_side_effectful_test_command("printf harmless"));
        assert!(!is_side_effectful_test_command("echo harmless"));
    }

    #[test]
    fn side_effect_guard_allows_harmless_mock_dispatch() {
        let config = parse_bar_config_str(
            r#"
            [layout]
            left = ["workspaces"]
            center = []
            right = []

            [[module]]
            id = "workspaces"
            type = "workspaces"
            switch_command_template = "printf %s {workspace_shell}"
            "#,
        )
        .unwrap();
        let graph = BarModuleGraph::from_config(&config).unwrap();
        let result = graph
            .dispatch_workspace_switch(
                "workspaces",
                "mock-space",
                &BarRuntimeContext::at_unix_timestamp(0),
            )
            .unwrap();
        assert_eq!(result.status, BarActionStatus::Success);
        assert_eq!(result.stdout, "mock-space");
    }

    #[test]
    fn script_module_guarded_command_is_blocked_in_tests() {
        let config = parse_bar_config_str(
            r#"
            [layout]
            left = ["script"]
            center = []
            right = []

            [[module]]
            id = "script"
            type = "script"
            command = "unilii-test-side-effect workspace-change"
            timeout_ms = 1000
            "#,
        )
        .unwrap();
        let mut graph = BarModuleGraph::from_config(&config).unwrap();
        let models = graph.update_all(&BarRuntimeContext::at_unix_timestamp(0));
        assert_eq!(models[0].state, BarModuleState::Unavailable);
        assert!(
            models[0]
                .last_error
                .as_deref()
                .unwrap()
                .contains("blocked command in test")
        );
    }

    #[test]
    fn network_command_guarded_command_is_blocked_in_tests() {
        let net_root = write_net_fixture(&[("eth0", "up", "aa:bb:cc:dd:ee:ff")]);
        let config = parse_bar_config_str(
            r#"
            [layout]
            left = ["network"]
            center = []
            right = []

            [[module]]
            id = "network"
            type = "network"
            interface = "eth0"
            ip_command = "unilii-test-side-effect network-change"
            format = "{interface}:{ip}"
            "#,
        )
        .unwrap();
        let mut graph = BarModuleGraph::from_config(&config).unwrap();
        let mut ctx = BarRuntimeContext::at_unix_timestamp(0);
        ctx.env.insert(
            "UNILII_BAR_SYS_CLASS_NET".to_string(),
            net_root.display().to_string(),
        );
        let models = graph.update_all(&ctx);
        assert_eq!(models[0].state, BarModuleState::Ok);
        assert_eq!(models[0].label, "eth0:");
    }
}
