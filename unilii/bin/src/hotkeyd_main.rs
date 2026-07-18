//! Standalone unilii hotkey daemon and lifecycle supervisor.
//!
//! The supervisor owns the process singleton and user-scoped control socket.
//! The evdev worker can therefore be replaced on configuration reload without
//! briefly allowing the bar or another daemon to acquire global-key ownership.

use clap::{Parser, ValueEnum};
use deskhalloumi_core::action_bus::default_action_bus_socket_path;
use deskhalloumi_core::hotkey_control::{
    HOTKEY_CONTROL_PROTOCOL_VERSION, HotkeyControlRequest, HotkeyControlResponse,
    HotkeyRuntimeStatus, default_control_socket_path, send_control_request,
};
use deskhalloumi_core::i3_config::{I3ConfigAudit, audit_i3_config};
use deskhalloumi_core::i3_keybindings::{I3ExportOptions, render_i3_bindings};
use deskhalloumi_core::key_engine::KeyTrigger;
use deskhalloumi_core::key_import_sxhkd::{ImportWarning, import_sxhkd_config};
use deskhalloumi_core::keys::{
    CommandType, KeyBackend, KeyBinding, KeybindingDaemon, KeybindingDaemonOptions,
    validate_binding,
};
use deskhalloumi_core::menu_process::{
    MenuProcessManager, acquire_process_instance, parse_menu_action, prepare_runtime_dir,
    process_instance_status,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};
use tokio::signal::unix::{SignalKind, signal};
use tokio::task::JoinHandle;
use tracing::{error, info, warn};

#[derive(Debug, Clone, Copy, ValueEnum)]
enum BackendArg {
    Evdev,
    X11,
}

impl From<BackendArg> for KeyBackend {
    fn from(value: BackendArg) -> Self {
        match value {
            BackendArg::Evdev => KeyBackend::Evdev,
            BackendArg::X11 => KeyBackend::X11,
        }
    }
}

fn run_i3_audit(path: &Path, bindings: &[KeyBinding]) -> I3ConfigAudit {
    let mut roots = Vec::new();
    if let Some(home) = std::env::var_os("HOME") {
        roots.push(PathBuf::from(home));
    }
    if let Some(parent) = path.parent() {
        roots.push(parent.to_path_buf());
    }
    if Path::new("/etc/i3").exists() {
        roots.push(PathBuf::from("/etc/i3"));
    }
    audit_i3_config(path, bindings, &roots).unwrap_or_else(|error| {
        exit_error_value(&format!("failed to audit i3 configuration: {error}"), 2)
    })
}

fn print_i3_audit(audit: &I3ConfigAudit, json: bool) {
    if json {
        println!("{}", serde_json::to_string_pretty(audit).unwrap());
        return;
    }
    println!(
        "i3 audit: root={} files={} bindings={} existing_conflicts={} generated_conflicts={} incomplete={}",
        audit.root.display(),
        audit.files.len(),
        audit.bindings.len(),
        audit.existing_conflicts.len(),
        audit.conflicts.len(),
        audit.incomplete
    );
    for conflict in &audit.existing_conflicts {
        println!(
            "  existing {:?}: {}:{} mode={} chord={} conflicts with {}:{} chord={}",
            conflict.kind,
            conflict.first.path.display(),
            conflict.first.line,
            conflict.first.mode,
            conflict.first.chord,
            conflict.second.path.display(),
            conflict.second.line,
            conflict.second.chord,
        );
    }
    for conflict in &audit.conflicts {
        println!(
            "  {:?}: generated '{}' ({}) conflicts with {}:{} mode={} chord={} command={}",
            conflict.kind,
            conflict.generated_binding,
            conflict.generated_chord,
            conflict.existing.path.display(),
            conflict.existing.line,
            conflict.existing.mode,
            conflict.existing.chord,
            conflict.existing.command
        );
    }
    for warning in &audit.warnings {
        println!("  warning: {warning}");
    }
}

#[derive(Debug, Clone, Parser)]
#[command(name = "deskhalloumi-hotkeyd")]
#[command(about = "Standalone global hotkey daemon for DeskHalloumi actions")]
struct Args {
    /// TOML file containing [[keybindings]] entries. Full DeskHalloumi config files also work.
    #[arg(long, short = 'c', value_name = "FILE")]
    config: Option<PathBuf>,

    /// sxhkdrc-compatible file to import at runtime.
    #[arg(long, value_name = "FILE")]
    sxhkd: Option<PathBuf>,

    /// Append built-in managed menu bindings for i3-vis/filter-tab/copyq.
    #[arg(long)]
    menu_defaults: bool,

    /// Print built-in managed menu bindings as TOML and exit.
    #[arg(long)]
    print_defaults: bool,

    /// Print an i3 include generated from the configured bindings and exit.
    #[arg(long)]
    print_i3_bindings: bool,

    /// Atomically write an i3 include generated from the configured bindings.
    #[arg(long, value_name = "FILE")]
    write_i3_bindings: Option<PathBuf>,

    /// Run `i3-msg reload` after successfully writing the generated include.
    #[arg(long)]
    reload_i3: bool,

    /// i3 IPC command used by --reload-i3 (overridable for tests/wrappers).
    #[arg(long, default_value = "i3-msg", value_name = "COMMAND")]
    i3_msg: PathBuf,

    /// Recursively audit an active i3 config and all resolvable includes.
    #[arg(long, value_name = "FILE")]
    audit_i3_config: Option<PathBuf>,

    /// Execute one managed menu action and exit, e.g. toggle:i3-vis.
    #[arg(long, value_name = "ACTION")]
    menu_action: Option<String>,

    /// Print daemon and managed-menu status and exit.
    #[arg(long)]
    status: bool,

    /// Verify that the running daemon answers its control socket.
    #[arg(long)]
    ping: bool,

    /// Ask the running daemon to reload all configured binding sources.
    #[arg(long)]
    reload: bool,

    /// Ask the running daemon to terminate cleanly.
    #[arg(long)]
    shutdown: bool,

