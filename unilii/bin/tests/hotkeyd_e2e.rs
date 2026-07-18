use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_deskhalloumi-hotkeyd")
}

fn legacy_bin() -> &'static str {
    env!("CARGO_BIN_EXE_unilii-hotkeyd")
}

#[test]
fn primary_and_legacy_hotkey_commands_coexist() {
    for command in [bin(), legacy_bin()] {
        let output = Command::new(command)
            .arg("--print-defaults")
            .output()
            .unwrap();
        assert!(output.status.success());
        assert!(String::from_utf8_lossy(&output.stdout).contains("menu_i3_vis"));
    }
}

#[test]
fn sxhkd_source_can_be_exported_as_safe_i3_include() {
    let temp = tempfile::tempdir().expect("tempdir");
    let sxhkd = write_file(
        temp.path(),
        "sxhkdrc",
        "super + Return\n    alacritty\n\n@super + space\n    rofi -show drun\n",
    );
    let target = temp.path().join("i3-bindings.conf");

    let output = Command::new(bin())
        .arg("--sxhkd")
        .arg(&sxhkd)
        .arg("--write-i3-bindings")
        .arg(&target)
        .output()
        .expect("export i3 include");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let generated = fs::read_to_string(target).expect("read generated include");
    assert!(generated.contains("bindsym Mod4+Return"));
    assert!(generated.contains("bindsym --release Mod4+space"));
    assert!(generated.contains("alacritty"));
    assert!(generated.contains("rofi -show drun"));
}

#[test]
fn strict_i3_export_rejects_unrepresentable_trigger() {
    let temp = tempfile::tempdir().expect("tempdir");
    let config = write_file(
        temp.path(),
        "hotkeys.toml",
        r#"
[[keybindings]]
name = "held"
keysym = "Super+space"
command = "echo held"
trigger = "modrelease"
hold_ms = 200
"#,
    );
    let target = temp.path().join("must-not-exist.conf");
    let output = Command::new(bin())
        .args([
            "--config",
            config.to_str().unwrap(),
            "--write-i3-bindings",
            target.to_str().unwrap(),
            "--strict",
        ])
        .output()
        .expect("strict i3 export");
    assert_eq!(output.status.code(), Some(3));
    assert!(String::from_utf8_lossy(&output.stderr).contains("modrelease"));
    assert!(
        !target.exists(),
        "strict validation must be side-effect free"
    );
}

fn write_file(dir: &Path, name: &str, content: &str) -> PathBuf {
    let path = dir.join(name);
    fs::write(&path, content).expect("write test file");
    path
}

#[test]
fn print_defaults_contains_menu_launchers() {
    let output = Command::new(bin())
        .arg("--print-defaults")
        .output()
        .expect("run unilii-hotkeyd");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("name = \"menu_i3_vis\""));
    assert!(stdout.contains("keysym = \"Super+i\""));
    assert!(stdout.contains("command = \"toggle:i3-vis\""));
    assert!(stdout.contains("name = \"menu_filter_tab\""));
    assert!(stdout.contains("command = \"toggle:filter-tab\""));
    assert!(stdout.contains("name = \"menu_copyq\""));
    assert!(stdout.contains("command = \"toggle:copyq\""));
    assert!(stdout.matches("type = \"menu\"").count() >= 3);
}

#[test]
fn dry_run_sxhkd_reports_duplicates_and_expands_simple_braces() {
    let temp = tempfile::tempdir().expect("tempdir");
    let sxhkd = write_file(
        temp.path(),
        "sxhkdrc",
        r#"
super + i
    unilii-i3-vis

super + i
    echo duplicate

super + {u,c}
    echo brace
"#,
    );

    let output = Command::new(bin())
        .arg("--sxhkd")
        .arg(&sxhkd)
        .arg("--dry-run")
        .output()
        .expect("run unilii-hotkeyd");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("deskhalloumi-hotkeyd report"));
    assert!(stdout.contains("bindings: 4"));
    assert!(stdout.contains("managed menu bindings: 1"));
    assert!(!stdout.contains("unsupported chord expansion"));
    assert!(stdout.contains("duplicate chords:"));
    assert!(stdout.contains("super+i trigger=press"));
    assert!(stdout.contains("sxhkd_import_1"));
    assert!(stdout.contains("sxhkd_import_2"));
}

