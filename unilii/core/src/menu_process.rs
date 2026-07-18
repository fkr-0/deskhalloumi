//! Cross-process lifecycle management for short-lived unilii menu frontends.
//!
//! Menus are intentionally external processes so the bar and standalone hotkey
//! daemon can share them. A small runtime registry prevents duplicate windows
//! and provides deterministic show/hide/toggle behavior across processes.

use crate::branding::preferred_env;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::env;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, SystemTime};

const LOCK_STALE_AFTER: Duration = Duration::from_secs(10);
const HIDE_WAIT: Duration = Duration::from_millis(500);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "action", content = "name", rename_all = "snake_case")]
pub enum MenuAction {
    Show(String),
    Hide(String),
    Toggle(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum MenuActionOutcome {
    Spawned { name: String, pid: u32 },
    AlreadyVisible { name: String, pid: u32 },
    Hidden { name: String, pid: u32 },
    TerminationRequested { name: String, pid: u32 },
    AlreadyHidden { name: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MenuStatus {
    pub name: String,
    pub running: bool,
    pub pid: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuSpec {
    pub name: String,
    pub executable: PathBuf,
    pub args: Vec<String>,
}

impl MenuSpec {
    pub fn new(name: impl Into<String>, executable: impl Into<PathBuf>, args: Vec<String>) -> Self {
        Self {
            name: normalize_menu_name(&name.into()),
            executable: executable.into(),
            args,
        }
    }
}

#[derive(Debug, Clone)]
pub struct MenuProcessManager {
    runtime_dir: PathBuf,
    specs: BTreeMap<String, MenuSpec>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct MenuProcessRecord {
    pid: u32,
    executable: String,
}

pub struct MenuInstanceGuard {
    record_path: Option<PathBuf>,
    pid: u32,
}

impl Drop for MenuInstanceGuard {
    fn drop(&mut self) {
        let Some(path) = &self.record_path else {
            return;
        };
        if record_pid(path) == Some(self.pid) {
            let _ = fs::remove_file(path);
        }
    }
}

#[derive(Debug)]
pub struct ProcessInstanceGuard {
    path: PathBuf,
    pid: u32,
}

impl Drop for ProcessInstanceGuard {
    fn drop(&mut self) {
        if record_pid(&self.path) == Some(self.pid) {
            let _ = fs::remove_file(&self.path);
        }
    }
}

struct DirectoryLock {
    path: PathBuf,
}

impl Drop for DirectoryLock {
    fn drop(&mut self) {
        let _ = fs::remove_dir(&self.path);
    }
}

impl Default for MenuProcessManager {
    fn default() -> Self {
        Self::with_runtime_dir(default_runtime_dir())
    }
}

impl MenuProcessManager {
    pub fn with_runtime_dir(runtime_dir: PathBuf) -> Self {
        let specs = known_menu_specs()
            .into_iter()
            .map(|spec| (spec.name.clone(), spec))
            .collect();
        Self { runtime_dir, specs }
    }

    pub fn with_specs(runtime_dir: PathBuf, specs: Vec<MenuSpec>) -> Self {
        Self {
            runtime_dir,
            specs: specs
                .into_iter()
                .map(|spec| (spec.name.clone(), spec))
                .collect(),
        }
    }

    pub fn runtime_dir(&self) -> &Path {
        &self.runtime_dir
    }

    pub fn execute(&self, action: &MenuAction) -> Result<MenuActionOutcome, String> {
        match action {
            MenuAction::Show(name) => self.show(name),
            MenuAction::Hide(name) => self.hide(name),
            MenuAction::Toggle(name) => self.toggle(name),
        }
    }

    pub fn show(&self, name: &str) -> Result<MenuActionOutcome, String> {
        let name = normalize_menu_name(name);
        let spec = self
            .specs
            .get(&name)
            .ok_or_else(|| format!("unknown managed menu '{name}'"))?
            .clone();
        prepare_runtime_dir(&self.runtime_dir)?;
        fs::create_dir_all(self.menu_dir()).map_err(|error| {
            format!(
                "failed to create menu runtime directory '{}': {error}",
                self.menu_dir().display()
            )
        })?;
        let _lock = self.acquire_menu_lock(&name)?;
        if let Some(record) = self.running_record(&name)? {
            return Ok(MenuActionOutcome::AlreadyVisible {
                name,
                pid: record.pid,
            });
        }

        let mut command = Command::new(&spec.executable);
        command
            .args(&spec.args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .env("UNILII_MENU_MANAGED", "1")
            .env("UNILII_MENU_NAME", &name)
            .env("UNILII_RUNTIME_DIR", &self.runtime_dir);
        let child = command.spawn().map_err(|error| {
            format!(
                "failed to spawn managed menu '{}' using '{}': {error}",
                name,
                spec.executable.display()
            )
        })?;
        let pid = child.id();
        self.write_record(
            &name,
            &MenuProcessRecord {
                pid,
                executable: executable_identity(&spec.executable),
            },
        )?;
        self.spawn_reaper(name.clone(), pid, child);
        Ok(MenuActionOutcome::Spawned { name, pid })
    }

    pub fn hide(&self, name: &str) -> Result<MenuActionOutcome, String> {
        let name = normalize_menu_name(name);
        prepare_runtime_dir(&self.runtime_dir)?;
        fs::create_dir_all(self.menu_dir()).map_err(|error| error.to_string())?;
        let _lock = self.acquire_menu_lock(&name)?;
        let Some(record) = self.running_record(&name)? else {
            return Ok(MenuActionOutcome::AlreadyHidden { name });
        };

        send_signal(record.pid, libc::SIGTERM).map_err(|error| {
            format!(
                "failed to terminate managed menu '{name}' pid={}: {error}",
                record.pid
            )
        })?;
        let deadline = std::time::Instant::now() + HIDE_WAIT;
        while std::time::Instant::now() < deadline && process_matches(&record) {
            thread::sleep(Duration::from_millis(20));
        }
        if !process_matches(&record) {
            let _ = self.remove_record_if_pid(&name, record.pid);
            Ok(MenuActionOutcome::Hidden {
                name,
                pid: record.pid,
            })
        } else {
            Ok(MenuActionOutcome::TerminationRequested {
                name,
                pid: record.pid,
            })
        }
    }

    pub fn toggle(&self, name: &str) -> Result<MenuActionOutcome, String> {
        let normalized = normalize_menu_name(name);
        if self.status(&normalized)?.running {
            self.hide(&normalized)
        } else {
            self.show(&normalized)
        }
    }

    pub fn status(&self, name: &str) -> Result<MenuStatus, String> {
        let name = normalize_menu_name(name);
        let record = self.running_record(&name)?;
        Ok(MenuStatus {
            name,
            running: record.is_some(),
            pid: record.map(|record| record.pid),
        })
    }

    pub fn known_statuses(&self) -> Vec<MenuStatus> {
        self.specs
            .keys()
            .map(|name| {
                self.status(name).unwrap_or(MenuStatus {
                    name: name.clone(),
                    running: false,
                    pid: None,
                })
            })
            .collect()
    }

    pub fn register_current_process(name: &str) -> Result<MenuInstanceGuard, String> {
        if env::var_os("UNILII_MENU_MANAGED").is_some() {
            return Ok(MenuInstanceGuard {
                record_path: None,
                pid: std::process::id(),
            });
        }
        let manager = Self::default();
        manager.register_current(name)
    }

    pub fn register_current(&self, name: &str) -> Result<MenuInstanceGuard, String> {
        let name = normalize_menu_name(name);
        prepare_runtime_dir(&self.runtime_dir)?;
        fs::create_dir_all(self.menu_dir()).map_err(|error| error.to_string())?;
        let _lock = self.acquire_menu_lock(&name)?;
        if let Some(record) = self.running_record(&name)? {
            return Err(format!(
                "managed menu '{}' is already running as pid {}",
                name, record.pid
            ));
        }
        let current = env::current_exe().map_err(|error| error.to_string())?;
        let pid = std::process::id();
        let path = self.record_path(&name);
        self.write_record(
            &name,
            &MenuProcessRecord {
                pid,
                executable: executable_identity(&current),
            },
        )?;
        Ok(MenuInstanceGuard {
            record_path: Some(path),
            pid,
        })
    }

    fn menu_dir(&self) -> PathBuf {
        self.runtime_dir.join("menus")
    }

    fn record_path(&self, name: &str) -> PathBuf {
        self.menu_dir().join(format!("{name}.json"))
    }

    fn lock_path(&self, name: &str) -> PathBuf {
        self.menu_dir().join(format!(".{name}.lock"))
    }

    fn acquire_menu_lock(&self, name: &str) -> Result<DirectoryLock, String> {
        let path = self.lock_path(name);
        match fs::create_dir(&path) {
            Ok(()) => Ok(DirectoryLock { path }),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                let stale = fs::metadata(&path)
                    .and_then(|metadata| metadata.modified())
                    .ok()
                    .and_then(|modified| SystemTime::now().duration_since(modified).ok())
                    .is_some_and(|age| age > LOCK_STALE_AFTER);
                if stale {
                    let _ = fs::remove_dir_all(&path);
                    fs::create_dir(&path).map_err(|retry| {
                        format!(
                            "failed to acquire stale menu lock '{}': {retry}",
                            path.display()
                        )
                    })?;
                    Ok(DirectoryLock { path })
                } else {
                    Err(format!("managed menu '{name}' is busy; retry shortly"))
                }
            }
            Err(error) => Err(format!(
                "failed to acquire menu lock '{}': {error}",
                path.display()
            )),
        }
    }

    fn running_record(&self, name: &str) -> Result<Option<MenuProcessRecord>, String> {
        let path = self.record_path(name);
        let content = match fs::read_to_string(&path) {
            Ok(content) => content,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(format!("failed to read '{}': {error}", path.display())),
        };
        let record = match serde_json::from_str::<MenuProcessRecord>(&content) {
            Ok(record) => record,
            Err(_) => {
                let _ = fs::remove_file(&path);
                return Ok(None);
            }
        };
        if process_matches(&record) {
            Ok(Some(record))
        } else {
            let _ = fs::remove_file(&path);
            Ok(None)
        }
    }

    fn write_record(&self, name: &str, record: &MenuProcessRecord) -> Result<(), String> {
        prepare_runtime_dir(&self.runtime_dir)?;
        fs::create_dir_all(self.menu_dir()).map_err(|error| error.to_string())?;
        let path = self.record_path(name);
        let tmp = path.with_extension(format!("json.{}.tmp", std::process::id()));
        let data = serde_json::to_vec(record).map_err(|error| error.to_string())?;
        fs::write(&tmp, data).map_err(|error| error.to_string())?;
        fs::rename(&tmp, &path).map_err(|error| error.to_string())
    }

    fn remove_record_if_pid(&self, name: &str, pid: u32) -> Result<(), String> {
        let path = self.record_path(name);
        if record_pid(&path) == Some(pid) {
            fs::remove_file(path).map_err(|error| error.to_string())?;
        }
        Ok(())
    }

    fn spawn_reaper(&self, name: String, pid: u32, mut child: Child) {
        let manager = self.clone();
        thread::spawn(move || {
            let _ = child.wait();
            let _ = manager.remove_record_if_pid(&name, pid);
        });
    }
}

pub fn parse_menu_action(input: &str) -> Result<MenuAction, String> {
    let parts = input
        .split(':')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();
    let parts = if parts
        .first()
        .is_some_and(|part| part.eq_ignore_ascii_case("menu"))
    {
        &parts[1..]
    } else {
        &parts[..]
    };
    if parts.len() != 2 {
        return Err(format!(
            "invalid menu action '{input}'; expected show:<name>, hide:<name>, or toggle:<name>"
        ));
    }
    let name = normalize_menu_name(parts[1]);
    validate_menu_name(&name)?;
    match parts[0].to_ascii_lowercase().as_str() {
        "show" | "open" => Ok(MenuAction::Show(name)),
        "hide" | "close" => Ok(MenuAction::Hide(name)),
        "toggle" => Ok(MenuAction::Toggle(name)),
        verb => Err(format!("unknown menu action verb '{verb}'")),
    }
}

pub fn known_menu_specs() -> Vec<MenuSpec> {
    vec![
        MenuSpec::new("i3-vis", "unilii-i3-vis", Vec::new()),
        MenuSpec::new("filter-tab", "unilii-filter-tab", Vec::new()),
        MenuSpec::new("copyq", "unilii-copyq", vec!["--i3-shortcut".to_string()]),
    ]
}

pub fn prepare_runtime_dir(path: &Path) -> Result<(), String> {
    fs::create_dir_all(path).map_err(|error| {
        format!(
            "failed to create runtime directory '{}': {error}",
            path.display()
        )
    })?;
    let metadata = fs::symlink_metadata(path).map_err(|error| {
        format!(
            "failed to inspect runtime directory '{}': {error}",
            path.display()
        )
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(format!(
            "unsafe runtime path '{}': expected a real directory",
            path.display()
        ));
    }
    let uid = unsafe { libc::geteuid() };
    if metadata.uid() != uid {
        return Err(format!(
            "unsafe runtime directory '{}': owned by uid {}, expected {}",
            path.display(),
            metadata.uid(),
            uid
        ));
    }
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).map_err(|error| {
        format!(
            "failed to secure runtime directory '{}': {error}",
            path.display()
        )
    })
}

pub fn default_runtime_dir() -> PathBuf {
    if let Some(path) = preferred_env("DESKHALLOUMI_RUNTIME_DIR", "UNILII_RUNTIME_DIR") {
        return PathBuf::from(path);
    }
    if let Some(path) = env::var_os("XDG_RUNTIME_DIR") {
        return PathBuf::from(path).join("deskhalloumi");
    }
    let uid = unsafe { libc::geteuid() };
    env::temp_dir().join(format!("deskhalloumi-{uid}"))
}

pub fn acquire_process_instance(name: &str) -> Result<ProcessInstanceGuard, String> {
    acquire_process_instance_in(&default_runtime_dir(), name)
}

fn acquire_process_instance_in(runtime: &Path, name: &str) -> Result<ProcessInstanceGuard, String> {
    let name = normalize_menu_name(name);
    validate_menu_name(&name)?;
    prepare_runtime_dir(runtime)?;
    let path = runtime.join(format!("{name}.instance.json"));
    let pid = std::process::id();

    if let Ok(content) = fs::read_to_string(&path)
        && let Ok(record) = serde_json::from_str::<MenuProcessRecord>(&content)
        && process_matches(&record)
    {
        return Err(format!("{name} is already running as pid {}", record.pid));
    }
    let _ = fs::remove_file(&path);
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .map_err(|error| {
            format!(
                "failed to acquire {name} singleton '{}': {error}",
                path.display()
            )
        })?;
    let current = env::current_exe().map_err(|error| error.to_string())?;
    let record = MenuProcessRecord {
        pid,
        executable: executable_identity(&current),
    };
    let bytes = serde_json::to_vec(&record).map_err(|error| error.to_string())?;
    file.write_all(&bytes).map_err(|error| error.to_string())?;
    Ok(ProcessInstanceGuard { path, pid })
}

pub fn process_instance_status(name: &str) -> Option<u32> {
    let path = default_runtime_dir().join(format!("{}.instance.json", normalize_menu_name(name)));
    let content = fs::read_to_string(&path).ok()?;
    let record = serde_json::from_str::<MenuProcessRecord>(&content).ok()?;
    if process_matches(&record) {
        Some(record.pid)
    } else {
        let _ = fs::remove_file(path);
        None
    }
}

fn normalize_menu_name(name: &str) -> String {
    match name.trim().to_ascii_lowercase().replace('_', "-").as_str() {
        "i3vis" | "unilii-i3-vis" | "deskhalloumi-i3-vis" => "i3-vis".to_string(),
        "filtertab" | "unilii-filter-tab" | "deskhalloumi-filter-tab" => "filter-tab".to_string(),
        "unilii-copyq" | "deskhalloumi-copyq" | "copyq-frontend" => "copyq".to_string(),
        other => other.to_string(),
    }
}

fn validate_menu_name(name: &str) -> Result<(), String> {
    if name.is_empty()
        || !name
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_'))
    {
        return Err(format!("invalid managed menu name '{name}'"));
    }
    Ok(())
}

fn executable_identity(path: &Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default()
        .to_string()
}

fn process_matches(record: &MenuProcessRecord) -> bool {
    let proc_dir = PathBuf::from("/proc").join(record.pid.to_string());
    if !proc_dir.exists() {
        return false;
    }
    if fs::read_to_string(proc_dir.join("stat"))
        .ok()
        .and_then(|stat| proc_state(&stat))
        .is_some_and(|state| matches!(state, 'Z' | 'X'))
    {
        return false;
    }
    let cmdline = fs::read(proc_dir.join("cmdline")).unwrap_or_default();
    if cmdline.is_empty() || record.executable.is_empty() {
        return true;
    }
    let identity = record.executable.as_bytes();
    cmdline
        .split(|byte| *byte == 0)
        .any(|part| part.ends_with(identity))
}

fn proc_state(stat: &str) -> Option<char> {
    let command_end = stat.rfind(") ")?;
    stat[command_end + 2..].chars().next()
}

fn record_pid(path: &Path) -> Option<u32> {
    let content = fs::read_to_string(path).ok()?;
    serde_json::from_str::<MenuProcessRecord>(&content)
        .ok()
        .map(|record| record.pid)
}

fn send_signal(pid: u32, signal: i32) -> Result<(), std::io::Error> {
    let result = unsafe { libc::kill(pid as i32, signal) };
    if result == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_menu_actions_and_aliases() {
        assert_eq!(
            parse_menu_action("toggle:i3vis").unwrap(),
            MenuAction::Toggle("i3-vis".to_string())
        );
        assert_eq!(
            parse_menu_action("menu:hide:unilii-filter-tab").unwrap(),
            MenuAction::Hide("filter-tab".to_string())
        );
        assert!(parse_menu_action("wat").is_err());
    }

    #[test]
    fn manager_toggles_one_cross_process_menu_instance() {
        let temp = tempfile::tempdir().unwrap();
        let script = temp.path().join("fake-menu.sh");
        fs::write(&script, "#!/bin/sh\nsleep 30\n").unwrap();
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = fs::metadata(&script).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&script, permissions).unwrap();
        let spec = MenuSpec::new("fake", &script, Vec::new());
        let first = MenuProcessManager::with_specs(temp.path().join("runtime"), vec![spec.clone()]);
        let second = MenuProcessManager::with_specs(temp.path().join("runtime"), vec![spec]);

        let spawned = first.show("fake").unwrap();
        assert!(matches!(spawned, MenuActionOutcome::Spawned { .. }));
        let pid = first.status("fake").unwrap().pid.unwrap();
        assert_eq!(
            second.show("fake").unwrap(),
            MenuActionOutcome::AlreadyVisible {
                name: "fake".to_string(),
                pid,
            }
        );
        assert!(matches!(
            second.toggle("fake").unwrap(),
            MenuActionOutcome::Hidden { .. }
        ));
        for _ in 0..30 {
            if !first.status("fake").unwrap().running {
                break;
            }
            thread::sleep(Duration::from_millis(20));
        }
        assert!(!first.status("fake").unwrap().running);
    }

    #[test]
    fn process_instance_guard_rejects_second_owner() {
        let temp = tempfile::tempdir().unwrap();
        let first = acquire_process_instance_in(temp.path(), "hotkeyd-test").unwrap();
        let duplicate = acquire_process_instance_in(temp.path(), "hotkeyd-test");
        assert!(duplicate.unwrap_err().contains("already running"));
        drop(first);
        let replacement = acquire_process_instance_in(temp.path(), "hotkeyd-test");
        assert!(replacement.is_ok());
    }

    #[test]
    fn proc_state_handles_commands_with_spaces_and_detects_zombies() {
        assert_eq!(proc_state("42 (menu with spaces) Z 1 2 3"), Some('Z'));
        assert_eq!(proc_state("42 (menu) S 1 2 3"), Some('S'));
        assert_eq!(proc_state("malformed"), None);
    }
}
