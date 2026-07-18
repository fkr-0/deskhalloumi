#![allow(dead_code)]

//! Shared, renderer-neutral menu presentation helpers.
//!
//! These helpers keep item construction, keyboard selectability, quick-jump
//! numbering, truncation and command context consistent across DBus, built-in
//! and user-configured menus.

use crate::enhanced_tray::{TrayMenuAction, TrayMenuItem, TrayWidgetType};
use deskhalloumi_core::config::CustomMenuEnvVarConfig;
use std::{env, path::Path};

pub const SECTION_PREFIX: &str = "section:";
pub const STATUS_PREFIX: &str = "status:";
pub const CONFIRM_PREFIX: &str = "confirm:";

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ActionItemOptions {
    pub subtitle: Option<String>,
    pub icon: Option<String>,
    pub shortcut: Option<String>,
    pub enabled: bool,
}

pub fn bounded_text(value: &str, max_chars: usize) -> String {
    let max_chars = max_chars.max(1);
    let mut chars = value.chars();
    let prefix = chars.by_ref().take(max_chars).collect::<String>();
    if chars.next().is_some() {
        format!("{prefix}…")
    } else {
        prefix
    }
}

/// Remove toolkit mnemonic markers while preserving escaped underscores.
///
/// DBusMenu labels commonly encode accelerators as `_Open` and literal
/// underscores as `__`. The popup has its own keyboard navigation, so those
/// provider-specific markers should not leak into displayed text.
pub fn strip_mnemonic_markers(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    let mut chars = value.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '_' {
            if chars.peek() == Some(&'_') {
                chars.next();
                output.push('_');
            }
            continue;
        }
        output.push(ch);
    }
    output
}

pub fn label_with_subtitle(title: &str, subtitle: Option<&str>) -> String {
    match subtitle.map(str::trim).filter(|value| !value.is_empty()) {
        Some(subtitle) => format!("{}\n{}", title.trim(), subtitle),
        None => title.trim().to_string(),
    }
}

pub fn split_label(label: &str) -> (&str, Option<&str>) {
    match label.split_once('\n') {
        Some((title, subtitle)) if !subtitle.trim().is_empty() => (title, Some(subtitle)),
        _ => (label, None),
    }
}

pub fn action_item(
    app_id: &str,
    id: impl Into<String>,
    title: impl Into<String>,
    action: TrayMenuAction,
    options: ActionItemOptions,
) -> TrayMenuItem {
    let id = id.into();
    let title = title.into();
    TrayMenuItem {
        id: id.clone(),
        label: label_with_subtitle(&title, options.subtitle.as_deref()),
        action,
        icon: options.icon,
        submenu: Vec::new(),
        enabled: options.enabled,
        visible: true,
        checkable: false,
        checked: false,
        shortcut: options.shortcut,
        is_separator: false,
        app_id: app_id.to_string(),
        full_path: title,
        widget_type: TrayWidgetType::Button,
        default_value: None,
        placeholder: None,
    }
}

pub fn checkable_item(
    app_id: &str,
    id: impl Into<String>,
    title: impl Into<String>,
    action: TrayMenuAction,
    checked: bool,
    options: ActionItemOptions,
) -> TrayMenuItem {
    let mut item = action_item(app_id, id, title, action, options);
    item.checkable = true;
    item.checked = checked;
    item
}

pub fn section_item(
    app_id: &str,
    id: impl AsRef<str>,
    title: impl Into<String>,
    count: Option<usize>,
) -> TrayMenuItem {
    let title = title.into();
    let label = count
        .map(|count| format!("{title} · {count}"))
        .unwrap_or_else(|| title.clone());
    let id = format!("{SECTION_PREFIX}{}", id.as_ref());
    action_item(
        app_id,
        id,
        label,
        TrayMenuAction::Activate,
        ActionItemOptions {
            subtitle: None,
            icon: None,
            shortcut: None,
            enabled: false,
        },
    )
}

pub fn status_item(
    app_id: &str,
    id: impl AsRef<str>,
    title: impl Into<String>,
    subtitle: Option<String>,
) -> TrayMenuItem {
    action_item(
        app_id,
        format!("{STATUS_PREFIX}{}", id.as_ref()),
        title,
        TrayMenuAction::Activate,
        ActionItemOptions {
            subtitle,
            icon: None,
            shortcut: None,
            enabled: false,
        },
    )
}