    /// Override the user-scoped Unix control socket path.
    #[arg(long, value_name = "PATH")]
    control_socket: Option<PathBuf>,

    /// Override the DeskHalloumi bar action-bus socket.
    #[arg(long, value_name = "PATH")]
    action_socket: Option<PathBuf>,

    /// Input backend: evdev observation or selective X11 passive grabs.
    #[arg(long, value_enum, default_value_t = BackendArg::Evdev)]
    backend: BackendArg,

    /// Print control replies as JSON.
    #[arg(long)]
    json: bool,

    /// Poll configuration sources and reload when their metadata changes.
    #[arg(long)]
    watch: bool,

    /// Configuration watch interval in milliseconds.
    #[arg(long, default_value_t = 1000, value_name = "MS")]
    watch_interval_ms: u64,

    /// Print migration/conflict report and exit without listening.
    #[arg(long)]
    dry_run: bool,

    /// Alias for --dry-run when only diagnostics are wanted.
    #[arg(long)]
    report: bool,

    /// Reject warnings, duplicates, shadowing, invalid actions, and unsafe runtime configs.
    #[arg(long)]
    strict: bool,

    /// Listen and report matches, but do not execute commands and do not grab devices.
    #[arg(long)]
    shadow: bool,

    /// Request an exclusive raw evdev grab. Requires explicit unsafe acknowledgement.
    #[arg(long)]
    grab: bool,

    /// Acknowledge that raw evdev grab currently suppresses all keyboard events.
    #[arg(long)]
    allow_unsafe_evdev_grab: bool,

    /// Verbose logging (-v, -vv, -vvv).
    #[arg(short, long, action = clap::ArgAction::Count)]
    verbose: u8,
}

fn handle_i3_export(args: &Args, loaded: &LoadedBindings, report: &HotkeyReport) {
    if args.reload_i3 && args.write_i3_bindings.is_none() {
        exit_error("--reload-i3 requires --write-i3-bindings FILE", 2);
    }
    if loaded.bindings.is_empty() {
        exit_error(
            "no bindings loaded; use --config, --sxhkd, or --menu-defaults",
            2,
        );
    }
    if !report.invalid_bindings.is_empty() {
        exit_error(
            &format!(
                "cannot export invalid bindings: {}",
                report.invalid_bindings.join("; ")
            ),
            2,
        );
    }

    let export = render_i3_bindings(&loaded.bindings, &I3ExportOptions::default());
    let audit = args
        .audit_i3_config
        .as_ref()
        .map(|path| run_i3_audit(path, &loaded.bindings));
    if let Some(audit) = &audit {
        print_i3_audit(audit, args.json);
    }
    for warning in report
        .import_warnings
        .iter()
        .map(|warning| format!("sxhkd line {}: {}", warning.line, warning.message))
        .chain(export.warnings.iter().cloned())
    {
        eprintln!("i3 export warning: {warning}");
    }

    let has_export_issues = report.has_issues()
        || !export.warnings.is_empty()
        || audit
            .as_ref()
            .is_some_and(|audit| audit.has_conflicts() || audit.incomplete);
    if args.strict && has_export_issues {
        exit_error("strict mode rejected i3 export warnings or conflicts", 3);
    }
    if export.exported_count == 0 {
        exit_error("no bindings could be represented safely by i3", 2);
    }

    if args.print_i3_bindings {
        print!("{}", export.config);
    }
    if let Some(path) = &args.write_i3_bindings {
        write_atomic(path, export.config.as_bytes()).unwrap_or_else(|error| {
            exit_error_value(
                &format!("failed to write i3 bindings '{}': {error}", path.display()),
                1,
            )
        });
        eprintln!(
            "wrote {} i3 bindings to {} ({} skipped)",
            export.exported_count,
            path.display(),
            export.skipped_count
        );
    }

    if args.reload_i3 {
        let status = std::process::Command::new(&args.i3_msg)
            .arg("reload")
            .status()
            .unwrap_or_else(|error| {
                exit_error_value(
                    &format!(
                        "failed to execute '{} reload': {error}",
                        args.i3_msg.display()
                    ),
                    1,
                )
            });
        if !status.success() {
            exit_error(
                &format!("'{} reload' exited with {status}", args.i3_msg.display()),
                1,
            );
        }
    }
}

fn write_atomic(path: &Path, content: &[u8]) -> Result<(), String> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)
        .map_err(|error| format!("failed to create '{}': {error}", parent.display()))?;
    let temporary = parent.join(format!(
        ".{}.tmp-{}",
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("i3-bindings"),
        std::process::id()
    ));
    fs::write(&temporary, content)
        .map_err(|error| format!("failed to write '{}': {error}", temporary.display()))?;
    fs::rename(&temporary, path).map_err(|error| {
        let _ = fs::remove_file(&temporary);
        format!(
            "failed to replace '{}' with '{}': {error}",
            path.display(),
            temporary.display()
        )
    })?;
    Ok(())
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
struct HotkeyConfig {
    #[serde(default)]
    keybindings: Vec<KeyBinding>,
}

#[derive(Debug, Clone)]
struct BindingSources {
    config: Option<PathBuf>,
    sxhkd: Option<PathBuf>,
    menu_defaults: bool,
}

impl BindingSources {
    fn from_args(args: &Args) -> Self {
        Self {
            config: args.config.clone(),
            sxhkd: args.sxhkd.clone(),
            menu_defaults: args.menu_defaults,
        }
    }

    fn labels(&self) -> Vec<String> {
        let mut labels = Vec::new();
        if let Some(path) = &self.config {
            labels.push(format!("toml:{}", path.display()));
        }
        if let Some(path) = &self.sxhkd {
            labels.push(format!("sxhkd:{}", path.display()));
        }
        if self.menu_defaults {
            labels.push("builtin:menu-defaults".to_string());
        }
        labels
    }

