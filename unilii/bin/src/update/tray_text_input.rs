use crate::enhanced_tray::{EnhancedTrayState, TrayMenuItem};

pub fn set_text_input_value(
    enhanced_tray_state: &mut Option<EnhancedTrayState>,
    item_id: &str,
    value: &str,
) -> bool {
    let Some(tray_state) = enhanced_tray_state.as_mut() else {
        return false;
    };

    for app in tray_state.tree.apps.values_mut() {
        if update_menu_item_value(&mut app.menu_items, item_id, value) {
            return true;
        }
    }
    false
}

fn update_menu_item_value(items: &mut [TrayMenuItem], item_id: &str, value: &str) -> bool {
    for item in items {
        if item.id == item_id {
            item.default_value = Some(value.to_string());
            return true;
        }
        if update_menu_item_value(&mut item.submenu, item_id, value) {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::set_text_input_value;
    use crate::enhanced_tray::{self, EnhancedTrayState};

    fn text_input_item(
        app_id: &str,
        item_id: &str,
        default_value: &str,
    ) -> enhanced_tray::TrayMenuItem {
        enhanced_tray::TrayMenuItem {
            id: item_id.into(),
            label: "Input".into(),
            action: enhanced_tray::TrayMenuAction::TextInputChanged {
                value: default_value.into(),
            },
            icon: None,
            submenu: vec![],
            enabled: true,
            visible: true,
            checkable: false,
            checked: false,
            shortcut: None,
            is_separator: false,
            app_id: app_id.into(),
            full_path: "Root → Input".into(),
            widget_type: enhanced_tray::TrayWidgetType::TextInput,
            default_value: Some(default_value.into()),
            placeholder: Some("enter".into()),
        }
    }

    fn submenu_item(
        app_id: &str,
        child: enhanced_tray::TrayMenuItem,
    ) -> enhanced_tray::TrayMenuItem {
        enhanced_tray::TrayMenuItem {
            id: "root".into(),
            label: "Root".into(),
            action: enhanced_tray::TrayMenuAction::NavigateToSubmenu {
                item_id: "root".into(),
                submenu_path: vec!["root".into()],
            },
            icon: None,
            submenu: vec![child],
            enabled: true,
            visible: true,
            checkable: false,
            checked: false,
            shortcut: None,
            is_separator: false,
            app_id: app_id.into(),
            full_path: "Root".into(),
            widget_type: enhanced_tray::TrayWidgetType::SubmenuButton,
            default_value: None,
            placeholder: None,
        }
    }

    fn state_with_nested_text_input() -> Option<EnhancedTrayState> {
        let mut state = EnhancedTrayState::new();
        let icon = enhanced_tray::TrayIcon {
            key: "app".into(),
            service: "app.service".into(),
            path: "/StatusNotifierItem".into(),
            id: "app".into(),
            title: "App".into(),
            icon_name: None,
            icon_pixmap: None,
            status: "Active".into(),
            has_menu: true,
            menu_object_path: None,
        };
        state.tree.update_app(icon);
        state.tree.update_app_menu(
            "app",
            vec![submenu_item("app", text_input_item("app", "input", "old"))],
        );
        Some(state)
    }

    #[test]
    fn set_text_input_value_updates_nested_item_in_any_app() {
        let mut state = state_with_nested_text_input();

        assert!(set_text_input_value(&mut state, "input", "new"));

        let state = state.expect("state remains present");
        let input = &state.tree.apps["app"].menu_items[0].submenu[0];
        assert_eq!(input.default_value.as_deref(), Some("new"));
    }
}
