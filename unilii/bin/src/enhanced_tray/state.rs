//! State management for enhanced tray following idiomatic Iced patterns
//! 
//! This module implements the proper Iced state management approach:
//! - Single message enum for all events 
//! - Exhaustive pattern matching
//! - Clear state transitions
//! - Separation of update logic from view

use crate::enhanced_tray::{
    core::*, 
    dbus::*
};
use iced::Task;
use tracing::{debug, warn, error};

/// Single message enum for all enhanced tray events (idiomatic Iced pattern)
#[derive(Debug, Clone)]
pub enum TrayMessage {
    // Core navigation
    NavigateLeft,
    NavigateRight,
    ShowAggregated,
    ShowFavorites,
    ToggleFavorite(String, String), // (app_id, item_id)
    FilterUpdate(String),

    // Menu interaction
    MenuItemClicked(String, TrayMenuAction), // (app_id, action)
    IconClicked(String), // app_id
    
    // Animation and lifecycle
    AnimationTick,
    Show,
    Hide,
    
    // Data updates
    IconsUpdated(Vec<TrayIcon>),
    MenuUpdated { app_id: String, menu: Vec<TrayMenuItem> },
    DbusMenuReceived { app_id: String, menu: Vec<DbusMenuItem> },
    
    // Network-specific (maintains compatibility with existing system)
    NetworkSnapshot(String, Result<crate::tray::NetworkSnapshot, String>),
    NetworkRefresh(String),
    NetworkToggle(String),
    NetworkSpawnCommand(String, String),
    
    // Error handling
    DbusError(String, DbusError),
}

/// Enhanced tray state manager implementing idiomatic Iced update pattern
pub struct TrayStateManager;

impl TrayStateManager {
    /// Handle tray messages and update state (idiomatic Iced pattern)
    pub fn update(
        state: &mut EnhancedTrayState, 
        message: TrayMessage
    ) -> Task<TrayMessage> {
        match message {
            // == Navigation Messages ==
            TrayMessage::NavigateLeft => {
                Self::handle_navigate_left(state)
            }

            TrayMessage::NavigateRight => {
                Self::handle_navigate_right(state)
            }

            TrayMessage::ShowAggregated => {
                Self::handle_show_aggregated(state)
            }

            TrayMessage::ShowFavorites => {
                Self::handle_show_favorites(state)
            }

            TrayMessage::ToggleFavorite(app_id, item_id) => {
                Self::handle_toggle_favorite(state, &app_id, &item_id)
            }

            TrayMessage::FilterUpdate(filter_text) => {
                Self::handle_filter_update(state, filter_text)
            }

            // == Menu Interaction ==
            TrayMessage::MenuItemClicked(app_id, action) => {
                Self::handle_menu_item_clicked(state, &app_id, action)
            }

            TrayMessage::IconClicked(app_id) => {
                Self::handle_icon_clicked(state, &app_id)
            }

            // == Animation and Lifecycle ==
            TrayMessage::AnimationTick => {
                Self::handle_animation_tick(state)
            }

            TrayMessage::Show => {
                state.show();
                Task::none()
            }

            TrayMessage::Hide => {
                state.hide();
                Task::none()
            }

            // == Data Updates ==
            TrayMessage::IconsUpdated(icons) => {
                Self::handle_icons_updated(state, icons)
            }

            TrayMessage::MenuUpdated { app_id, menu } => {
                Self::handle_menu_updated(state, &app_id, menu)
            }

            TrayMessage::DbusMenuReceived { app_id, menu } => {
                Self::handle_dbus_menu_received(state, &app_id, menu)
            }

            // == Network Messages ==
            TrayMessage::NetworkSnapshot(app_id, result) => {
                Self::handle_network_snapshot(state, &app_id, result)
            }

            TrayMessage::NetworkRefresh(app_id) => {
                Self::handle_network_refresh(state, &app_id)
            }

            TrayMessage::NetworkToggle(app_id) => {
                Self::handle_network_toggle(state, &app_id)
            }

            TrayMessage::NetworkSpawnCommand(app_id, command) => {
                Self::handle_network_spawn_command(state, &app_id, command)
            }

            // == Error Handling ==
            TrayMessage::DbusError(app_id, error) => {
                error!("DBus error for {}: {}", app_id, error);
                Task::none()
            }
        }
    }

    // == Navigation Handlers ==

