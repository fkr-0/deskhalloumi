use std::{
    fmt,
    future::Future,
    pin::Pin,
    sync::{Arc, Mutex},
    time::Duration,
};

use tokio::{
    sync::mpsc,
    task::{JoinHandle, JoinSet},
    time,
};
use tokio_util::sync::CancellationToken;
use tracing::{error, info, warn};

use super::metrics::{RuntimeMetrics, global_runtime_metrics};

type BoxTask = Pin<Box<dyn Future<Output = ()> + Send + 'static>>;

struct SpawnRequest {
    name: String,
    future: BoxTask,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpawnError {
    ShuttingDown,
    QueueFull,
}

impl fmt::Display for SpawnError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ShuttingDown => f.write_str("runtime supervisor is shutting down"),
            Self::QueueFull => f.write_str("runtime supervisor spawn queue is full"),
        }
    }
}

impl std::error::Error for SpawnError {}

#[derive(Clone)]
pub struct TaskSpawner {
    sender: mpsc::Sender<SpawnRequest>,
    cancellation: CancellationToken,
    metrics: Arc<RuntimeMetrics>,
}

impl TaskSpawner {
    pub fn cancellation_token(&self) -> CancellationToken {
        self.cancellation.clone()
    }

    pub fn is_shutting_down(&self) -> bool {
        self.cancellation.is_cancelled()
    }

    pub async fn spawn<F>(&self, name: impl Into<String>, future: F) -> Result<(), SpawnError>
    where
        F: Future<Output = ()> + Send + 'static,
    {
        if self.cancellation.is_cancelled() {
            return Err(SpawnError::ShuttingDown);
        }
        self.sender
            .send(SpawnRequest {
                name: name.into(),
                future: Box::pin(future),
            })
            .await
            .map_err(|_| SpawnError::ShuttingDown)
    }

    pub fn try_spawn<F>(&self, name: impl Into<String>, future: F) -> Result<(), SpawnError>
    where
        F: Future<Output = ()> + Send + 'static,
    {
        if self.cancellation.is_cancelled() {
            return Err(SpawnError::ShuttingDown);
        }
        self.sender
            .try_send(SpawnRequest {
                name: name.into(),
                future: Box::pin(future),
            })
            .map_err(|error| match error {
                mpsc::error::TrySendError::Full(_) => {
                    self.metrics.record_update_dropped();
                    SpawnError::QueueFull
                }
                mpsc::error::TrySendError::Closed(_) => SpawnError::ShuttingDown,
            })
    }
}

pub struct RuntimeSupervisor {
    name: String,
    spawner: TaskSpawner,
    join: Mutex<Option<JoinHandle<()>>>,
}

impl RuntimeSupervisor {
    pub fn start(name: impl Into<String>, spawn_queue_capacity: usize) -> Self {
        Self::with_metrics(name, spawn_queue_capacity, global_runtime_metrics())
    }

    pub fn with_metrics(
        name: impl Into<String>,
        spawn_queue_capacity: usize,
        metrics: Arc<RuntimeMetrics>,
    ) -> Self {
        let name = name.into();
        let cancellation = CancellationToken::new();
        let (sender, receiver) = mpsc::channel(spawn_queue_capacity.max(1));
        let spawner = TaskSpawner {
            sender,
            cancellation: cancellation.clone(),
            metrics: Arc::clone(&metrics),
        };
        let supervisor_name = name.clone();
        let join = tokio::spawn(run_supervisor(
            supervisor_name,
            receiver,
            cancellation,
            metrics,
        ));
        Self {
            name,
            spawner,
            join: Mutex::new(Some(join)),
        }
    }

    pub fn spawner(&self) -> TaskSpawner {
        self.spawner.clone()
    }

    pub async fn shutdown(&self, timeout: Duration) -> Result<(), String> {
        self.spawner.cancellation.cancel();
        let handle = self
            .join
            .lock()
            .map_err(|_| format!("{} supervisor join lock was poisoned", self.name))?
            .take();
        let Some(mut handle) = handle else {
            return Ok(());
        };
        match time::timeout(timeout, &mut handle).await {
            Ok(Ok(())) => Ok(()),
            Ok(Err(error)) => Err(format!("{} supervisor failed: {error}", self.name)),
            Err(_) => {
                handle.abort();
                let _ = handle.await;
                Err(format!(
                    "{} supervisor exceeded {:?} shutdown timeout and was aborted",
                    self.name, timeout
                ))
            }
        }
    }
}

