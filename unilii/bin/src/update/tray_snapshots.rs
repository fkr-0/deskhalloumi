use crate::enhanced_tray::{EnhancedTrayState, TrayViewState};
use crate::menus::calendar::CalendarMenuSnapshot;
use crate::menus::mount::MountMenuSnapshot;
use crate::tray::NetworkSnapshot;

pub fn mark_special_view_loading(
    enhanced_tray_state: &mut Option<EnhancedTrayState>,
    icon_key: &str,
    icon_key_for_app_id: impl Fn(&str) -> Option<String>,
) -> bool {
    let Some(tray_state) = enhanced_tray_state.as_mut() else {
        return false;
    };

    match &mut tray_state.current_view {
        TrayViewState::Network {
            app_id,
            loading,
            error,
            ..
        }
        | TrayViewState::Mount {
            app_id,
            loading,
            error,
            ..
        }
        | TrayViewState::Calendar {
            app_id,
            loading,
            error,
            ..
        } if icon_key_for_app_id(app_id).as_deref() == Some(icon_key) => {
            *loading = true;
            *error = None;
            return true;
        }
        _ => {}
    }

    false
}

pub fn apply_network_snapshot(
    enhanced_tray_state: &mut Option<EnhancedTrayState>,
    icon_key: &str,
    result: Result<NetworkSnapshot, String>,
    icon_key_for_app_id: impl FnOnce(&str) -> Option<String>,
) -> bool {
    let Some(tray_state) = enhanced_tray_state.as_mut() else {
        return false;
    };

    if let TrayViewState::Network {
        app_id,
        data,
        loading,
        error,
        ..
    } = &mut tray_state.current_view
    {
        if icon_key_for_app_id(app_id).as_deref() == Some(icon_key) {
            *loading = false;
            match result {
                Ok(snapshot) => {
                    *data = Some(snapshot);
                    *error = None;
                }
                Err(message) => {
                    *error = Some(message);
                }
            }
            return true;
        }
    }

    false
}

pub fn apply_mount_snapshot(
    enhanced_tray_state: &mut Option<EnhancedTrayState>,
    icon_key: &str,
    result: Result<MountMenuSnapshot, String>,
    icon_key_for_app_id: impl FnOnce(&str) -> Option<String>,
) -> bool {
    let Some(tray_state) = enhanced_tray_state.as_mut() else {
        return false;
    };

    if let TrayViewState::Mount {
        app_id,
        data,
        loading,
        error,
        ..
    } = &mut tray_state.current_view
    {
        if icon_key_for_app_id(app_id).as_deref() == Some(icon_key) {
            *loading = false;
            match result {
                Ok(snapshot) => {
                    *data = Some(snapshot);
                    *error = None;
                }
                Err(message) => {
                    *error = Some(message);
                }
            }
            return true;
        }
    }

    false
}

pub fn apply_calendar_snapshot(
    enhanced_tray_state: &mut Option<EnhancedTrayState>,
    icon_key: &str,
    result: Result<CalendarMenuSnapshot, String>,
    icon_key_for_app_id: impl FnOnce(&str) -> Option<String>,
) -> bool {
    let Some(tray_state) = enhanced_tray_state.as_mut() else {
        return false;
    };

    if let TrayViewState::Calendar {
        app_id,
        data,
        loading,
        error,
        ..
    } = &mut tray_state.current_view
    {
        if icon_key_for_app_id(app_id).as_deref() == Some(icon_key) {
            *loading = false;
            match result {
                Ok(snapshot) => {
                    *data = Some(snapshot);
                    *error = None;
                }
                Err(message) => {
                    *error = Some(message);
                }
            }
            return true;
        }
    }

    false
}