    fn handle_navigate_left(state: &mut EnhancedTrayState) -> Task<TrayMessage> {
        if let TrayViewState::SingleApp { app_id, .. } = &state.current_view.clone() {
            let navigation = state.tree.get_app_navigation(app_id);
            if navigation.can_go_left
                && let Some(new_app_id) = navigation.app_order.get(navigation.current_app_index - 1) {
                    let new_navigation = state.tree.get_app_navigation(new_app_id);
                    state.current_view = TrayViewState::SingleApp {
                        app_id: new_app_id.clone(),
                        navigation: new_navigation,
                    };
                }
        }
        Task::none()
    }

    fn handle_navigate_right(state: &mut EnhancedTrayState) -> Task<TrayMessage> {
        if let TrayViewState::SingleApp { app_id, .. } = &state.current_view.clone() {
            let navigation = state.tree.get_app_navigation(app_id);
            if navigation.can_go_right
                && let Some(new_app_id) = navigation.app_order.get(navigation.current_app_index + 1) {
                    let new_navigation = state.tree.get_app_navigation(new_app_id);
                    state.current_view = TrayViewState::SingleApp {
                        app_id: new_app_id.clone(),
                        navigation: new_navigation,
                    };
                }
        }
        Task::none()
    }

    fn handle_show_aggregated(state: &mut EnhancedTrayState) -> Task<TrayMessage> {
        let items = state.tree.get_aggregated_menu(None);
        state.current_view = TrayViewState::Aggregated {
            items,
            filter: None,
        };
        state.selected_index = Some(0);
        Task::none()
    }

    fn handle_show_favorites(state: &mut EnhancedTrayState) -> Task<TrayMessage> {
        let items = state.tree.get_favorites_menu();
        state.current_view = TrayViewState::Favorites { items };
        state.selected_index = Some(0);
        Task::none()
    }

    fn handle_toggle_favorite(
        state: &mut EnhancedTrayState, 
        _app_id: &str, 
        item_id: &str
    ) -> Task<TrayMessage> {
        let was_favorited = state.tree.toggle_favorite(item_id);
        
        // Update current view if showing favorites
        if let TrayViewState::Favorites { items } = &mut state.current_view {
            *items = state.tree.get_favorites_menu();
        }
        
        debug!("Item {} favorite status: {}", item_id, was_favorited);
        Task::none()
    }

    fn handle_filter_update(
        state: &mut EnhancedTrayState, 
        filter_text: String
    ) -> Task<TrayMessage> {
        state.filter_text = filter_text.clone();
        
        if let TrayViewState::Aggregated { items, filter } = &mut state.current_view {
            *filter = if filter_text.is_empty() { None } else { Some(filter_text.clone()) };
            *items = state.tree.get_aggregated_menu(filter.as_deref());
        }
        
        Task::none()
    }

    // == Menu Interaction Handlers ==

    fn handle_menu_item_clicked(
        _state: &mut EnhancedTrayState,
        app_id: &str, 
        action: TrayMenuAction
    ) -> Task<TrayMessage> {
        debug!("Menu item clicked for {}: {:?}", app_id, action);
        
        // Convert to async task for DBus operations
        match action {
            TrayMenuAction::SpawnCommand(command) => {
                Task::perform(
                    async move {
                        if let Err(e) = crate::enhanced_tray::spawn_command(command.clone()).await {
                            warn!("Failed to spawn command: {}", e);
                        }
                    },
                    |()| TrayMessage::AnimationTick // Dummy message
                )
            }
            
            TrayMenuAction::DbusMenuAction { item_id: _, event_id: _ } => {
                let app_id = app_id.to_string();
                Task::perform(
                    async move {
                        // We need the icon to invoke the DBus action
                        // For now, emit an error - this should be handled by the caller
                        TrayMessage::DbusError(app_id, DbusError::NoMenu)
                    },
                    |msg| msg
                )
            }
            
            _ => Task::none()
        }
    }

