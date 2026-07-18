use crate::enhanced_tray::{EnhancedTrayState, TrayMenuItem, TrayMenuTree, TrayViewState};

fn first_selectable_index(items: &[TrayMenuItem]) -> Option<usize> {
    items
        .iter()
        .filter(|item| item.visible)
        .enumerate()
        .find_map(|(index, item)| (item.enabled && !item.is_separator).then_some(index))
}

fn items_at_path<'a>(
    tree: &'a TrayMenuTree,
    app_id: &str,
    submenu_path: &[String],
) -> Option<&'a [TrayMenuItem]> {
    let app = tree.apps.get(app_id)?;
    let mut items = app.menu_items.as_slice();
    for segment in submenu_path {
        let next = items.iter().find(|item| item.id == *segment)?;
        items = next.submenu.as_slice();
    }
    Some(items)
}

pub fn show_aggregated(enhanced_tray_state: &mut Option<EnhancedTrayState>) {
    let Some(tray_state) = enhanced_tray_state.as_mut() else {
        return;
    };

    let items = tray_state.tree.get_aggregated_menu(None);
    tray_state.selected_index = first_selectable_index(&items);
    tray_state.current_view = TrayViewState::Aggregated {
        items,
        filter: None,
    };
}

pub fn show_favorites(enhanced_tray_state: &mut Option<EnhancedTrayState>) {
    let Some(tray_state) = enhanced_tray_state.as_mut() else {
        return;
    };

    let items = tray_state.tree.get_favorites_menu();
    tray_state.selected_index = first_selectable_index(&items);
    tray_state.current_view = TrayViewState::Favorites { items };
}

pub fn update_filter(enhanced_tray_state: &mut Option<EnhancedTrayState>, filter_text: String) {
    let Some(tray_state) = enhanced_tray_state.as_mut() else {
        return;
    };

    if let TrayViewState::Aggregated { items, filter } = &mut tray_state.current_view {
        *filter = if filter_text.is_empty() {
            None
        } else {
            Some(filter_text.clone())
        };
        *items = tray_state.tree.get_aggregated_menu(filter.as_deref());
        tray_state.selected_index = first_selectable_index(items);
    }
    tray_state.filter_text = filter_text;
}

pub fn enter_submenu(
    enhanced_tray_state: &mut Option<EnhancedTrayState>,
    app_id: &str,
    submenu_path: Vec<String>,
) {
    let Some(tray_state) = enhanced_tray_state.as_mut() else {
        return;
    };

    if let TrayViewState::SingleApp {
        app_id: current_app_id,
        navigation,
        ..
    } = &tray_state.current_view
        && current_app_id == app_id
    {
        let Some(items) = items_at_path(&tray_state.tree, app_id, &submenu_path) else {
            return;
        };
        let selected_index = first_selectable_index(items);
        tray_state.current_view = TrayViewState::SingleApp {
            app_id: app_id.to_string(),
            navigation: navigation.clone(),
            submenu_path,
        };
        tray_state.selected_index = selected_index;
    }
}

pub fn exit_submenu(enhanced_tray_state: &mut Option<EnhancedTrayState>) {
    let Some(tray_state) = enhanced_tray_state.as_mut() else {
        return;
    };

    if let TrayViewState::SingleApp {
        app_id,
        navigation,
        submenu_path,
    } = &tray_state.current_view
    {
        let mut new_path = submenu_path.clone();
        new_path.pop();
        let selected_index =
            items_at_path(&tray_state.tree, app_id, &new_path).and_then(first_selectable_index);
        tray_state.current_view = TrayViewState::SingleApp {
            app_id: app_id.clone(),
            navigation: navigation.clone(),
            submenu_path: new_path,
        };
        tray_state.selected_index = selected_index;
    }
}

#[cfg(test)]
mod tests {
    use super::{enter_submenu, exit_submenu, show_aggregated, show_favorites, update_filter};
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

    fn state_with_menu() -> Option<EnhancedTrayState> {
        let mut state = EnhancedTrayState::new();
        state.tree.update_app(tray_icon("app"));
        state.tree.update_app_menu(
            "app",
            vec![
                menu_item("app", "open", "Open"),
                menu_item("app", "quit", "Quit"),
            ],
        );
        Some(state)
    }

    #[test]
    fn show_aggregated_and_filter_update_refresh_view_items() {
        let mut state = state_with_menu();

        show_aggregated(&mut state);
        update_filter(&mut state, "open".to_string());

        let state = state.expect("state remains present");
        assert_eq!(state.filter_text, "open");
        match state.current_view {
            TrayViewState::Aggregated { items, filter } => {
                assert_eq!(filter.as_deref(), Some("open"));
                assert_eq!(items.len(), 1);
                assert_eq!(items[0].id, "open");
            }
            other => panic!("expected aggregated view, got {other:?}"),
        }
    }

    #[test]
    fn show_favorites_uses_tree_favorites() {
        let mut state = state_with_menu();
        state.as_mut().unwrap().tree.toggle_favorite("app", "quit");

        show_favorites(&mut state);

        let state = state.expect("state remains present");
        match state.current_view {
            TrayViewState::Favorites { items } => {
                assert_eq!(items.len(), 1);
                assert_eq!(items[0].id, "quit");
            }
            other => panic!("expected favorites view, got {other:?}"),
        }
    }

    #[test]
    fn submenu_enter_and_exit_update_path_and_selected_index() {
        let mut state = state_with_menu();
        let mut status = menu_item("app", "status", "Review this action");
        status.enabled = false;
        let mut child = menu_item("app", "child", "Child");
        child.widget_type = enhanced_tray::TrayWidgetType::SubmenuButton;
        child.submenu = vec![status, menu_item("app", "confirm", "Confirm")];
        let mut root = menu_item("app", "root", "Root");
        root.widget_type = enhanced_tray::TrayWidgetType::SubmenuButton;
        root.submenu = vec![child];
        state
            .as_mut()
            .unwrap()
            .tree
            .update_app_menu("app", vec![root]);
        let navigation = state.as_ref().unwrap().tree.get_app_navigation("app");
        state.as_mut().unwrap().current_view = TrayViewState::SingleApp {
            app_id: "app".into(),
            navigation,
            submenu_path: vec![],
        };
        state.as_mut().unwrap().selected_index = Some(3);

        enter_submenu(&mut state, "app", vec!["root".into(), "child".into()]);
        assert_eq!(state.as_ref().unwrap().selected_index, Some(1));
        exit_submenu(&mut state);

        let state = state.expect("state remains present");
        assert_eq!(state.selected_index, Some(0));
        match state.current_view {
            TrayViewState::SingleApp {
                app_id,
                submenu_path,
                ..
            } => {
                assert_eq!(app_id, "app");
                assert_eq!(submenu_path, vec!["root".to_string()]);
            }
            other => panic!("expected single app view, got {other:?}"),
        }
    }

    #[test]
    fn invalid_submenu_path_does_not_change_the_current_view() {
        let mut state = state_with_menu();
        let navigation = state.as_ref().unwrap().tree.get_app_navigation("app");
        state.as_mut().unwrap().current_view = TrayViewState::SingleApp {
            app_id: "app".into(),
            navigation,
            submenu_path: vec![],
        };
        state.as_mut().unwrap().selected_index = Some(1);

        enter_submenu(&mut state, "app", vec!["missing".into()]);

        let state = state.expect("state remains present");
        assert_eq!(state.selected_index, Some(1));
        assert!(matches!(
            state.current_view,
            TrayViewState::SingleApp { submenu_path, .. } if submenu_path.is_empty()
        ));
    }
}
