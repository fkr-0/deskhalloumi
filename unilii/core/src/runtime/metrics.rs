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
    active_actions: AtomicUsize,
    actions_completed: AtomicU64,
    actions_failed: AtomicU64,
    actions_cancelled: AtomicU64,
    actions_rejected: AtomicU64,
    queued_actions: AtomicUsize,
    action_timeouts: AtomicU64,
    action_duration_ms_total: AtomicU64,
    action_duration_ms_max: AtomicU64,
    truncated_outputs: AtomicU64,
    truncated_bytes: AtomicU64,
    provider_refreshes_started: AtomicU64,
    provider_refreshes_completed: AtomicU64,
    provider_refreshes_coalesced: AtomicU64,
    provider_refreshes_saturated: AtomicU64,
    provider_refreshes_failed: AtomicU64,
    provider_refreshes_timed_out: AtomicU64,
    provider_shutdown_failures: AtomicU64,
    provider_shutdown_timeouts: AtomicU64,
    updates_coalesced: AtomicU64,
    updates_dropped: AtomicU64,
}

pub struct ActiveActionGuard {
    metrics: Arc<RuntimeMetrics>,
    finished: bool,
}

impl ActiveActionGuard {
    pub fn finish(mut self) {
        self.finished = true;
    }
}

impl Drop for ActiveActionGuard {
    fn drop(&mut self) {
        self.metrics.active_actions.fetch_sub(1, Ordering::Relaxed);
        if !self.finished {
            self.metrics
                .actions_cancelled
                .fetch_add(1, Ordering::Relaxed);
        }
    }
}

pub struct QueuedActionGuard {
    metrics: Arc<RuntimeMetrics>,
}

