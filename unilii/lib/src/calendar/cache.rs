use std::collections::HashMap;

use super::{CalendarEvent, ReminderState};

#[derive(Debug, Default, Clone)]
pub struct CalendarCache {
    events_by_id: HashMap<String, CalendarEvent>,
    sync_tokens: HashMap<String, String>,
    reminders: HashMap<String, ReminderState>,
}

impl CalendarCache {
    pub fn upsert_events(
        &mut self,
        account_id: &str,
        sync_token: Option<String>,
        events: Vec<CalendarEvent>,
    ) {
        for event in events {
            self.events_by_id.insert(event.id.clone(), event);
        }
        if let Some(token) = sync_token {
            self.sync_tokens.insert(account_id.to_string(), token);
        }
    }

    pub fn sync_token(&self, account_id: &str) -> Option<&str> {
        self.sync_tokens.get(account_id).map(String::as_str)
    }

    pub fn events_for_account(&self, account_id: &str) -> Vec<CalendarEvent> {
        let mut out: Vec<CalendarEvent> = self
            .events_by_id
            .values()
            .filter(|event| event.account_id == account_id)
            .cloned()
            .collect();
        out.sort_by(|left, right| left.start_rfc3339.cmp(&right.start_rfc3339));
        out
    }

    pub fn set_reminder_state(&mut self, state: ReminderState) {
        self.reminders.insert(state.event_id.clone(), state);
    }

    pub fn reminder_state(&self, event_id: &str) -> Option<&ReminderState> {
        self.reminders.get(event_id)
    }
}
