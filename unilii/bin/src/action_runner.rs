#![allow(dead_code)]
// FIXME(T6): Transitional action execution API is covered by unit tests and will be wired through the typed action bus task.

use std::{
    ffi::OsString,
    future::Future,
    io,
    path::PathBuf,
    process::Stdio,
    time::{Duration, Instant},
};
use tokio::{
    io::{AsyncRead, AsyncReadExt},
    process::{Child, Command},
    task::JoinHandle,
    time,
};

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionRunner {
    pub menu: String,
    pub action: String,
    timeout: Duration,
    output_limit_bytes: usize,
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
        }
    }

    pub fn with_output_limit(mut self, output_limit_bytes: usize) -> Self {
        self.output_limit_bytes = output_limit_bytes.max(1);
        self
    }

    pub async fn run<T, E, F>(&self, work: F) -> ActionOutcome<T>
    where
        F: Future<Output = Result<T, E>> + Send,
        T: Send,
        E: ToString,
    {
        let started_at = Instant::now();
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

        ActionOutcome {
            menu: self.menu.clone(),
            action: self.action.clone(),
            duration_ms: started_at.elapsed().as_millis(),
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
                let message = format!(
                    "failed to spawn action `{}` for menu `{}` action `{}`: {source}",
                    command.program.display(),
                    self.menu,
                    self.action
                );
                return ActionOutcome {
                    menu: self.menu.clone(),
                    action: self.action.clone(),
                    duration_ms: started_at.elapsed().as_millis(),
                    exit_code: None,
                    error_class: Some("spawn".to_string()),
                    stdout: String::new(),
                    stderr: String::new(),
                    stdout_truncated: false,
                    stderr_truncated: false,
                    stdout_bytes: 0,
                    stderr_bytes: 0,
                    result: Err(message),
                };
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
        let duration_ms = started_at.elapsed().as_millis();

        if timed_out {
            return ActionOutcome {
                menu: self.menu.clone(),
                action: self.action.clone(),
                duration_ms,
                exit_code: None,
                error_class: Some("timeout".to_string()),
                stdout: stdout.text,
                stderr: stderr.text,
                stdout_truncated: stdout.truncated,
                stderr_truncated: stderr.truncated,
                stdout_bytes: stdout.total_bytes,
                stderr_bytes: stderr.total_bytes,
                result: Err(format!(
                    "action `{}` for menu `{}` timed out after {:?}",
                    self.action, self.menu, self.timeout
                )),
            };
        }

        if let Some(source) = wait_error {
            return ActionOutcome {
                menu: self.menu.clone(),
                action: self.action.clone(),
                duration_ms: started_at.elapsed().as_millis(),
                exit_code: None,
                error_class: Some("wait".to_string()),
                stdout: stdout.text,
                stderr: stderr.text,
                stdout_truncated: stdout.truncated,
                stderr_truncated: stderr.truncated,
                stdout_bytes: stdout.total_bytes,
                stderr_bytes: stderr.total_bytes,
                result: Err(format!(
                    "failed to wait for action `{}` in menu `{}`: {source}",
                    self.action, self.menu
                )),
            };
        }

        let status = status.expect("status exists when wait succeeded");
        let exit_code = status.code();
        let success = status.success();
        ActionOutcome {
            menu: self.menu.clone(),
            action: self.action.clone(),
            duration_ms,
            exit_code,
            error_class: if success {
                None
            } else {
                Some("non_zero_exit".to_string())
            },
            stdout: stdout.text,
            stderr: stderr.text,
            stdout_truncated: stdout.truncated,
            stderr_truncated: stderr.truncated,
            stdout_bytes: stdout.total_bytes,
            stderr_bytes: stderr.total_bytes,
            result: if success {
                Ok(())
            } else {
                Err(format!(
                    "action `{}` for menu `{}` exited with status {:?}",
                    self.action, self.menu, exit_code
                ))
            },
        }
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

#[derive(Debug, Default)]
struct BoundedOutput {
    text: String,
    truncated: bool,
    total_bytes: u64,
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
        text: String::from_utf8_lossy(&retained).into_owned(),
        truncated: total_bytes > retained.len() as u64,
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

        let mut perms = fs::metadata(&path).expect("metadata").permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&path, perms).expect("chmod");

        let _ = Box::leak(Box::new(dir));
        path
    }

    #[tokio::test]
    async fn success_capture_collects_stdout_stderr_and_metadata() {
        let script = write_script(
            "success.sh",
            r#"
printf 'hello stdout\n'
printf 'hello stderr\n' >&2
"#,
        );
        let runner = ActionRunner::with_timeout("menu-a", "action-success", Duration::from_secs(1));

        let outcome = runner.run_command(ActionCommand::new(script, vec![])).await;

        assert_eq!(outcome.menu, "menu-a");
        assert_eq!(outcome.action, "action-success");
        assert_eq!(outcome.exit_code, Some(0));
        assert_eq!(outcome.error_class, None);
        assert_eq!(outcome.result, Ok(()));
        assert_eq!(outcome.stdout, "hello stdout\n");
        assert_eq!(outcome.stderr, "hello stderr\n");
        assert!(!outcome.stdout_truncated);
        assert!(!outcome.stderr_truncated);
    }

    #[tokio::test]
    async fn timeout_returns_timeout_result() {
        let script = write_script(
            "timeout.sh",
            r#"
sleep 2
"#,
        );
        let runner =
            ActionRunner::with_timeout("menu-b", "action-timeout", Duration::from_millis(100));

        let outcome = runner.run_command(ActionCommand::new(script, vec![])).await;

        assert_eq!(outcome.menu, "menu-b");
        assert_eq!(outcome.action, "action-timeout");
        assert_eq!(outcome.exit_code, None);
        assert_eq!(outcome.error_class.as_deref(), Some("timeout"));
        assert!(outcome.result.is_err());
    }

    #[tokio::test]
    async fn non_zero_exit_captures_stderr() {
        let script = write_script(
            "failure.sh",
            r#"
printf 'failure details\n' >&2
exit 23
"#,
        );
        let runner = ActionRunner::with_timeout("menu-c", "action-failure", Duration::from_secs(1));

        let outcome = runner.run_command(ActionCommand::new(script, vec![])).await;

        assert_eq!(outcome.menu, "menu-c");
        assert_eq!(outcome.action, "action-failure");
        assert_eq!(outcome.exit_code, Some(23));
        assert_eq!(outcome.error_class.as_deref(), Some("non_zero_exit"));
        assert_eq!(outcome.stdout, "");
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
        assert!(outcome.result.is_err());
    }

    #[tokio::test]
    async fn command_output_is_drained_but_retained_within_limit() {
        let script = write_script(
            "large-output.sh",
            r#"
head -c 16384 /dev/zero | tr '\0' x
head -c 12288 /dev/zero | tr '\0' y >&2
"#,
        );
        let runner =
            ActionRunner::with_timeout("menu-output", "bounded-output", Duration::from_secs(2))
                .with_output_limit(1024);

        let outcome = runner.run_command(ActionCommand::new(script, vec![])).await;

        assert_eq!(outcome.result, Ok(()));
        assert_eq!(outcome.stdout.len(), 1024);
        assert_eq!(outcome.stderr.len(), 1024);
        assert!(outcome.stdout_truncated);
        assert!(outcome.stderr_truncated);
        assert_eq!(outcome.stdout_bytes, 16384);
        assert_eq!(outcome.stderr_bytes, 12288);
    }

    #[tokio::test]
    async fn command_supports_async_safe_environment_and_working_directory() {
        let dir = tempdir().expect("tempdir");
        let runner = ActionRunner::with_timeout("menu-env", "env", Duration::from_secs(1));
        let command = ActionCommand::new(
            "sh",
            vec![
                OsString::from("-c"),
                OsString::from("printf '%s:%s' \"$DESKHALLOUMI_TEST\" \"$PWD\""),
            ],
        )
        .current_dir(dir.path())
        .env("DESKHALLOUMI_TEST", "ready");

        let outcome = runner.run_command(command).await;

        assert_eq!(outcome.result, Ok(()));
        assert_eq!(outcome.stdout, format!("ready:{}", dir.path().display()));
    }
}