    fn handle_icon_clicked(
        state: &mut EnhancedTrayState,
        app_id: &str
    ) -> Task<TrayMessage> {
        // Check if clicking on the same icon - if so, close the menu
        if let TrayViewState::SingleApp { app_id: current_app_id, .. } | 
           TrayViewState::Network { app_id: current_app_id, .. } = &state.current_view
            && current_app_id == app_id {
                state.hide();
                return Task::none();
            }

        // Show menu for the clicked app
        if state.tree.apps.contains_key(app_id) {
            let navigation = state.tree.get_app_navigation(app_id);
            
            // Check if this is a network icon
            if let Some(app) = state.tree.apps.get(app_id) {
                if crate::enhanced_tray::is_network_icon(&convert_to_legacy_icon(&app.icon)) {
                    state.current_view = TrayViewState::Network {
                        app_id: app_id.to_string(),
                        data: None,
                        loading: true,
                        error: None,
                    };
                    state.show();
                    
                    // Start network data fetch
                    let app_id_clone = app_id.to_string();
                    return Task::perform(
                        async move {
                            match crate::enhanced_tray::read_network_snapshot("/usr/bin/nmcli".to_string(), false).await {
                                Ok(snapshot) => TrayMessage::NetworkSnapshot(app_id_clone, Ok(snapshot)),
                                Err(e) => TrayMessage::NetworkSnapshot(app_id_clone, Err(e)),
                            }
                        },
                        |msg| msg
                    );
                } else {
                    state.current_view = TrayViewState::SingleApp {
                        app_id: app_id.to_string(),
                        navigation,
                    };
                }
            }
            
            state.show();
            state.selected_index = Some(0);
        }
        
        Task::none()
    }

    // == Animation Handler ==

    fn handle_animation_tick(state: &mut EnhancedTrayState) -> Task<TrayMessage> {
        state.tick_animation(0.12);
        Task::none()
    }

    // == Data Update Handlers ==

    fn handle_icons_updated(
        state: &mut EnhancedTrayState,
        icons: Vec<TrayIcon>
    ) -> Task<TrayMessage> {
        debug!("Updating {} tray icons", icons.len());
        
        // If no icons match the existing apps, clear the tree
        if state.tree.apps.is_empty() || !icons.iter().any(|icon| state.tree.apps.contains_key(&icon.id)) {
            state.tree = TrayMenuTree::new();
        }

        // Update each icon in the tree
        for icon in icons {
            state.tree.update_app(icon);
        }
        
        Task::none()
    }

    fn handle_menu_updated(
        state: &mut EnhancedTrayState,
        app_id: &str,
        menu: Vec<TrayMenuItem>
    ) -> Task<TrayMessage> {
        debug!("Updating menu for {} with {} items", app_id, menu.len());
        state.tree.update_app_menu(app_id, menu);
        Task::none()
    }

    fn handle_dbus_menu_received(
        state: &mut EnhancedTrayState,
        app_id: &str,
        dbus_menu: Vec<DbusMenuItem>
    ) -> Task<TrayMessage> {
        debug!("Converting DBus menu for {} with {} items", app_id, dbus_menu.len());
        
        let tray_menu = convert_dbus_to_tray_menu(dbus_menu, app_id);
        state.tree.update_app_menu(app_id, tray_menu);
        
        Task::none()
    }

    // == Network Handlers ==

    fn handle_network_snapshot(
        state: &mut EnhancedTrayState,
        app_id: &str,
        result: Result<crate::tray::NetworkSnapshot, String>
    ) -> Task<TrayMessage> {
        if let TrayViewState::Network { app_id: current_app_id, data, loading, error } = &mut state.current_view
            && current_app_id == app_id {
                *loading = false;
                match result {
                    Ok(snapshot) => {
                        *data = Some(snapshot);
                        *error = None;
                    }
                    Err(err) => {
                        *error = Some(err);
                    }
                }
            }
        Task::none()
    }

    fn handle_network_refresh(
        state: &mut EnhancedTrayState,
        app_id: &str
    ) -> Task<TrayMessage> {
        if let TrayViewState::Network { app_id: current_app_id, loading, error, .. } = &mut state.current_view
            && current_app_id == app_id {
                *loading = true;
                *error = None;
                
                let app_id_clone = app_id.to_string();
                return Task::perform(
                    async move {
                        match crate::enhanced_tray::read_network_snapshot("/usr/bin/nmcli".to_string(), true).await {
                            Ok(snapshot) => TrayMessage::NetworkSnapshot(app_id_clone, Ok(snapshot)),
                            Err(e) => TrayMessage::NetworkSnapshot(app_id_clone, Err(e)),
                        }
                    },
                    |msg| msg
                );
            }
        Task::none()
    }