pub fn separator_item(app_id: &str, id: impl AsRef<str>) -> TrayMenuItem {
    TrayMenuItem {
        id: format!("separator:{}", id.as_ref()),
        label: String::new(),
        action: TrayMenuAction::Activate,
        icon: None,
        submenu: Vec::new(),
        enabled: false,
        visible: true,
        checkable: false,
        checked: false,
        shortcut: None,
        is_separator: true,
        app_id: app_id.to_string(),
        full_path: String::new(),
        widget_type: TrayWidgetType::Separator,
        default_value: None,
        placeholder: None,
    }
}

pub fn confirmation_submenu(
    app_id: &str,
    id: impl Into<String>,
    title: impl Into<String>,
    subtitle: impl Into<String>,
    command: String,
    icon: Option<String>,
    shortcut: Option<String>,
) -> TrayMenuItem {
    let id = id.into();
    let title = title.into();
    let subtitle = subtitle.into();
    let mut parent = action_item(
        app_id,
        id.clone(),
        title.clone(),
        TrayMenuAction::NavigateToSubmenu {
            item_id: id.clone(),
            submenu_path: vec![id.clone()],
        },
        ActionItemOptions {
            subtitle: Some(subtitle.clone()),
            icon,
            shortcut,
            enabled: true,
        },
    );
    parent.widget_type = TrayWidgetType::SubmenuButton;
    parent.submenu = vec![
        status_item(
            app_id,
            format!("{CONFIRM_PREFIX}{id}:warning"),
            format!("Confirm {title}"),
            Some(subtitle),
        ),
        action_item(
            app_id,
            format!("{CONFIRM_PREFIX}{id}:execute"),
            "Run action",
            TrayMenuAction::SpawnCommand(command),
            ActionItemOptions {
                subtitle: None,
                icon: Some("dialog-warning".to_string()),
                shortcut: Some("Enter".to_string()),
                enabled: true,
            },
        ),
        action_item(
            app_id,
            format!("{CONFIRM_PREFIX}{id}:cancel"),
            "Cancel",
            TrayMenuAction::NavigateToSubmenu {
                item_id: id.clone(),
                submenu_path: Vec::new(),
            },
            ActionItemOptions {
                subtitle: Some("Return without running the command".to_string()),
                icon: Some("window-close".to_string()),
                shortcut: Some("Esc".to_string()),
                enabled: true,
            },
        ),
    ];
    parent
}

pub fn is_section_item(item: &TrayMenuItem) -> bool {
    item.id.starts_with(SECTION_PREFIX)
}

pub fn is_status_item(item: &TrayMenuItem) -> bool {
    item.id.starts_with(STATUS_PREFIX) || item.id.contains(":warning")
}

pub fn is_selectable(item: &TrayMenuItem) -> bool {
    item.visible && item.enabled && !item.is_separator
}

pub fn selectable_visible_indices(items: &[TrayMenuItem]) -> Vec<usize> {
    items
        .iter()
        .filter(|item| item.visible)
        .enumerate()
        .filter_map(|(index, item)| is_selectable(item).then_some(index))
        .collect()
}

pub fn quickjump_hint_for_visible_index(
    items: &[TrayMenuItem],
    visible_index: usize,
    labels: &[String],
) -> Option<String> {
    let selectable = selectable_visible_indices(items);
    selectable
        .iter()
        .position(|index| *index == visible_index)
        .and_then(|position| labels.get(position).cloned())
}

