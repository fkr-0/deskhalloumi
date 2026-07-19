use std::sync::{
    Arc, OnceLock,
    atomic::{AtomicU64, AtomicUsize, Ordering},
};
use std::time::Duration;

use serde::{Deserialize, Serialize};

#[derive(Debug, Default)]
pub struct RuntimeMetrics {
    active_tasks: AtomicUsize,
    tasks_started: AtomicU64,
    tasks_completed: AtomicU64,
    tasks_cancelled: AtomicU64,
    tasks_panicked: AtomicU64,
    actions_started: AtomicU64,
    actions_completed: AtomicU64,
    actions_failed: AtomicU64,
    action_timeouts: AtomicU64,
    action_duration_ms_total: AtomicU64,
    action_duration_ms_max: AtomicU64,
    truncated_outputs: AtomicU64,
    truncated_bytes: AtomicU64,
    provider_refreshes_started: AtomicU64,
    provider_refreshes_completed: AtomicU64,
    provider_refreshes_coalesced: AtomicU64,
    provider_refreshes_saturated: AtomicU64,
    updates_coalesced: AtomicU64,
    updates_dropped: AtomicU64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeMetricsSnapshot {
    pub active_tasks: usize,
    pub tasks_started: u64,
    pub tasks_completed: u64,
    pub tasks_cancelled: u64,
    pub tasks_panicked: u64,
    pub actions_started: u64,
    pub actions_completed: u64,
    pub actions_failed: u64,
    pub action_timeouts: u64,
    pub action_duration_ms_total: u64,
    pub action_duration_ms_max: u64,
    pub truncated_outputs: u64,
    pub truncated_bytes: u64,
    pub provider_refreshes_started: u64,
    pub provider_refreshes_completed: u64,
    pub provider_refreshes_coalesced: u64,
    pub provider_refreshes_saturated: u64,
    pub updates_coalesced: u64,
    pub updates_dropped: u64,
}

static GLOBAL_RUNTIME_METRICS: OnceLock<Arc<RuntimeMetrics>> = OnceLock::new();

pub fn global_runtime_metrics() -> Arc<RuntimeMetrics> {
    GLOBAL_RUNTIME_METRICS
        .get_or_init(|| Arc::new(RuntimeMetrics::default()))
        .clone()
}

impl RuntimeMetrics {
    pub fn task_guard(self: &Arc<Self>) -> ActiveTaskGuard {
        self.tasks_started.fetch_add(1, Ordering::Relaxed);
        self.active_tasks.fetch_add(1, Ordering::Relaxed);
        ActiveTaskGuard {
            metrics: Arc::clone(self),
        }
    }

