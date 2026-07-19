//! Renderer-neutral menu and typed action model.

use serde::{Deserialize, Serialize};
use std::time::{Duration, SystemTime};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum MenuAction {
    Activate,
    ContextMenu,
    SecondaryActivate,
    SpawnCommand(String),
    DbusMenuAction {
        item_id: i32,
        event_id: String,
    },
    NavigateToApp(String),
    ShowAggregated,
    ShowFavorites,
    ToggleFavorite(String),
    NavigateToSubmenu {
        item_id: String,
        submenu_path: Vec<String>,
    },
    TextInputChanged {
        value: String,
    },
    TextInputFocusGained,
    TextInputFocusLost,
    TextInputCleared,
    Typed {
        kind: String,
        payload: String,
    },
}

impl MenuAction {
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Activate => "activate",
            Self::ContextMenu => "context_menu",
            Self::SecondaryActivate => "secondary_activate",
            Self::SpawnCommand(_) => "spawn_command",
            Self::DbusMenuAction { .. } => "dbus_menu_action",
            Self::NavigateToApp(_) => "navigate_to_app",
            Self::ShowAggregated => "show_aggregated",
            Self::ShowFavorites => "show_favorites",
            Self::ToggleFavorite(_) => "toggle_favorite",
            Self::NavigateToSubmenu { .. } => "navigate_to_submenu",
            Self::TextInputChanged { .. } => "text_input_changed",
            Self::TextInputFocusGained => "text_input_focus_gained",
            Self::TextInputFocusLost => "text_input_focus_lost",
            Self::TextInputCleared => "text_input_cleared",
            Self::Typed { .. } => "typed",
        }
    }
}

