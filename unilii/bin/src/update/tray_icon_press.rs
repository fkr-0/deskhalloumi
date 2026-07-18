use crate::enhanced_tray::{self, EnhancedTrayState, TrayMenuItem, TrayViewState};
use crate::tray;
use crate::update::tray_menu_fetch::build_simple_visible_menu;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayIconOpenKind {
    Network,
    Mount,
    Calendar,
    Regular,
}

pub fn should_close_current_tray_view(
    current: Option<&EnhancedTrayState>,
    icon: &tray::TrayIcon,
) -> bool {
    let Some(current) = current else {
        return false;
    };

    match &current.current_view {
        TrayViewState::SingleApp { app_id, .. }
        | TrayViewState::Network { app_id, .. }
        | TrayViewState::Mount { app_id, .. }
        | TrayViewState::Calendar { app_id, .. } => icon.id == *app_id,
        _ => false,
    }
}

pub fn open_tray_icon_state(icon: &tray::TrayIcon, kind: TrayIconOpenKind) -> EnhancedTrayState {
    open_tray_icon_state_with_menu(icon, kind, None)
}

pub fn open_tray_icon_state_with_menu(
    icon: &tray::TrayIcon,
    kind: TrayIconOpenKind,
    custom_menu: Option<Vec<TrayMenuItem>>,
) -> EnhancedTrayState {
    let mut tree = enhanced_tray::TrayMenuTree::new();
    let enhanced_icon = to_enhanced_tray_icon(
        icon,
        !matches!(kind, TrayIconOpenKind::Regular) || icon.has_menu,
    );
    tree.update_app(enhanced_icon.clone());

    let current_view = match kind {
        TrayIconOpenKind::Network => TrayViewState::Network {
            app_id: icon.id.clone(),
            data: None,
            loading: true,
            error: None,
        },
        TrayIconOpenKind::Mount => TrayViewState::Mount {
            app_id: icon.id.clone(),
            data: None,
            loading: true,
            error: None,
        },
        TrayIconOpenKind::Calendar => TrayViewState::Calendar {
            app_id: icon.id.clone(),
            data: None,
            loading: true,
            error: None,
        },
        TrayIconOpenKind::Regular => {
            let menu = custom_menu.unwrap_or_else(|| build_simple_visible_menu(&enhanced_icon));
            if !menu.is_empty() {
                tree.update_app_menu(&icon.id, menu);
            }
            let navigation = tree.get_app_navigation(&icon.id);
            TrayViewState::SingleApp {
                app_id: icon.id.clone(),
                navigation,
                submenu_path: Vec::new(),
            }
        }
    };

    EnhancedTrayState {
        tree,
        current_view,
        animation_progress: 0.0,
        animation_target: 1.0,
        selected_index: Some(0),
        filter_text: String::new(),
    }
}

pub fn to_enhanced_tray_icon(icon: &tray::TrayIcon, has_menu: bool) -> enhanced_tray::TrayIcon {
    enhanced_tray::TrayIcon {
        key: icon.key.clone(),
        service: icon.service.clone(),
        path: icon.path.clone(),
        id: icon.id.clone(),
        title: icon.title.clone(),
        icon_name: icon.icon_name.clone(),
        icon_pixmap: icon.icon_pixmap.clone(),
        status: icon.status.clone(),
        has_menu,
        menu_object_path: icon.menu_object_path.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::{TrayIconOpenKind, open_tray_icon_state, should_close_current_tray_view};
    use crate::enhanced_tray::TrayViewState;
    use crate::tray;

    fn tray_icon(id: &str, title: &str, icon_name: Option<&str>) -> tray::TrayIcon {
        tray::TrayIcon {
            key: format!("key-{id}"),
            service: format!("{id}.service"),
            path: "/StatusNotifierItem".into(),
            id: id.into(),
            title: title.into(),
            icon_name: icon_name.map(str::to_string),
            icon_pixmap: None,
            status: "Active".into(),
            has_menu: true,
            menu_object_path: None,
        }
    }

    #[test]
    fn network_icon_opens_loading_network_view() {
        let icon = tray_icon("nm-applet", "Network", Some("network-wireless"));

        let state = open_tray_icon_state(&icon, TrayIconOpenKind::Network);

        match state.current_view {
            TrayViewState::Network {
                app_id,
                data,
                loading,
                error,
            } => {
                assert_eq!(app_id, "nm-applet");
                assert!(data.is_none());
                assert!(loading);
                assert!(error.is_none());
            }
            other => panic!("expected network view, got {other:?}"),
        }
        assert_eq!(state.animation_target, 1.0);
        assert_eq!(state.selected_index, Some(0));
    }

    #[test]
    fn mount_and_calendar_icons_open_loading_specialized_views() {
        let mount = tray_icon("usb-drive", "USB Drive", Some("drive-removable-media"));
        let calendar = tray_icon("calendar", "Calendar", Some("office-calendar"));

        let mount_state = open_tray_icon_state(&mount, TrayIconOpenKind::Mount);
        let calendar_state = open_tray_icon_state(&calendar, TrayIconOpenKind::Calendar);

        match mount_state.current_view {
            TrayViewState::Mount {
                app_id,
                data,
                loading,
                error,
            } => {
                assert_eq!(app_id, "usb-drive");
                assert!(data.is_none());
                assert!(loading);
                assert!(error.is_none());
            }
            other => panic!("expected mount view, got {other:?}"),
        }
        match calendar_state.current_view {
            TrayViewState::Calendar {
                app_id,
                data,
                loading,
                error,
            } => {
                assert_eq!(app_id, "calendar");
                assert!(data.is_none());
                assert!(loading);
                assert!(error.is_none());
            }
            other => panic!("expected calendar view, got {other:?}"),
        }
    }

    #[test]
    fn regular_icon_opens_single_app_with_fallback_menu() {
        let icon = tray_icon("volume", "Volume", Some("audio-volume-high"));

        let state = open_tray_icon_state(&icon, TrayIconOpenKind::Regular);

        match state.current_view {
            TrayViewState::SingleApp {
                app_id,
                submenu_path,
                ..
            } => {
                assert_eq!(app_id, "volume");
                assert!(submenu_path.is_empty());
            }
            other => panic!("expected single app view, got {other:?}"),
        }
        let app = state.tree.apps.get("volume").expect("app exists");
        assert_eq!(app.menu_items.len(), 3);
        assert_eq!(app.menu_items[0].id, "activate");
    }

    #[test]
    fn clicking_current_single_app_should_close_existing_view() {
        let icon = tray_icon("volume", "Volume", Some("audio-volume-high"));
        let current = open_tray_icon_state(&icon, TrayIconOpenKind::Regular);

        assert!(should_close_current_tray_view(Some(&current), &icon));
        assert!(!should_close_current_tray_view(None, &icon));
    }
}