    fn watched_paths(&self) -> Vec<PathBuf> {
        self.config
            .iter()
            .chain(self.sxhkd.iter())
            .cloned()
            .collect()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SourceFingerprint(Vec<(PathBuf, Option<u128>, Option<u64>)>);

impl SourceFingerprint {
    fn capture(sources: &BindingSources) -> Self {
        let entries = sources
            .watched_paths()
            .into_iter()
            .map(|path| {
                let metadata = fs::metadata(&path).ok();
                let modified = metadata
                    .as_ref()
                    .and_then(|value| value.modified().ok())
                    .and_then(|value| value.duration_since(UNIX_EPOCH).ok())
                    .map(|value| value.as_nanos());
                let len = metadata.map(|value| value.len());
                (path, modified, len)
            })
            .collect();
        Self(entries)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct HotkeyReport {
    binding_count: usize,
    managed_menu_count: usize,
    import_warnings: Vec<ImportWarning>,
    duplicate_chords: Vec<String>,
    shadowed_bindings: Vec<String>,
    invalid_bindings: Vec<String>,
}

impl HotkeyReport {
    fn has_issues(&self) -> bool {
        !self.import_warnings.is_empty()
            || !self.duplicate_chords.is_empty()
            || !self.shadowed_bindings.is_empty()
            || !self.invalid_bindings.is_empty()
    }
}

#[derive(Debug, Clone)]
struct LoadedBindings {
    bindings: Vec<KeyBinding>,
    import_warnings: Vec<ImportWarning>,
}

#[derive(Debug)]
struct SupervisorState {
    backend: KeyBackend,
    generation: u64,
    binding_count: usize,
    managed_menu_count: usize,
    started_at_unix_ms: u128,
    loaded_at_unix_ms: u128,
    config_sources: Vec<String>,
    last_reload_error: Option<String>,
    shadow: bool,
    grab: bool,
}

impl SupervisorState {
    fn status(&self, menu_manager: &MenuProcessManager) -> HotkeyRuntimeStatus {
        HotkeyRuntimeStatus {
            protocol_version: HOTKEY_CONTROL_PROTOCOL_VERSION,
            pid: std::process::id(),
            backend: format!("{:?}", self.backend).to_ascii_lowercase(),
            generation: self.generation,
            binding_count: self.binding_count,
            managed_menu_count: self.managed_menu_count,
            shadow: self.shadow,
            grab: self.grab,
            started_at_unix_ms: self.started_at_unix_ms,
            loaded_at_unix_ms: self.loaded_at_unix_ms,
            config_sources: self.config_sources.clone(),
            last_reload_error: self.last_reload_error.clone(),
            menus: menu_manager.known_statuses(),
        }
    }
}

type DaemonTask = JoinHandle<Result<(), String>>;
type WorkerReady = tokio::sync::oneshot::Receiver<std::result::Result<(), String>>;

struct ControlContext<'a> {
    args: &'a Args,
    sources: &'a BindingSources,
    options: KeybindingDaemonOptions,
    menu_manager: &'a MenuProcessManager,
    state: &'a mut SupervisorState,
    current_bindings: &'a mut Vec<KeyBinding>,
    daemon_task: &'a mut DaemonTask,
}

struct ControlSocketGuard {
    path: PathBuf,
}

impl Drop for ControlSocketGuard {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

fn main() {
    let args = Args::parse();
    init_logging(args.verbose);
    let menu_manager = MenuProcessManager::default();
    let socket_path = args
        .control_socket
        .clone()
        .unwrap_or_else(default_control_socket_path);

    if args.print_defaults {
        match menu_launcher_defaults_toml() {
            Ok(output) => println!("{output}"),
            Err(error) => exit_error(&format!("failed to render defaults: {error}"), 1),
        }
        return;
    }

    if let Some(request) = client_request(&args) {
        handle_client_request(&args, &socket_path, &menu_manager, request);
        return;
    }

    let sources = BindingSources::from_args(&args);
    let loaded = load_bindings(&sources).unwrap_or_else(|error| {
        exit_error_value(&format!("failed to load hotkey bindings: {error}"), 1)
    });
    let report = analyze_hotkeys(&loaded.bindings, loaded.import_warnings.clone());
    if args.print_i3_bindings || args.write_i3_bindings.is_some() {
        handle_i3_export(&args, &loaded, &report);
        return;
    }
    if args.dry_run || args.report {
        println!("{}", render_hotkey_report(&report));
        let audit = args
            .audit_i3_config
            .as_ref()
            .map(|path| run_i3_audit(path, &loaded.bindings));
        if let Some(audit) = &audit {
            print_i3_audit(audit, args.json);
        }
        if args.strict
            && (report.has_issues()
                || audit
                    .as_ref()
                    .is_some_and(|audit| audit.has_conflicts() || audit.incomplete))
        {
            std::process::exit(3);
        }
        return;
    }
    if let Some(path) = &args.audit_i3_config {
        let audit = run_i3_audit(path, &loaded.bindings);
        print_i3_audit(&audit, args.json);
        if args.strict && (audit.has_conflicts() || audit.incomplete) {
            std::process::exit(3);
        }
        return;
    }

    emit_runtime_report(&args, &report);
    validate_runtime_configuration(&args, &loaded, &report);
    let runtime = tokio::runtime::Runtime::new().unwrap_or_else(|error| {
        exit_error_value(&format!("failed to create tokio runtime: {error}"), 1)
    });
    if let Err(error) = runtime.block_on(run_supervisor(
        args.clone(),
        sources,
        loaded,
        report,
        menu_manager,
        socket_path,
    )) {
        error!("deskhalloumi-hotkeyd exited with error: {error}");
        exit_error(&format!("deskhalloumi-hotkeyd: {error}"), 1);
    }
}

fn client_request(args: &Args) -> Option<HotkeyControlRequest> {
    let actions = [
        args.status,
        args.ping,
        args.reload,
        args.shutdown,
        args.menu_action.is_some(),
    ];
    if actions.into_iter().filter(|active| *active).count() > 1 {
        exit_error(
            "choose only one of --status, --ping, --reload, --shutdown, or --menu-action",
            2,
        );
    }
    if args.status {
        Some(HotkeyControlRequest::Status)
    } else if args.ping {
        Some(HotkeyControlRequest::Ping)
    } else if args.reload {
        Some(HotkeyControlRequest::Reload)
    } else if args.shutdown {
        Some(HotkeyControlRequest::Shutdown)
    } else {
        args.menu_action
            .as_ref()
            .map(|action| HotkeyControlRequest::Menu {
                action: action.clone(),
            })
    }
}

fn handle_client_request(
    args: &Args,
    socket_path: &Path,
    menu_manager: &MenuProcessManager,
    request: HotkeyControlRequest,
) {
    match send_control_request(socket_path, &request) {
        Ok(response) => {
            print_control_response(&response, args.json);
            if !response.ok {
                std::process::exit(1);
            }
        }
        Err(control_error) => match request {
            HotkeyControlRequest::Status => print_fallback_status(menu_manager, args.json),
            HotkeyControlRequest::Menu { action } => {
                let action = parse_menu_action(&action).unwrap_or_else(|error| {
                    exit_error_value(&format!("invalid --menu-action: {error}"), 2)
                });
                let outcome = menu_manager.execute(&action).unwrap_or_else(|error| {
                    exit_error_value(&format!("managed menu action failed: {error}"), 1)
                });
                if args.json {
                    println!("{}", serde_json::to_string_pretty(&outcome).unwrap());
                } else {
                    println!("{outcome:?}");
                }
            }
            _ => exit_error(&control_error, 1),
        },
    }
}

fn print_control_response(response: &HotkeyControlResponse, json: bool) {
    if json {
        println!("{}", serde_json::to_string_pretty(response).unwrap());
        return;
    }
    println!("{}", response.message);
    if let Some(status) = &response.status {
        print_human_status(status);
    }
}

fn print_human_status(status: &HotkeyRuntimeStatus) {
    println!(
        "hotkeyd: running pid={} backend={} generation={} bindings={} managed_menus={} shadow={} grab={}",
        status.pid,
        status.backend,
        status.generation,
        status.binding_count,
        status.managed_menu_count,
        status.shadow,
        status.grab
    );
    if !status.config_sources.is_empty() {
        println!("sources: {}", status.config_sources.join(", "));
    }
    if let Some(error) = &status.last_reload_error {
        println!("last reload error: {error}");
    }
    for menu in &status.menus {
        match menu.pid {
            Some(pid) if menu.running => println!("menu {}: running pid={pid}", menu.name),
            _ => println!("menu {}: hidden", menu.name),
        }
    }
}

fn print_fallback_status(menu_manager: &MenuProcessManager, json: bool) {
    let pid = process_instance_status("hotkeyd");
    let menus = menu_manager.known_statuses();
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "control_available": false,
                "hotkeyd_pid": pid,
                "menus": menus,
            }))
            .unwrap()
        );
        return;
    }
    match pid {
        Some(pid) => {
            println!("hotkeyd: process record present pid={pid}; control socket unavailable")
        }
        None => println!("hotkeyd: stopped"),
    }
    for menu in menus {
        match menu.pid {
            Some(pid) if menu.running => println!("menu {}: running pid={pid}", menu.name),
            _ => println!("menu {}: hidden", menu.name),
        }
    }
}