    fn handle_network_toggle(
        state: &mut EnhancedTrayState,
        app_id: &str
    ) -> Task<TrayMessage> {
        if let TrayViewState::Network { app_id: current_app_id, data, loading, error } = &mut state.current_view
            && current_app_id == app_id {
                let desired_state = data.as_ref()
                    .map(|snapshot| !snapshot.enabled)
                    .unwrap_or(true);
                
                *loading = true;
                *error = None;

                let app_id_clone = app_id.to_string();
                return Task::perform(
                    async move {
                        match crate::enhanced_tray::set_wifi_enabled("/usr/bin/nmcli".to_string(), desired_state).await {
                            Ok(()) => {
                                // Fetch updated status after toggle
                                match crate::enhanced_tray::read_network_snapshot("/usr/bin/nmcli".to_string(), true).await {
                                    Ok(snapshot) => TrayMessage::NetworkSnapshot(app_id_clone, Ok(snapshot)),
                                    Err(e) => TrayMessage::NetworkSnapshot(app_id_clone, Err(e)),
                                }
                            }
                            Err(e) => TrayMessage::NetworkSnapshot(app_id_clone, Err(e)),
                        }
                    },
                    |msg| msg
                );
            }
        Task::none()
    }

    fn handle_network_spawn_command(
        _state: &mut EnhancedTrayState,
        _app_id: &str,
        command: String
    ) -> Task<TrayMessage> {
        Task::perform(
            async move {
                if let Err(e) = crate::enhanced_tray::spawn_command(command).await {
                    warn!("Failed to spawn network command: {}", e);
                }
            },
            |()| TrayMessage::AnimationTick // Dummy message
        )
    }
}

// == Helper Functions ==

/// Convert enhanced tray icon to legacy format for compatibility
fn convert_to_legacy_icon(icon: &TrayIcon) -> crate::tray::TrayIcon {
    crate::tray::TrayIcon {
        key: icon.key.clone(),
        service: icon.service.clone(),
        path: icon.path.clone(),
        id: icon.id.clone(),
        title: icon.title.clone(),
        icon_name: icon.icon_name.clone(),
        status: icon.status.clone(),
        has_menu: icon.has_menu,
    }
}

