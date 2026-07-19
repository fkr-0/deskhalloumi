#![allow(dead_code)]
// FIXME(T6): Custom menu model is planned toolbar/menu integration surface pending canonical MenuModel wiring.

use deskhalloumi_core::config::{CustomMenuActionConfig, CustomMenuConfig, CustomMenuItemConfig};

use super::presentation::{contextual_shell_command, shell_escape, visible_if_matches};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CustomMenuItem {
    pub id: String,
    pub title: String,
    pub subtitle: Option<String>,
    pub action_command: String,
    pub icon_theme: Option<String>,
    pub icon_svg_path: Option<String>,
    pub icon_image_path: Option<String>,
    pub filter_tokens: Vec<String>,
    pub confirm: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CustomMenuSnapshot {
    pub items: Vec<CustomMenuItem>,
    pub quickjump_alphabet: String,
}

impl CustomMenuSnapshot {
    pub fn from_config(config: &CustomMenuConfig) -> Self {
        let items = config
            .items
            .iter()
            .filter(|item| visible_if_matches(item.visible_if.as_deref()))
            .take(config.max_rows)
            .map(|item| build_custom_menu_item(item, config.show_subtitles))
            .collect::<Vec<_>>();
        Self {
            items,
            quickjump_alphabet: config.quickjump_alphabet.clone(),
        }
    }
}

pub fn build_action_command(item: &CustomMenuItemConfig) -> String {
    let command = match &item.action {
        CustomMenuActionConfig::Shell { command } => command.clone(),
        CustomMenuActionConfig::Launcher { command, args, .. } => {
            let args = args
                .iter()
                .map(|arg| shell_escape(arg))
                .collect::<Vec<_>>()
                .join(" ");
            if args.is_empty() {
                command.clone()
            } else {
                format!("{command} {args}")
            }
        }
    };
    contextual_shell_command(&command, item.working_dir.as_deref(), &item.env)
}

fn build_custom_menu_item(item: &CustomMenuItemConfig, show_subtitles: bool) -> CustomMenuItem {
    let mut filter_tokens = Vec::new();
    for field in &item.filter_fields {
        match field.as_str() {
            "title" => filter_tokens.push(item.title.clone()),
            "subtitle" => {
                if let Some(subtitle) = &item.subtitle {
                    filter_tokens.push(subtitle.clone());
                }
            }
            "id" => filter_tokens.push(item.id.clone()),
            "command" => filter_tokens.push(build_action_command(item)),
            "tags" => filter_tokens.extend(item.tags.clone()),
            _ => {}
        }
    }
    if filter_tokens.is_empty() {
        filter_tokens.push(item.title.clone());
    }
    CustomMenuItem {
        id: item.id.clone(),
        title: item.title.clone(),
        subtitle: show_subtitles.then(|| item.subtitle.clone()).flatten(),
        action_command: build_action_command(item),
        icon_theme: item.icon.theme_icon.clone(),
        icon_svg_path: item.icon.svg_path.clone(),
        icon_image_path: item.icon.image_path.clone(),
        filter_tokens,
        confirm: item.confirm,
    }
}

#[cfg(test)]
mod tests {
    use super::{CustomMenuSnapshot, build_action_command};
    use deskhalloumi_core::config::{
        CustomMenuActionConfig, CustomMenuConfig, CustomMenuIconConfig, CustomMenuItemConfig,
    };
    use deskhalloumi_core::quick_select::QuickSelectSession;

    #[test]
    fn builds_launcher_command() {
        let item = CustomMenuItemConfig {
            id: "launcher".to_string(),
            title: "Launcher".to_string(),
            subtitle: None,
            action: CustomMenuActionConfig::Launcher {
                command: "rofi".to_string(),
                args: vec!["-show".to_string(), "drun".to_string()],
                desktop_id: None,
            },
            icon: CustomMenuIconConfig::default(),
            filter_fields: vec!["title".to_string()],
            tags: Vec::new(),
            working_dir: None,
            env: Vec::new(),
            confirm: false,
            visible_if: None,
        };
        assert_eq!(build_action_command(&item), "rofi '-show' 'drun'");
    }

    #[test]
    fn snapshot_supports_filter_and_quickjump() {
        let config = CustomMenuConfig {
            enabled: true,
            max_rows: 40,
            show_subtitles: true,
            app_ids: Vec::new(),
            icon_name_patterns: Vec::new(),
            include: Vec::new(),
            sources: Vec::new(),
            items: vec![CustomMenuItemConfig {
                id: "display.docked".to_string(),
                title: "Docked Layout".to_string(),
                subtitle: Some("xrandr profile".to_string()),
                action: CustomMenuActionConfig::Shell {
                    command: "~/.local/bin/xrandr-docked.sh".to_string(),
                },
                icon: CustomMenuIconConfig::default(),
                filter_fields: vec![
                    "title".to_string(),
                    "subtitle".to_string(),
                    "command".to_string(),
                ],
                tags: vec!["display".to_string()],
                working_dir: None,
                env: Vec::new(),
                confirm: false,
                visible_if: None,
            }],
            quickjump_alphabet: "asdf".to_string(),
        };
        let snapshot = CustomMenuSnapshot::from_config(&config);
        let session = QuickSelectSession::new(
            snapshot
                .items
                .iter()
                .map(|item| (item.title.clone(), item.id.clone())),
        )
        .unwrap();
        assert_eq!(session.options()[0].shortcut, 'a');
        assert_eq!(session.options()[0].action, "display.docked");
        let tokens = snapshot.items[0]
            .filter_tokens
            .iter()
            .map(|token| token.to_ascii_lowercase())
            .collect::<Vec<_>>();
        assert!(tokens.iter().any(|token| token.contains("docked")));
        assert!(tokens.iter().any(|token| token.contains("xrandr")));
    }
    #[test]
    fn snapshot_applies_visibility_and_row_limit() {
        let config = CustomMenuConfig {
            enabled: true,
            max_rows: 1,
            items: vec![
                CustomMenuItemConfig {
                    id: "hidden".into(),
                    title: "Hidden".into(),
                    subtitle: None,
                    action: CustomMenuActionConfig::Shell {
                        command: "true".into(),
                    },
                    icon: CustomMenuIconConfig::default(),
                    filter_fields: vec!["title".into()],
                    tags: vec![],
                    working_dir: None,
                    env: vec![],
                    confirm: false,
                    visible_if: Some("env:UNILII_TEST_VARIABLE_THAT_SHOULD_NOT_EXIST".into()),
                },
                CustomMenuItemConfig {
                    id: "visible".into(),
                    title: "Visible".into(),
                    subtitle: None,
                    action: CustomMenuActionConfig::Shell {
                        command: "true".into(),
                    },
                    icon: CustomMenuIconConfig::default(),
                    filter_fields: vec!["title".into()],
                    tags: vec![],
                    working_dir: None,
                    env: vec![],
                    confirm: false,
                    visible_if: None,
                },
                CustomMenuItemConfig {
                    id: "clipped".into(),
                    title: "Clipped".into(),
                    subtitle: None,
                    action: CustomMenuActionConfig::Shell {
                        command: "true".into(),
                    },
                    icon: CustomMenuIconConfig::default(),
                    filter_fields: vec!["title".into()],
                    tags: vec![],
                    working_dir: None,
                    env: vec![],
                    confirm: false,
                    visible_if: None,
                },
            ],
            ..Default::default()
        };
        let snapshot = CustomMenuSnapshot::from_config(&config);
        assert_eq!(snapshot.items.len(), 1);
        assert_eq!(snapshot.items[0].id, "visible");
    }
}
