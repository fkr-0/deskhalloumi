use std::time::{Duration, Instant};

use deskhalloumi_core::key_engine::{EngineBinding, KeyEngine, KeyTrigger};

fn binding(
    name: &str,
    groups: Vec<Vec<&str>>,
    trigger: KeyTrigger,
    priority: u16,
    consume: bool,
    hold_ms: u64,
) -> EngineBinding {
    let groups_owned = groups
        .into_iter()
        .map(|g| g.into_iter().map(|s| s.to_string()).collect::<Vec<_>>())
        .collect::<Vec<_>>();
    let trigger_keys = groups_owned.last().cloned().unwrap_or_default();

    EngineBinding::new(
        name.to_string(),
        groups_owned,
        trigger,
        priority,
        consume,
        hold_ms,
        None,
        trigger_keys,
    )
}

#[test]
fn press_binding_triggers_on_key_press() {
    let mut engine = KeyEngine::new(vec![binding(
        "terminal",
        vec![vec!["KEY_LEFTMETA"], vec!["KEY_RETURN"]],
        KeyTrigger::Press,
        10,
        false,
        0,
    )]);

    let now = Instant::now();
    assert!(
        engine
            .process_event("KEY_LEFTMETA", 1, now)
            .triggered
            .is_empty()
    );
    let output = engine.process_event("KEY_RETURN", 1, now + Duration::from_millis(1));
    assert_eq!(output.triggered, vec![0]);
}

#[test]
fn release_binding_triggers_on_trigger_key_release() {
    let mut engine = KeyEngine::new(vec![binding(
        "launcher",
        vec![vec!["KEY_LEFTMETA"], vec!["KEY_SPACE"]],
        KeyTrigger::Release,
        10,
        false,
        0,
    )]);

    let now = Instant::now();
    assert!(
        engine
            .process_event("KEY_LEFTMETA", 1, now)
            .triggered
            .is_empty()
    );
    assert!(
        engine
            .process_event("KEY_SPACE", 1, now + Duration::from_millis(1))
            .triggered
            .is_empty()
    );

    let output = engine.process_event("KEY_SPACE", 0, now + Duration::from_millis(2));
    assert_eq!(output.triggered, vec![0]);
}

#[test]
fn modrelease_requires_modifier_release_and_hold_threshold() {
    let mut engine = KeyEngine::new(vec![binding(
        "menu",
        vec![vec!["KEY_LEFTMETA"], vec!["KEY_Q"]],
        KeyTrigger::Modrelease,
        10,
        false,
        100,
    )]);

    let now = Instant::now();
    assert!(
        engine
            .process_event("KEY_LEFTMETA", 1, now)
            .triggered
            .is_empty()
    );
    assert!(
        engine
            .process_event("KEY_Q", 1, now + Duration::from_millis(1))
            .triggered
            .is_empty()
    );

    // Non-modifier release must not trigger modrelease.
    assert!(
        engine
            .process_event("KEY_Q", 0, now + Duration::from_millis(50))
            .triggered
            .is_empty()
    );

    // Re-arm and satisfy hold threshold on modifier release.
    assert!(
        engine
            .process_event("KEY_Q", 1, now + Duration::from_millis(60))
            .triggered
            .is_empty()
    );
    let output = engine.process_event("KEY_LEFTMETA", 0, now + Duration::from_millis(120));
    assert_eq!(output.triggered, vec![0]);
}

#[test]
fn higher_priority_consuming_binding_suppresses_lower_binding() {
    let mut engine = KeyEngine::new(vec![
        binding(
            "high",
            vec![vec!["KEY_LEFTMETA"], vec!["KEY_RETURN"]],
            KeyTrigger::Press,
            100,
            true,
            0,
        ),
        binding(
            "low",
            vec![vec!["KEY_LEFTMETA"], vec!["KEY_RETURN"]],
            KeyTrigger::Press,
            10,
            false,
            0,
        ),
    ]);

    let now = Instant::now();
    assert!(
        engine
            .process_event("KEY_LEFTMETA", 1, now)
            .triggered
            .is_empty()
    );
    let output = engine.process_event("KEY_RETURN", 1, now + Duration::from_millis(1));
    assert_eq!(output.triggered, vec![0]);
}

#[test]
fn repeat_binding_triggers_on_evdev_repeat_events() {
    let mut engine = KeyEngine::new(vec![binding(
        "volume",
        vec![vec!["KEY_LEFTMETA"], vec!["KEY_UP"]],
        KeyTrigger::Repeat,
        10,
        false,
        0,
    )]);
    let now = Instant::now();
    assert!(
        engine
            .process_event("KEY_LEFTMETA", 1, now)
            .triggered
            .is_empty()
    );
    assert_eq!(
        engine
            .process_event("KEY_UP", 1, now + Duration::from_millis(1))
            .triggered,
        vec![0]
    );
    assert_eq!(
        engine
            .process_event("KEY_UP", 2, now + Duration::from_millis(20))
            .triggered,
        vec![0]
    );
    assert_eq!(
        engine
            .process_event("KEY_UP", 2, now + Duration::from_millis(40))
            .triggered,
        vec![0]
    );
}
