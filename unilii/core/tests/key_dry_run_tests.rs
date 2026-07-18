use deskhalloumi_core::key_engine::KeyTrigger;
use deskhalloumi_core::keys::{CommandType, KeyBinding, KeyDryRunEvent, dry_run_bindings};

#[test]
fn dry_run_reports_triggered_binding_names() {
    let bindings = vec![KeyBinding {
        name: "terminal".to_string(),
        keysym: "Super+Return".to_string(),
        command: "alacritty".to_string(),
        command_type: CommandType::Shell,
        release: false,
        trigger: KeyTrigger::Press,
        hold_ms: None,
        cooldown_ms: None,
        priority: 10,
        consume: true,
    }];

    let events = vec![
        KeyDryRunEvent {
            key: "KEY_LEFTMETA".to_string(),
            value: 1,
            at_ms: 0,
        },
        KeyDryRunEvent {
            key: "KEY_ENTER".to_string(),
            value: 1,
            at_ms: 10,
        },
    ];

    let steps = dry_run_bindings(&bindings, &events).expect("dry-run should succeed");
    assert_eq!(steps.len(), 2);
    assert!(steps[0].triggered_binding_names.is_empty());
    assert_eq!(steps[1].triggered_binding_names, vec!["terminal"]);
}
