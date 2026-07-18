use std::fmt;

use super::CalendarEvent;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CalendarError {
    NotImplemented,
    Backend(String),
}

impl fmt::Display for CalendarError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotImplemented => f.write_str("provider not implemented"),
            Self::Backend(msg) => write!(f, "backend error: {msg}"),
        }
    }
}

impl std::error::Error for CalendarError {}

#[async_trait::async_trait]
pub trait CalendarProvider: Send + Sync {
    async fn fetch_events(
        &self,
        account_id: &str,
        window_start_rfc3339: &str,
        window_end_rfc3339: &str,
        sync_token: Option<&str>,
    ) -> Result<(Vec<CalendarEvent>, Option<String>), CalendarError>;
}