#[test]
fn dry_run_config_reports_shadowed_bindings() {
    let temp = tempfile::tempdir().expect("tempdir");
    let config = write_file(
        temp.path(),
        "hotkeys.toml",
        r#"
[[keybindings]]
name = "high"
keysym = "Super+i"
command = "unilii-i3-vis"
type = "shell"
priority = 100
consume = true

[[keybindings]]
name = "low"
keysym = "super + i"
command = "echo low"
type = "shell"
priority = 1
consume = false
"#,
    );

    let output = Command::new(bin())
        .arg("--config")
        .arg(&config)
        .arg("--dry-run")
        .output()
        .expect("run unilii-hotkeyd");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("bindings: 2"));
    assert!(stdout.contains("duplicate chords:"));
    assert!(stdout.contains("high"));
    assert!(stdout.contains("low"));
    assert!(stdout.contains("shadowed bindings:"));
    assert!(stdout.contains("low is shadowed by high"));
}

#[test]
fn shadow_and_grab_is_diagnosed_without_starting_listener_in_dry_run() {
    let output = Command::new(bin())
        .arg("--menu-defaults")
        .arg("--shadow")
        .arg("--grab")
        .arg("--dry-run")
        .output()
        .expect("run unilii-hotkeyd");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("bindings: 3"));
}

#[test]
fn status_and_hide_are_safe_without_running_menu() {
    let temp = tempfile::tempdir().expect("tempdir");
    let status = Command::new(bin())
        .env("UNILII_RUNTIME_DIR", temp.path())
        .arg("--status")
        .output()
        .expect("run status");
    assert!(status.status.success());
    let stdout = String::from_utf8_lossy(&status.stdout);
    assert!(stdout.contains("hotkeyd: stopped"));
    assert!(stdout.contains("menu i3-vis: hidden"));
    assert!(stdout.contains("menu filter-tab: hidden"));
    assert!(stdout.contains("menu copyq: hidden"));

    let hide = Command::new(bin())
        .env("UNILII_RUNTIME_DIR", temp.path())
        .args(["--menu-action", "hide:i3-vis"])
        .output()
        .expect("hide missing menu");
    assert!(hide.status.success());
    assert!(String::from_utf8_lossy(&hide.stdout).contains("AlreadyHidden"));
}

#[test]
fn strict_report_fails_for_migration_warnings() {
    let temp = tempfile::tempdir().expect("tempdir");
    let sxhkd = write_file(
        temp.path(),
        "sxhkdrc",
        "super + {1-3}\n    echo unsupported\n",
    );
    let output = Command::new(bin())
        .args(["--sxhkd", sxhkd.to_str().unwrap(), "--dry-run", "--strict"])
        .output()
        .expect("run strict report");
    assert_eq!(output.status.code(), Some(3));
    assert!(String::from_utf8_lossy(&output.stdout).contains("simple comma-separated expansion"));
}

#[test]
fn active_grab_requires_explicit_unsafe_acknowledgement() {
    let output = Command::new(bin())
        .args(["--menu-defaults", "--grab"])
        .output()
        .expect("run grab safety check");
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("--allow-unsafe-evdev-grab"));
}

#[test]
fn status_json_falls_back_cleanly_without_daemon() {
    let temp = tempfile::tempdir().expect("tempdir");
    let output = Command::new(bin())
        .env("UNILII_RUNTIME_DIR", temp.path())
        .args(["--status", "--json"])
        .output()
        .expect("run json status");
    assert!(output.status.success());
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["control_available"], false);
    assert!(value["hotkeyd_pid"].is_null());
    assert_eq!(value["menus"].as_array().unwrap().len(), 3);
}

#[test]
fn reload_without_running_daemon_fails_with_control_diagnostic() {
    let temp = tempfile::tempdir().expect("tempdir");
    let output = Command::new(bin())
        .env("UNILII_RUNTIME_DIR", temp.path())
        .arg("--reload")
        .output()
        .expect("run reload client");
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("control socket"));
}

#[test]
fn conflicting_control_commands_are_rejected() {
    let output = Command::new(bin())
        .args(["--status", "--reload"])
        .output()
        .expect("run conflicting controls");
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("choose only one"));
}

#[test]
fn standalone_report_accepts_versioned_bar_action() {
    let temp = tempfile::tempdir().expect("tempdir");
    let config = write_file(
        temp.path(),
        "bar-action.toml",
        r#"
[[keybindings]]
name = "reload_bar"
keysym = "Super+r"
command = "reload-config"
type = "bar"
"#,
    );
    let output = Command::new(bin())
        .args(["--config", config.to_str().unwrap(), "--dry-run"])
        .output()
        .expect("run report");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("invalid bindings:\n  <none>"));
}
