use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::Duration;

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_unilii-i3-vis")
}

fn fixture_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("i3_vis_tree.json")
}

fn fake_i3_msg(dir: &Path) -> PathBuf {
    let script = dir.join("fake-i3-msg");
    let fixture = fixture_path();
    let log = dir.join("i3-msg.log");
    fs::write(
        &script,
        format!(
            r#"#!/bin/sh
set -eu
if [ "$#" -ge 2 ] && [ "$1" = "-t" ] && [ "$2" = "get_tree" ]; then
  cat '{}'
  exit 0
fi
printf '%s\n' "$*" >> '{}'
exit 0
"#,
            fixture.display(),
            log.display()
        ),
    )
    .expect("write fake i3-msg");
    let mut perms = fs::metadata(&script).expect("metadata").permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&script, perms).expect("chmod fake i3-msg");
    script
}

#[test]
fn dump_text_renders_workspace_tree_from_fake_i3_msg() {
    let temp = tempfile::tempdir().expect("tempdir");
    let fake = fake_i3_msg(temp.path());

    let output = Command::new(bin())
        .arg("--i3-msg")
        .arg(&fake)
        .arg("--dump-text")
        .output()
        .expect("run unilii-i3-vis");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("i3-vis"),
        "expected headless tree output from {}; stdout={stdout:?}; stderr={:?}",
        bin(),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(stdout.contains("3 windows · selected Firefox · ChatGPT"));
    assert!(stdout.contains("Selected: Firefox · ChatGPT"));
    assert!(stdout.contains("◇   workspace 2 · workspace"));
    assert!(stdout.contains("↕   2 vertical · vertical"));
    assert!(stdout.contains("↔   2 horizontal · horizontal"));
    assert!(stdout.contains("Emacs · main.rs - Emacs"));
    assert!(stdout.contains("▶"));
    assert!(stdout.contains("★ Firefox · ChatGPT"));
    assert!(stdout.contains("XTerm · logs"));
    assert!(
        !stdout.contains("unilii-i3-vis"),
        "popup should be filtered out: {stdout}"
    );
}

#[test]
fn dump_text_actions_confirm_next_window_through_fake_i3_msg() {
    let temp = tempfile::tempdir().expect("tempdir");
    let fake = fake_i3_msg(temp.path());
    let log = temp.path().join("i3-msg.log");

    let output = Command::new(bin())
        .arg("--i3-msg")
        .arg(&fake)
        .arg("--dump-text")
        .arg("--e2e-actions")
        .arg("next,release")
        .output()
        .expect("run unilii-i3-vis");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Focused #24"));
    assert!(stdout.contains("Selected: XTerm · logs"));
    assert!(stdout.contains("▶"));
    assert!(stdout.contains("XTerm · logs"));

    let log = fs::read_to_string(log).expect("read fake i3-msg log");
    assert_eq!(log.trim(), "[con_id=24] focus");
}

#[test]
fn dump_text_hjkl_passthrough_commands_execute_through_fake_i3_msg() {
    let temp = tempfile::tempdir().expect("tempdir");
    let fake = fake_i3_msg(temp.path());
    let log = temp.path().join("i3-msg.log");

    let output = Command::new(bin())
        .arg("--i3-msg")
        .arg(&fake)
        .arg("--dump-text")
        .arg("--e2e-actions")
        .arg("h,j,k,l,H,J,K,L")
        .output()
        .expect("run unilii-i3-vis");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Executed i3 move right"));

    let log = fs::read_to_string(log).expect("read fake i3-msg log");
    assert_eq!(
        log.lines().collect::<Vec<_>>(),
        vec![
            "focus left",
            "focus down",
            "focus up",
            "focus right",
            "move left",
            "move down",
            "move up",
            "move right",
        ]
    );
}

#[test]
fn dump_text_escape_restores_startup_focus_after_passthrough() {
    let temp = tempfile::tempdir().expect("tempdir");
    let fake = fake_i3_msg(temp.path());
    let log = temp.path().join("i3-msg.log");

    let output = Command::new(bin())
        .arg("--i3-msg")
        .arg(&fake)
        .arg("--dump-text")
        .arg("--e2e-actions")
        .arg("h,escape")
        .output()
        .expect("run unilii-i3-vis");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Restored startup focus #23"));
    assert!(stdout.contains("Selected: Firefox · ChatGPT"));

    let log = fs::read_to_string(log).expect("read fake i3-msg log");
    assert_eq!(log, "focus left\n[con_id=23] focus\n");
}

#[test]
fn dump_text_unknown_action_fails() {
    let temp = tempfile::tempdir().expect("tempdir");
    let fake = fake_i3_msg(temp.path());

    let output = Command::new(bin())
        .arg("--i3-msg")
        .arg(&fake)
        .arg("--dump-text")
        .arg("--e2e-actions")
        .arg("wat")
        .output()
        .expect("run unilii-i3-vis");

    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("unknown e2e action: wat"));
}

#[test]
#[ignore = "requires DISPLAY, i3-compatible focus/window search tools, and ImageMagick import"]
fn screenshot_smoke_writes_png_artifact_when_enabled() -> Result<(), Box<dyn std::error::Error>> {
    if std::env::var_os("RUN_I3_VIS_SCREENSHOT_E2E").is_none() {
        eprintln!("set RUN_I3_VIS_SCREENSHOT_E2E=1 to run screenshot smoke test");
        return Ok(());
    }
    if std::env::var_os("DISPLAY").is_none() {
        eprintln!("DISPLAY is not set; skipping screenshot smoke test");
        return Ok(());
    }

    let artifact = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("i3-vis-smoke.png");
    if let Some(parent) = artifact.parent() {
        fs::create_dir_all(parent)?;
    }

    let mut child = Command::new(bin())
        .arg("--mock")
        .arg("--no-exec")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;

    thread::sleep(Duration::from_millis(900));

    let capture = Command::new("sh")
        .arg("-c")
        .arg(format!(
            "window=$(xdotool search --class unilii-i3-vis | tail -n1); test -n \"$window\"; import -window \"$window\" '{}'",
            artifact.display()
        ))
        .status();

    let _ = child.kill();
    let _ = child.wait();

    match capture {
        Ok(status) if status.success() => {
            assert!(artifact.exists());
            assert!(fs::metadata(&artifact)?.len() > 0);
        }
        Ok(status) => eprintln!("screenshot tools returned {status}; artifact not asserted"),
        Err(error) => eprintln!("screenshot tools unavailable: {error}"),
    }

    Ok(())
}
