//! Core data structures for the enhanced tray system
//!
//! Follows idiomatic Iced patterns:
//! - Clear state separation
//! - Serializable structures for persistence
//! - Single message enum for all UI events

use crate::menus::common::FilterableMenu;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::time::SystemTime;

// == Core Data Structures ==

/// Enhanced tray icon with menu capabilities
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrayIcon {
    pub key: String,
    pub service: String,
    pub path: String,
    pub id: String,
    pub title: String,
    pub icon_name: Option<String>,
    pub icon_pixmap: Option<crate::tray::TrayIconPixmap>,
    pub status: String,
    pub has_menu: bool,
    pub menu_object_path: Option<String>,
}

/// Actions that can be performed on tray menu items
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TrayMenuAction {
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
}

impl std::fmt::Display for TrayMenuAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TrayMenuAction::Activate => write!(f, "Activate"),
            TrayMenuAction::ContextMenu => write!(f, "ContextMenu"),
            TrayMenuAction::SecondaryActivate => write!(f, "SecondaryActivate"),
            TrayMenuAction::SpawnCommand(cmd) => write!(f, "SpawnCommand({})", cmd),
            TrayMenuAction::DbusMenuAction { item_id, event_id } => {
                write!(f, "DbusMenuAction({}, {})", item_id, event_id)
            }
            TrayMenuAction::NavigateToApp(app) => write!(f, "NavigateToApp({})", app),
            TrayMenuAction::ShowAggregated => write!(f, "ShowAggregated"),
            TrayMenuAction::ShowFavorites => write!(f, "ShowFavorites"),
            TrayMenuAction::ToggleFavorite(id) => write!(f, "ToggleFavorite({})", id),
            TrayMenuAction::NavigateToSubmenu {
                item_id,
                submenu_path,
            } => {
                write!(f, "NavigateToSubmenu({}, {:?})", item_id, submenu_path)
            }
            TrayMenuAction::TextInputChanged { value } => {
                write!(f, "TextInputChanged({})", value)
            }
            TrayMenuAction::TextInputFocusGained => write!(f, "TextInputFocusGained"),
            TrayMenuAction::TextInputFocusLost => write!(f, "TextInputFocusLost"),
            TrayMenuAction::TextInputCleared => write!(f, "TextInputCleared"),
        }
    }
}

/// Single menu item with hierarchical structure
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrayMenuItem {
    pub id: String,
    pub label: String,
    pub action: TrayMenuAction,
    pub icon: Option<String>,
    pub submenu: Vec<TrayMenuItem>,
    pub enabled: bool,
    pub visible: bool,
    pub checkable: bool,
    pub checked: bool,
    pub shortcut: Option<String>,
    pub is_separator: bool,
    pub app_id: String,
    pub full_path: String,
    // Widget type and properties
    pub widget_type: TrayWidgetType,
    pub default_value: Option<String>,
    pub placeholder: Option<String>,
}

/// Type of widget to display for this menu item
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TrayWidgetType {
    Button,
    SubmenuButton,
    TextInput,
    Separator,
}

/// Application with its tray menu
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrayApp {
    pub icon: TrayIcon,
    pub menu_items: Vec<TrayMenuItem>,
    pub last_updated: SystemTime,
}

/// Hierarchical menu tree managing all tray applications
#[derive(Debug, Clone)]
pub struct TrayMenuTree {
    pub apps: HashMap<String, TrayApp>,
    pub favorites: HashSet<String>,
    pub icon_order: Vec<String>, // Maintains display order
}

