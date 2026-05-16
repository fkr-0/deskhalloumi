pub fn apply_enhanced_tray_event(
    enhanced_tray_state: &mut Option<crate::enhanced_tray::EnhancedTrayState>,
    event: crate::enhanced_tray::TrayEvent,
) {
    let tray_state = enhanced_tray_state.get_or_insert_with(crate::enhanced_tray::EnhancedTrayState::new);

    match event {
        crate::enhanced_tray::TrayEvent::IconsUpdated(icons) => {
            for icon in icons {
                tray_state.tree.update_app(icon);
            }
        }
        crate::enhanced_tray::TrayEvent::MenuUpdated { app_id, menu } => {
            tray_state.tree.update_app_menu(&app_id, menu);
        }
        crate::enhanced_tray::TrayEvent::DbusMenuReceived { app_id, menu } => {
            let tray_menu = crate::enhanced_tray::convert_dbus_to_tray_menu(menu, &app_id);
            tray_state.tree.update_app_menu(&app_id, tray_menu);
        }
        crate::enhanced_tray::TrayEvent::FavoritesChanged(favorites) => {
            tray_state.tree.favorites = favorites;
            if let crate::enhanced_tray::TrayViewState::Favorites { items } = &mut tray_state.current_view {
                *items = tray_state.tree.get_favorites_menu();
            }
        }
        crate::enhanced_tray::TrayEvent::NavigationChanged(navigation) => {
            let target_app_id = navigation.app_order.get(navigation.current_app_index);
            if let (Some(target_app_id), crate::enhanced_tray::TrayViewState::SingleApp {
                app_id,
                navigation: current_navigation,
                ..
            }) = (target_app_id, &mut tray_state.current_view)
                && app_id == target_app_id
            {
                *current_navigation = navigation;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::enhanced_tray::{self, EnhancedTrayState};
    use super::apply_enhanced_tray_event;

    fn tray_icon(app_id: &str) -> enhanced_tray::TrayIcon {
        enhanced_tray::TrayIcon {
            key: app_id.into(),
            service: format!("{app_id}.service"),
            path: "/StatusNotifierItem".into(),
            id: app_id.into(),
            title: app_id.into(),
            icon_name: None,
            icon_pixmap: None,
            status: "Active".into(),
            has_menu: true,
            menu_object_path: None,
        }
    }

    fn menu_item(app_id: &str, item_id: &str, label: &str) -> enhanced_tray::TrayMenuItem {
        enhanced_tray::TrayMenuItem {
            id: item_id.into(),
            label: label.into(),
            action: enhanced_tray::TrayMenuAction::Activate,
            icon: None,
            submenu: vec![],
            enabled: true,
            visible: true,
            checkable: false,
            checked: false,
            shortcut: None,
            is_separator: false,
            app_id: app_id.into(),
            full_path: label.into(),
            widget_type: enhanced_tray::TrayWidgetType::Button,
            default_value: None,
            placeholder: None,
        }
    }

    #[test]
    fn favorites_changed_creates_state_and_refreshes_favorites_view() {
        let mut favorites = std::collections::HashSet::new();
        favorites.insert("open".to_string());
        let mut state = None;

        apply_enhanced_tray_event(
            &mut state,
            enhanced_tray::TrayEvent::FavoritesChanged(favorites),
        );

        let state = state.expect("state should be created for favorites update");
        assert!(state.tree.favorites.contains("open"));
    }

    #[test]
    fn enhanced_tray_event_updates_existing_tree_and_menu() {
        let mut state = Some(EnhancedTrayState::new());

        apply_enhanced_tray_event(
            &mut state,
            enhanced_tray::TrayEvent::IconsUpdated(vec![tray_icon("app")]),
        );
        assert!(state.as_ref().unwrap().tree.apps.contains_key("app"));

        apply_enhanced_tray_event(
            &mut state,
            enhanced_tray::TrayEvent::MenuUpdated {
                app_id: "app".into(),
                menu: vec![menu_item("app", "open", "Open")],
            },
        );
        let app = state.as_ref().unwrap().tree.apps.get("app").unwrap();
        assert_eq!(app.menu_items.len(), 1);
        assert_eq!(app.menu_items[0].label, "Open");
    }
}
