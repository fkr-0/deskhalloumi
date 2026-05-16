#![allow(dead_code)]
// FIXME(T6): Transitional action execution API is covered by unit tests and will be wired through the typed action bus task.

use std::{
    ffi::OsString,
    future::Future,
    io::{self, Read},
    path::PathBuf,
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionCommand {
    pub program: PathBuf,
    pub args: Vec<OsString>,
}

impl ActionCommand {
    pub fn new(program: impl Into<PathBuf>, args: Vec<OsString>) -> Self {
        Self {
            program: program.into(),
            args,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionRunner {
    pub menu: String,
    pub action: String,
    timeout: Duration,
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
        }
    }

    pub async fn run<T, E, F>(&self, work: F) -> ActionOutcome<T>
    where
        F: Future<Output = Result<T, E>> + Send,
        T: Send,
        E: ToString,
    {
        let started_at = Instant::now();
        let result = work.await.map_err(|error| error.to_string());

        ActionOutcome {
            menu: self.menu.clone(),
            action: self.action.clone(),
            duration_ms: started_at.elapsed().as_millis(),
            exit_code: None,
            error_class: result.as_ref().err().map(|_| "error".to_string()),
            stdout: String::new(),
            stderr: String::new(),
            result,
        }
    }

    pub async fn run_command(&self, command: ActionCommand) -> ActionOutcome<()> {
        let menu = self.menu.clone();
        let action = self.action.clone();
        let timeout = self.timeout;

        let started_at = Instant::now();
        match tokio::task::spawn_blocking({
            let menu = menu.clone();
            let action = action.clone();
            move || run_command_blocking(menu, action, timeout, command)
        })
        .await
        {
            Ok(outcome) => outcome,
            Err(error) => ActionOutcome {
                menu,
                action,
                duration_ms: started_at.elapsed().as_millis(),
                exit_code: None,
                error_class: Some("task_join_error".to_string()),
                stdout: String::new(),
                stderr: String::new(),
                result: Err(format!("blocking action runner task failed: {error}")),
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
    pub result: Result<T, String>,
}

fn run_command_blocking(
    menu: String,
    action: String,
    timeout: Duration,
    command: ActionCommand,
) -> ActionOutcome<()> {
    let started_at = Instant::now();
    let mut child = Command::new(&command.program);
    child.args(&command.args);
    child.stdin(Stdio::null());
    child.stdout(Stdio::piped());
    child.stderr(Stdio::piped());

    let mut child = match child.spawn() {
        Ok(child) => child,
        Err(source) => {
            let message = format!(
                "failed to spawn action `{}` for menu `{}` action `{}`: {source}",
                command.program.display(),
                menu,
                action
            );
            return ActionOutcome {
                menu,
                action,
                duration_ms: started_at.elapsed().as_millis(),
                exit_code: None,
                error_class: Some("spawn".to_string()),
                stdout: String::new(),
                stderr: String::new(),
                result: Err(message),
            };
        }
    };

    let stdout_handle = child.stdout.take().map(read_to_string_thread);
    let stderr_handle = child.stderr.take().map(read_to_string_thread);
    let deadline = Instant::now() + timeout;

    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let stdout = join_reader(stdout_handle);
                let stderr = join_reader(stderr_handle);
                let duration_ms = started_at.elapsed().as_millis();
                let exit_code = status.code();
                let success = status.success();
                let result = if success {
                    Ok(())
                } else {
                    Err(format!(
                        "action `{}` for menu `{}` exited with status {:?}",
                        action, menu, exit_code
                    ))
                };

                return ActionOutcome {
                    menu,
                    action,
                    duration_ms,
                    exit_code,
                    error_class: if success {
                        None
                    } else {
                        Some("non_zero_exit".to_string())
                    },
                    stdout,
                    stderr,
                    result,
                };
            }
            Ok(None) => {}
            Err(source) => {
                let stdout = join_reader(stdout_handle);
                let stderr = join_reader(stderr_handle);
                let message = format!(
                    "failed to wait for action `{}` in menu `{}`: {source}",
                    action, menu
                );
                return ActionOutcome {
                    menu,
                    action,
                    duration_ms: started_at.elapsed().as_millis(),
                    exit_code: None,
                    error_class: Some("wait".to_string()),
                    stdout,
                    stderr,
                    result: Err(message),
                };
            }
        }

        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            let stdout = join_reader(stdout_handle);
            let stderr = join_reader(stderr_handle);
            let message = format!(
                "action `{}` for menu `{}` timed out after {:?}",
                action, menu, timeout
            );
            return ActionOutcome {
                menu,
                action,
                duration_ms: started_at.elapsed().as_millis(),
                exit_code: None,
                error_class: Some("timeout".to_string()),
                stdout,
                stderr,
                result: Err(message),
            };
        }

        thread::sleep(Duration::from_millis(10));
    }
}

fn read_to_string_thread<R>(mut reader: R) -> thread::JoinHandle<io::Result<String>>
where
    R: Read + Send + 'static,
{
    thread::spawn(move || {
        let mut buffer = String::new();
        reader.read_to_string(&mut buffer)?;
        Ok(buffer)
    })
}

fn join_reader(handle: Option<thread::JoinHandle<io::Result<String>>>) -> String {
    match handle {
        Some(handle) => match handle.join() {
            Ok(Ok(output)) => output,
            Ok(Err(_)) | Err(_) => String::new(),
        },
        None => String::new(),
    }
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
}