/// Different view modes for the tray menu
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TrayViewState {
    SingleApp {
        app_id: String,
        navigation: TrayMenuNavigation,
        submenu_path: Vec<String>, // Track current submenu path
    },
    Aggregated {
        items: Vec<TrayMenuItem>,
        filter: Option<String>,
    },
    Favorites {
        items: Vec<TrayMenuItem>,
    },
    Network {
        app_id: String,
        data: Option<crate::tray::NetworkSnapshot>,
        loading: bool,
        error: Option<String>,
    },
    Mount {
        app_id: String,
        data: Option<crate::menus::mount::MountMenuSnapshot>,
        loading: bool,
        error: Option<String>,
    },
    Calendar {
        app_id: String,
        data: Option<crate::menus::calendar::CalendarMenuSnapshot>,
        loading: bool,
        error: Option<String>,
    },
}

/// Navigation state for moving between apps
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrayMenuNavigation {
    pub current_app_index: usize,
    pub app_order: Vec<String>,
    pub can_go_left: bool,
    pub can_go_right: bool,
}

/// Complete enhanced tray state following Iced state management patterns
#[derive(Debug, Clone)]
pub struct EnhancedTrayState {
    pub tree: TrayMenuTree,
    pub current_view: TrayViewState,
    pub animation_progress: f32,
    pub animation_target: f32,
    pub selected_index: Option<usize>,
    pub filter_text: String,
}

/// Events from the enhanced tray system
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TrayEvent {
    IconsUpdated(Vec<TrayIcon>),
    MenuUpdated {
        app_id: String,
        menu: Vec<TrayMenuItem>,
    },
    DbusMenuReceived {
        app_id: String,
        menu: Vec<super::dbus::DbusMenuItem>,
    },
    FavoritesChanged(HashSet<String>),
    NavigationChanged(TrayMenuNavigation),
}

// == Helper Implementations ==

impl TrayViewState {
    /// Get navigation info if this view supports it
    pub fn get_navigation(&self) -> Option<&TrayMenuNavigation> {
        match self {
            TrayViewState::SingleApp { navigation, .. } => Some(navigation),
            _ => None,
        }
    }

    /// Get the number of items in this view
    pub fn item_count(&self) -> usize {
        match self {
            TrayViewState::SingleApp { .. } => 0, // Will be computed from tree
            TrayViewState::Aggregated { items, .. } | TrayViewState::Favorites { items } => {
                items.len()
            }
            TrayViewState::Network { data, .. } => {
                // Basic network menu: toggle, refresh, settings, + networks
                3 + data.as_ref().map(|d| d.networks.len()).unwrap_or(0)
            }
            TrayViewState::Mount { data, .. } => {
                2 + data.as_ref().map(|d| d.local_devices.len()).unwrap_or(0)
            }
            TrayViewState::Calendar { data, .. } => {
                2 + data.as_ref().map(|d| d.events.len()).unwrap_or(0)
            }
        }
    }
}

impl TrayMenuTree {
    /// Create new empty menu tree
    pub fn new() -> Self {
        Self {
            apps: HashMap::new(),
            favorites: HashSet::new(),
            icon_order: Vec::new(),
        }
    }

    /// Update or add an app to the tree
    pub fn update_app(&mut self, icon: TrayIcon) {
        let app_id = icon.id.clone();

        // Maintain icon order for display
        if !self.icon_order.contains(&app_id) {
            self.icon_order.push(app_id.clone());
        }

        let app = TrayApp {
            icon,
            menu_items: Vec::new(), // Will be populated by menu builders
            last_updated: SystemTime::now(),
        };

        self.apps.insert(app_id, app);
    }

    /// Remove an app from the tree
    pub fn remove_app(&mut self, app_id: &str) {
        self.apps.remove(app_id);
        self.icon_order.retain(|id| id != app_id);
    }

    /// Update menu items for an app
    pub fn update_app_menu(&mut self, app_id: &str, menu_items: Vec<TrayMenuItem>) {
        if let Some(app) = self.apps.get_mut(app_id) {
            app.menu_items = menu_items;
            app.last_updated = SystemTime::now();
        }
    }