impl std::fmt::Display for MenuAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Activate => write!(f, "Activate"),
            Self::ContextMenu => write!(f, "ContextMenu"),
            Self::SecondaryActivate => write!(f, "SecondaryActivate"),
            Self::SpawnCommand(command) => write!(f, "SpawnCommand({command})"),
            Self::DbusMenuAction { item_id, event_id } => {
                write!(f, "DbusMenuAction({item_id}, {event_id})")
            }
            Self::NavigateToApp(app) => write!(f, "NavigateToApp({app})"),
            Self::ShowAggregated => write!(f, "ShowAggregated"),
            Self::ShowFavorites => write!(f, "ShowFavorites"),
            Self::ToggleFavorite(id) => write!(f, "ToggleFavorite({id})"),
            Self::NavigateToSubmenu {
                item_id,
                submenu_path,
            } => write!(f, "NavigateToSubmenu({item_id}, {submenu_path:?})"),
            Self::TextInputChanged { value } => write!(f, "TextInputChanged({value})"),
            Self::TextInputFocusGained => write!(f, "TextInputFocusGained"),
            Self::TextInputFocusLost => write!(f, "TextInputFocusLost"),
            Self::TextInputCleared => write!(f, "TextInputCleared"),
            Self::Typed { kind, payload } => write!(f, "Typed({kind}, {payload})"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum MenuWidgetType {
    Button,
    SubmenuButton,
    TextInput,
    Separator,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MenuItem {
    pub id: String,
    pub label: String,
    pub action: MenuAction,
    pub icon: Option<String>,
    pub submenu: Vec<MenuItem>,
    pub enabled: bool,
    pub visible: bool,
    pub checkable: bool,
    pub checked: bool,
    pub shortcut: Option<String>,
    pub is_separator: bool,
    pub app_id: String,
    pub full_path: String,
    pub widget_type: MenuWidgetType,
    pub default_value: Option<String>,
    pub placeholder: Option<String>,
}

impl MenuItem {
    pub fn is_selectable(&self) -> bool {
        self.visible && self.enabled && !self.is_separator
    }

    pub fn flatten_actions<'a>(&'a self, output: &mut Vec<&'a MenuItem>) {
        if self.is_selectable() {
            output.push(self);
        }
        for child in &self.submenu {
            child.flatten_actions(output);
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MenuSource {
    Tray,
    Widget,
    Custom,
    FilterTab,
    System,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MenuError {
    pub scope: String,
    pub message: String,
    pub recoverable: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum MenuLifecycle {
    #[default]
    Closed,
    Loading,
    Busy {
        action_id: String,
    },
    Fresh,
    Stale {
        detail: String,
    },
    Disabled {
        reason: String,
    },
    Error {
        scope: String,
        message: String,
        recoverable: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MenuModel {
    pub id: String,
    pub title: String,
    pub source: MenuSource,
    pub lifecycle: MenuLifecycle,
    pub generation: u64,
    pub last_updated_unix_ms: Option<u128>,
    pub items: Vec<MenuItem>,
}

impl MenuModel {
    pub fn new(id: impl Into<String>, title: impl Into<String>, source: MenuSource) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
            source,
            lifecycle: MenuLifecycle::Loading,
            generation: 0,
            last_updated_unix_ms: None,
            items: Vec::new(),
        }
    }

    pub fn with_items(
        id: impl Into<String>,
        title: impl Into<String>,
        source: MenuSource,
        generation: u64,
        items: Vec<MenuItem>,
    ) -> Self {
        let mut model = Self::new(id, title, source);
        model.publish(generation, items);
        model
    }

    pub fn loading(
        id: impl Into<String>,
        title: impl Into<String>,
        source: MenuSource,
        generation: u64,
        items: Vec<MenuItem>,
    ) -> Self {
        let mut model = Self::new(id, title, source);
        model.generation = generation;
        model.lifecycle = MenuLifecycle::Loading;
        model.items = items;
        model
    }

    pub fn stale(
        id: impl Into<String>,
        title: impl Into<String>,
        source: MenuSource,
        generation: u64,
        detail: impl Into<String>,
        items: Vec<MenuItem>,
    ) -> Self {
        let mut model = Self::with_items(id, title, source, generation, items);
        model.lifecycle = MenuLifecycle::Stale {
            detail: detail.into(),
        };
        model
    }

    pub fn error(
        id: impl Into<String>,
        title: impl Into<String>,
        source: MenuSource,
        generation: u64,
        error: MenuError,
        items: Vec<MenuItem>,
    ) -> Self {
        let mut model = Self::new(id, title, source);
        model.generation = generation;
        model.lifecycle = MenuLifecycle::Error {
            scope: error.scope,
            message: error.message,
            recoverable: error.recoverable,
        };
        model.items = items;
        model
    }

    pub fn disabled(
        id: impl Into<String>,
        title: impl Into<String>,
        source: MenuSource,
        reason: impl Into<String>,
    ) -> Self {
        let mut model = Self::new(id, title, source);
        model.lifecycle = MenuLifecycle::Disabled {
            reason: reason.into(),
        };
        model
    }

    pub fn publish(&mut self, generation: u64, items: Vec<MenuItem>) -> bool {
        if generation < self.generation {
            return false;
        }
        self.generation = generation;
        self.items = items;
        self.lifecycle = MenuLifecycle::Fresh;
        self.last_updated_unix_ms = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .ok()
            .map(|duration| duration.as_millis());
        true
    }

    pub fn selectable_actions(&self) -> Vec<&MenuItem> {
        let mut items = Vec::new();
        for item in &self.items {
            item.flatten_actions(&mut items);
        }
        items
    }

    pub fn last_update_age(&self, now: SystemTime) -> Option<Duration> {
        let updated = self.last_updated_unix_ms?;
        let updated = SystemTime::UNIX_EPOCH + Duration::from_millis(updated as u64);
        now.duration_since(updated).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(id: &str) -> MenuItem {
        MenuItem {
            id: id.to_string(),
            label: id.to_string(),
            action: MenuAction::Activate,
            icon: None,
            submenu: Vec::new(),
            enabled: true,
            visible: true,
            checkable: false,
            checked: false,
            shortcut: None,
            is_separator: false,
            app_id: "test".to_string(),
            full_path: id.to_string(),
            widget_type: MenuWidgetType::Button,
            default_value: None,
            placeholder: None,
        }
    }

    #[test]
    fn rejects_stale_menu_generation() {
        let mut model = MenuModel::new("system", "System", MenuSource::System);
        assert!(model.publish(2, vec![item("new")]));
        assert!(!model.publish(1, vec![item("old")]));
        assert_eq!(model.items[0].id, "new");
    }

    #[test]
    fn flattens_only_selectable_actions() {
        let mut parent = item("parent");
        parent.submenu.push(item("child"));
        let mut model = MenuModel::new("test", "Test", MenuSource::Tray);
        model.publish(1, vec![parent]);
        assert_eq!(model.selectable_actions().len(), 2);
    }
}