async fn run_supervisor(
    args: Args,
    sources: BindingSources,
    initial: LoadedBindings,
    initial_report: HotkeyReport,
    menu_manager: MenuProcessManager,
    socket_path: PathBuf,
) -> Result<(), String> {
    let _instance_guard = acquire_process_instance("hotkeyd")
        .map_err(|error| format!("global hotkey listener unavailable: {error}"))?;
    let (listener, _socket_guard) = bind_control_socket(&socket_path)?;

    let options = KeybindingDaemonOptions {
        backend: args.backend.into(),
        execute: !args.shadow,
        grab: args.grab && !args.shadow,
        allow_unsafe_grab: args.allow_unsafe_evdev_grab,
        singleton: false,
    };
    let started = unix_ms(SystemTime::now());
    let mut state = SupervisorState {
        backend: options.backend,
        generation: 1,
        binding_count: initial.bindings.len(),
        managed_menu_count: initial_report.managed_menu_count,
        started_at_unix_ms: started,
        loaded_at_unix_ms: started,
        config_sources: sources.labels(),
        last_reload_error: None,
        shadow: args.shadow,
        grab: options.grab,
    };
    let mut current_bindings = initial.bindings;
    let action_socket = args
        .action_socket
        .clone()
        .unwrap_or_else(default_action_bus_socket_path);
    let (mut daemon_task, ready) = spawn_daemon(
        current_bindings.clone(),
        options,
        menu_manager.clone(),
        action_socket,
    );
    await_worker_ready(ready).await.map_err(|error| {
        daemon_task.abort();
        format!("input worker failed to initialize: {error}")
    })?;
    let mut fingerprint = SourceFingerprint::capture(&sources);

    let mut sighup = signal(SignalKind::hangup()).map_err(|error| error.to_string())?;
    let mut sigterm = signal(SignalKind::terminate()).map_err(|error| error.to_string())?;
    let mut sigint = signal(SignalKind::interrupt()).map_err(|error| error.to_string())?;
    let mut watch_interval =
        tokio::time::interval(Duration::from_millis(args.watch_interval_ms.max(100)));
    watch_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

    info!(
        "hotkeyd supervisor ready socket='{}' generation=1 bindings={} watch={}",
        socket_path.display(),
        state.binding_count,
        args.watch
    );

    let mut shutdown = false;
    while !shutdown {
        tokio::select! {
            result = &mut daemon_task => {
                return match result {
                    Ok(Ok(())) => Err("input worker ended unexpectedly".to_string()),
                    Ok(Err(error)) => Err(format!("input worker failed: {error}")),
                    Err(error) if error.is_cancelled() => Err("input worker was cancelled unexpectedly".to_string()),
                    Err(error) => Err(format!("input worker join failed: {error}")),
                };
            }
            accepted = listener.accept() => {
                let (stream, _) = accepted.map_err(|error| error.to_string())?;
                match read_control_request(stream).await {
                    Ok((request, stream)) => {
                        let mut context = ControlContext {
                            args: &args,
                            sources: &sources,
                            options,
                            menu_manager: &menu_manager,
                            state: &mut state,
                            current_bindings: &mut current_bindings,
                            daemon_task: &mut daemon_task,
                        };
                        let (response, should_shutdown) =
                            handle_control_request(request, &mut context).await;
                        write_control_response(stream, &response).await;
                        shutdown = should_shutdown;
                    }
                    Err((error, stream)) => {
                        write_control_response(stream, &HotkeyControlResponse::error(error)).await;
                    }
                }
            }
            _ = sighup.recv() => {
                info!("received SIGHUP; reloading hotkey configuration");
                reload_worker(
                    &args,
                    &sources,
                    options,
                    &menu_manager,
                    &mut state,
                    &mut current_bindings,
                    &mut daemon_task,
                ).await;
                fingerprint = SourceFingerprint::capture(&sources);
            }
            _ = sigterm.recv() => {
                info!("received SIGTERM; shutting down");
                shutdown = true;
            }
            _ = sigint.recv() => {
                info!("received SIGINT; shutting down");
                shutdown = true;
            }
            _ = watch_interval.tick(), if args.watch => {
                let current = SourceFingerprint::capture(&sources);
                if current != fingerprint {
                    info!("configuration source metadata changed; reloading");
                    fingerprint = current;
                    reload_worker(
                    &args,
                    &sources,
                    options,
                    &menu_manager,
                    &mut state,
                    &mut current_bindings,
                    &mut daemon_task,
                ).await;
                }
            }
        }
    }

    daemon_task.abort();
    let _ = daemon_task.await;
    Ok(())
}