    /// Get navigation state for an app
    pub fn get_app_navigation(&self, current_app_id: &str) -> TrayMenuNavigation {
        let current_index = self
            .icon_order
            .iter()
            .position(|id| id == current_app_id)
            .unwrap_or(0);

        TrayMenuNavigation {
            current_app_index: current_index,
            app_order: self.icon_order.clone(),
            can_go_left: current_index > 0,
            can_go_right: current_index < self.icon_order.len().saturating_sub(1),
        }
    }

    /// Get aggregated menu from all apps
    pub fn get_aggregated_menu(&self, filter: Option<&str>) -> Vec<TrayMenuItem> {
        let mut items = Vec::new();

        for app in self.apps.values() {
            for item in &app.menu_items {
                self.flatten_menu_items(item, &app.icon.id, &mut items);
            }
        }

        // Apply filter if provided
        if let Some(filter_text) = filter {
            let filter_lower = filter_text.to_lowercase();
            items.retain(|item| {
                item.label.to_lowercase().contains(&filter_lower)
                    || item.full_path.to_lowercase().contains(&filter_lower)
            });
        }

        items.sort_by(|a, b| a.full_path.cmp(&b.full_path));
        items
    }

    /// Get favorite menu items
    pub fn get_favorites_menu(&self) -> Vec<TrayMenuItem> {
        let mut favorites = Vec::new();

        for app in self.apps.values() {
            for item in &app.menu_items {
                self.collect_favorites(item, &mut favorites);
            }
        }

        favorites.sort_by(|a, b| a.full_path.cmp(&b.full_path));
        favorites
    }

    /// Toggle favorite status of a menu item
    pub fn toggle_favorite(&mut self, item_id: &str) -> bool {
        if self.favorites.contains(item_id) {
            self.favorites.remove(item_id);
            false
        } else {
            self.favorites.insert(item_id.to_string());
            true
        }
    }

    /// Helper to flatten menu hierarchy for aggregated view
    fn flatten_menu_items(
        &self,
        item: &TrayMenuItem,
        app_id: &str,
        result: &mut Vec<TrayMenuItem>,
    ) {
        if !item.is_separator && item.visible && item.enabled {
            let mut flattened = item.clone();
            flattened.full_path = format!("{} → {}", app_id, item.label);
            result.push(flattened);
        }

        for subitem in &item.submenu {
            let mut nested = subitem.clone();
            nested.full_path = format!("{} → {} → {}", app_id, item.label, subitem.label);
            self.flatten_menu_items(&nested, app_id, result);
        }
    }

    /// Helper to collect favorite items
    fn collect_favorites(&self, item: &TrayMenuItem, result: &mut Vec<TrayMenuItem>) {
        if self.favorites.contains(&item.id) {
            result.push(item.clone());
        }
        for subitem in &item.submenu {
            self.collect_favorites(subitem, result);
        }
    }
}

impl FilterableMenu for TrayMenuTree {
    type ItemId = String;

    fn filter_tokens_for(&self, item_id: &Self::ItemId) -> Vec<String> {
        for app in self.apps.values() {
            if let Some(tokens) = filter_tokens_in_tree(&app.menu_items, item_id) {
                return tokens;
            }
        }
        Vec::new()
    }
}

fn filter_tokens_in_tree(items: &[TrayMenuItem], item_id: &str) -> Option<Vec<String>> {
    for item in items {
        if item.id == item_id {
            return Some(vec![
                item.label.clone(),
                item.full_path.clone(),
                item.app_id.clone(),
            ]);
        }
        if let Some(tokens) = filter_tokens_in_tree(&item.submenu, item_id) {
            return Some(tokens);
        }
    }
    None
}

impl Default for TrayMenuTree {
    fn default() -> Self {
        Self::new()
    }
}

impl EnhancedTrayState {
    /// Create new enhanced tray state
    pub fn new() -> Self {
        Self {
            tree: TrayMenuTree::new(),
            current_view: TrayViewState::Aggregated {
                items: Vec::new(),
                filter: None,
            },
            animation_progress: 0.0,
            animation_target: 0.0,
            selected_index: None,
            filter_text: String::new(),
        }
    }