pub fn shell_escape(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

pub fn contextual_shell_command(
    command: &str,
    working_dir: Option<&str>,
    variables: &[CustomMenuEnvVarConfig],
) -> String {
    let mut parts = Vec::new();
    if let Some(directory) = working_dir.map(str::trim).filter(|value| !value.is_empty()) {
        parts.push(format!("cd {}", shell_escape(directory)));
    }
    if !variables.is_empty() {
        let assignments = variables
            .iter()
            .filter(|entry| valid_env_key(&entry.key))
            .map(|entry| format!("{}={}", entry.key, shell_escape(&entry.value)))
            .collect::<Vec<_>>()
            .join(" ");
        if !assignments.is_empty() {
            parts.push(format!("export {assignments}"));
        }
    }
    parts.push(command.to_string());
    parts.join(" && ")
}

fn valid_env_key(key: &str) -> bool {
    let mut chars = key.chars();
    matches!(chars.next(), Some(first) if first == '_' || first.is_ascii_alphabetic())
        && chars.all(|ch| ch == '_' || ch.is_ascii_alphanumeric())
}

pub fn visible_if_matches(condition: Option<&str>) -> bool {
    let Some(condition) = condition.map(str::trim).filter(|value| !value.is_empty()) else {
        return true;
    };
    if let Some(inner) = condition.strip_prefix("not:") {
        return !visible_if_matches(Some(inner));
    }
    if let Some(name) = condition.strip_prefix("env:") {
        return env::var_os(name.trim()).is_some();
    }
    if let Some(path) = condition.strip_prefix("path:") {
        return expand_home(path.trim()).is_some_and(|path| path.exists());
    }
    if let Some(program) = condition.strip_prefix("command:") {
        return executable_on_path(program.trim());
    }
    false
}

fn expand_home(raw: &str) -> Option<std::path::PathBuf> {
    if raw == "~" {
        return env::var_os("HOME").map(std::path::PathBuf::from);
    }
    if let Some(rest) = raw.strip_prefix("~/") {
        return env::var_os("HOME").map(|home| std::path::PathBuf::from(home).join(rest));
    }
    Some(std::path::PathBuf::from(raw))
}

fn executable_on_path(program: &str) -> bool {
    if program.is_empty() {
        return false;
    }
    if program.contains('/') {
        return Path::new(program).is_file();
    }
    env::var_os("PATH")
        .map(|paths| env::split_paths(&paths).any(|path| path.join(program).is_file()))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounded_text_is_unicode_safe() {
        assert_eq!(bounded_text("abcdef", 4), "abcd…");
        assert_eq!(bounded_text("äöü", 3), "äöü");
    }

    #[test]
    fn mnemonic_markers_are_removed_without_destroying_literal_underscores() {
        assert_eq!(strip_mnemonic_markers("_Open"), "Open");
        assert_eq!(strip_mnemonic_markers("Save __As"), "Save _As");
        assert_eq!(
            strip_mnemonic_markers("Title\n_Subtitle"),
            "Title\nSubtitle"
        );
    }

    #[test]
    fn confirmation_submenu_exposes_run_and_cancel_actions() {
        let confirmation = confirmation_submenu(
            "app",
            "delete",
            "Delete",
            "This cannot be undone",
            "rm -- file".to_string(),
            None,
            None,
        );
        assert_eq!(confirmation.submenu.len(), 3);
        assert!(!confirmation.submenu[0].enabled);
        assert_eq!(
            selectable_visible_indices(&confirmation.submenu),
            vec![1, 2]
        );
        assert!(matches!(
            confirmation.submenu[2].action,
            TrayMenuAction::NavigateToSubmenu { ref submenu_path, .. } if submenu_path.is_empty()
        ));
    }

    #[test]
    fn quickjump_hints_skip_sections_and_disabled_rows() {
        let items = vec![
            section_item("app", "one", "One", None),
            action_item(
                "app",
                "a",
                "A",
                TrayMenuAction::Activate,
                ActionItemOptions {
                    subtitle: None,
                    icon: None,
                    shortcut: None,
                    enabled: true,
                },
            ),
            action_item(
                "app",
                "b",
                "B",
                TrayMenuAction::Activate,
                ActionItemOptions {
                    subtitle: None,
                    icon: None,
                    shortcut: None,
                    enabled: false,
                },
            ),
            action_item(
                "app",
                "c",
                "C",
                TrayMenuAction::Activate,
                ActionItemOptions {
                    subtitle: None,
                    icon: None,
                    shortcut: None,
                    enabled: true,
                },
            ),
        ];
        let labels = vec!["a".to_string(), "s".to_string()];
        assert_eq!(
            quickjump_hint_for_visible_index(&items, 1, &labels),
            Some("a".into())
        );
        assert_eq!(quickjump_hint_for_visible_index(&items, 2, &labels), None);
        assert_eq!(
            quickjump_hint_for_visible_index(&items, 3, &labels),
            Some("s".into())
        );
    }

    #[test]
    fn contextual_commands_quote_directory_and_environment() {
        let command = contextual_shell_command(
            "printf ok",
            Some("/tmp/my dir"),
            &[CustomMenuEnvVarConfig {
                key: "MODE".into(),
                value: "a b".into(),
            }],
        );
        assert_eq!(
            command,
            "cd '/tmp/my dir' && export MODE='a b' && printf ok"
        );
    }
}