pub fn network_toggle_desired_state_and_mark_loading(
    enhanced_tray_state: &mut Option<EnhancedTrayState>,
    icon_key: &str,
    icon_key_for_app_id: impl FnOnce(&str) -> Option<String>,
) -> bool {
    let Some(tray_state) = enhanced_tray_state.as_mut() else {
        return true;
    };

    if let TrayViewState::Network {
        app_id,
        data,
        loading,
        error,
        ..
    } = &mut tray_state.current_view
    {
        if icon_key_for_app_id(app_id).as_deref() == Some(icon_key) {
            let desired_state = data.as_ref().is_none_or(|snapshot| !snapshot.enabled);
            *loading = true;
            *error = None;
            return desired_state;
        }
    }

    true
}

pub fn apply_spawn_command_started(
    enhanced_tray_state: &mut Option<EnhancedTrayState>,
    icon_key: &str,
    icon_key_for_app_id: impl FnOnce(&str) -> Option<String>,
) -> bool {
    let Some(tray_state) = enhanced_tray_state.as_mut() else {
        return false;
    };

    if let TrayViewState::Network {
        app_id,
        loading,
        error,
        ..
    } = &mut tray_state.current_view
    {
        if icon_key_for_app_id(app_id).as_deref() == Some(icon_key) {
            *loading = true;
            *error = None;
            return true;
        }
    }

    false
}

