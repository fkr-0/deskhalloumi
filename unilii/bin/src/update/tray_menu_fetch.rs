use crate::enhanced_tray::{EnhancedTrayState, TrayIcon, TrayMenuAction, TrayMenuItem, TrayWidgetType};

pub fn apply_menu_fetch_result(
    enhanced_tray_state: &mut Option<EnhancedTrayState>,
    app_id: &str,
    result: Result<Vec<TrayMenuItem>, String>,
) -> TrayMenuFetchOutcome {
    let Some(tray_state) = enhanced_tray_state.as_mut() else {
        return TrayMenuFetchOutcome::NoState;
    };

    match result {
        Ok(menu_items) if !menu_items.is_empty() => {
            let item_count = menu_items.len();
            tray_state.tree.update_app_menu(app_id, menu_items);
            TrayMenuFetchOutcome::Populated { item_count }
        }
        Ok(_) => TrayMenuFetchOutcome::KeptExistingEmptyFetch,
        Err(error) => {
            let Some(app) = tray_state.tree.apps.get(app_id).cloned() else {
                return TrayMenuFetchOutcome::FetchFailedNoKnownApp { error };
            };
            let fallback_menu = build_simple_visible_menu(&app.icon);
            let item_count = fallback_menu.len();
            if item_count > 0 {
                tray_state.tree.update_app_menu(app_id, fallback_menu);
            }
            TrayMenuFetchOutcome::FallbackPopulated { item_count, error }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TrayMenuFetchOutcome {
    NoState,
    Populated { item_count: usize },
    KeptExistingEmptyFetch,
    FallbackPopulated { item_count: usize, error: String },
    FetchFailedNoKnownApp { error: String },
}

pub fn build_simple_visible_menu(icon: &TrayIcon) -> Vec<TrayMenuItem> {
    let app_id = icon.id.clone();
    vec![
        TrayMenuItem {
            id: "activate".into(),
            label: "Activate".into(),
            action: TrayMenuAction::Activate,
            icon: None,
            submenu: vec![],
            enabled: true,
            visible: true,
            checkable: false,
            checked: false,
            shortcut: None,
            is_separator: false,
            app_id: app_id.clone(),
            full_path: "Activate".into(),
            widget_type: TrayWidgetType::Button,
            default_value: None,
            placeholder: None,
        },
        TrayMenuItem {
            id: "secondary".into(),
            label: "Secondary Action".into(),
            action: TrayMenuAction::SecondaryActivate,
            icon: None,
            submenu: vec![],
            enabled: true,
            visible: true,
            checkable: false,
            checked: false,
            shortcut: None,
            is_separator: false,
            app_id: app_id.clone(),
            full_path: "Secondary Action".into(),
            widget_type: TrayWidgetType::Button,
            default_value: None,
            placeholder: None,
        },
        TrayMenuItem {
            id: "more".into(),
            label: "More".into(),
            action: TrayMenuAction::NavigateToSubmenu {
                item_id: "more".into(),
                submenu_path: vec!["more".into()],
            },
            icon: None,
            submenu: vec![TrayMenuItem {
                id: "context".into(),
                label: "Context Menu".into(),
                action: TrayMenuAction::ContextMenu,
                icon: None,
                submenu: vec![],
                enabled: true,
                visible: true,
                checkable: false,
                checked: false,
                shortcut: None,
                is_separator: false,
                app_id: app_id.clone(),
                full_path: "More → Context Menu".into(),
                widget_type: TrayWidgetType::Button,
                default_value: None,
                placeholder: None,
            }],
            enabled: true,
            visible: true,
            checkable: false,
            checked: false,
            shortcut: None,
            is_separator: false,
            app_id: app_id.clone(),
            full_path: "More".into(),
            widget_type: TrayWidgetType::SubmenuButton,
            default_value: None,
            placeholder: None,
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::{apply_menu_fetch_result, build_simple_visible_menu, TrayMenuFetchOutcome};
    use crate::enhanced_tray::{self, EnhancedTrayState, TrayMenuAction, TrayMenuItem, TrayWidgetType};

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

    fn menu_item(app_id: &str, item_id: &str, label: &str) -> TrayMenuItem {
        TrayMenuItem {
            id: item_id.into(),
            label: label.into(),
            action: TrayMenuAction::Activate,
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
            widget_type: TrayWidgetType::Button,
            default_value: None,
            placeholder: None,
        }
    }

    fn state_with_app() -> Option<EnhancedTrayState> {
        let mut state = EnhancedTrayState::new();
        state.tree.update_app(tray_icon("app"));
        state.tree.update_app_menu("app", vec![menu_item("app", "old", "Old")]);
        Some(state)
    }

    #[test]
    fn populated_fetch_replaces_existing_menu() {
        let mut state = state_with_app();

        let outcome = apply_menu_fetch_result(
            &mut state,
            "app",
            Ok(vec![menu_item("app", "new", "New")]),
        );

        assert_eq!(outcome, TrayMenuFetchOutcome::Populated { item_count: 1 });
        let app = state.as_ref().unwrap().tree.apps.get("app").unwrap();
        assert_eq!(app.menu_items.len(), 1);
        assert_eq!(app.menu_items[0].id, "new");
    }

    #[test]
    fn empty_fetch_keeps_existing_menu() {
        let mut state = state_with_app();

        let outcome = apply_menu_fetch_result(&mut state, "app", Ok(vec![]));

        assert_eq!(outcome, TrayMenuFetchOutcome::KeptExistingEmptyFetch);
        let app = state.as_ref().unwrap().tree.apps.get("app").unwrap();
        assert_eq!(app.menu_items.len(), 1);
        assert_eq!(app.menu_items[0].id, "old");
    }

    #[test]
    fn fetch_error_populates_fallback_for_known_app() {
        let mut state = state_with_app();

        let outcome = apply_menu_fetch_result(&mut state, "app", Err("dbus failed".into()));

        assert_eq!(
            outcome,
            TrayMenuFetchOutcome::FallbackPopulated {
                item_count: 3,
                error: "dbus failed".into(),
            }
        );
        let app = state.as_ref().unwrap().tree.apps.get("app").unwrap();
        assert_eq!(app.menu_items, build_simple_visible_menu(&tray_icon("app")));
    }
}
