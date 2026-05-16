use unilii_core::config::{CustomMenuActionConfig, CustomMenuConfig, CustomMenuItemConfig};

use super::common::{FilterableMenu, QuickjumpMenu};

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
            .map(build_custom_menu_item)
            .collect::<Vec<_>>();
        Self {
            items,
            quickjump_alphabet: config.quickjump_alphabet.clone(),
        }
    }
}

impl FilterableMenu for CustomMenuSnapshot {
    type ItemId = String;

    fn filter_tokens_for(&self, item_id: &Self::ItemId) -> Vec<String> {
        self.items
            .iter()
            .find(|item| &item.id == item_id)
            .map(|item| item.filter_tokens.clone())
            .unwrap_or_default()
    }
}

impl QuickjumpMenu for CustomMenuSnapshot {
    type ItemId = String;

    fn quickjump_targets(&self) -> Vec<Self::ItemId> {
        self.items.iter().map(|item| item.id.clone()).collect()
    }

    fn quickjump_alphabet(&self) -> String {
        self.quickjump_alphabet.clone()
    }
}

pub fn build_action_command(item: &CustomMenuItemConfig) -> String {
    match &item.action {
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
    }
}

fn build_custom_menu_item(item: &CustomMenuItemConfig) -> CustomMenuItem {
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
        subtitle: item.subtitle.clone(),
        action_command: build_action_command(item),
        icon_theme: item.icon.theme_icon.clone(),
        icon_svg_path: item.icon.svg_path.clone(),
        icon_image_path: item.icon.image_path.clone(),
        filter_tokens,
        confirm: item.confirm,
    }
}

fn shell_escape(input: &str) -> String {
    format!("'{}'", input.replace('\'', "'\\''"))
}

#[cfg(test)]
mod tests {
    use super::{CustomMenuSnapshot, build_action_command};
    use crate::menus::common::{FilterableMenu, QuickjumpMenu};
    use unilii_core::config::{
        CustomMenuActionConfig, CustomMenuConfig, CustomMenuIconConfig, CustomMenuItemConfig,
    };

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
        assert_eq!(
            snapshot.quickjump_bindings(),
            vec![("a".to_string(), "display.docked".to_string())]
        );
        assert!(snapshot.matches_filter_query(&"display.docked".to_string(), "docked xrandr"));
    }
}
