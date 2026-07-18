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
fn expands_numeric_ranges_pairwise_with_commands() {
    let sxhkd = "super + {1-3}\n    i3-msg workspace {1-3}\n";
    let imported = import_sxhkd_config(sxhkd);
    assert!(imported.warnings.is_empty());
    assert_eq!(imported.bindings.len(), 3);
    assert_eq!(imported.bindings[0].keysym, "super+1");
    assert_eq!(imported.bindings[0].command, "i3-msg workspace 1");
    assert_eq!(imported.bindings[2].keysym, "super+3");
    assert_eq!(imported.bindings[2].command, "i3-msg workspace 3");
}

#[test]
fn expands_empty_sequence_elements_after_chord_normalization() {
    let sxhkd = r#"
super + {_,shift + } Return
    {alacritty,alacritty --class alternate}
"#;
    let imported = import_sxhkd_config(sxhkd);
    assert!(imported.warnings.is_empty());
    assert_eq!(imported.bindings.len(), 2);
    assert_eq!(imported.bindings[0].keysym, "super+Return");
    assert_eq!(imported.bindings[1].keysym, "super+shift+Return");
}

#[test]
fn strips_synchronous_prefix_with_explicit_semantic_warning() {
    let sxhkd = "super + n\n    ; notify-send ready\n";
    let imported = import_sxhkd_config(sxhkd);
    assert_eq!(imported.bindings.len(), 1);
    assert_eq!(imported.bindings[0].command, "notify-send ready");
    assert_eq!(imported.warnings.len(), 1);
    assert!(imported.warnings[0].message.contains("synchronous"));
}

#[test]
fn migration_fixture_classifies_exact_approximate_and_unsupported_constructs() {
    // unilii-audit: allow-live-session-command-reference -- this test parses fixture text and never executes i3 commands.
    let imported = import_sxhkd_config(include_str!("fixtures/sxhkd/migration-corpus.sxhkd"));

    assert_eq!(imported.bindings.len(), 8);
    assert!(
        imported.bindings.iter().any(|binding| {
            binding.keysym == "super+3" && binding.command == "i3-msg workspace 3"
        })
    );
    assert!(imported.bindings.iter().any(|binding| {
        binding.keysym == "super+shift+Return" && binding.command == "alacritty --class alternate"
    }));
    assert!(
        imported.bindings.iter().any(|binding| {
            binding.keysym == "super+x" && binding.command.contains("'{literal}'")
        })
    );
    assert!(
        imported
            .warnings
            .iter()
            .any(|warning| warning.message.contains("synchronous"))
    );
    assert!(
        imported
            .warnings
            .iter()
            .any(|warning| warning.message.contains("replay"))
    );
    assert!(
        imported
            .warnings
            .iter()
            .any(|warning| warning.message.contains("chains/modes"))
    );
    assert!(
        imported
            .warnings
            .iter()
            .any(|warning| warning.message.contains("mixed") || warning.message.contains("range"))
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