    /// Check if the tray is currently visible
    pub fn is_visible(&self) -> bool {
        self.animation_progress > 0.01
    }

    /// Show the tray with animation
    pub fn show(&mut self) {
        self.animation_target = 1.0;
    }

    /// Hide the tray with animation
    pub fn hide(&mut self) {
        self.animation_target = 0.0;
    }

    /// Update animation progress
    pub fn tick_animation(&mut self, rate: f32) {
        let threshold = 0.01;
        if (self.animation_progress - self.animation_target).abs() > threshold {
            self.animation_progress += (self.animation_target - self.animation_progress) * rate;
        } else {
            self.animation_progress = self.animation_target;
        }
    }
}

impl Default for EnhancedTrayState {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tray_menu_tree_creation() {
        let tree = TrayMenuTree::new();
        assert!(tree.apps.is_empty());
        assert!(tree.favorites.is_empty());
        assert!(tree.icon_order.is_empty());
    }

    #[test]
    fn test_app_update_and_ordering() {
        let mut tree = TrayMenuTree::new();

        let icon1 = TrayIcon {
            key: "app1".to_string(),
            id: "app1".to_string(),
            service: "com.example.app1".to_string(),
            path: "/StatusNotifierItem".to_string(),
            title: "App 1".to_string(),
            icon_name: Some("app1-icon".to_string()),
            icon_pixmap: None,
            status: "Active".to_string(),
            has_menu: true,
            menu_object_path: Some("/MenuBar".to_string()),
        };

        let icon2 = TrayIcon {
            key: "app2".to_string(),
            id: "app2".to_string(),
            service: "com.example.app2".to_string(),
            path: "/StatusNotifierItem".to_string(),
            title: "App 2".to_string(),
            icon_name: Some("app2-icon".to_string()),
            icon_pixmap: None,
            status: "Active".to_string(),
            has_menu: false,
            menu_object_path: None,
        };

        tree.update_app(icon1);
        tree.update_app(icon2);

        assert_eq!(tree.apps.len(), 2);
        assert_eq!(tree.icon_order, vec!["app1", "app2"]);

        // Test navigation
        let nav = tree.get_app_navigation("app1");
        assert_eq!(nav.current_app_index, 0);
        assert!(!nav.can_go_left);
        assert!(nav.can_go_right);

        let nav = tree.get_app_navigation("app2");
        assert_eq!(nav.current_app_index, 1);
        assert!(nav.can_go_left);
        assert!(!nav.can_go_right);
    }

    #[test]
    fn test_menu_item_favorites() {
        let mut tree = TrayMenuTree::new();

        let item_id = "test_item_1";
        assert!(!tree.favorites.contains(item_id));

        // Toggle on
        let result = tree.toggle_favorite(item_id);
        assert!(result);
        assert!(tree.favorites.contains(item_id));

        // Toggle off
        let result = tree.toggle_favorite(item_id);
        assert!(!result);
        assert!(!tree.favorites.contains(item_id));
    }

    #[test]
    fn test_enhanced_tray_state_animation() {
        let mut state = EnhancedTrayState::new();

        assert_eq!(state.animation_progress, 0.0);
        assert_eq!(state.animation_target, 0.0);
        assert!(!state.is_visible());

        state.show();
        assert_eq!(state.animation_target, 1.0);

        // Simulate animation steps
        state.tick_animation(0.1);
        assert!(state.animation_progress > 0.0);
        assert!(state.animation_progress < 1.0);

        // Complete animation (use many iterations to converge)
        for _ in 0..50 {
            state.tick_animation(0.1);
        }
        assert!((state.animation_progress - 1.0).abs() < 0.01);
        assert!(state.is_visible());
    }

