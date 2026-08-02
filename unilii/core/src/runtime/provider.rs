use std::{
    collections::{HashMap, VecDeque},
    future::Future,
    pin::Pin,
    sync::{
        Arc, OnceLock, RwLock,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tokio::{
    sync::{Mutex, watch},
    time::{self, MissedTickBehavior},
};
use tokio_util::sync::CancellationToken;

use crate::ModuleUpdate;

use super::{
    metrics::{RuntimeMetrics, global_runtime_metrics},
    refresh::{ProviderRefreshRegistry, RefreshRejected},
};

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
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
    /// Most recent bounded provider error, retained across shutdown/stopped
    /// transitions so diagnostics do not lose the reason a provider degraded.
    pub last_error: Option<String>,
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
            last_error: None,
        }
    }

    pub fn health(&self) -> ProviderHealth {
        self.state.health()
    }

    pub fn value(&self) -> Option<&T> {
        self.state.value()
    }

    pub fn error(&self) -> Option<&str> {
        self.state.error().or(self.last_error.as_deref())
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

const PROVIDER_ERROR_MAX_BYTES: usize = 4096;

fn bounded_provider_error(error: impl Into<String>) -> String {
    let error = error.into();
    if error.len() <= PROVIDER_ERROR_MAX_BYTES {
        return error;
    }
    let mut end = PROVIDER_ERROR_MAX_BYTES.saturating_sub("…".len());
    while !error.is_char_boundary(end) {
        end = end.saturating_sub(1);
    }
    format!("{}…", &error[..end])
}

fn system_time_millis(value: SystemTime) -> u64 {
    value
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u64::MAX as u128) as u64
}

