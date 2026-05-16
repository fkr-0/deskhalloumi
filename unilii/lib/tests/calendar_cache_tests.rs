use unilii_lib::calendar::{CalendarCache, CalendarEvent, ReminderState};

fn event(id: &str, account_id: &str, start: &str) -> CalendarEvent {
    CalendarEvent {
        id: id.to_string(),
        account_id: account_id.to_string(),
        title: format!("event-{id}"),
        start_rfc3339: start.to_string(),
        end_rfc3339: start.to_string(),
        location: None,
        all_day: false,
    }
}

#[test]
fn upsert_updates_sync_token_and_sorts_events() {
    let mut cache = CalendarCache::default();
    cache.upsert_events(
        "primary",
        Some("token-1".to_string()),
        vec![
            event("b", "primary", "2026-04-16T11:00:00Z"),
            event("a", "primary", "2026-04-16T10:00:00Z"),
        ],
    );

    let events = cache.events_for_account("primary");
    assert_eq!(events.len(), 2);
    assert_eq!(events[0].id, "a");
    assert_eq!(events[1].id, "b");
    assert_eq!(cache.sync_token("primary"), Some("token-1"));
}

#[test]
fn reminder_state_roundtrip() {
    let mut cache = CalendarCache::default();
    cache.set_reminder_state(ReminderState {
        event_id: "ev-1".to_string(),
        dismissed: false,
        snoozed_until_rfc3339: Some("2026-04-16T12:15:00Z".to_string()),
    });

    let reminder = cache.reminder_state("ev-1").expect("reminder must exist");
    assert_eq!(
        reminder.snoozed_until_rfc3339.as_deref(),
        Some("2026-04-16T12:15:00Z")
    );
}