    #[test]
    fn test_view_state_item_count() {
        let single_app_view = TrayViewState::SingleApp {
            app_id: "test".to_string(),
            navigation: TrayMenuNavigation {
                current_app_index: 0,
                app_order: vec!["test".to_string()],
                can_go_left: false,
                can_go_right: false,
            },
            submenu_path: Vec::new(),
        };

        let aggregated_view = TrayViewState::Aggregated {
            items: vec![TrayMenuItem {
                id: "item1".to_string(),
                label: "Item 1".to_string(),
                action: TrayMenuAction::Activate,
                icon: None,
                submenu: Vec::new(),
                enabled: true,
                visible: true,
                checkable: false,
                checked: false,
                shortcut: None,
                is_separator: false,
                app_id: "test".to_string(),
                full_path: "Test → Item 1".to_string(),
                widget_type: TrayWidgetType::Button,
                default_value: None,
                placeholder: None,
            }],
            filter: None,
        };

        assert!(single_app_view.get_navigation().is_some());
        assert!(aggregated_view.get_navigation().is_none());
        assert_eq!(aggregated_view.item_count(), 1);
    }

    #[test]
    fn test_submenu_path_tracking() {
        // Test SingleApp view with submenu path tracking
        let empty_path = TrayViewState::SingleApp {
            app_id: "app1".to_string(),
            navigation: TrayMenuNavigation {
                current_app_index: 0,
                app_order: vec!["app1".to_string()],
                can_go_left: false,
                can_go_right: false,
            },
            submenu_path: Vec::new(),
        };

        // Access submenu_path through pattern matching
        match empty_path {
            TrayViewState::SingleApp { submenu_path, .. } => {
                assert!(submenu_path.is_empty());
            }
            _ => panic!("Expected SingleApp"),
        }

        let nested_path = TrayViewState::SingleApp {
            app_id: "app1".to_string(),
            navigation: TrayMenuNavigation {
                current_app_index: 0,
                app_order: vec!["app1".to_string()],
                can_go_left: false,
                can_go_right: false,
            },
            submenu_path: vec!["settings".to_string(), "advanced".to_string()],
        };

        // Access submenu_path through pattern matching
        match nested_path {
            TrayViewState::SingleApp { submenu_path, .. } => {
                assert_eq!(submenu_path.len(), 2);
                assert_eq!(submenu_path[0], "settings");
                assert_eq!(submenu_path[1], "advanced");
            }
            _ => panic!("Expected SingleApp"),
        }
    }

    #[test]
    fn test_widget_type_variants() {
        // Test all widget type variants
        let button_item = TrayMenuItem {
            id: "btn1".to_string(),
            label: "Button".to_string(),
            action: TrayMenuAction::Activate,
            icon: None,
            submenu: Vec::new(),
            enabled: true,
            visible: true,
            checkable: false,
            checked: false,
            shortcut: None,
            is_separator: false,
            app_id: "test".to_string(),
            full_path: "Button".to_string(),
            widget_type: TrayWidgetType::Button,
            default_value: None,
            placeholder: None,
        };

        assert_eq!(button_item.widget_type, TrayWidgetType::Button);
        assert!(button_item.default_value.is_none());
        assert!(button_item.placeholder.is_none());

        let text_input_item = TrayMenuItem {
            id: "input1".to_string(),
            label: "INPUT: Search".to_string(),
            action: TrayMenuAction::TextInputChanged {
                value: "".to_string(),
            },
            icon: None,
            submenu: Vec::new(),
            enabled: true,
            visible: true,
            checkable: false,
            checked: false,
            shortcut: None,
            is_separator: false,
            app_id: "test".to_string(),
            full_path: "Search Input".to_string(),
            widget_type: TrayWidgetType::TextInput,
            default_value: Some("default".to_string()),
            placeholder: Some("Enter search text...".to_string()),
        };

        assert_eq!(text_input_item.widget_type, TrayWidgetType::TextInput);
        assert_eq!(text_input_item.default_value, Some("default".to_string()));
        assert_eq!(
            text_input_item.placeholder,
            Some("Enter search text...".to_string())
        );

        let separator_item = TrayMenuItem {
            id: "sep1".to_string(),
            label: "".to_string(),
            action: TrayMenuAction::Activate,
            icon: None,
            submenu: Vec::new(),
            enabled: true,
            visible: true,
            checkable: false,
            checked: false,
            shortcut: None,
            is_separator: true,
            app_id: "test".to_string(),
            full_path: "".to_string(),
            widget_type: TrayWidgetType::Separator,
            default_value: None,
            placeholder: None,
        };

        assert_eq!(separator_item.widget_type, TrayWidgetType::Separator);
        assert!(separator_item.is_separator);

        let submenu_item = TrayMenuItem {
            id: "sub1".to_string(),
            label: "Settings".to_string(),
            action: TrayMenuAction::NavigateToSubmenu {
                item_id: "settings".to_string(),
                submenu_path: vec!["settings".to_string()],
            },
            icon: None,
            submenu: vec![],
            enabled: true,
            visible: true,
            checkable: false,
            checked: false,
            shortcut: None,
            is_separator: false,
            app_id: "test".to_string(),
            full_path: "Settings".to_string(),
            widget_type: TrayWidgetType::SubmenuButton,
            default_value: None,
            placeholder: None,
        };

        assert_eq!(submenu_item.widget_type, TrayWidgetType::SubmenuButton);
    }