fn duration_millis(value: Duration) -> u64 {
    value.as_millis().min(u64::MAX as u128) as u64
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderStatusSnapshot {
    pub id: String,
    pub display_name: String,
    pub instance_generation: u64,
    pub generation: u64,
    pub health: ProviderHealth,
    pub refresh_started_unix_ms: Option<u64>,
    pub last_updated_unix_ms: Option<u64>,
    pub last_update_age_ms: Option<u64>,
    pub error: Option<String>,
    pub interval_ms: u64,
    pub timeout_ms: u64,
    pub stale_after_ms: u64,
}

impl ProviderStatusSnapshot {
    fn from_snapshot<T>(snapshot: &ProviderSnapshot<T>, now: SystemTime) -> Self {
        Self {
            id: snapshot.contract.id.clone(),
            display_name: snapshot.contract.display_name.clone(),
            instance_generation: snapshot.instance_generation,
            generation: snapshot.generation,
            health: snapshot.health(),
            refresh_started_unix_ms: snapshot.refresh_started_at.map(system_time_millis),
            last_updated_unix_ms: snapshot.last_updated_at.map(system_time_millis),
            last_update_age_ms: snapshot.last_update_age(now).map(duration_millis),
            error: snapshot.error().map(bounded_provider_error),
            interval_ms: duration_millis(snapshot.contract.refresh.interval),
            timeout_ms: duration_millis(snapshot.contract.refresh.timeout),
            stale_after_ms: duration_millis(snapshot.contract.refresh.stale_after),
        }
    }

    fn refresh_age(&mut self, now: SystemTime) {
        self.last_update_age_ms = self
            .last_updated_unix_ms
            .map(|updated| system_time_millis(now).saturating_sub(updated));
    }
}

#[derive(Clone, Default)]
pub struct ProviderStatusRegistry {
    entries: Arc<RwLock<HashMap<String, ProviderStatusSnapshot>>>,
}

impl ProviderStatusRegistry {
    pub fn record<T>(&self, snapshot: &ProviderSnapshot<T>) -> bool {
        let candidate = ProviderStatusSnapshot::from_snapshot(snapshot, SystemTime::now());
        let mut entries = self
            .entries
            .write()
            .unwrap_or_else(|error| error.into_inner());
        if let Some(active) = entries.get(&candidate.id)
            && (active.instance_generation > candidate.instance_generation
                || (active.instance_generation == candidate.instance_generation
                    && active.generation > candidate.generation))
        {
            return false;
        }
        entries.insert(candidate.id.clone(), candidate);
        true
    }

    pub fn snapshots(&self) -> Vec<ProviderStatusSnapshot> {
        let now = SystemTime::now();
        let mut snapshots = self
            .entries
            .read()
            .unwrap_or_else(|error| error.into_inner())
            .values()
            .cloned()
            .collect::<Vec<_>>();
        for snapshot in &mut snapshots {
            snapshot.refresh_age(now);
        }
        snapshots.sort_by(|left, right| left.id.cmp(&right.id));
        snapshots
    }

    pub fn get(&self, id: &str) -> Option<ProviderStatusSnapshot> {
        self.snapshots()
            .into_iter()
            .find(|snapshot| snapshot.id == id)
    }

    pub fn clear(&self) {
        self.entries
            .write()
            .unwrap_or_else(|error| error.into_inner())
            .clear();
    }
}

static GLOBAL_PROVIDER_STATUS: OnceLock<ProviderStatusRegistry> = OnceLock::new();

pub fn global_provider_status_registry() -> ProviderStatusRegistry {
    GLOBAL_PROVIDER_STATUS
        .get_or_init(ProviderStatusRegistry::default)
        .clone()
}

#[derive(Clone)]
pub struct ProviderPublisher<T> {
    sender: watch::Sender<ProviderSnapshot<T>>,
    current_generation: Arc<AtomicU64>,
    pending: Arc<AtomicBool>,
    metrics: Arc<RuntimeMetrics>,
    status: ProviderStatusRegistry,
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
            last_error: current.last_error,
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
        let (state, last_error) = match result {
            Ok(value) => (ProviderState::Fresh { value }, None),
            Err(error) => {
                let error = bounded_provider_error(error);
                let state = match previous {
                    Some(value) => ProviderState::Stale {
                        value,
                        error: error.clone(),
                    },
                    None => ProviderState::Error {
                        error: error.clone(),
                    },
                };
                (state, Some(error))
            }
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
            last_error,
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
        let reason = bounded_provider_error(reason.into());
        self.publish_snapshot(ProviderSnapshot {
            contract: current.contract,
            instance_generation: current.instance_generation,
            generation: current.generation,
            state: ProviderState::Stale {
                value,
                error: reason.clone(),
            },
            refresh_started_at: None,
            last_updated_at: current.last_updated_at,
            last_error: Some(reason),
        });
        true
    }

    pub fn disable(&self, reason: impl Into<String>) {
        let current = self.sender.borrow().clone();
        let reason = bounded_provider_error(reason.into());
        self.publish_snapshot(ProviderSnapshot {
            contract: current.contract,
            instance_generation: current.instance_generation,
            generation: current.generation,
            state: ProviderState::Disabled {
                reason: reason.clone(),
            },
            refresh_started_at: None,
            last_updated_at: current.last_updated_at,
            last_error: Some(reason),
        });
    }

    pub fn shutdown(&self) {
        self.shutdown_with_error(None);
    }

    pub fn shutdown_with_error(&self, error: Option<String>) {
        let current = self.sender.borrow().clone();
        self.publish_snapshot(ProviderSnapshot {
            contract: current.contract,
            instance_generation: current.instance_generation,
            generation: current.generation,
            state: ProviderState::ShuttingDown,
            refresh_started_at: None,
            last_updated_at: current.last_updated_at,
            last_error: error.map(bounded_provider_error).or(current.last_error),
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
            last_error: current.last_error,
        });
    }

    pub fn is_closed(&self) -> bool {
        self.sender.is_closed()
    }

    pub async fn closed(&self) {
        self.sender.closed().await;
    }

    fn publish_snapshot(&self, snapshot: ProviderSnapshot<T>) {
        self.status.record(&snapshot);
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
    provider_channel_with_status_registry(contract, metrics, global_provider_status_registry())
}

pub fn provider_channel_with_status_registry<T: Clone>(
    contract: ProviderContract,
    metrics: Arc<RuntimeMetrics>,
    status: ProviderStatusRegistry,
) -> (ProviderPublisher<T>, ProviderReceiver<T>) {
    let instance_generation = next_provider_instance_generation();
    let startup = ProviderSnapshot::startup(contract, instance_generation);
    status.record(&startup);
    let (sender, receiver) = watch::channel(startup);
    let pending = Arc::new(AtomicBool::new(false));
    (
        ProviderPublisher {
            sender,
            current_generation: Arc::new(AtomicU64::new(0)),
            pending: Arc::clone(&pending),
            metrics,
            status,
        },
        ProviderReceiver { receiver, pending },
    )
}

#[derive(Debug, Clone, PartialEq)]
pub enum ProviderRefreshOutcome<T> {
    Fresh(T),
    Disabled(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderRefreshAttempt {
    Published,
    Disabled,
    Coalesced,
    Saturated,
    Cancelled,
    Failed,
    TimedOut,
}

#[async_trait]
pub trait ProviderBackend: Send + Sync + 'static {
    type Value: Clone + Send + Sync + 'static;

    async fn refresh(&self) -> Result<Self::Value, String>;

    async fn refresh_outcome(&self) -> Result<ProviderRefreshOutcome<Self::Value>, String> {
        self.refresh().await.map(ProviderRefreshOutcome::Fresh)
    }

    async fn shutdown(&self) -> Result<(), String> {
        Ok(())
    }
}

pub async fn run_provider_operation<T, F, Fut>(
    publisher: &ProviderPublisher<T>,
    refreshes: &ProviderRefreshRegistry,
    cancellation: &CancellationToken,
    operation: F,
) -> ProviderRefreshAttempt
where
    T: Clone,
    F: FnOnce() -> Fut,
    Fut: Future<Output = Result<ProviderRefreshOutcome<T>, String>>,
{
    if cancellation.is_cancelled() {
        return ProviderRefreshAttempt::Cancelled;
    }
    let key = format!(
        "{}:{}",
        publisher.contract().id,
        publisher.instance_generation()
    );
    let _permit = match refreshes.try_start(key) {
        Ok(permit) => permit,
        Err(RefreshRejected::Coalesced) => return ProviderRefreshAttempt::Coalesced,
        Err(RefreshRejected::Saturated) => return ProviderRefreshAttempt::Saturated,
    };
    let generation = publisher.begin_refresh();
    let timeout = publisher.contract().refresh.timeout;
    let result = tokio::select! {
        _ = cancellation.cancelled() => return ProviderRefreshAttempt::Cancelled,
        result = time::timeout(timeout, operation()) => result,
    };
    match result {
        Ok(Ok(ProviderRefreshOutcome::Fresh(value))) => {
            publisher.publish_result(generation, Ok(value));
            ProviderRefreshAttempt::Published
        }
        Ok(Ok(ProviderRefreshOutcome::Disabled(reason))) => {
            publisher.disable(reason);
            ProviderRefreshAttempt::Disabled
        }
        Ok(Err(error)) => {
            publisher.metrics.record_provider_refresh_failed();
            publisher.publish_result(generation, Err(error));
            ProviderRefreshAttempt::Failed
        }
        Err(_) => {
            publisher.metrics.record_provider_refresh_timed_out();
            publisher.publish_result(
                generation,
                Err(format!("provider refresh timed out after {timeout:?}")),
            );
            ProviderRefreshAttempt::TimedOut
        }
    }
}

pub async fn refresh_provider_once<B>(
    publisher: &ProviderPublisher<B::Value>,
    backend: &B,
    refreshes: &ProviderRefreshRegistry,
    cancellation: &CancellationToken,
) -> ProviderRefreshAttempt
where
    B: ProviderBackend,
{
    run_provider_operation(publisher, refreshes, cancellation, || {
        backend.refresh_outcome()
    })
    .await
}

pub async fn shutdown_provider_backend<B>(publisher: &ProviderPublisher<B::Value>, backend: &B)
where
    B: ProviderBackend,
{
    let contract = publisher.contract();
    publisher.shutdown();
    let shutdown_timeout = contract.shutdown.graceful_timeout;
    match time::timeout(shutdown_timeout, backend.shutdown()).await {
        Ok(Ok(())) => {}
        Ok(Err(error)) => {
            publisher.metrics.record_provider_shutdown_failed();
            publisher.shutdown_with_error(Some(error));
        }
        Err(_) => {
            publisher.metrics.record_provider_shutdown_timed_out();
            publisher.shutdown_with_error(Some(format!(
                "provider shutdown timed out after {shutdown_timeout:?}"
            )));
        }
    }
    publisher.stopped();
}

pub async fn run_provider_backend<B>(
    publisher: ProviderPublisher<B::Value>,
    backend: Arc<B>,
    refreshes: ProviderRefreshRegistry,
    cancellation: CancellationToken,
) where
    B: ProviderBackend,
{
    let contract = publisher.contract();
    let mut interval = time::interval(contract.refresh.interval.max(Duration::from_millis(1)));
    interval.set_missed_tick_behavior(MissedTickBehavior::Skip);
    interval.tick().await;
    if contract.refresh.refresh_on_start {
        let _ =
            refresh_provider_once(&publisher, backend.as_ref(), &refreshes, &cancellation).await;
    }
    loop {
        tokio::select! {
            _ = cancellation.cancelled() => break,
            _ = publisher.closed() => break,
            _ = interval.tick() => {
                let _ = refresh_provider_once(
                    &publisher,
                    backend.as_ref(),
                    &refreshes,
                    &cancellation,
                ).await;
            }
        }
    }

    shutdown_provider_backend(&publisher, backend.as_ref()).await;
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
type BoxWorkerFactory =
    Box<dyn FnOnce(CancellationToken, ProviderRefreshRegistry) -> BoxWorker + Send + 'static>;

pub struct ModuleSubscription {
    receiver: ModuleProviderReceiver,
    worker_factory: Option<BoxWorkerFactory>,
}

impl ModuleSubscription {
    pub fn new<F, Fut>(worker: F) -> Self
    where
        F: FnOnce(ModuleUpdateSender) -> Fut + Send + 'static,
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
        F: FnOnce(ModuleUpdateSender) -> Fut + Send + 'static,
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
        F: FnOnce(ModuleUpdateSender) -> Fut + Send + 'static,
        Fut: Future<Output = ()> + Send + 'static,
    {
        let (publisher, receiver) = provider_channel(contract, metrics);
        let worker_factory = Box::new(move |cancellation: CancellationToken, _refreshes| {
            Box::pin(async move {
                let future = worker(publisher.clone());
                tokio::select! {
                    _ = cancellation.cancelled() => {
                        publisher.shutdown();
                        publisher.stopped();
                    }
                    _ = future => {}
                }
            }) as BoxWorker
        });
        Self {
            receiver,
            worker_factory: Some(worker_factory),
        }
    }

    pub fn with_backend<B>(contract: ProviderContract, backend: Arc<B>) -> Self
    where
        B: ProviderBackend<Value = ModuleUpdate>,
    {
        Self::with_backend_and_metrics(contract, backend, global_runtime_metrics())
    }

    pub fn with_backend_and_metrics<B>(
        contract: ProviderContract,
        backend: Arc<B>,
        metrics: Arc<RuntimeMetrics>,
    ) -> Self
    where
        B: ProviderBackend<Value = ModuleUpdate>,
    {
        let (publisher, receiver) = provider_channel(contract, metrics);
        let worker_factory = Box::new(move |cancellation, refreshes| {
            Box::pin(run_provider_backend(
                publisher,
                backend,
                refreshes,
                cancellation,
            )) as BoxWorker
        });
        Self {
            receiver,
            worker_factory: Some(worker_factory),
        }
    }

    pub fn with_runtime_worker<F, Fut>(contract: ProviderContract, worker: F) -> Self
    where
        F: FnOnce(ModuleUpdateSender, CancellationToken, ProviderRefreshRegistry) -> Fut
            + Send
            + 'static,
        Fut: Future<Output = ()> + Send + 'static,
    {
        let (publisher, receiver) = provider_channel(contract, global_runtime_metrics());
        let worker_factory = Box::new(move |cancellation, refreshes| {
            Box::pin(worker(publisher, cancellation, refreshes)) as BoxWorker
        });
        Self {
            receiver,
            worker_factory: Some(worker_factory),
        }
    }

    pub fn take_worker_with_runtime(
        &mut self,
        cancellation: CancellationToken,
        refreshes: ProviderRefreshRegistry,
    ) -> Option<BoxWorker> {
        self.worker_factory
            .take()
            .map(|factory| factory(cancellation, refreshes))
    }

    pub fn take_worker(&mut self) -> Option<BoxWorker> {
        self.take_worker_with_runtime(CancellationToken::new(), ProviderRefreshRegistry::new(4))
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

    #[tokio::test]
    async fn common_runner_publishes_fresh_then_shuts_down_and_stops() {
        let mut contract = contract();
        contract.refresh.interval = Duration::from_secs(60);
        contract.refresh.timeout = Duration::from_millis(50);
        contract.shutdown.graceful_timeout = Duration::from_millis(50);
        let metrics = Arc::new(RuntimeMetrics::default());
        let status = ProviderStatusRegistry::default();
        let (publisher, mut receiver) =
            provider_channel_with_status_registry(contract, Arc::clone(&metrics), status.clone());
        let backend = Arc::new(TestProviderBackend::new([Ok("fixture".to_string())]));
        let cancellation = CancellationToken::new();
        let runner = tokio::spawn(run_provider_backend(
            publisher,
            Arc::clone(&backend),
            ProviderRefreshRegistry::with_metrics(2, Arc::clone(&metrics)),
            cancellation.clone(),
        ));

        let fresh = loop {
            let snapshot = receiver.changed().await.unwrap();
            if snapshot.health() == ProviderHealth::Fresh {
                break snapshot;
            }
        };
        assert_eq!(fresh.value().map(String::as_str), Some("fixture"));
        cancellation.cancel();
        runner.await.unwrap();
        assert_eq!(receiver.current().health(), ProviderHealth::Stopped);
        assert!(backend.shutdown_called());
        assert_eq!(status.get("test").unwrap().health, ProviderHealth::Stopped);
    }

    struct SlowBackend;

    #[async_trait]
    impl ProviderBackend for SlowBackend {
        type Value = String;

        async fn refresh(&self) -> Result<Self::Value, String> {
            time::sleep(Duration::from_millis(50)).await;
            Ok("late".to_string())
        }
    }

    #[tokio::test]
    async fn common_runner_records_refresh_timeout_as_bounded_error() {
        let mut contract = contract();
        contract.refresh.timeout = Duration::from_millis(5);
        let metrics = Arc::new(RuntimeMetrics::default());
        let (publisher, receiver) = provider_channel::<String>(contract, Arc::clone(&metrics));
        let attempt = refresh_provider_once(
            &publisher,
            &SlowBackend,
            &ProviderRefreshRegistry::with_metrics(1, Arc::clone(&metrics)),
            &CancellationToken::new(),
        )
        .await;
        assert_eq!(attempt, ProviderRefreshAttempt::TimedOut);
        let snapshot = receiver.current();
        assert_eq!(snapshot.health(), ProviderHealth::Error);
        assert!(snapshot.error().unwrap().contains("timed out"));
        assert_eq!(metrics.snapshot().provider_refreshes_timed_out, 1);
    }

    struct DisabledBackend;

    #[async_trait]
    impl ProviderBackend for DisabledBackend {
        type Value = String;

        async fn refresh(&self) -> Result<Self::Value, String> {
            unreachable!("refresh_outcome supplies the disabled state")
        }

        async fn refresh_outcome(&self) -> Result<ProviderRefreshOutcome<Self::Value>, String> {
            Ok(ProviderRefreshOutcome::Disabled(
                "fixture service is disabled".to_string(),
            ))
        }
    }

    #[tokio::test]
    async fn common_runner_supports_explicit_disabled_state() {
        let metrics = Arc::new(RuntimeMetrics::default());
        let (publisher, receiver) = provider_channel::<String>(contract(), Arc::clone(&metrics));
        let attempt = refresh_provider_once(
            &publisher,
            &DisabledBackend,
            &ProviderRefreshRegistry::with_metrics(1, metrics),
            &CancellationToken::new(),
        )
        .await;
        assert_eq!(attempt, ProviderRefreshAttempt::Disabled);
        assert_eq!(receiver.current().health(), ProviderHealth::Disabled);
    }

    #[test]
    fn provider_status_registry_rejects_stopped_state_from_replaced_instance() {
        let metrics = Arc::new(RuntimeMetrics::default());
        let status = ProviderStatusRegistry::default();
        let (old, _) = provider_channel_with_status_registry::<String>(
            contract(),
            Arc::clone(&metrics),
            status.clone(),
        );
        let (new, _) =
            provider_channel_with_status_registry::<String>(contract(), metrics, status.clone());
        assert!(new.instance_generation() > old.instance_generation());
        old.stopped();
        let active = status.get("test").unwrap();
        assert_eq!(active.instance_generation, new.instance_generation());
        assert_eq!(active.health, ProviderHealth::Startup);
    }

    #[tokio::test]
    async fn repeated_provider_replacement_releases_refresh_permits_and_keeps_newest_status() {
        let metrics = Arc::new(RuntimeMetrics::default());
        let refreshes = ProviderRefreshRegistry::with_metrics(4, Arc::clone(&metrics));
        let status = ProviderStatusRegistry::default();
        let cancellation = CancellationToken::new();
        let mut previous: Option<ProviderPublisher<String>> = None;
        let mut newest_instance = 0;

        for index in 0..128 {
            let (publisher, receiver) = provider_channel_with_status_registry::<String>(
                contract(),
                Arc::clone(&metrics),
                status.clone(),
            );
            newest_instance = publisher.instance_generation();
            let backend = TestProviderBackend::new([Ok(format!("fixture-{index}"))]);
            assert_eq!(
                refresh_provider_once(&publisher, &backend, &refreshes, &cancellation).await,
                ProviderRefreshAttempt::Published
            );
            assert_eq!(receiver.current().health(), ProviderHealth::Fresh);

            if let Some(old) = previous.replace(publisher) {
                old.stopped();
                let active = status.get("test").unwrap();
                assert_eq!(active.instance_generation, newest_instance);
                assert_eq!(active.health, ProviderHealth::Fresh);
            }
        }

        assert_eq!(refreshes.active_count(), 0);
        let runtime = metrics.snapshot();
        assert_eq!(runtime.provider_refreshes_started, 128);
        assert_eq!(runtime.provider_refreshes_completed, 128);
        let active = status.get("test").unwrap();
        assert_eq!(active.instance_generation, newest_instance);
        assert_eq!(active.health, ProviderHealth::Fresh);
    }

    #[test]
    fn provider_errors_are_utf8_safe_and_bounded() {
        let metrics = Arc::new(RuntimeMetrics::default());
        let (publisher, receiver) = provider_channel::<String>(contract(), metrics);
        let generation = publisher.begin_refresh();
        publisher.publish_result(generation, Err("ü".repeat(5000)));
        let error = receiver.current().error().unwrap().to_string();
        assert!(error.len() <= PROVIDER_ERROR_MAX_BYTES);
        assert!(error.ends_with('…'));
    }
}