impl Drop for RuntimeSupervisor {
    fn drop(&mut self) {
        self.spawner.cancellation.cancel();
        if let Ok(mut join) = self.join.lock()
            && let Some(handle) = join.take()
        {
            handle.abort();
        }
    }
}

async fn run_supervisor(
    name: String,
    mut receiver: mpsc::Receiver<SpawnRequest>,
    cancellation: CancellationToken,
    metrics: Arc<RuntimeMetrics>,
) {
    let mut tasks = JoinSet::new();
    info!(supervisor = %name, "runtime supervisor started");

    loop {
        tokio::select! {
            _ = cancellation.cancelled() => break,
            request = receiver.recv() => {
                let Some(request) = request else { break; };
                spawn_request(&mut tasks, request, &metrics);
            }
            result = tasks.join_next(), if !tasks.is_empty() => {
                observe_task_result(&name, result, &metrics);
            }
        }
    }

    receiver.close();
    while let Ok(request) = receiver.try_recv() {
        spawn_request(&mut tasks, request, &metrics);
    }
    let graceful = time::timeout(Duration::from_millis(750), async {
        while let Some(result) = tasks.join_next().await {
            observe_task_result(&name, Some(result), &metrics);
        }
    })
    .await;

    if graceful.is_err() {
        warn!(supervisor = %name, remaining = tasks.len(), "runtime tasks exceeded graceful shutdown window");
        tasks.abort_all();
        while let Some(result) = tasks.join_next().await {
            observe_task_result(&name, Some(result), &metrics);
        }
    }

    info!(supervisor = %name, "runtime supervisor stopped");
}

fn spawn_request(
    tasks: &mut JoinSet<String>,
    request: SpawnRequest,
    metrics: &Arc<RuntimeMetrics>,
) {
    let task_metrics = Arc::clone(metrics);
    tasks.spawn(async move {
        let _active = task_metrics.task_guard();
        request.future.await;
        request.name
    });
}

fn observe_task_result(
    supervisor: &str,
    result: Option<Result<String, tokio::task::JoinError>>,
    metrics: &RuntimeMetrics,
) {
    let Some(result) = result else {
        return;
    };
    match result {
        Ok(task) => {
            metrics.record_task_completed();
            info!(supervisor, task, "runtime task completed");
        }
        Err(error) if error.is_panic() => {
            metrics.record_task_panicked();
            error!(supervisor, %error, "runtime task panicked");
        }
        Err(error) => {
            metrics.record_task_cancelled();
            info!(supervisor, %error, "runtime task cancelled");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};

    #[tokio::test]
    async fn supervisor_owns_tasks_and_shuts_down_with_cancellation() {
        let metrics = Arc::new(RuntimeMetrics::default());
        let supervisor = RuntimeSupervisor::with_metrics("test", 4, Arc::clone(&metrics));
        let spawner = supervisor.spawner();
        let token = spawner.cancellation_token();
        let stopped = Arc::new(AtomicBool::new(false));
        let stopped_worker = Arc::clone(&stopped);
        spawner
            .spawn("worker", async move {
                token.cancelled().await;
                stopped_worker.store(true, Ordering::Relaxed);
            })
            .await
            .unwrap();

        supervisor.shutdown(Duration::from_secs(1)).await.unwrap();
        assert!(stopped.load(Ordering::Relaxed));
        let snapshot = metrics.snapshot();
        assert_eq!(snapshot.active_tasks, 0);
        assert_eq!(snapshot.tasks_started, 1);
        assert_eq!(snapshot.tasks_completed, 1);
    }

    #[tokio::test]
    async fn bounded_spawn_queue_reports_pressure() {
        let metrics = Arc::new(RuntimeMetrics::default());
        let supervisor = RuntimeSupervisor::with_metrics("pressure", 1, Arc::clone(&metrics));
        let spawner = supervisor.spawner();
        let _ = spawner.try_spawn("one", async {});
        let second = spawner.try_spawn("two", async {});
        if second == Err(SpawnError::QueueFull) {
            assert_eq!(metrics.snapshot().updates_dropped, 1);
        }
        supervisor.shutdown(Duration::from_secs(1)).await.unwrap();
    }
}
