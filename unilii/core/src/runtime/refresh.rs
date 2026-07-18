use std::{
    collections::HashSet,
    fmt,
    sync::{Arc, Mutex},
};

use tokio::sync::{OwnedSemaphorePermit, Semaphore};

use super::metrics::{RuntimeMetrics, global_runtime_metrics};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefreshRejected {
    Coalesced,
    Saturated,
}

impl fmt::Display for RefreshRejected {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Coalesced => f.write_str("an equivalent provider refresh is already running"),
            Self::Saturated => f.write_str("the provider refresh concurrency limit is saturated"),
        }
    }
}

impl std::error::Error for RefreshRejected {}

#[derive(Clone)]
pub struct ProviderRefreshRegistry {
    active: Arc<Mutex<HashSet<String>>>,
    permits: Arc<Semaphore>,
    metrics: Arc<RuntimeMetrics>,
}

impl ProviderRefreshRegistry {
    pub fn new(max_concurrent: usize) -> Self {
        Self::with_metrics(max_concurrent, global_runtime_metrics())
    }

    pub fn with_metrics(max_concurrent: usize, metrics: Arc<RuntimeMetrics>) -> Self {
        Self {
            active: Arc::new(Mutex::new(HashSet::new())),
            permits: Arc::new(Semaphore::new(max_concurrent.max(1))),
            metrics,
        }
    }

    pub fn try_start(
        &self,
        provider: impl Into<String>,
    ) -> Result<ProviderRefreshPermit, RefreshRejected> {
        let provider = provider.into();
        {
            let active = self
                .active
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            if active.contains(&provider) {
                self.metrics.record_provider_refresh_coalesced();
                return Err(RefreshRejected::Coalesced);
            }
        }

        let permit = Arc::clone(&self.permits).try_acquire_owned().map_err(|_| {
            self.metrics.record_provider_refresh_saturated();
            RefreshRejected::Saturated
        })?;

        let mut active = self
            .active
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if !active.insert(provider.clone()) {
            self.metrics.record_provider_refresh_coalesced();
            return Err(RefreshRejected::Coalesced);
        }
        drop(active);
        self.metrics.record_provider_refresh_started();

        Ok(ProviderRefreshPermit {
            provider,
            active: Arc::clone(&self.active),
            metrics: Arc::clone(&self.metrics),
            _permit: permit,
        })
    }

    pub fn active_count(&self) -> usize {
        self.active
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .len()
    }
}

pub struct ProviderRefreshPermit {
    provider: String,
    active: Arc<Mutex<HashSet<String>>>,
    metrics: Arc<RuntimeMetrics>,
    _permit: OwnedSemaphorePermit,
}

impl Drop for ProviderRefreshPermit {
    fn drop(&mut self) {
        self.active
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .remove(&self.provider);
        self.metrics.record_provider_refresh_completed();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refreshes_are_bounded_and_same_provider_requests_coalesce() {
        let metrics = Arc::new(RuntimeMetrics::default());
        let registry = ProviderRefreshRegistry::with_metrics(1, Arc::clone(&metrics));
        let first = registry.try_start("network").unwrap();
        assert!(matches!(
            registry.try_start("network"),
            Err(RefreshRejected::Coalesced)
        ));
        assert!(matches!(
            registry.try_start("calendar"),
            Err(RefreshRejected::Saturated)
        ));
        assert_eq!(registry.active_count(), 1);
        drop(first);
        assert_eq!(registry.active_count(), 0);
        let snapshot = metrics.snapshot();
        assert_eq!(snapshot.provider_refreshes_started, 1);
        assert_eq!(snapshot.provider_refreshes_completed, 1);
        assert_eq!(snapshot.provider_refreshes_coalesced, 1);
        assert_eq!(snapshot.provider_refreshes_saturated, 1);
    }
}
