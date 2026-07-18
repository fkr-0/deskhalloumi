use crate::enhanced_tray::{EnhancedTrayState, TrayViewState};

pub fn toggle_favorite(
    enhanced_tray_state: &mut Option<EnhancedTrayState>,
    app_id: &str,
    item_id: &str,
) {
    let Some(tray_state) = enhanced_tray_state.as_mut() else {
        return;
    };

    tray_state.tree.toggle_favorite(app_id, item_id);
    if let TrayViewState::Favorites { items } = &mut tray_state.current_view {
        *items = tray_state.tree.get_favorites_menu();
    }
}

#[cfg(test)]
mod tests {
    use super::toggle_favorite;
    use crate::enhanced_tray::{self, EnhancedTrayState, TrayViewState};

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

    fn state_with_favorites_view() -> Option<EnhancedTrayState> {
        let mut state = EnhancedTrayState::new();
        state.tree.update_app(tray_icon("app"));
        state
            .tree
            .update_app_menu("app", vec![menu_item("app", "open", "Open")]);
        state.current_view = TrayViewState::Favorites { items: vec![] };
        Some(state)
    }

    #[test]
    fn toggle_favorite_updates_tree_and_refreshes_favorites_view() {
        let mut state = state_with_favorites_view();

        toggle_favorite(&mut state, "app", "open");

        let state = state.expect("state remains present");
        assert!(state.tree.is_favorite("app", "open"));
        match state.current_view {
            TrayViewState::Favorites { items } => {
                assert_eq!(items.len(), 1);
                assert_eq!(items[0].id, "open");
            }
            other => panic!("expected favorites view, got {other:?}"),
        }
    }
}
