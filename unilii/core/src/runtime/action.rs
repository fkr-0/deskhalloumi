//! Bounded asynchronous action and subprocess execution.

use std::{
    ffi::OsString,
    future::Future,
    io,
    path::PathBuf,
    process::Stdio,
    sync::Arc,
    time::{Duration, Instant},
};

use tokio::{
    io::{AsyncRead, AsyncReadExt},
    process::{Child, Command},
    task::JoinHandle,
    time,
};

use super::metrics::{RuntimeMetrics, global_runtime_metrics};

const DEFAULT_OUTPUT_LIMIT_BYTES: usize = 64 * 1024;
const OUTPUT_DRAIN_TIMEOUT: Duration = Duration::from_secs(1);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionCommand {
    pub program: PathBuf,
    pub args: Vec<OsString>,
    pub current_dir: Option<PathBuf>,
    pub env: Vec<(OsString, OsString)>,
}

impl ActionCommand {
    pub fn new(program: impl Into<PathBuf>, args: Vec<OsString>) -> Self {
        Self {
            program: program.into(),
            args,
            current_dir: None,
            env: Vec::new(),
        }
    }

    pub fn current_dir(mut self, current_dir: impl Into<PathBuf>) -> Self {
        self.current_dir = Some(current_dir.into());
        self
    }

    pub fn env(mut self, key: impl Into<OsString>, value: impl Into<OsString>) -> Self {
        self.env.push((key.into(), value.into()));
        self
    }
}

#[derive(Debug, Clone)]
pub struct ActionRunner {
    pub menu: String,
    pub action: String,
    timeout: Duration,
    output_limit_bytes: usize,
    metrics: Arc<RuntimeMetrics>,
}

impl ActionRunner {
    pub fn new(menu: impl Into<String>, action: impl Into<String>) -> Self {
        Self::with_timeout(menu, action, Duration::from_secs(30))
    }

    pub fn with_timeout(
        menu: impl Into<String>,
        action: impl Into<String>,
        timeout: Duration,
    ) -> Self {
        Self {
            menu: menu.into(),
            action: action.into(),
            timeout,
            output_limit_bytes: DEFAULT_OUTPUT_LIMIT_BYTES,
            metrics: global_runtime_metrics(),
        }
    }

    pub fn with_output_limit(mut self, output_limit_bytes: usize) -> Self {
        self.output_limit_bytes = output_limit_bytes.max(1);
        self
    }

    pub fn with_metrics(mut self, metrics: Arc<RuntimeMetrics>) -> Self {
        self.metrics = metrics;
        self
    }

    pub async fn run<T, E, F>(&self, work: F) -> ActionOutcome<T>
    where
        F: Future<Output = Result<T, E>> + Send,
        T: Send,
        E: ToString,
    {
        let started_at = Instant::now();
        self.metrics.record_action_started();
        let (result, error_class) = match time::timeout(self.timeout, work).await {
            Ok(result) => {
                let result = result.map_err(|error| error.to_string());
                let error_class = result.as_ref().err().map(|_| "error".to_string());
                (result, error_class)
            }
            Err(_) => (
                Err(format!(
                    "action `{}` for menu `{}` timed out after {:?}",
                    self.action, self.menu, self.timeout
                )),
                Some("timeout".to_string()),
            ),
        };

        let duration = started_at.elapsed();
        self.record_completion(duration, result.is_ok(), error_class.as_deref(), 0, 0);
        ActionOutcome {
            menu: self.menu.clone(),
            action: self.action.clone(),
            duration_ms: duration.as_millis(),
            exit_code: None,
            error_class,
            stdout: String::new(),
            stderr: String::new(),
            stdout_truncated: false,
            stderr_truncated: false,
            stdout_bytes: 0,
            stderr_bytes: 0,
            result,
        }
    }

