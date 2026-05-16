use unilii_core::key_engine::KeyTrigger;
use unilii_core::key_import_sxhkd::import_sxhkd_config;

#[test]
fn imports_simple_sxhkd_bindings() {
    let sxhkd = r#"
super + Return
    alacritty

super + shift + r
    unilii reload
"#;

    let imported = import_sxhkd_config(sxhkd);
    assert!(imported.warnings.is_empty());
    assert_eq!(imported.bindings.len(), 2);

    assert_eq!(imported.bindings[0].keysym, "super+Return");
    assert_eq!(imported.bindings[0].command, "alacritty");
    assert_eq!(imported.bindings[0].trigger, KeyTrigger::Press);

    assert_eq!(imported.bindings[1].keysym, "super+shift+r");
    assert_eq!(imported.bindings[1].command, "unilii reload");
}

#[test]
fn imports_release_prefixed_chords_as_release_trigger() {
    let sxhkd = r#"
@super + space
    rofi -show drun
"#;

    let imported = import_sxhkd_config(sxhkd);
    assert_eq!(imported.bindings.len(), 1);
    assert_eq!(imported.bindings[0].keysym, "super+space");
    assert!(imported.bindings[0].release);
    assert_eq!(imported.bindings[0].trigger, KeyTrigger::Release);
}

#[test]
fn warns_on_chord_without_command() {
    let sxhkd = r#"
super + Return
"#;

    let imported = import_sxhkd_config(sxhkd);
    assert!(imported.bindings.is_empty());
    assert_eq!(imported.warnings.len(), 1);
    assert!(imported.warnings[0].message.contains("no command"));
}
