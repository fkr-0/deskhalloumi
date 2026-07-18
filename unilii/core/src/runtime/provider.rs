use std::{
    future::Future,
    pin::Pin,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use tokio::sync::watch;

use crate::ModuleUpdate;

use super::metrics::{RuntimeMetrics, global_runtime_metrics};

pub type BoxWorker = Pin<Box<dyn Future<Output = ()> + Send + 'static>>;

#[derive(Clone)]
pub struct ModuleUpdateSender {
    sender: watch::Sender<Option<ModuleUpdate>>,
    pending: Arc<AtomicBool>,
    metrics: Arc<RuntimeMetrics>,
}

impl ModuleUpdateSender {
    pub fn send(&self, update: ModuleUpdate) -> bool {
        if self.sender.is_closed() {
            self.metrics.record_update_dropped();
            return false;
        }
        if self.pending.swap(true, Ordering::AcqRel) {
            self.metrics.record_update_coalesced();
        }
        self.sender.send_replace(Some(update));
        true
    }

    pub fn is_closed(&self) -> bool {
        self.sender.is_closed()
    }
}

pub struct ModuleSubscription {
    receiver: watch::Receiver<Option<ModuleUpdate>>,
    pending: Arc<AtomicBool>,
    worker: Option<BoxWorker>,
}

impl ModuleSubscription {
    pub fn new<F, Fut>(worker: F) -> Self
    where
        F: FnOnce(ModuleUpdateSender) -> Fut,
        Fut: Future<Output = ()> + Send + 'static,
    {
        Self::with_metrics(worker, global_runtime_metrics())
    }

    pub fn with_metrics<F, Fut>(worker: F, metrics: Arc<RuntimeMetrics>) -> Self
    where
        F: FnOnce(ModuleUpdateSender) -> Fut,
        Fut: Future<Output = ()> + Send + 'static,
    {
        let (sender, receiver) = watch::channel(None);
        let pending = Arc::new(AtomicBool::new(false));
        let worker_sender = ModuleUpdateSender {
            sender,
            pending: Arc::clone(&pending),
            metrics,
        };
        Self {
            receiver,
            pending,
            worker: Some(Box::pin(worker(worker_sender))),
        }
    }

    pub fn take_worker(&mut self) -> Option<BoxWorker> {
        self.worker.take()
    }

    pub async fn recv(&mut self) -> Option<ModuleUpdate> {
        loop {
            self.receiver.changed().await.ok()?;
            let update = self.receiver.borrow_and_update().clone();
            self.pending.store(false, Ordering::Release);
            if update.is_some() {
                return update;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn latest_value_channel_coalesces_unread_updates() {
        let metrics = Arc::new(RuntimeMetrics::default());
        let mut subscription = ModuleSubscription::with_metrics(
            |sender| async move {
                sender.send(ModuleUpdate::Text("one".to_string()));
                sender.send(ModuleUpdate::Text("two".to_string()));
            },
            Arc::clone(&metrics),
        );
        subscription.take_worker().unwrap().await;
        assert!(matches!(
            subscription.recv().await,
            Some(ModuleUpdate::Text(value)) if value == "two"
        ));
        assert_eq!(metrics.snapshot().updates_coalesced, 1);
    }
}