fn spawn_daemon(
    bindings: Vec<KeyBinding>,
    options: KeybindingDaemonOptions,
    menu_manager: MenuProcessManager,
    action_socket: PathBuf,
) -> (DaemonTask, WorkerReady) {
    let (ready_tx, ready_rx) = tokio::sync::oneshot::channel();
    let task = tokio::spawn(async move {
        let mut daemon = KeybindingDaemon::with_options(bindings, options);
        daemon.set_menu_manager(menu_manager);
        daemon.set_action_bus_socket(action_socket);
        daemon
            .run_with_ready(Some(ready_tx))
            .await
            .map_err(|error| error.to_string())
    });
    (task, ready_rx)
}

async fn await_worker_ready(ready: WorkerReady) -> Result<(), String> {
    match tokio::time::timeout(Duration::from_secs(5), ready).await {
        Ok(Ok(result)) => result,
        Ok(Err(_)) => Err("input worker exited before reporting readiness".to_string()),
        Err(_) => Err("timed out waiting for input worker readiness".to_string()),
    }
}

async fn reload_worker(
    args: &Args,
    sources: &BindingSources,
    options: KeybindingDaemonOptions,
    menu_manager: &MenuProcessManager,
    state: &mut SupervisorState,
    current_bindings: &mut Vec<KeyBinding>,
    daemon_task: &mut DaemonTask,
) -> HotkeyControlResponse {
    let loaded = match load_bindings(sources) {
        Ok(loaded) => loaded,
        Err(error) => {
            state.last_reload_error = Some(error.clone());
            warn!("hotkey reload rejected: {error}");
            return HotkeyControlResponse::error(format!("reload rejected: {error}"))
                .with_status(state.status(menu_manager));
        }
    };
    let report = analyze_hotkeys(&loaded.bindings, loaded.import_warnings.clone());
    if let Err(error) = runtime_validation_error(args, &loaded, &report) {
        state.last_reload_error = Some(error.clone());
        warn!("hotkey reload rejected: {error}");
        return HotkeyControlResponse::error(format!("reload rejected: {error}"))
            .with_status(state.status(menu_manager));
    }

    let candidate_bindings = loaded.bindings;
    let previous_bindings = current_bindings.clone();
    daemon_task.abort();
    let _ = (&mut *daemon_task).await;

    let action_socket = args
        .action_socket
        .clone()
        .unwrap_or_else(default_action_bus_socket_path);
    let (candidate_task, candidate_ready) = spawn_daemon(
        candidate_bindings.clone(),
        options,
        menu_manager.clone(),
        action_socket.clone(),
    );
    *daemon_task = candidate_task;
    if let Err(error) = await_worker_ready(candidate_ready).await {
        daemon_task.abort();
        let _ = (&mut *daemon_task).await;
        let (restored_task, restored_ready) = spawn_daemon(
            previous_bindings.clone(),
            options,
            menu_manager.clone(),
            action_socket,
        );
        *daemon_task = restored_task;
        let restore_result = await_worker_ready(restored_ready).await;
        let message = match restore_result {
            Ok(()) => {
                format!("candidate worker failed readiness: {error}; previous generation restored")
            }
            Err(restore_error) => format!(
                "candidate worker failed readiness: {error}; previous generation also failed to restore: {restore_error}"
            ),
        };
        state.last_reload_error = Some(message.clone());
        warn!("hotkey reload rolled back: {message}");
        return HotkeyControlResponse::error(format!("reload rolled back: {message}"))
            .with_status(state.status(menu_manager));
    }

    *current_bindings = candidate_bindings;
    state.generation += 1;
    state.binding_count = report.binding_count;
    state.managed_menu_count = report.managed_menu_count;
    state.loaded_at_unix_ms = unix_ms(SystemTime::now());
    state.last_reload_error = None;
    info!(
        "hotkey configuration reloaded generation={} bindings={}",
        state.generation, state.binding_count
    );
    HotkeyControlResponse::ok(format!(
        "reloaded generation {} with {} bindings",
        state.generation, state.binding_count
    ))
    .with_status(state.status(menu_manager))
}