    pub async fn run_command(&self, command: ActionCommand) -> ActionOutcome<()> {
        let started_at = Instant::now();
        self.metrics.record_action_started();
        let raw = self.execute_command(command).await;
        let duration = started_at.elapsed();
        let stdout = String::from_utf8_lossy(&raw.stdout.bytes).into_owned();
        let stderr = String::from_utf8_lossy(&raw.stderr.bytes).into_owned();
        let truncated_streams = u64::from(raw.stdout.truncated) + u64::from(raw.stderr.truncated);
        let discarded_bytes = raw
            .stdout
            .total_bytes
            .saturating_add(raw.stderr.total_bytes)
            .saturating_sub(stdout.len().saturating_add(stderr.len()) as u64);
        self.record_completion(
            duration,
            raw.result.is_ok(),
            raw.error_class.as_deref(),
            truncated_streams,
            discarded_bytes,
        );
        ActionOutcome {
            menu: self.menu.clone(),
            action: self.action.clone(),
            duration_ms: duration.as_millis(),
            exit_code: raw.exit_code,
            error_class: raw.error_class,
            stdout,
            stderr,
            stdout_truncated: raw.stdout.truncated,
            stderr_truncated: raw.stderr.truncated,
            stdout_bytes: raw.stdout.total_bytes,
            stderr_bytes: raw.stderr.total_bytes,
            result: raw.result,
        }
    }

    pub async fn run_command_bytes(&self, command: ActionCommand) -> BinaryActionOutcome {
        let started_at = Instant::now();
        self.metrics.record_action_started();
        let raw = self.execute_command(command).await;
        let duration = started_at.elapsed();
        let stderr = String::from_utf8_lossy(&raw.stderr.bytes).into_owned();
        let truncated_streams = u64::from(raw.stdout.truncated) + u64::from(raw.stderr.truncated);
        let discarded_bytes = raw
            .stdout
            .total_bytes
            .saturating_add(raw.stderr.total_bytes)
            .saturating_sub(raw.stdout.bytes.len().saturating_add(stderr.len()) as u64);
        self.record_completion(
            duration,
            raw.result.is_ok(),
            raw.error_class.as_deref(),
            truncated_streams,
            discarded_bytes,
        );
        BinaryActionOutcome {
            menu: self.menu.clone(),
            action: self.action.clone(),
            duration_ms: duration.as_millis(),
            exit_code: raw.exit_code,
            error_class: raw.error_class,
            stdout: raw.stdout.bytes,
            stderr,
            stdout_truncated: raw.stdout.truncated,
            stderr_truncated: raw.stderr.truncated,
            stdout_bytes: raw.stdout.total_bytes,
            stderr_bytes: raw.stderr.total_bytes,
            result: raw.result,
        }
    }

    async fn execute_command(&self, command: ActionCommand) -> RawCommandOutcome {
        let mut process = Command::new(&command.program);
        process.args(&command.args);
        process.stdin(Stdio::null());
        process.stdout(Stdio::piped());
        process.stderr(Stdio::piped());
        process.kill_on_drop(true);
        #[cfg(unix)]
        process.process_group(0);
        if let Some(current_dir) = &command.current_dir {
            process.current_dir(current_dir);
        }
        process.envs(command.env.iter().cloned());

        let mut child = match process.spawn() {
            Ok(child) => child,
            Err(source) => {
                return RawCommandOutcome::failure(
                    "spawn",
                    format!(
                        "failed to spawn action `{}` for menu `{}` action `{}`: {source}",
                        command.program.display(),
                        self.menu,
                        self.action
                    ),
                );
            }
        };

        let stdout_task = child
            .stdout
            .take()
            .map(|stdout| tokio::spawn(read_bounded(stdout, self.output_limit_bytes)));
        let stderr_task = child
            .stderr
            .take()
            .map(|stderr| tokio::spawn(read_bounded(stderr, self.output_limit_bytes)));

        let (status, wait_error, timed_out) = match time::timeout(self.timeout, child.wait()).await
        {
            Ok(Ok(status)) => (Some(status), None, false),
            Ok(Err(source)) => (None, Some(source), false),
            Err(_) => {
                terminate_child_tree(&mut child).await;
                (None, None, true)
            }
        };

        let stdout = join_reader(stdout_task).await;
        let stderr = join_reader(stderr_task).await;

        if timed_out {
            return RawCommandOutcome {
                exit_code: None,
                error_class: Some("timeout".to_string()),
                stdout,
                stderr,
                result: Err(format!(
                    "action `{}` for menu `{}` timed out after {:?}",
                    self.action, self.menu, self.timeout
                )),
            };
        }

        if let Some(source) = wait_error {
            return RawCommandOutcome {
                exit_code: None,
                error_class: Some("wait".to_string()),
                stdout,
                stderr,
                result: Err(format!(
                    "failed to wait for action `{}` in menu `{}`: {source}",
                    self.action, self.menu
                )),
            };
        }

        let status = status.expect("status exists when wait succeeded");
        let exit_code = status.code();
        if status.success() {
            RawCommandOutcome {
                exit_code,
                error_class: None,
                stdout,
                stderr,
                result: Ok(()),
            }
        } else {
            RawCommandOutcome {
                exit_code,
                error_class: Some("non_zero_exit".to_string()),
                stdout,
                stderr,
                result: Err(format!(
                    "action `{}` for menu `{}` exited with status {:?}",
                    self.action, self.menu, exit_code
                )),
            }
        }
    }

