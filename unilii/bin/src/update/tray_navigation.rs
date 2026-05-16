use crate::enhanced_tray::{EnhancedTrayState, TrayViewState};

pub fn navigate_left(enhanced_tray_state: &mut Option<EnhancedTrayState>) {
    let Some(tray_state) = enhanced_tray_state.as_mut() else {
        return;
    };

    if let TrayViewState::SingleApp { navigation, .. } = &tray_state.current_view {
        if navigation.can_go_left && navigation.current_app_index > 0 {
            let new_index = navigation.current_app_index - 1;
            if let Some(new_app_id) = navigation.app_order.get(new_index) {
                let new_navigation = tray_state.tree.get_app_navigation(new_app_id);
                tray_state.current_view = TrayViewState::SingleApp {
                    app_id: new_app_id.clone(),
                    navigation: new_navigation,
                    submenu_path: Vec::new(),
                };
            }
        }
    }
}

pub fn navigate_right(enhanced_tray_state: &mut Option<EnhancedTrayState>) {
    let Some(tray_state) = enhanced_tray_state.as_mut() else {
        return;
    };

    if let TrayViewState::SingleApp { navigation, .. } = &tray_state.current_view {
        if navigation.can_go_right
            && navigation.current_app_index < navigation.app_order.len().saturating_sub(1)
        {
            let new_index = navigation.current_app_index + 1;
            if let Some(new_app_id) = navigation.app_order.get(new_index) {
                let new_navigation = tray_state.tree.get_app_navigation(new_app_id);
                tray_state.current_view = TrayViewState::SingleApp {
                    app_id: new_app_id.clone(),
                    navigation: new_navigation,
                    submenu_path: Vec::new(),
                };
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::enhanced_tray::{self, EnhancedTrayState, TrayViewState};
    use super::{navigate_left, navigate_right};

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

    fn state_on(app_id: &str) -> Option<EnhancedTrayState> {
        let mut state = EnhancedTrayState::new();
        state.tree.update_app(tray_icon("left"));
        state.tree.update_app(tray_icon("right"));
        state.current_view = TrayViewState::SingleApp {
            app_id: app_id.into(),
            navigation: state.tree.get_app_navigation(app_id),
            submenu_path: vec!["nested".into()],
        };
        Some(state)
    }

    #[test]
    fn navigate_left_moves_to_previous_app_and_resets_submenu_path() {
        let mut state = state_on("right");

        navigate_left(&mut state);

        let state = state.expect("state remains present");
        match state.current_view {
            TrayViewState::SingleApp { app_id, navigation, submenu_path } => {
                assert_eq!(app_id, "left");
                assert_eq!(navigation.current_app_index, 0);
                assert!(!navigation.can_go_left);
                assert!(navigation.can_go_right);
                assert!(submenu_path.is_empty());
            }
            other => panic!("expected single app view, got {other:?}"),
        }
    }

    #[test]
    fn navigate_right_moves_to_next_app_and_resets_submenu_path() {
        let mut state = state_on("left");

        navigate_right(&mut state);

        let state = state.expect("state remains present");
        match state.current_view {
            TrayViewState::SingleApp { app_id, navigation, submenu_path } => {
                assert_eq!(app_id, "right");
                assert_eq!(navigation.current_app_index, 1);
                assert!(navigation.can_go_left);
                assert!(!navigation.can_go_right);
                assert!(submenu_path.is_empty());
            }
            other => panic!("expected single app view, got {other:?}"),
        }
    }
}