async fn handle_control_request(
    request: HotkeyControlRequest,
    context: &mut ControlContext<'_>,
) -> (HotkeyControlResponse, bool) {
    match request {
        HotkeyControlRequest::Ping => (
            HotkeyControlResponse::ok("pong")
                .with_status(context.state.status(context.menu_manager)),
            false,
        ),
        HotkeyControlRequest::Status => (
            HotkeyControlResponse::ok("status")
                .with_status(context.state.status(context.menu_manager)),
            false,
        ),
        HotkeyControlRequest::Reload => (
            reload_worker(
                context.args,
                context.sources,
                context.options,
                context.menu_manager,
                context.state,
                context.current_bindings,
                context.daemon_task,
            )
            .await,
            false,
        ),
        HotkeyControlRequest::Shutdown => (
            HotkeyControlResponse::ok("shutdown requested")
                .with_status(context.state.status(context.menu_manager)),
            true,
        ),
        HotkeyControlRequest::Menu { action } => {
            let response = match parse_menu_action(&action)
                .and_then(|action| context.menu_manager.execute(&action))
            {
                Ok(outcome) => HotkeyControlResponse::ok(format!("{outcome:?}"))
                    .with_status(context.state.status(context.menu_manager)),
                Err(error) => HotkeyControlResponse::error(error)
                    .with_status(context.state.status(context.menu_manager)),
            };
            (response, false)
        }
    }
}

fn bind_control_socket(path: &Path) -> Result<(UnixListener, ControlSocketGuard), String> {
    let parent = path
        .parent()
        .ok_or_else(|| format!("control socket '{}' has no parent", path.display()))?;
    prepare_runtime_dir(parent)?;
    if path.exists() {
        if std::os::unix::net::UnixStream::connect(path).is_ok() {
            return Err(format!(
                "hotkeyd control socket '{}' is already active",
                path.display()
            ));
        }
        fs::remove_file(path).map_err(|error| {
            format!(
                "failed to remove stale socket '{}': {error}",
                path.display()
            )
        })?;
    }
    let listener = UnixListener::bind(path).map_err(|error| {
        format!(
            "failed to bind control socket '{}': {error}",
            path.display()
        )
    })?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600)).map_err(|error| {
        format!(
            "failed to secure control socket '{}': {error}",
            path.display()
        )
    })?;
    Ok((
        listener,
        ControlSocketGuard {
            path: path.to_path_buf(),
        },
    ))
}

async fn read_control_request(
    stream: UnixStream,
) -> Result<(HotkeyControlRequest, UnixStream), (String, UnixStream)> {
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    let read = tokio::time::timeout(Duration::from_secs(2), reader.read_line(&mut line)).await;
    match read {
        Err(_) => {
            let stream = reader.into_inner();
            Err(("timed out reading control request".to_string(), stream))
        }
        Ok(Ok(0)) => {
            let stream = reader.into_inner();
            Err(("empty control request".to_string(), stream))
        }
        Ok(Ok(_)) => {
            let stream = reader.into_inner();
            if line.len() > 65_536 {
                return Err(("control request exceeds 64 KiB".to_string(), stream));
            }
            match serde_json::from_str(line.trim()) {
                Ok(request) => Ok((request, stream)),
                Err(error) => Err((format!("invalid control request: {error}"), stream)),
            }
        }
        Ok(Err(error)) => {
            let stream = reader.into_inner();
            Err((format!("failed to read control request: {error}"), stream))
        }
    }
}

async fn write_control_response(mut stream: UnixStream, response: &HotkeyControlResponse) {
    match serde_json::to_vec(response) {
        Ok(mut payload) => {
            payload.push(b'\n');
            let _ = stream.write_all(&payload).await;
            let _ = stream.shutdown().await;
        }
        Err(error) => error!("failed to encode control response: {error}"),
    }
}

fn emit_runtime_report(args: &Args, report: &HotkeyReport) {
    if args.strict || !report.has_issues() {
        return;
    }
    eprintln!("deskhalloumi-hotkeyd startup diagnostics:");
    for line in render_hotkey_report(report).lines().skip(3) {
        eprintln!("  {line}");
    }
}

fn validate_runtime_configuration(args: &Args, loaded: &LoadedBindings, report: &HotkeyReport) {
    if let Err(error) = runtime_validation_error(args, loaded, report) {
        exit_error(&error, 2);
    }
}

fn runtime_validation_error(
    args: &Args,
    loaded: &LoadedBindings,
    report: &HotkeyReport,
) -> Result<(), String> {
    if loaded.bindings.is_empty() {
        return Err("no bindings loaded; use --config, --sxhkd, or --menu-defaults".to_string());
    }
    if !report.invalid_bindings.is_empty() {
        return Err(format!(
            "invalid bindings: {}",
            report.invalid_bindings.join("; ")
        ));
    }
    if args.strict && report.has_issues() {
        return Err("strict mode rejected migration warnings or binding conflicts".to_string());
    }
    if matches!(args.backend, BackendArg::X11) && args.grab {
        return Err(
            "--grab is an evdev-only option; the X11 backend already uses selective passive grabs"
                .to_string(),
        );
    }
    if matches!(args.backend, BackendArg::Evdev)
        && args.grab
        && !args.allow_unsafe_evdev_grab
        && !args.shadow
    {
        return Err(
            "refusing --grab without --allow-unsafe-evdev-grab: raw evdev grabbing suppresses every key because unmatched events are not re-injected"
                .to_string(),
        );
    }
    Ok(())
}

fn load_bindings(sources: &BindingSources) -> Result<LoadedBindings, String> {
    let mut bindings = Vec::new();
    let mut import_warnings = Vec::new();
    if let Some(path) = &sources.config {
        bindings.extend(load_keybindings_from_toml(path)?);
    }
    if let Some(path) = &sources.sxhkd {
        let content = fs::read_to_string(path)
            .map_err(|error| format!("failed to read sxhkd file '{}': {error}", path.display()))?;
        let imported = import_sxhkd_config(&content);
        import_warnings.extend(imported.warnings);
        bindings.extend(imported.bindings);
    }
    if sources.menu_defaults {
        bindings.extend(menu_launcher_default_bindings());
    }
    promote_known_menu_bindings(&mut bindings);
    Ok(LoadedBindings {
        bindings,
        import_warnings,
    })
}