    #[test]
    fn test_text_input_actions() {
        // Test new text input action variants
        let input_changed = TrayMenuAction::TextInputChanged {
            value: "hello world".to_string(),
        };

        match input_changed {
            TrayMenuAction::TextInputChanged { value } => {
                assert_eq!(value, "hello world");
            }
            _ => panic!("Expected TextInputChanged"),
        }

        let focus_gained = TrayMenuAction::TextInputFocusGained;
        let focus_lost = TrayMenuAction::TextInputFocusLost;
        let cleared = TrayMenuAction::TextInputCleared;

        // Verify Display impl for text input actions
        assert_eq!(
            format!(
                "{}",
                TrayMenuAction::TextInputChanged {
                    value: "test".to_string()
                }
            ),
            "TextInputChanged(test)"
        );
        assert_eq!(
            format!("{}", TrayMenuAction::TextInputFocusGained),
            "TextInputFocusGained"
        );
        assert_eq!(
            format!("{}", TrayMenuAction::TextInputFocusLost),
            "TextInputFocusLost"
        );
        assert_eq!(
            format!("{}", TrayMenuAction::TextInputCleared),
            "TextInputCleared"
        );

        // Verify action variants can be created and matched
        let _focus_gained = TrayMenuAction::TextInputFocusGained;
        let _focus_lost = TrayMenuAction::TextInputFocusLost;
        let _cleared = TrayMenuAction::TextInputCleared;
    }

    #[test]
    fn test_submenu_navigation_action() {
        let nav_action = TrayMenuAction::NavigateToSubmenu {
            item_id: "settings_item".to_string(),
            submenu_path: vec!["settings".to_string(), "advanced".to_string()],
        };

        match nav_action {
            TrayMenuAction::NavigateToSubmenu {
                ref item_id,
                ref submenu_path,
            } => {
                assert_eq!(item_id, "settings_item");
                assert_eq!(submenu_path.len(), 2);
                assert_eq!(submenu_path[0], "settings");
            }
            _ => panic!("Expected NavigateToSubmenu"),
        }

        // Verify Display impl for submenu action
        let display_str = format!("{}", nav_action);
        assert!(display_str.contains("NavigateToSubmenu"));
        assert!(display_str.contains("settings"));
    }

