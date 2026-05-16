use super::common::{FilterableMenu, QuickjumpMenu};

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