fn load_keybindings_from_toml(path: &Path) -> Result<Vec<KeyBinding>, String> {
    let content = fs::read_to_string(path)
        .map_err(|error| format!("failed to read config '{}': {error}", path.display()))?;
    let config = toml::from_str::<HotkeyConfig>(&content)
        .map_err(|error| format!("failed to parse config '{}': {error}", path.display()))?;
    Ok(config.keybindings)
}

fn menu_launcher_default_bindings() -> Vec<KeyBinding> {
    vec![
        menu_binding("menu_i3_vis", "Super+i", "toggle:i3-vis"),
        menu_binding("menu_filter_tab", "Super+u", "toggle:filter-tab"),
        menu_binding("menu_copyq", "Super+c", "toggle:copyq"),
    ]
}

fn menu_binding(name: &str, keysym: &str, command: &str) -> KeyBinding {
    KeyBinding {
        name: name.to_string(),
        keysym: keysym.to_string(),
        command: command.to_string(),
        command_type: CommandType::Menu,
        release: false,
        trigger: KeyTrigger::Press,
        hold_ms: None,
        cooldown_ms: Some(250),
        priority: 100,
        consume: true,
    }
}

fn menu_launcher_defaults_toml() -> Result<String, String> {
    toml::to_string_pretty(&HotkeyConfig {
        keybindings: menu_launcher_default_bindings(),
    })
    .map_err(|error| error.to_string())
}

fn promote_known_menu_bindings(bindings: &mut [KeyBinding]) {
    for binding in bindings {
        if binding.command_type != CommandType::Shell {
            continue;
        }
        if let Some(action) = known_menu_action_for_shell_command(&binding.command) {
            binding.command_type = CommandType::Menu;
            binding.command = action;
        }
    }
}

fn known_menu_action_for_shell_command(command: &str) -> Option<String> {
    let parts = command.split_whitespace().collect::<Vec<_>>();
    let executable = Path::new(*parts.first()?).file_name()?.to_str()?;
    match (executable, parts.get(1..).unwrap_or_default()) {
        ("unilii-i3-vis", []) | ("deskhalloumi-i3-vis", []) => Some("toggle:i3-vis".to_string()),
        ("unilii-filter-tab", []) | ("deskhalloumi-filter-tab", []) => {
            Some("toggle:filter-tab".to_string())
        }
        ("unilii-copyq", [])
        | ("unilii-copyq", ["--i3-shortcut"])
        | ("deskhalloumi-copyq", [])
        | ("deskhalloumi-copyq", ["--i3-shortcut"]) => Some("toggle:copyq".to_string()),
        _ => None,
    }
}

fn analyze_hotkeys(bindings: &[KeyBinding], import_warnings: Vec<ImportWarning>) -> HotkeyReport {
    HotkeyReport {
        binding_count: bindings.len(),
        managed_menu_count: bindings
            .iter()
            .filter(|binding| binding.command_type == CommandType::Menu)
            .count(),
        duplicate_chords: duplicate_chord_lines(bindings),
        shadowed_bindings: shadowed_binding_lines(bindings),
        invalid_bindings: invalid_binding_lines(bindings),
        import_warnings,
    }
}

fn invalid_binding_lines(bindings: &[KeyBinding]) -> Vec<String> {
    let mut lines = Vec::new();
    for binding in bindings {
        if binding.name.trim().is_empty() {
            lines.push("binding has an empty name".to_string());
        }
        if binding.keysym.trim().is_empty() {
            lines.push(format!("{} has an empty keysym", binding.name));
        }
        if binding.command.trim().is_empty() {
            lines.push(format!("{} has an empty command", binding.name));
        }
        if let Err(error) = validate_binding(binding) {
            lines.push(format!("{} has an invalid keysym: {error}", binding.name));
        }
        if binding.command_type == CommandType::Menu
            && let Err(error) = parse_menu_action(&binding.command)
        {
            lines.push(format!("{}: {error}", binding.name));
        }
    }
    lines.sort();
    lines
}

fn render_hotkey_report(report: &HotkeyReport) -> String {
    let mut lines = Vec::new();
    lines.push("deskhalloumi-hotkeyd report".to_string());
    lines.push(format!("bindings: {}", report.binding_count));
    lines.push(format!(
        "managed menu bindings: {}",
        report.managed_menu_count
    ));
    render_section(
        &mut lines,
        "migration warnings",
        report
            .import_warnings
            .iter()
            .map(|warning| format!("line {}: {}", warning.line, warning.message)),
    );
    render_section(
        &mut lines,
        "duplicate chords",
        report.duplicate_chords.iter().cloned(),
    );
    render_section(
        &mut lines,
        "shadowed bindings",
        report.shadowed_bindings.iter().cloned(),
    );
    render_section(
        &mut lines,
        "invalid bindings",
        report.invalid_bindings.iter().cloned(),
    );
    lines.join("\n")
}

fn render_section<I>(lines: &mut Vec<String>, title: &str, values: I)
where
    I: IntoIterator<Item = String>,
{
    lines.push(format!("{title}:"));
    let values = values.into_iter().collect::<Vec<_>>();
    if values.is_empty() {
        lines.push("  <none>".to_string());
    } else {
        lines.extend(values.into_iter().map(|value| format!("  {value}")));
    }
}

