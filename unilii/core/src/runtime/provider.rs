use std::{
    collections::VecDeque,
    future::Future,
    pin::Pin,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::{Duration, SystemTime},
};

use async_trait::async_trait;
use tokio::sync::{Mutex, watch};

use crate::ModuleUpdate;

use super::metrics::{RuntimeMetrics, global_runtime_metrics};

pub type BoxWorker = Pin<Box<dyn Future<Output = ()> + Send + 'static>>;

static NEXT_PROVIDER_INSTANCE_GENERATION: AtomicU64 = AtomicU64::new(1);

fn next_provider_instance_generation() -> u64 {
    NEXT_PROVIDER_INSTANCE_GENERATION.fetch_add(1, Ordering::Relaxed)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderRefreshPolicy {
    pub interval: Duration,
    pub timeout: Duration,
    pub stale_after: Duration,
    pub refresh_on_start: bool,
}

impl ProviderRefreshPolicy {
    pub fn periodic(interval: Duration) -> Self {
        Self {
            interval,
            timeout: interval
                .min(Duration::from_secs(10))
                .max(Duration::from_millis(100)),
            stale_after: interval.saturating_mul(3),
            refresh_on_start: true,
        }
    }
}

impl Default for ProviderRefreshPolicy {
    fn default() -> Self {
        Self::periodic(Duration::from_secs(5))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderShutdownBehavior {
    pub graceful_timeout: Duration,
}

impl Default for ProviderShutdownBehavior {
    fn default() -> Self {
        Self {
            graceful_timeout: Duration::from_secs(2),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderContract {
    pub id: String,
    pub display_name: String,
    pub refresh: ProviderRefreshPolicy,
    pub shutdown: ProviderShutdownBehavior,
    pub test_backend: String,
}

impl ProviderContract {
    pub fn new(
        id: impl Into<String>,
        display_name: impl Into<String>,
        refresh: ProviderRefreshPolicy,
        test_backend: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            display_name: display_name.into(),
            refresh,
            shutdown: ProviderShutdownBehavior::default(),
            test_backend: test_backend.into(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderHealth {
    Startup,
    Loading,
    Fresh,
    Stale,
    Error,
    Disabled,
    ShuttingDown,
    Stopped,
}

impl ProviderHealth {
    pub fn label(self) -> &'static str {
        match self {
            Self::Startup => "startup",
            Self::Loading => "loading",
            Self::Fresh => "fresh",
            Self::Stale => "stale",
            Self::Error => "error",
            Self::Disabled => "disabled",
            Self::ShuttingDown => "shutting_down",
            Self::Stopped => "stopped",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ProviderState<T> {
    Startup,
    Loading { previous: Option<T> },
    Fresh { value: T },
    Stale { value: T, error: String },
    Error { error: String },
    Disabled { reason: String },
    ShuttingDown,
    Stopped,
}

impl<T> ProviderState<T> {
    pub fn health(&self) -> ProviderHealth {
        match self {
            Self::Startup => ProviderHealth::Startup,
            Self::Loading { .. } => ProviderHealth::Loading,
            Self::Fresh { .. } => ProviderHealth::Fresh,
            Self::Stale { .. } => ProviderHealth::Stale,
            Self::Error { .. } => ProviderHealth::Error,
            Self::Disabled { .. } => ProviderHealth::Disabled,
            Self::ShuttingDown => ProviderHealth::ShuttingDown,
            Self::Stopped => ProviderHealth::Stopped,
        }
    }

    pub fn value(&self) -> Option<&T> {
        match self {
            Self::Loading {
                previous: Some(value),
            }
            | Self::Fresh { value }
            | Self::Stale { value, .. } => Some(value),
            _ => None,
        }
    }

    pub fn error(&self) -> Option<&str> {
        match self {
            Self::Stale { error, .. } | Self::Error { error } => Some(error),
            Self::Disabled { reason } => Some(reason),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ProviderSnapshot<T> {
    pub contract: ProviderContract,
    /// Identifies one concrete provider instance. A replacement created during
    /// reload receives a larger value, so queued snapshots from the old
    /// instance can be rejected even when their per-refresh generation is high.
    pub instance_generation: u64,
    /// Monotonic refresh generation within one provider instance.
    pub generation: u64,
    pub state: ProviderState<T>,
    pub refresh_started_at: Option<SystemTime>,
    pub last_updated_at: Option<SystemTime>,
}

impl<T> ProviderSnapshot<T> {
    pub fn startup(contract: ProviderContract, instance_generation: u64) -> Self {
        Self {
            contract,
            instance_generation,
            generation: 0,
            state: ProviderState::Startup,
            refresh_started_at: None,
            last_updated_at: None,
        }
    }

    pub fn health(&self) -> ProviderHealth {
        self.state.health()
    }

    pub fn value(&self) -> Option<&T> {
        self.state.value()
    }

    pub fn error(&self) -> Option<&str> {
        self.state.error()
    }

    pub fn last_update_age(&self, now: SystemTime) -> Option<Duration> {
        self.last_updated_at
            .and_then(|last_updated| now.duration_since(last_updated).ok())
    }

    pub fn is_stale_by_policy(&self, now: SystemTime) -> bool {
        self.last_update_age(now)
            .is_some_and(|age| age > self.contract.refresh.stale_after)
    }

    pub fn belongs_to_instance(&self, instance_generation: u64) -> bool {
        self.instance_generation == instance_generation
    }
}

#[derive(Clone)]
pub struct ProviderPublisher<T> {
    sender: watch::Sender<ProviderSnapshot<T>>,
    current_generation: Arc<AtomicU64>,
    pending: Arc<AtomicBool>,
    metrics: Arc<RuntimeMetrics>,
}

impl<T: Clone> ProviderPublisher<T> {
    pub fn contract(&self) -> ProviderContract {
        self.sender.borrow().contract.clone()
    }

    pub fn instance_generation(&self) -> u64 {
        self.sender.borrow().instance_generation
    }

    pub fn begin_refresh(&self) -> u64 {
        let generation = self.current_generation.fetch_add(1, Ordering::AcqRel) + 1;
        let current = self.sender.borrow().clone();
        let previous = current.state.value().cloned();
        self.publish_snapshot(ProviderSnapshot {
            contract: current.contract,
            instance_generation: current.instance_generation,
            generation,
            state: ProviderState::Loading { previous },
            refresh_started_at: Some(SystemTime::now()),
            last_updated_at: current.last_updated_at,
        });
        generation
    }

    pub fn publish_result(&self, generation: u64, result: Result<T, String>) -> bool {
        if generation != self.current_generation.load(Ordering::Acquire) {
            self.metrics.record_update_coalesced();
            return false;
        }
        let current = self.sender.borrow().clone();
        let previous = current.state.value().cloned();
        let now = SystemTime::now();
        let state = match result {
            Ok(value) => ProviderState::Fresh { value },
            Err(error) => match previous {
                Some(value) => ProviderState::Stale { value, error },
                None => ProviderState::Error { error },
            },
        };
        let last_updated_at = matches!(&state, ProviderState::Fresh { .. })
            .then_some(now)
            .or(current.last_updated_at);
        self.publish_snapshot(ProviderSnapshot {
            contract: current.contract,
            instance_generation: current.instance_generation,
            generation,
            state,
            refresh_started_at: None,
            last_updated_at,
        });
        true
    }

    pub fn send(&self, value: T) -> bool {
        if self.sender.is_closed() {
            self.metrics.record_update_dropped();
            return false;
        }
        let generation = self.begin_refresh();
        self.publish_result(generation, Ok(value))
    }

    pub fn mark_stale(&self, reason: impl Into<String>) -> bool {
        let current = self.sender.borrow().clone();
        let Some(value) = current.state.value().cloned() else {
            return false;
        };
        self.publish_snapshot(ProviderSnapshot {
            contract: current.contract,
            instance_generation: current.instance_generation,
            generation: current.generation,
            state: ProviderState::Stale {
                value,
                error: reason.into(),
            },
            refresh_started_at: None,
            last_updated_at: current.last_updated_at,
        });
        true
    }

    pub fn disable(&self, reason: impl Into<String>) {
        let current = self.sender.borrow().clone();
        self.publish_snapshot(ProviderSnapshot {
            contract: current.contract,
            instance_generation: current.instance_generation,
            generation: current.generation,
            state: ProviderState::Disabled {
                reason: reason.into(),
            },
            refresh_started_at: None,
            last_updated_at: current.last_updated_at,
        });
    }

    pub fn shutdown(&self) {
        let current = self.sender.borrow().clone();
        self.publish_snapshot(ProviderSnapshot {
            contract: current.contract,
            instance_generation: current.instance_generation,
            generation: current.generation,
            state: ProviderState::ShuttingDown,
            refresh_started_at: None,
            last_updated_at: current.last_updated_at,
        });
    }

    pub fn stopped(&self) {
        let current = self.sender.borrow().clone();
        self.publish_snapshot(ProviderSnapshot {
            contract: current.contract,
            instance_generation: current.instance_generation,
            generation: current.generation,
            state: ProviderState::Stopped,
            refresh_started_at: None,
            last_updated_at: current.last_updated_at,
        });
    }

    pub fn is_closed(&self) -> bool {
        self.sender.is_closed()
    }

    fn publish_snapshot(&self, snapshot: ProviderSnapshot<T>) {
        if self.sender.is_closed() {
            self.metrics.record_update_dropped();
            return;
        }
        if self.pending.swap(true, Ordering::AcqRel) {
            self.metrics.record_update_coalesced();
        }
        self.sender.send_replace(snapshot);
    }
}

#[derive(Clone)]
pub struct ProviderReceiver<T> {
    receiver: watch::Receiver<ProviderSnapshot<T>>,
    pending: Arc<AtomicBool>,
}

impl<T: Clone> ProviderReceiver<T> {
    pub fn current(&self) -> ProviderSnapshot<T> {
        self.receiver.borrow().clone()
    }

    pub async fn changed(&mut self) -> Option<ProviderSnapshot<T>> {
        self.receiver.changed().await.ok()?;
        let snapshot = self.receiver.borrow_and_update().clone();
        self.pending.store(false, Ordering::Release);
        Some(snapshot)
    }

    pub fn instance_generation(&self) -> u64 {
        self.receiver.borrow().instance_generation
    }
}

pub fn provider_channel<T: Clone>(
    contract: ProviderContract,
    metrics: Arc<RuntimeMetrics>,
) -> (ProviderPublisher<T>, ProviderReceiver<T>) {
    let instance_generation = next_provider_instance_generation();
    let (sender, receiver) =
        watch::channel(ProviderSnapshot::startup(contract, instance_generation));
    let pending = Arc::new(AtomicBool::new(false));
    (
        ProviderPublisher {
            sender,
            current_generation: Arc::new(AtomicU64::new(0)),
            pending: Arc::clone(&pending),
            metrics,
        },
        ProviderReceiver { receiver, pending },
    )
}

#[async_trait]
pub trait ProviderBackend: Send + Sync + 'static {
    type Value: Clone + Send + Sync + 'static;

    async fn refresh(&self) -> Result<Self::Value, String>;

    async fn shutdown(&self) -> Result<(), String> {
        Ok(())
    }
}

#[derive(Debug)]
pub struct TestProviderBackend<T> {
    results: Mutex<VecDeque<Result<T, String>>>,
    shutdown_called: AtomicBool,
}

impl<T> TestProviderBackend<T> {
    pub fn new(results: impl IntoIterator<Item = Result<T, String>>) -> Self {
        Self {
            results: Mutex::new(results.into_iter().collect()),
            shutdown_called: AtomicBool::new(false),
        }
    }

    pub fn shutdown_called(&self) -> bool {
        self.shutdown_called.load(Ordering::Acquire)
    }
}

#[async_trait]
impl<T: Clone + Send + Sync + 'static> ProviderBackend for TestProviderBackend<T> {
    type Value = T;

    async fn refresh(&self) -> Result<Self::Value, String> {
        self.results
            .lock()
            .await
            .pop_front()
            .unwrap_or_else(|| Err("test backend has no queued result".to_string()))
    }

    async fn shutdown(&self) -> Result<(), String> {
        self.shutdown_called.store(true, Ordering::Release);
        Ok(())
    }
}

pub type ModuleUpdateSender = ProviderPublisher<ModuleUpdate>;
pub type ModuleProviderReceiver = ProviderReceiver<ModuleUpdate>;

pub struct ModuleSubscription {
    receiver: ModuleProviderReceiver,
    worker: Option<BoxWorker>,
}

impl ModuleSubscription {
    pub fn new<F, Fut>(worker: F) -> Self
    where
        F: FnOnce(ModuleUpdateSender) -> Fut,
        Fut: Future<Output = ()> + Send + 'static,
    {
        Self::with_contract(
            ProviderContract::new(
                "module",
                "Module",
                ProviderRefreshPolicy::default(),
                "TestProviderBackend<ModuleUpdate>",
            ),
            worker,
        )
    }

    pub fn with_contract<F, Fut>(contract: ProviderContract, worker: F) -> Self
    where
        F: FnOnce(ModuleUpdateSender) -> Fut,
        Fut: Future<Output = ()> + Send + 'static,
    {
        Self::with_metrics(contract, worker, global_runtime_metrics())
    }

    pub fn with_metrics<F, Fut>(
        contract: ProviderContract,
        worker: F,
        metrics: Arc<RuntimeMetrics>,
    ) -> Self
    where
        F: FnOnce(ModuleUpdateSender) -> Fut,
        Fut: Future<Output = ()> + Send + 'static,
    {
        let (publisher, receiver) = provider_channel(contract, metrics);
        Self {
            receiver,
            worker: Some(Box::pin(worker(publisher))),
        }
    }

    pub fn take_worker(&mut self) -> Option<BoxWorker> {
        self.worker.take()
    }

    pub fn receiver(&self) -> ModuleProviderReceiver {
        self.receiver.clone()
    }

    pub async fn recv(&mut self) -> Option<ProviderSnapshot<ModuleUpdate>> {
        self.receiver.changed().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn contract() -> ProviderContract {
        ProviderContract::new(
            "test",
            "Test",
            ProviderRefreshPolicy::periodic(Duration::from_secs(1)),
            "TestProviderBackend<String>",
        )
    }

    #[tokio::test]
    async fn latest_value_channel_coalesces_unread_updates() {
        let metrics = Arc::new(RuntimeMetrics::default());
        let mut subscription = ModuleSubscription::with_metrics(
            contract(),
            |sender| async move {
                sender.send(ModuleUpdate::Text("one".to_string()));
                sender.send(ModuleUpdate::Text("two".to_string()));
            },
            Arc::clone(&metrics),
        );
        subscription.take_worker().unwrap().await;
        assert!(matches!(
            subscription.recv().await.and_then(|snapshot| snapshot.value().cloned()),
            Some(ModuleUpdate::Text(value)) if value == "two"
        ));
        assert!(metrics.snapshot().updates_coalesced >= 1);
    }

    #[tokio::test]
    async fn stale_generation_cannot_replace_newer_value() {
        let metrics = Arc::new(RuntimeMetrics::default());
        let (publisher, receiver) = provider_channel::<String>(contract(), metrics);
        let first = publisher.begin_refresh();
        let second = publisher.begin_refresh();
        assert!(publisher.publish_result(second, Ok("new".to_string())));
        assert!(!publisher.publish_result(first, Ok("old".to_string())));
        assert_eq!(receiver.current().value().map(String::as_str), Some("new"));
    }

    #[tokio::test]
    async fn replacement_provider_has_distinct_instance_generation() {
        let metrics = Arc::new(RuntimeMetrics::default());
        let (old_publisher, old_receiver) =
            provider_channel::<String>(contract(), Arc::clone(&metrics));
        let (new_publisher, new_receiver) = provider_channel::<String>(contract(), metrics);

        assert_ne!(
            old_publisher.instance_generation(),
            new_publisher.instance_generation()
        );
        assert!(
            old_receiver
                .current()
                .belongs_to_instance(old_publisher.instance_generation())
        );
        assert!(
            new_receiver
                .current()
                .belongs_to_instance(new_publisher.instance_generation())
        );
    }

    #[tokio::test]
    async fn failed_refresh_retains_last_good_value_as_stale() {
        let metrics = Arc::new(RuntimeMetrics::default());
        let (publisher, receiver) = provider_channel::<String>(contract(), metrics);
        let first = publisher.begin_refresh();
        publisher.publish_result(first, Ok("good".to_string()));
        let second = publisher.begin_refresh();
        publisher.publish_result(second, Err("offline".to_string()));
        let snapshot = receiver.current();
        assert_eq!(snapshot.health(), ProviderHealth::Stale);
        assert_eq!(snapshot.value().map(String::as_str), Some("good"));
        assert_eq!(snapshot.error(), Some("offline"));
        assert!(snapshot.last_update_age(SystemTime::now()).is_some());
    }

    #[tokio::test]
    async fn test_backend_requires_no_live_service() {
        let backend = TestProviderBackend::new([
            Ok("fixture".to_string()),
            Err("fixture failure".to_string()),
        ]);
        assert_eq!(backend.refresh().await.unwrap(), "fixture");
        assert_eq!(backend.refresh().await.unwrap_err(), "fixture failure");
        backend.shutdown().await.unwrap();
        assert!(backend.shutdown_called());
    }
}
