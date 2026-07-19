//! Bounded renderer-neutral action history.

use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::time::{Duration, SystemTime};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionStatus {
    Running,
    Succeeded,
    Failed,
    TimedOut,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActionRecord {
    pub sequence: u64,
    pub action_id: String,
    pub title: String,
    pub source: String,
    pub status: ActionStatus,
    pub started_unix_ms: u128,
    pub duration_ms: Option<u128>,
    pub detail: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ActionHistory {
    capacity: usize,
    next_sequence: u64,
    records: VecDeque<ActionRecord>,
}

impl Default for ActionHistory {
    fn default() -> Self {
        Self::new(32)
    }
}

impl ActionHistory {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity: capacity.max(1),
            next_sequence: 1,
            records: VecDeque::new(),
        }
    }

    pub fn start(
        &mut self,
        action_id: impl Into<String>,
        title: impl Into<String>,
        source: impl Into<String>,
    ) -> u64 {
        let sequence = self.next_sequence;
        self.next_sequence = self.next_sequence.saturating_add(1);
        self.push(ActionRecord {
            sequence,
            action_id: action_id.into(),
            title: title.into(),
            source: source.into(),
            status: ActionStatus::Running,
            started_unix_ms: unix_ms(SystemTime::now()),
            duration_ms: None,
            detail: None,
        });
        sequence
    }

    pub fn finish(
        &mut self,
        sequence: u64,
        status: ActionStatus,
        duration: Duration,
        detail: Option<String>,
    ) -> bool {
        let Some(record) = self
            .records
            .iter_mut()
            .find(|record| record.sequence == sequence)
        else {
            return false;
        };
        record.status = status;
        record.duration_ms = Some(duration.as_millis());
        record.detail = detail;
        true
    }

    pub fn record(
        &mut self,
        action_id: impl Into<String>,
        title: impl Into<String>,
        source: impl Into<String>,
        status: ActionStatus,
        duration: Duration,
        detail: Option<String>,
    ) -> u64 {
        let sequence = self.start(action_id, title, source);
        self.finish(sequence, status, duration, detail);
        sequence
    }

    pub fn records(&self) -> impl DoubleEndedIterator<Item = &ActionRecord> {
        self.records.iter()
    }

    pub fn recent(&self, limit: usize) -> Vec<ActionRecord> {
        self.records.iter().rev().take(limit).cloned().collect()
    }

    fn push(&mut self, record: ActionRecord) {
        while self.records.len() >= self.capacity {
            self.records.pop_front();
        }
        self.records.push_back(record);
    }
}

fn unix_ms(time: SystemTime) -> u128 {
    time.duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_bounded_recent_history_with_failure_details() {
        let mut history = ActionHistory::new(2);
        history.record(
            "one",
            "One",
            "test",
            ActionStatus::Succeeded,
            Duration::from_millis(1),
            None,
        );
        history.record(
            "two",
            "Two",
            "test",
            ActionStatus::Failed,
            Duration::from_millis(2),
            Some("boom".to_string()),
        );
        history.record(
            "three",
            "Three",
            "test",
            ActionStatus::TimedOut,
            Duration::from_millis(3),
            Some("timeout".to_string()),
        );
        let recent = history.recent(10);
        assert_eq!(recent.len(), 2);
        assert_eq!(recent[0].action_id, "three");
        assert_eq!(recent[1].action_id, "two");
        assert_eq!(recent[1].detail.as_deref(), Some("boom"));
    }
}