fn duplicate_chord_lines(bindings: &[KeyBinding]) -> Vec<String> {
    let mut groups: HashMap<String, Vec<&KeyBinding>> = HashMap::new();
    for binding in bindings {
        groups.entry(report_key(binding)).or_default().push(binding);
    }
    let mut lines = groups
        .into_iter()
        .filter_map(|(key, group)| {
            (group.len() > 1).then(|| {
                let names = group
                    .iter()
                    .map(|binding| binding.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("{key}: {names}")
            })
        })
        .collect::<Vec<_>>();
    lines.sort();
    lines
}

fn shadowed_binding_lines(bindings: &[KeyBinding]) -> Vec<String> {
    let mut groups: HashMap<String, Vec<&KeyBinding>> = HashMap::new();
    for binding in bindings {
        groups.entry(report_key(binding)).or_default().push(binding);
    }
    let mut lines = Vec::new();
    for (key, mut group) in groups {
        group.sort_by(|left, right| {
            right
                .priority
                .cmp(&left.priority)
                .then_with(|| right.consume.cmp(&left.consume))
                .then_with(|| left.name.cmp(&right.name))
        });
        let Some(winner) = group.first() else {
            continue;
        };
        let winner_name = winner.name.clone();
        let winner_priority = winner.priority;
        if winner.consume {
            for binding in group.iter().skip(1) {
                if winner_priority >= binding.priority {
                    lines.push(format!(
                        "{} is shadowed by {} on {} (priority {} <= {}, consume=true)",
                        binding.name, winner_name, key, binding.priority, winner_priority
                    ));
                }
            }
        }
    }
    lines.sort();
    lines
}

fn report_key(binding: &KeyBinding) -> String {
    format!(
        "{} trigger={}",
        normalize_keysym_for_report(&binding.keysym),
        trigger_for_report(&binding.trigger)
    )
}

fn normalize_keysym_for_report(keysym: &str) -> String {
    keysym
        .split('+')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .map(|part| part.to_ascii_lowercase().replace('-', "_"))
        .collect::<Vec<_>>()
        .join("+")
}

fn trigger_for_report(trigger: &KeyTrigger) -> &'static str {
    match trigger {
        KeyTrigger::Press => "press",
        KeyTrigger::Release => "release",
        KeyTrigger::Modrelease => "modrelease",
        KeyTrigger::Repeat => "repeat",
    }
}

fn unix_ms(time: SystemTime) -> u128 {
    time.duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

fn init_logging(verbose: u8) {
    let level = match verbose {
        0 => tracing::Level::INFO,
        1 => tracing::Level::DEBUG,
        _ => tracing::Level::TRACE,
    };
    let _ = tracing_subscriber::fmt().with_max_level(level).try_init();
}

fn exit_error(message: &str, code: i32) {
    eprintln!("{message}");
    std::process::exit(code);
}

fn exit_error_value<T>(message: &str, code: i32) -> T {
    exit_error(message, code);
    unreachable!()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn menu_defaults_use_managed_toggle_actions() {
        let defaults = menu_launcher_default_bindings();
        assert_eq!(defaults.len(), 3);
        assert!(
            defaults
                .iter()
                .all(|binding| binding.command_type == CommandType::Menu)
        );
        assert!(
            defaults
                .iter()
                .any(|binding| binding.command == "toggle:i3-vis")
        );
        assert!(
            defaults
                .iter()
                .any(|binding| binding.command == "toggle:filter-tab")
        );
        assert!(
            defaults
                .iter()
                .any(|binding| binding.command == "toggle:copyq")
        );
    }

    #[test]
    fn known_shell_menu_launchers_are_promoted() {
        let mut bindings = vec![KeyBinding {
            name: "legacy".to_string(),
            keysym: "Super+i".to_string(),
            command: "/usr/local/bin/unilii-i3-vis".to_string(),
            command_type: CommandType::Shell,
            release: false,
            trigger: KeyTrigger::Press,
            hold_ms: None,
            cooldown_ms: None,
            priority: 1,
            consume: false,
        }];
        promote_known_menu_bindings(&mut bindings);
        assert_eq!(bindings[0].command_type, CommandType::Menu);
        assert_eq!(bindings[0].command, "toggle:i3-vis");
    }

    #[test]
    fn report_detects_duplicates_shadowing_and_invalid_menu_actions() {
        let high = KeyBinding {
            name: "high".to_string(),
            keysym: "Super+i".to_string(),
            command: "toggle:i3-vis".to_string(),
            command_type: CommandType::Menu,
            release: false,
            trigger: KeyTrigger::Press,
            hold_ms: None,
            cooldown_ms: None,
            priority: 100,
            consume: true,
        };
        let mut low = high.clone();
        low.name = "low".to_string();
        low.keysym = "super + i".to_string();
        low.priority = 1;
        low.consume = false;
        low.command = "toggle:no/such/menu".to_string();

        let report = analyze_hotkeys(&[high, low], Vec::new());
        assert_eq!(report.duplicate_chords.len(), 1);
        assert_eq!(report.shadowed_bindings.len(), 1);
        assert_eq!(report.invalid_bindings.len(), 1);
    }

    #[test]
    fn standalone_report_accepts_versioned_internal_bar_actions() {
        let binding = KeyBinding {
            name: "reload_bar".to_string(),
            keysym: "Super+r".to_string(),
            command: "reload-config".to_string(),
            command_type: CommandType::Bar,
            release: false,
            trigger: KeyTrigger::Press,
            hold_ms: None,
            cooldown_ms: None,
            priority: 1,
            consume: false,
        };
        let report = analyze_hotkeys(&[binding], Vec::new());
        assert!(report.invalid_bindings.is_empty());
    }

    #[test]
    fn source_fingerprint_changes_with_file_metadata() {
        let temp = tempfile::tempdir().unwrap();
        let config = temp.path().join("hotkeys.toml");
        fs::write(&config, "one").unwrap();
        let sources = BindingSources {
            config: Some(config.clone()),
            sxhkd: None,
            menu_defaults: false,
        };
        let before = SourceFingerprint::capture(&sources);
        fs::write(&config, "different-length").unwrap();
        let after = SourceFingerprint::capture(&sources);
        assert_ne!(before, after);
    }
    #[test]
    fn control_socket_rejects_second_owner_and_cleans_up() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime.block_on(async {
            let temp = tempfile::tempdir().unwrap();
            let socket = temp.path().join("hotkeyd.sock");
            let (_listener, guard) = bind_control_socket(&socket).unwrap();
            assert!(socket.exists());
            assert!(bind_control_socket(&socket).is_err());
            drop(guard);
            assert!(!socket.exists());
        });
    }
}