pub fn apply_spawn_command_done(
    enhanced_tray_state: &mut Option<EnhancedTrayState>,
    icon_key: &str,
    result: Result<(), String>,
    icon_key_for_app_id: impl Fn(&str) -> Option<String>,
) -> bool {
    let Some(tray_state) = enhanced_tray_state.as_mut() else {
        return false;
    };

    match &mut tray_state.current_view {
        TrayViewState::Network {
            app_id,
            loading,
            error,
            ..
        }
        | TrayViewState::Mount {
            app_id,
            loading,
            error,
            ..
        }
        | TrayViewState::Calendar {
            app_id,
            loading,
            error,
            ..
        } if icon_key_for_app_id(app_id).as_deref() == Some(icon_key) => {
            *loading = false;
            if let Err(message) = result {
                *error = Some(message);
            }
            return true;
        }
        _ => {}
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::enhanced_tray::EnhancedTrayState;
    use crate::menus::calendar::CalendarMenuSnapshot;
    use crate::menus::mount::MountMenuSnapshot;
    use crate::tray::NetworkSnapshot;

    fn network_snapshot(enabled: bool) -> NetworkSnapshot {
        NetworkSnapshot {
            interface: "wlan0".into(),
            state: "connected".into(),
            enabled,
            connected_ssid: Some("Cafe".into()),
            known_networks: vec![],
            networks: vec![],
        }
    }

    fn state_with_network() -> Option<EnhancedTrayState> {
        let mut state = EnhancedTrayState::new();
        state.current_view = TrayViewState::Network {
            app_id: "network".into(),
            data: None,
            loading: true,
            error: Some("old".into()),
        };
        Some(state)
    }

    fn state_with_mount() -> Option<EnhancedTrayState> {
        let mut state = EnhancedTrayState::new();
        state.current_view = TrayViewState::Mount {
            app_id: "mount".into(),
            data: None,
            loading: true,
            error: Some("old".into()),
        };
        Some(state)
    }

    fn state_with_calendar() -> Option<EnhancedTrayState> {
        let mut state = EnhancedTrayState::new();
        state.current_view = TrayViewState::Calendar {
            app_id: "calendar".into(),
            data: None,
            loading: true,
            error: Some("old".into()),
        };
        Some(state)
    }

    #[test]
    fn network_snapshot_success_updates_data_and_clears_error() {
        let mut state = state_with_network();

        assert!(apply_network_snapshot(
            &mut state,
            "net-key",
            Ok(network_snapshot(true)),
            |_| Some("net-key".into())
        ));

        match state.unwrap().current_view {
            TrayViewState::Network {
                data,
                loading,
                error,
                ..
            } => {
                assert!(!loading);
                assert!(error.is_none());
                assert_eq!(data.unwrap().connected_ssid.as_deref(), Some("Cafe"));
            }
            other => panic!("expected network view, got {other:?}"),
        }
    }

    #[test]
    fn snapshot_error_keeps_data_empty_and_records_error() {
        let mut state = state_with_mount();

        assert!(apply_mount_snapshot(
            &mut state,
            "mount-key",
            Err("boom".into()),
            |_| Some("mount-key".into())
        ));

        match state.unwrap().current_view {
            TrayViewState::Mount {
                data,
                loading,
                error,
                ..
            } => {
                assert!(!loading);
                assert!(data.is_none());
                assert_eq!(error.as_deref(), Some("boom"));
            }
            other => panic!("expected mount view, got {other:?}"),
        }
    }

    #[test]
    fn calendar_snapshot_success_updates_data() {
        let mut state = state_with_calendar();
        let snapshot = CalendarMenuSnapshot::from_accounts(vec!["work".into()]);

        assert!(apply_calendar_snapshot(
            &mut state,
            "calendar-key",
            Ok(snapshot),
            |_| Some("calendar-key".into())
        ));

        match state.unwrap().current_view {
            TrayViewState::Calendar {
                data,
                loading,
                error,
                ..
            } => {
                assert!(!loading);
                assert!(error.is_none());
                assert_eq!(data.unwrap().account_ids, vec!["work".to_string()]);
            }
            other => panic!("expected calendar view, got {other:?}"),
        }
    }

    #[test]
    fn refresh_marks_any_special_view_loading_and_clears_error() {
        let mut state = state_with_mount();

        assert!(mark_special_view_loading(
            &mut state,
            "mount-key",
            |_| Some("mount-key".into())
        ));

        match state.unwrap().current_view {
            TrayViewState::Mount { loading, error, .. } => {
                assert!(loading);
                assert!(error.is_none());
            }
            other => panic!("expected mount view, got {other:?}"),
        }
    }

    #[test]
    fn network_toggle_computes_desired_state_from_snapshot_and_marks_loading() {
        let mut state = state_with_network();
        apply_network_snapshot(&mut state, "net-key", Ok(network_snapshot(true)), |_| {
            Some("net-key".into())
        });

        let desired = network_toggle_desired_state_and_mark_loading(&mut state, "net-key", |_| {
            Some("net-key".into())
        });

        assert!(!desired);
        match state.unwrap().current_view {
            TrayViewState::Network { loading, error, .. } => {
                assert!(loading);
                assert!(error.is_none());
            }
            other => panic!("expected network view, got {other:?}"),
        }
    }

    #[test]
    fn spawn_command_started_marks_network_loading_and_clears_error() {
        let mut state = state_with_network();

        assert!(apply_spawn_command_started(
            &mut state,
            "net-key",
            |_| Some("net-key".into())
        ));

        match state.unwrap().current_view {
            TrayViewState::Network { loading, error, .. } => {
                assert!(loading);
                assert!(error.is_none());
            }
            other => panic!("expected network view, got {other:?}"),
        }
    }

    #[test]
    fn spawn_command_done_clears_loading_and_records_error_for_special_views() {
        let mut state = state_with_calendar();

        assert!(apply_spawn_command_done(
            &mut state,
            "calendar-key",
            Err("command failed".into()),
            |_| Some("calendar-key".into())
        ));

        match state.unwrap().current_view {
            TrayViewState::Calendar { loading, error, .. } => {
                assert!(!loading);
                assert_eq!(error.as_deref(), Some("command failed"));
            }
            other => panic!("expected calendar view, got {other:?}"),
        }
    }

    #[test]
    fn mount_snapshot_success_updates_data_and_clears_error() {
        let mut state = state_with_mount();

        assert!(apply_mount_snapshot(
            &mut state,
            "mount-key",
            Ok(MountMenuSnapshot::default()),
            |_| Some("mount-key".into())
        ));

        match state.unwrap().current_view {
            TrayViewState::Mount {
                data,
                loading,
                error,
                ..
            } => {
                assert!(!loading);
                assert!(error.is_none());
                assert!(data.is_some());
            }
            other => panic!("expected mount view, got {other:?}"),
        }
    }
}