    pub fn record_task_completed(&self) {
        self.tasks_completed.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_task_cancelled(&self) {
        self.tasks_cancelled.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_task_panicked(&self) {
        self.tasks_panicked.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_action_started(&self) {
        self.actions_started.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_action_finished(
        &self,
        duration: Duration,
        success: bool,
        timed_out: bool,
        truncated_streams: u64,
        truncated_bytes: u64,
    ) {
        self.actions_completed.fetch_add(1, Ordering::Relaxed);
        if !success {
            self.actions_failed.fetch_add(1, Ordering::Relaxed);
        }
        if timed_out {
            self.action_timeouts.fetch_add(1, Ordering::Relaxed);
        }
        let duration_ms = duration.as_millis().min(u64::MAX as u128) as u64;
        self.action_duration_ms_total
            .fetch_add(duration_ms, Ordering::Relaxed);
        self.action_duration_ms_max
            .fetch_max(duration_ms, Ordering::Relaxed);
        self.truncated_outputs
            .fetch_add(truncated_streams, Ordering::Relaxed);
        self.truncated_bytes
            .fetch_add(truncated_bytes, Ordering::Relaxed);
    }

    pub fn record_provider_refresh_started(&self) {
        self.provider_refreshes_started
            .fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_provider_refresh_completed(&self) {
        self.provider_refreshes_completed
            .fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_provider_refresh_coalesced(&self) {
        self.provider_refreshes_coalesced
            .fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_provider_refresh_saturated(&self) {
        self.provider_refreshes_saturated
            .fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_update_coalesced(&self) {
        self.updates_coalesced.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_update_dropped(&self) {
        self.updates_dropped.fetch_add(1, Ordering::Relaxed);
    }

    pub fn snapshot(&self) -> RuntimeMetricsSnapshot {
        RuntimeMetricsSnapshot {
            active_tasks: self.active_tasks.load(Ordering::Relaxed),
            tasks_started: self.tasks_started.load(Ordering::Relaxed),
            tasks_completed: self.tasks_completed.load(Ordering::Relaxed),
            tasks_cancelled: self.tasks_cancelled.load(Ordering::Relaxed),
            tasks_panicked: self.tasks_panicked.load(Ordering::Relaxed),
            actions_started: self.actions_started.load(Ordering::Relaxed),
            actions_completed: self.actions_completed.load(Ordering::Relaxed),
            actions_failed: self.actions_failed.load(Ordering::Relaxed),
            action_timeouts: self.action_timeouts.load(Ordering::Relaxed),
            action_duration_ms_total: self.action_duration_ms_total.load(Ordering::Relaxed),
            action_duration_ms_max: self.action_duration_ms_max.load(Ordering::Relaxed),
            truncated_outputs: self.truncated_outputs.load(Ordering::Relaxed),
            truncated_bytes: self.truncated_bytes.load(Ordering::Relaxed),
            provider_refreshes_started: self.provider_refreshes_started.load(Ordering::Relaxed),
            provider_refreshes_completed: self.provider_refreshes_completed.load(Ordering::Relaxed),
            provider_refreshes_coalesced: self.provider_refreshes_coalesced.load(Ordering::Relaxed),
            provider_refreshes_saturated: self.provider_refreshes_saturated.load(Ordering::Relaxed),
            updates_coalesced: self.updates_coalesced.load(Ordering::Relaxed),
            updates_dropped: self.updates_dropped.load(Ordering::Relaxed),
        }
    }
}

pub struct ActiveTaskGuard {
    metrics: Arc<RuntimeMetrics>,
}

impl Drop for ActiveTaskGuard {
    fn drop(&mut self) {
        self.metrics.active_tasks.fetch_sub(1, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_records_actions_tasks_and_provider_pressure() {
        let metrics = Arc::new(RuntimeMetrics::default());
        {
            let _guard = metrics.task_guard();
            assert_eq!(metrics.snapshot().active_tasks, 1);
        }
        metrics.record_task_completed();
        metrics.record_action_started();
        metrics.record_action_finished(Duration::from_millis(17), false, true, 2, 500);
        metrics.record_provider_refresh_started();
        metrics.record_provider_refresh_completed();
        metrics.record_provider_refresh_coalesced();
        metrics.record_provider_refresh_saturated();
        metrics.record_update_coalesced();
        metrics.record_update_dropped();

        let snapshot = metrics.snapshot();
        assert_eq!(snapshot.active_tasks, 0);
        assert_eq!(snapshot.tasks_started, 1);
        assert_eq!(snapshot.tasks_completed, 1);
        assert_eq!(snapshot.actions_started, 1);
        assert_eq!(snapshot.actions_completed, 1);
        assert_eq!(snapshot.actions_failed, 1);
        assert_eq!(snapshot.action_timeouts, 1);
        assert_eq!(snapshot.action_duration_ms_total, 17);
        assert_eq!(snapshot.action_duration_ms_max, 17);
        assert_eq!(snapshot.truncated_outputs, 2);
        assert_eq!(snapshot.truncated_bytes, 500);
        assert_eq!(snapshot.provider_refreshes_started, 1);
        assert_eq!(snapshot.provider_refreshes_completed, 1);
        assert_eq!(snapshot.provider_refreshes_coalesced, 1);
        assert_eq!(snapshot.provider_refreshes_saturated, 1);
        assert_eq!(snapshot.updates_coalesced, 1);
        assert_eq!(snapshot.updates_dropped, 1);
    }
}
