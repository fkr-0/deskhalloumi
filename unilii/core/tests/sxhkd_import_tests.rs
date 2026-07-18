use deskhalloumi_core::key_engine::KeyTrigger;
use deskhalloumi_core::key_import_sxhkd::import_sxhkd_config;

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
fn expands_simple_chord_and_command_braces_pairwise() {
    let sxhkd = r#"
super + {h,l}
    i3-msg focus {left,right}
"#;
    let imported = import_sxhkd_config(sxhkd);
    assert!(imported.warnings.is_empty());
    assert_eq!(imported.bindings.len(), 2);
    assert_eq!(imported.bindings[0].keysym, "super+h");
    assert_eq!(imported.bindings[0].command, "i3-msg focus left");
    assert_eq!(imported.bindings[1].keysym, "super+l");
    assert_eq!(imported.bindings[1].command, "i3-msg focus right");
}

#[test]
fn skips_ranges_instead_of_importing_a_literal_invalid_chord() {
    let sxhkd = "super + {1-3}\n    echo workspace\n";
    let imported = import_sxhkd_config(sxhkd);
    assert!(imported.bindings.is_empty());
    assert_eq!(imported.warnings.len(), 1);
    assert!(
        imported.warnings[0]
            .message
            .contains("simple comma-separated")
    );
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