    #[test]
    fn test_menu_item_with_nested_submenu() {
        // Create a menu item with nested submenu hierarchy
        let nested_item = TrayMenuItem {
            id: "parent".to_string(),
            label: "Parent Menu".to_string(),
            action: TrayMenuAction::NavigateToSubmenu {
                item_id: "parent".to_string(),
                submenu_path: vec!["parent".to_string()],
            },
            icon: None,
            submenu: vec![
                TrayMenuItem {
                    id: "child1".to_string(),
                    label: "Child Item 1".to_string(),
                    action: TrayMenuAction::Activate,
                    icon: None,
                    submenu: vec![],
                    enabled: true,
                    visible: true,
                    checkable: false,
                    checked: false,
                    shortcut: None,
                    is_separator: false,
                    app_id: "test".to_string(),
                    full_path: "Parent → Child Item 1".to_string(),
                    widget_type: TrayWidgetType::Button,
                    default_value: None,
                    placeholder: None,
                },
                TrayMenuItem {
                    id: "child2".to_string(),
                    label: "Child Item 2".to_string(),
                    action: TrayMenuAction::Activate,
                    icon: None,
                    submenu: vec![],
                    enabled: true,
                    visible: true,
                    checkable: false,
                    checked: false,
                    shortcut: None,
                    is_separator: false,
                    app_id: "test".to_string(),
                    full_path: "Parent → Child Item 2".to_string(),
                    widget_type: TrayWidgetType::Button,
                    default_value: None,
                    placeholder: None,
                },
            ],
            enabled: true,
            visible: true,
            checkable: false,
            checked: false,
            shortcut: None,
            is_separator: false,
            app_id: "test".to_string(),
            full_path: "Parent Menu".to_string(),
            widget_type: TrayWidgetType::SubmenuButton,
            default_value: None,
            placeholder: None,
        };

        assert_eq!(nested_item.submenu.len(), 2);
        assert_eq!(nested_item.widget_type, TrayWidgetType::SubmenuButton);
        assert_eq!(nested_item.submenu[0].label, "Child Item 1");
        assert_eq!(nested_item.submenu[1].label, "Child Item 2");
    }

    #[test]
    fn test_aggregated_menu_with_filters() {
        let mut tree = TrayMenuTree::new();

        // Add test apps with menu items
        let icon = TrayIcon {
            key: "test_app".to_string(),
            id: "test_app".to_string(),
            service: "com.example.test".to_string(),
            path: "/StatusNotifierItem".to_string(),
            title: "Test App".to_string(),
            icon_name: Some("test-icon".to_string()),
            icon_pixmap: None,
            status: "Active".to_string(),
            has_menu: true,
            menu_object_path: Some("/MenuBar".to_string()),
        };

        tree.update_app(icon);

        let menu_items = vec![
            TrayMenuItem {
                id: "item1".to_string(),
                label: "Search Option".to_string(),
                action: TrayMenuAction::Activate,
                icon: None,
                submenu: Vec::new(),
                enabled: true,
                visible: true,
                checkable: false,
                checked: false,
                shortcut: None,
                is_separator: false,
                app_id: "test_app".to_string(),
                full_path: "Test App → Search Option".to_string(),
                widget_type: TrayWidgetType::Button,
                default_value: None,
                placeholder: None,
            },
            TrayMenuItem {
                id: "item2".to_string(),
                label: "Settings Option".to_string(),
                action: TrayMenuAction::Activate,
                icon: None,
                submenu: Vec::new(),
                enabled: true,
                visible: true,
                checkable: false,
                checked: false,
                shortcut: None,
                is_separator: false,
                app_id: "test_app".to_string(),
                full_path: "Test App → Settings Option".to_string(),
                widget_type: TrayWidgetType::Button,
                default_value: None,
                placeholder: None,
            },
        ];

        tree.update_app_menu("test_app", menu_items);

        // Test without filter
        let all_items = tree.get_aggregated_menu(None);
        assert_eq!(all_items.len(), 2);

        // Test with filter
        let filtered_items = tree.get_aggregated_menu(Some("Search"));
        assert_eq!(filtered_items.len(), 1);
        assert!(filtered_items[0].label.contains("Search"));

        // Test filter that matches nothing
        let no_match = tree.get_aggregated_menu(Some("nonexistent"));
        assert_eq!(no_match.len(), 0);
    }
}