impl Drop for QueuedActionGuard {
    fn drop(&mut self) {
        self.metrics.queued_actions.fetch_sub(1, Ordering::Relaxed);
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeMetricsSnapshot {
    pub active_tasks: usize,
    pub tasks_started: u64,
    pub tasks_completed: u64,
    pub tasks_cancelled: u64,
    pub tasks_panicked: u64,
    pub actions_started: u64,
    #[serde(default)]
    pub active_actions: usize,
    pub actions_completed: u64,
    pub actions_failed: u64,
    #[serde(default)]
    pub actions_cancelled: u64,
    pub actions_rejected: u64,
    #[serde(default)]
    pub queued_actions: usize,
    pub action_timeouts: u64,
    pub action_duration_ms_total: u64,
    pub action_duration_ms_max: u64,
    pub truncated_outputs: u64,
    pub truncated_bytes: u64,
    pub provider_refreshes_started: u64,
    pub provider_refreshes_completed: u64,
    pub provider_refreshes_coalesced: u64,
    pub provider_refreshes_saturated: u64,
    #[serde(default)]
    pub provider_refreshes_failed: u64,
    #[serde(default)]
    pub provider_refreshes_timed_out: u64,
    #[serde(default)]
    pub provider_shutdown_failures: u64,
    #[serde(default)]
    pub provider_shutdown_timeouts: u64,
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

    pub fn record_action_rejected(&self) {
        self.actions_rejected.fetch_add(1, Ordering::Relaxed);
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

    pub fn action_guard(self: &Arc<Self>) -> ActiveActionGuard {
        self.actions_started.fetch_add(1, Ordering::Relaxed);
        self.active_actions.fetch_add(1, Ordering::Relaxed);
        ActiveActionGuard {
            metrics: Arc::clone(self),
            finished: false,
        }
    }

    pub fn queued_action_guard(self: &Arc<Self>) -> QueuedActionGuard {
        self.queued_actions.fetch_add(1, Ordering::Relaxed);
        QueuedActionGuard {
            metrics: Arc::clone(self),
        }
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

    pub fn record_provider_refresh_failed(&self) {
        self.provider_refreshes_failed
            .fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_provider_refresh_timed_out(&self) {
        self.provider_refreshes_timed_out
            .fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_provider_shutdown_failed(&self) {
        self.provider_shutdown_failures
            .fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_provider_shutdown_timed_out(&self) {
        self.provider_shutdown_timeouts
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
            active_actions: self.active_actions.load(Ordering::Relaxed),
            actions_completed: self.actions_completed.load(Ordering::Relaxed),
            actions_failed: self.actions_failed.load(Ordering::Relaxed),
            actions_cancelled: self.actions_cancelled.load(Ordering::Relaxed),
            actions_rejected: self.actions_rejected.load(Ordering::Relaxed),
            queued_actions: self.queued_actions.load(Ordering::Relaxed),
            action_timeouts: self.action_timeouts.load(Ordering::Relaxed),
            action_duration_ms_total: self.action_duration_ms_total.load(Ordering::Relaxed),
            action_duration_ms_max: self.action_duration_ms_max.load(Ordering::Relaxed),
            truncated_outputs: self.truncated_outputs.load(Ordering::Relaxed),
            truncated_bytes: self.truncated_bytes.load(Ordering::Relaxed),
            provider_refreshes_started: self.provider_refreshes_started.load(Ordering::Relaxed),
            provider_refreshes_completed: self.provider_refreshes_completed.load(Ordering::Relaxed),
            provider_refreshes_coalesced: self.provider_refreshes_coalesced.load(Ordering::Relaxed),
            provider_refreshes_saturated: self.provider_refreshes_saturated.load(Ordering::Relaxed),
            provider_refreshes_failed: self.provider_refreshes_failed.load(Ordering::Relaxed),
            provider_refreshes_timed_out: self.provider_refreshes_timed_out.load(Ordering::Relaxed),
            provider_shutdown_failures: self.provider_shutdown_failures.load(Ordering::Relaxed),
            provider_shutdown_timeouts: self.provider_shutdown_timeouts.load(Ordering::Relaxed),
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
        let action = metrics.action_guard();
        metrics.record_action_finished(Duration::from_millis(17), false, true, 2, 500);
        action.finish();
        metrics.record_action_rejected();
        let queued = metrics.queued_action_guard();
        metrics.record_provider_refresh_started();
        metrics.record_provider_refresh_completed();
        metrics.record_provider_refresh_coalesced();
        metrics.record_provider_refresh_saturated();
        metrics.record_provider_refresh_failed();
        metrics.record_provider_refresh_timed_out();
        metrics.record_provider_shutdown_failed();
        metrics.record_provider_shutdown_timed_out();
        metrics.record_update_coalesced();
        metrics.record_update_dropped();

        let snapshot = metrics.snapshot();
        assert_eq!(snapshot.active_tasks, 0);
        assert_eq!(snapshot.tasks_started, 1);
        assert_eq!(snapshot.tasks_completed, 1);
        assert_eq!(snapshot.actions_started, 1);
        assert_eq!(snapshot.active_actions, 0);
        assert_eq!(snapshot.actions_completed, 1);
        assert_eq!(snapshot.actions_failed, 1);
        assert_eq!(snapshot.actions_cancelled, 0);
        assert_eq!(snapshot.actions_rejected, 1);
        assert_eq!(snapshot.queued_actions, 1);
        assert_eq!(snapshot.action_timeouts, 1);
        assert_eq!(snapshot.action_duration_ms_total, 17);
        assert_eq!(snapshot.action_duration_ms_max, 17);
        assert_eq!(snapshot.truncated_outputs, 2);
        assert_eq!(snapshot.truncated_bytes, 500);
        assert_eq!(snapshot.provider_refreshes_started, 1);
        assert_eq!(snapshot.provider_refreshes_completed, 1);
        assert_eq!(snapshot.provider_refreshes_coalesced, 1);
        assert_eq!(snapshot.provider_refreshes_saturated, 1);
        assert_eq!(snapshot.provider_refreshes_failed, 1);
        assert_eq!(snapshot.provider_refreshes_timed_out, 1);
        assert_eq!(snapshot.provider_shutdown_failures, 1);
        assert_eq!(snapshot.provider_shutdown_timeouts, 1);
        assert_eq!(snapshot.updates_coalesced, 1);
        assert_eq!(snapshot.updates_dropped, 1);
        drop(queued);
        assert_eq!(metrics.snapshot().queued_actions, 0);
    }

    #[test]
    fn cancelled_action_guard_releases_active_gauge_and_counts_cancellation() {
        let metrics = Arc::new(RuntimeMetrics::default());
        let action = metrics.action_guard();
        assert_eq!(metrics.snapshot().active_actions, 1);
        drop(action);
        let snapshot = metrics.snapshot();
        assert_eq!(snapshot.active_actions, 0);
        assert_eq!(snapshot.actions_cancelled, 1);
    }
}
