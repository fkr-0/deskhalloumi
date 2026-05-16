pub mod cache;
pub mod caldav;
pub mod provider;

pub use cache::CalendarCache;
pub use provider::{CalendarError, CalendarProvider};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CalendarEvent {
    pub id: String,
    pub account_id: String,
    pub title: String,
    pub start_rfc3339: String,
    pub end_rfc3339: String,
    pub location: Option<String>,
    pub all_day: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReminderState {
    pub event_id: String,
    pub dismissed: bool,
    pub snoozed_until_rfc3339: Option<String>,
}