/// Build default menu items for icons without DBus menus
pub fn build_default_menu_items(icon: &TrayIcon) -> Vec<TrayMenuItem> {
    let mut items = vec![
        TrayMenuItem {
            id: format!("{}_activate", icon.key),
            label: format!("Activate {}", icon.title),
            action: TrayMenuAction::Activate,
            icon: icon.icon_name.clone(),
            submenu: vec![],
            enabled: true,
            visible: true,
            checkable: false,
            checked: false,
            shortcut: None,
            is_separator: false,
            app_id: icon.id.clone(),
            full_path: format!("Activate {}", icon.title),
        },
        TrayMenuItem {
            id: format!("{}_secondary", icon.key),
            label: "Secondary action".to_string(),
            action: TrayMenuAction::SecondaryActivate,
            icon: None,
            submenu: vec![],
            enabled: true,
            visible: true,
            checkable: false,
            checked: false,
            shortcut: None,
            is_separator: false,
            app_id: icon.id.clone(),
            full_path: "Secondary action".to_string(),
        },
    ];

    // Add special actions for network icons
    if crate::enhanced_tray::is_network_icon(&convert_to_legacy_icon(icon)) {
        items.push(TrayMenuItem {
            id: format!("{}_network_settings", icon.key),
            label: "Open Network Settings".to_string(),
            action: TrayMenuAction::SpawnCommand("nm-connection-editor".to_string()),
            icon: Some("preferences-system-network".to_string()),
            submenu: vec![],
            enabled: true,
            visible: true,
            checkable: false,
            checked: false,
            shortcut: None,
            is_separator: false,
            app_id: icon.id.clone(),
            full_path: "Open Network Settings".to_string(),
        });
    }

    items
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_state() -> EnhancedTrayState {
        let mut state = EnhancedTrayState::new();
        
        // Add test apps
        let icon1 = TrayIcon {
            key: "app1".to_string(),
            id: "app1".to_string(),
            service: "com.example.app1".to_string(),
            path: "/StatusNotifierItem".to_string(),
            title: "App 1".to_string(),
            icon_name: Some("app1-icon".to_string()),
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
            status: "Active".to_string(),
            has_menu: false,
            menu_object_path: None,
        };

        state.tree.update_app(icon1);
        state.tree.update_app(icon2);
        
        state
    }

    #[test]
    fn test_navigation_messages() {
        let mut state = create_test_state();
        
        // Set up single app view
        let nav = state.tree.get_app_navigation("app1");
        state.current_view = TrayViewState::SingleApp {
            app_id: "app1".to_string(),
            navigation: nav,
        };

        // Test navigate right
        let _ = TrayStateManager::handle_navigate_right(&mut state);
        if let TrayViewState::SingleApp { app_id, .. } = &state.current_view {
            assert_eq!(app_id, "app2");
        } else {
            panic!("Expected SingleApp view");
        }

        // Test navigate left
        let _ = TrayStateManager::handle_navigate_left(&mut state);
        if let TrayViewState::SingleApp { app_id, .. } = &state.current_view {
            assert_eq!(app_id, "app1");
        } else {
            panic!("Expected SingleApp view");
        }
    }

    #[test]
    fn test_aggregated_view() {
        let mut state = create_test_state();
        
        // Test show aggregated
        let _ = TrayStateManager::handle_show_aggregated(&mut state);
        if let TrayViewState::Aggregated { items, filter } = &state.current_view {
            assert!(filter.is_none());
            // Items will be empty since we haven't added menu items to the test apps
            assert!(items.is_empty());
        } else {
            panic!("Expected Aggregated view");
        }
    }

    #[test]
    fn test_favorites_operations() {
        let mut state = create_test_state();
        
        let item_id = "test_item";
        
        // Toggle favorite on
        let _ = TrayStateManager::handle_toggle_favorite(&mut state, "app1", item_id);
        assert!(state.tree.favorites.contains(item_id));
        
        // Test show favorites
        let _ = TrayStateManager::handle_show_favorites(&mut state);
        if let TrayViewState::Favorites { items } = &state.current_view {
            // Items will be empty since the test item doesn't exist in the menu tree
            assert!(items.is_empty());
        } else {
            panic!("Expected Favorites view");
        }
    }

    #[test]
    fn test_filter_update() {
        let mut state = create_test_state();
        
        // Set aggregated view
        state.current_view = TrayViewState::Aggregated {
            items: Vec::new(),
            filter: None,
        };

        // Update filter
        let _ = TrayStateManager::handle_filter_update(&mut state, "test filter".to_string());
        
        assert_eq!(state.filter_text, "test filter");
        if let TrayViewState::Aggregated { filter, .. } = &state.current_view {
            assert_eq!(filter.as_ref(), Some(&"test filter".to_string()));
        } else {
            panic!("Expected Aggregated view");
        }
    }

    #[test]
    fn test_animation_control() {
        let mut state = create_test_state();
        
        // Test show
        let _ = TrayStateManager::update(&mut state, TrayMessage::Show);
        assert_eq!(state.animation_target, 1.0);
        
        // Test hide  
        let _ = TrayStateManager::update(&mut state, TrayMessage::Hide);
        assert_eq!(state.animation_target, 0.0);
        
        // Test animation tick
        state.animation_target = 1.0;
        state.animation_progress = 0.0;
        let _ = TrayStateManager::handle_animation_tick(&mut state);
        assert!(state.animation_progress > 0.0);
    }

    #[test]
    fn test_icon_clicked_handling() {
        let mut state = create_test_state();
        
        // Test clicking on app1
        let _ = TrayStateManager::handle_icon_clicked(&mut state, "app1");
        
        if let TrayViewState::SingleApp { app_id, .. } = &state.current_view {
            assert_eq!(app_id, "app1");
        } else {
            panic!("Expected SingleApp view");
        }
        
        assert_eq!(state.animation_target, 1.0);
        assert_eq!(state.selected_index, Some(0));
        
        // Test clicking on same app again (should close)
        let _ = TrayStateManager::handle_icon_clicked(&mut state, "app1");
        assert_eq!(state.animation_target, 0.0);
    }

    #[test]
    fn test_menu_building_functions() {
        let icon = TrayIcon {
            key: "test".to_string(),
            id: "test".to_string(),
            service: "com.example.test".to_string(),
            path: "/StatusNotifierItem".to_string(),
            title: "Test App".to_string(),
            icon_name: Some("test-icon".to_string()),
            status: "Active".to_string(),
            has_menu: false,
            menu_object_path: None,
        };

        let menu_items = build_default_menu_items(&icon);
        
        assert!(!menu_items.is_empty());
        assert_eq!(menu_items[0].label, "Activate Test App");
        assert_eq!(menu_items[1].label, "Secondary action");
        assert_eq!(menu_items[0].app_id, "test");
        assert!(menu_items[0].enabled);
    }
}