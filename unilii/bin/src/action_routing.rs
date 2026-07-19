//! Pure typed-action routing between the action bus and the bar update channel.

use deskhalloumi_core::{
    action_bus::{ActionBusResponse, DesktopAction},
    keys::KeybindingResult,
    runtime::global_runtime_metrics,
};

pub enum ActionBusRoute {
    Queue(KeybindingResult),
    Respond(ActionBusResponse),
}

pub fn route_action_bus_request(
    request_id: String,
    action: DesktopAction,
) -> Result<ActionBusRoute, String> {
    if matches!(
        &action,
        DesktopAction::Bar(command)
            if command == "runtime-metrics" || command == "diagnostics:runtime-metrics"
    ) {
        let data = serde_json::to_value(global_runtime_metrics().snapshot())
            .map_err(|error| format!("failed to serialize runtime metrics: {error}"))?;
        return Ok(ActionBusRoute::Respond(ActionBusResponse::ok_with_data(
            request_id,
            "runtime metrics",
            data,
        )));
    }

    route_desktop_action(action).map(ActionBusRoute::Queue)
}

pub fn route_desktop_action(action: DesktopAction) -> Result<KeybindingResult, String> {
    match action {
        DesktopAction::Bar(command) => Ok(KeybindingResult::BarAction(command)),
        DesktopAction::Tray(command) => Ok(KeybindingResult::TrayAction(command)),
        DesktopAction::Widget(command) => Ok(KeybindingResult::WidgetAction(command)),
        DesktopAction::Shell(_) | DesktopAction::Menu(_) => {
            Err("shell and managed-menu actions are executed by hotkeyd, not the bar".to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn routes_typed_bar_tray_and_widget_actions() {
        assert!(matches!(
            route_desktop_action(DesktopAction::Bar("reload".into())),
            Ok(KeybindingResult::BarAction(command)) if command == "reload"
        ));
        assert!(matches!(
            route_desktop_action(DesktopAction::Tray("show-favorites".into())),
            Ok(KeybindingResult::TrayAction(command)) if command == "show-favorites"
        ));
        assert!(matches!(
            route_desktop_action(DesktopAction::Widget("wifi:refresh".into())),
            Ok(KeybindingResult::WidgetAction(command)) if command == "wifi:refresh"
        ));
    }

    #[test]
    fn rejects_actions_owned_by_hotkeyd() {
        assert!(route_desktop_action(DesktopAction::Shell("true".into())).is_err());
        assert!(route_desktop_action(DesktopAction::Menu("filter-tab:toggle".into())).is_err());
    }

    #[test]
    fn runtime_metrics_is_answered_without_queueing_an_action() {
        let route = route_action_bus_request(
            "metrics-1".to_string(),
            DesktopAction::Bar("runtime-metrics".to_string()),
        )
        .unwrap();
        let ActionBusRoute::Respond(response) = route else {
            panic!("runtime metrics must be answered synchronously");
        };
        assert!(response.ok);
        assert!(response.data.unwrap().get("active_tasks").is_some());
    }
}
