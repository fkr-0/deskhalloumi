#![allow(dead_code)]
// FIXME(T6): Calendar menu model is a planned menu-system slice pending canonical MenuModel integration.

use super::common::{FilterableMenu, QuickjumpMenu};
use super::presentation::{ActionItemOptions, action_item, section_item, status_item};
use crate::enhanced_tray::{TrayMenuAction, TrayMenuItem};
use chrono::{DateTime, Local};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CalendarAgendaItem {
    pub account_id: String,
    pub title: String,
    pub start_rfc3339: String,
    pub location: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CalendarAccountError {
    pub account_id: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CalendarMenuSnapshot {
    pub account_ids: Vec<String>,
    pub events: Vec<CalendarAgendaItem>,
    pub account_errors: Vec<CalendarAccountError>,
    pub stale: bool,
    pub status: String,
}

impl CalendarMenuSnapshot {
    pub fn from_accounts(account_ids: Vec<String>) -> Self {
        let status = if account_ids.is_empty() {
            "No calendar accounts configured".to_string()
        } else {
            format!("{} account(s) configured", account_ids.len())
        };
        Self {
            account_ids,
            events: Vec::new(),
            account_errors: Vec::new(),
            stale: false,
            status,
        }
    }
}

pub fn format_event_start(value: &str) -> (String, String) {
    DateTime::parse_from_rfc3339(value)
        .map(|event| {
            let local = event.with_timezone(&Local);
            (
                local.format("%A, %Y-%m-%d").to_string(),
                local.format("%H:%M").to_string(),
            )
        })
        .unwrap_or_else(|_| ("Other dates".to_string(), value.to_string()))
}

pub fn build_menu_items(
    app_id: &str,
    snapshot: Option<&CalendarMenuSnapshot>,
    loading: bool,
    error: Option<&str>,
    config: &deskhalloumi_core::config::CalendarMenuConfig,
) -> Vec<TrayMenuItem> {
    let mut items = vec![
        action_item(
            app_id,
            "calendar-refresh",
            "Refresh calendars",
            TrayMenuAction::SpawnCommand("calendar:refresh".to_string()),
            ActionItemOptions {
                subtitle: Some(format!(
                    "Synchronize the next {} day(s)",
                    config.agenda_days
                )),
                icon: Some("view-refresh".to_string()),
                shortcut: Some("R".to_string()),
                enabled: true,
            },
        ),
        action_item(
            app_id,
            "calendar-open",
            "Open calendar",
            TrayMenuAction::SpawnCommand(config.application_command.clone()),
            ActionItemOptions {
                subtitle: Some("Open the configured desktop calendar application".to_string()),
                icon: Some("x-office-calendar".to_string()),
                shortcut: None,
                enabled: !config.application_command.trim().is_empty(),
            },
        ),
    ];

    if loading {
        items.push(status_item(
            app_id,
            "calendar-loading",
            "Loading calendar data…",
            Some("Configured accounts are being synchronized".to_string()),
        ));
        return items;
    }
    if let Some(error) = error {
        items.push(status_item(
            app_id,
            "calendar-error",
            "Calendar refresh failed",
            Some(error.to_string()),
        ));
    }
    let Some(snapshot) = snapshot else {
        items.push(status_item(
            app_id,
            "calendar-no-snapshot",
            "No calendar snapshot available",
            Some("Use Refresh calendars to retry".to_string()),
        ));
        return items;
    };

    items.push(status_item(
        app_id,
        "calendar-status",
        snapshot.status.clone(),
        snapshot
            .stale
            .then(|| "Showing partial or cached data".to_string()),
    ));
    items.push(section_item(
        app_id,
        "calendar-accounts",
        "Accounts",
        Some(snapshot.account_ids.len().min(config.max_account_rows)),
    ));
    if snapshot.account_ids.is_empty() {
        items.push(status_item(
            app_id,
            "calendar-accounts-empty",
            "No calendar accounts configured",
            Some("Add accounts under menus.calendar.accounts".to_string()),
        ));
    } else {
        for account in snapshot.account_ids.iter().take(config.max_account_rows) {
            let account_error = snapshot
                .account_errors
                .iter()
                .find(|entry| entry.account_id == *account);
            items.push(status_item(
                app_id,
                format!("calendar-account:{account}"),
                account.clone(),
                Some(match account_error {
                    Some(error) => format!("Sync error · {}", error.message),
                    None => "Configured and synchronized".to_string(),
                }),
            ));
        }
    }

    items.push(section_item(
        app_id,
        "calendar-upcoming",
        "Upcoming events",
        Some(snapshot.events.len().min(config.max_event_rows)),
    ));
    if snapshot.events.is_empty() {
        items.push(status_item(
            app_id,
            "calendar-events-empty",
            format!("No events in the next {} day(s)", config.agenda_days),
            None,
        ));
    } else {
        let mut current_day = String::new();
        for (index, event) in snapshot
            .events
            .iter()
            .take(config.max_event_rows)
            .enumerate()
        {
            let (day, time) = format_event_start(&event.start_rfc3339);
            if day != current_day {
                current_day = day.clone();
                items.push(section_item(
                    app_id,
                    format!("calendar-day:{index}"),
                    day,
                    None,
                ));
            }
            let mut details = vec![time, event.account_id.clone()];
            if config.show_locations
                && let Some(location) = event
                    .location
                    .as_deref()
                    .filter(|value| !value.trim().is_empty())
            {
                details.push(location.to_string());
            }
            items.push(action_item(
                app_id,
                format!("calendar-event:{index}:{}", event.title),
                event.title.clone(),
                TrayMenuAction::SpawnCommand(config.application_command.clone()),
                ActionItemOptions {
                    subtitle: Some(details.join(" · ")),
                    icon: Some("appointment-new".to_string()),
                    shortcut: Some("Open".to_string()),
                    enabled: !config.application_command.trim().is_empty(),
                },
            ));
        }
    }

    if !snapshot.account_errors.is_empty() {
        items.push(section_item(
            app_id,
            "calendar-errors",
            "Account issues",
            Some(snapshot.account_errors.len()),
        ));
        for account_error in &snapshot.account_errors {
            items.push(status_item(
                app_id,
                format!("calendar-error:{}", account_error.account_id),
                account_error.account_id.clone(),
                Some(account_error.message.clone()),
            ));
        }
    }
    items
}

impl FilterableMenu for CalendarMenuSnapshot {
    type ItemId = String;

    fn filter_tokens_for(&self, item_id: &Self::ItemId) -> Vec<String> {
        if let Some(event) = self.events.iter().find(|row| &row.title == item_id) {
            return vec![
                event.title.clone(),
                event.account_id.clone(),
                event.start_rfc3339.clone(),
                event.location.clone().unwrap_or_default(),
            ];
        }
        if self
            .account_ids
            .iter()
            .any(|account_id| account_id == item_id)
        {
            return vec![item_id.clone(), "account".to_string()];
        }
        Vec::new()
    }
}

impl QuickjumpMenu for CalendarMenuSnapshot {
    type ItemId = String;

    fn quickjump_targets(&self) -> Vec<Self::ItemId> {
        self.events
            .iter()
            .map(|event| event.title.clone())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_start_is_humanized_or_preserved() {
        let (day, time) = format_event_start("2026-07-10T14:30:00+02:00");
        assert!(day.contains("2026-07-10"));
        assert_eq!(time, "14:30");
        assert_eq!(format_event_start("unknown").1, "unknown");
    }

    #[test]
    fn calendar_items_group_events_and_show_partial_errors() {
        let snapshot = CalendarMenuSnapshot {
            account_ids: vec!["work".into()],
            events: vec![CalendarAgendaItem {
                account_id: "work".into(),
                title: "Release review".into(),
                start_rfc3339: "2026-07-10T14:30:00+02:00".into(),
                location: Some("Studio".into()),
            }],
            account_errors: vec![CalendarAccountError {
                account_id: "work".into(),
                message: "temporary failure".into(),
            }],
            stale: true,
            status: "Partial calendar data".into(),
        };
        let items = build_menu_items(
            "calendar",
            Some(&snapshot),
            false,
            None,
            &deskhalloumi_core::config::CalendarMenuConfig::default(),
        );
        assert!(
            items
                .iter()
                .any(|item| item.id.starts_with("section:calendar-day:"))
        );
        let event = items
            .iter()
            .find(|item| item.id.contains("Release review"))
            .unwrap();
        assert!(event.label.contains("Studio"));
        assert!(
            items
                .iter()
                .any(|item| item.id == "section:calendar-errors")
        );
    }

    #[test]
    fn calendar_row_limits_are_respected() {
        let config = deskhalloumi_core::config::CalendarMenuConfig {
            max_event_rows: 1,
            ..Default::default()
        };
        let snapshot = CalendarMenuSnapshot {
            events: vec![
                CalendarAgendaItem {
                    account_id: "a".into(),
                    title: "One".into(),
                    start_rfc3339: "2026-07-10T10:00:00+02:00".into(),
                    location: None,
                },
                CalendarAgendaItem {
                    account_id: "a".into(),
                    title: "Two".into(),
                    start_rfc3339: "2026-07-10T11:00:00+02:00".into(),
                    location: None,
                },
            ],
            ..CalendarMenuSnapshot::default()
        };
        let items = build_menu_items("calendar", Some(&snapshot), false, None, &config);
        assert_eq!(
            items
                .iter()
                .filter(|item| item.id.starts_with("calendar-event:"))
                .count(),
            1
        );
    }
}