    fn record_completion(
        &self,
        duration: Duration,
        success: bool,
        error_class: Option<&str>,
        truncated_streams: u64,
        discarded_bytes: u64,
    ) {
        tracing::info!(
            menu = %self.menu,
            action = %self.action,
            duration_ms = duration.as_millis(),
            success,
            error_class = ?error_class,
            truncated_streams,
            discarded_bytes,
            "runtime action completed"
        );
        self.metrics.record_action_finished(
            duration,
            success,
            error_class == Some("timeout"),
            truncated_streams,
            discarded_bytes,
        );
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionOutcome<T> {
    pub menu: String,
    pub action: String,
    pub duration_ms: u128,
    pub exit_code: Option<i32>,
    pub error_class: Option<String>,
    pub stdout: String,
    pub stderr: String,
    pub stdout_truncated: bool,
    pub stderr_truncated: bool,
    pub stdout_bytes: u64,
    pub stderr_bytes: u64,
    pub result: Result<T, String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BinaryActionOutcome {
    pub menu: String,
    pub action: String,
    pub duration_ms: u128,
    pub exit_code: Option<i32>,
    pub error_class: Option<String>,
    pub stdout: Vec<u8>,
    pub stderr: String,
    pub stdout_truncated: bool,
    pub stderr_truncated: bool,
    pub stdout_bytes: u64,
    pub stderr_bytes: u64,
    pub result: Result<(), String>,
}

#[derive(Debug, Default)]
struct BoundedOutput {
    bytes: Vec<u8>,
    truncated: bool,
    total_bytes: u64,
}

struct RawCommandOutcome {
    exit_code: Option<i32>,
    error_class: Option<String>,
    stdout: BoundedOutput,
    stderr: BoundedOutput,
    result: Result<(), String>,
}

impl RawCommandOutcome {
    fn failure(error_class: &str, message: String) -> Self {
        Self {
            exit_code: None,
            error_class: Some(error_class.to_string()),
            stdout: BoundedOutput::default(),
            stderr: BoundedOutput::default(),
            result: Err(message),
        }
    }
}

async fn read_bounded<R>(mut reader: R, limit: usize) -> io::Result<BoundedOutput>
where
    R: AsyncRead + Unpin,
{
    let mut retained = Vec::with_capacity(limit.min(8192));
    let mut total_bytes = 0_u64;
    let mut chunk = [0_u8; 8192];

    loop {
        let read = reader.read(&mut chunk).await?;
        if read == 0 {
            break;
        }
        total_bytes = total_bytes.saturating_add(read as u64);
        let remaining = limit.saturating_sub(retained.len());
        if remaining > 0 {
            retained.extend_from_slice(&chunk[..read.min(remaining)]);
        }
    }

    Ok(BoundedOutput {
        bytes: retained,
        truncated: total_bytes > limit as u64,
        total_bytes,
    })
}

async fn join_reader(handle: Option<JoinHandle<io::Result<BoundedOutput>>>) -> BoundedOutput {
    let Some(mut handle) = handle else {
        return BoundedOutput::default();
    };

    match time::timeout(OUTPUT_DRAIN_TIMEOUT, &mut handle).await {
        Ok(Ok(Ok(output))) => output,
        Ok(Ok(Err(_))) | Ok(Err(_)) => BoundedOutput::default(),
        Err(_) => {
            handle.abort();
            BoundedOutput::default()
        }
    }
}

async fn terminate_child_tree(child: &mut Child) {
    #[cfg(unix)]
    if let Some(pid) = child.id() {
        // The child starts in its own process group, so a timeout also stops
        // shell descendants that could otherwise keep stdout/stderr pipes open.
        let result = unsafe { libc::kill(-(pid as i32), libc::SIGKILL) };
        if result == -1 {
            let error = io::Error::last_os_error();
            if error.raw_os_error() != Some(libc::ESRCH) {
                tracing::warn!(pid, %error, "failed to terminate action process group");
            }
        }
    }

    let _ = child.start_kill();
    let _ = time::timeout(OUTPUT_DRAIN_TIMEOUT, child.wait()).await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, os::unix::fs::PermissionsExt};
    use tempfile::tempdir;

    fn write_script(name: &str, body: &str) -> PathBuf {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join(name);
        fs::write(&path, format!("#!/bin/sh\nset -eu\n{body}\n")).expect("write script");
        let mut permissions = fs::metadata(&path).expect("metadata").permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&path, permissions).expect("chmod");
        let _ = Box::leak(Box::new(dir));
        path
    }

    fn script_command(script: PathBuf) -> ActionCommand {
        ActionCommand::new("sh", vec![script.into_os_string()])
    }

    #[tokio::test]
    async fn success_capture_collects_stdout_stderr_and_metadata() {
        let script = write_script(
            "success.sh",
            "printf 'hello stdout\\n'\nprintf 'hello stderr\\n' >&2",
        );
        let runner = ActionRunner::with_timeout("menu-a", "action-success", Duration::from_secs(1));
        let outcome = runner.run_command(script_command(script)).await;
        assert_eq!(outcome.exit_code, Some(0));
        assert_eq!(outcome.result, Ok(()));
        assert_eq!(outcome.stdout, "hello stdout\n");
        assert_eq!(outcome.stderr, "hello stderr\n");
    }

    #[tokio::test]
    async fn timeout_returns_timeout_result() {
        let script = write_script("timeout.sh", "sleep 2");
        let runner =
            ActionRunner::with_timeout("menu-b", "action-timeout", Duration::from_millis(100));
        let outcome = runner.run_command(script_command(script)).await;
        assert_eq!(outcome.error_class.as_deref(), Some("timeout"));
        assert!(outcome.result.is_err());
    }

    #[tokio::test]
    async fn non_zero_exit_captures_stderr() {
        let script = write_script("failure.sh", "printf 'failure details\\n' >&2\nexit 23");
        let runner = ActionRunner::with_timeout("menu-c", "action-failure", Duration::from_secs(1));
        let outcome = runner.run_command(script_command(script)).await;
        assert_eq!(outcome.exit_code, Some(23));
        assert_eq!(outcome.stderr, "failure details\n");
        assert!(outcome.result.is_err());
    }

    #[tokio::test]
    async fn generic_actions_are_timeout_bounded() {
        let runner =
            ActionRunner::with_timeout("menu-generic", "slow-work", Duration::from_millis(25));
        let outcome = runner
            .run(async {
                time::sleep(Duration::from_secs(5)).await;
                Ok::<_, io::Error>(())
            })
            .await;
        assert_eq!(outcome.error_class.as_deref(), Some("timeout"));
    }

    #[tokio::test]
    async fn command_output_is_drained_but_retained_within_limit() {
        let script = write_script(
            "large-output.sh",
            "head -c 16384 /dev/zero | tr '\\0' x\nhead -c 12288 /dev/zero | tr '\\0' y >&2",
        );
        let runner =
            ActionRunner::with_timeout("menu-output", "bounded-output", Duration::from_secs(2))
                .with_output_limit(1024);
        let outcome = runner.run_command(script_command(script)).await;
        assert_eq!(outcome.stdout.len(), 1024);
        assert_eq!(outcome.stderr.len(), 1024);
        assert!(outcome.stdout_truncated);
        assert!(outcome.stderr_truncated);
        assert_eq!(outcome.stdout_bytes, 16384);
        assert_eq!(outcome.stderr_bytes, 12288);
    }

    #[tokio::test]
    async fn binary_output_is_retained_without_utf8_conversion() {
        let script = write_script("binary.sh", "printf '\\377PNG'");
        let runner = ActionRunner::with_timeout("preview", "binary", Duration::from_secs(1));
        let outcome = runner.run_command_bytes(script_command(script)).await;
        assert_eq!(outcome.result, Ok(()));
        assert_eq!(outcome.stdout, vec![0xff, b'P', b'N', b'G']);
    }

    #[tokio::test]
    async fn command_supports_environment_and_working_directory() {
        let directory = tempdir().expect("tempdir");
        let runner = ActionRunner::with_timeout("menu-env", "env", Duration::from_secs(1));
        let command = ActionCommand::new(
            "sh",
            vec![
                OsString::from("-c"),
                OsString::from("printf '%s:%s' \"$DESKHALLOUMI_TEST\" \"$PWD\""),
            ],
        )
        .current_dir(directory.path())
        .env("DESKHALLOUMI_TEST", "ready");
        let outcome = runner.run_command(command).await;
        assert_eq!(
            outcome.stdout,
            format!("ready:{}", directory.path().display())
        );
    }
}
