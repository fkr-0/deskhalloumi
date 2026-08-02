//! Shared lifecycle ownership for the built-in audio, network, and system providers.
//!
//! The Iced update loop requests operations and applies returned snapshots, but
//! timeout, admission, generations, health publication, and shutdown live here
//! and in `deskhalloumi-core::runtime`.

use std::sync::Arc;

use async_trait::async_trait;
use deskhalloumi_core::runtime::{
    ProviderBackend, ProviderContract, ProviderPublisher, ProviderReceiver, ProviderRefreshOutcome,
    ProviderRefreshRegistry, ProviderSnapshot, global_runtime_metrics, provider_channel,
    refresh_provider_once, run_provider_operation, shutdown_provider_backend,
};
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

use crate::widgets::{
    audio::{AudioSelectionAction, AudioSnapshot, apply_audio_selection, read_audio_snapshot},
    sysmonitor::{SysMonitor, SystemStatsSnapshot},
    wifi::{WifiSnapshot, read_wifi_snapshot, set_wifi_enabled},
};

pub struct ProviderHandle<B>
where
    B: ProviderBackend,
{
    publisher: ProviderPublisher<B::Value>,
    receiver: ProviderReceiver<B::Value>,
    backend: Arc<B>,
}

impl<B> Clone for ProviderHandle<B>
where
    B: ProviderBackend,
{
    fn clone(&self) -> Self {
        Self {
            publisher: self.publisher.clone(),
            receiver: self.receiver.clone(),
            backend: Arc::clone(&self.backend),
        }
    }
}

impl<B> ProviderHandle<B>
where
    B: ProviderBackend,
{
    fn new(contract: ProviderContract, backend: Arc<B>) -> Self {
        let (publisher, receiver) = provider_channel(contract, global_runtime_metrics());
        Self {
            publisher,
            receiver,
            backend,
        }
    }

    pub fn current(&self) -> ProviderSnapshot<B::Value> {
        self.receiver.current()
    }

    pub fn accepts(&self, snapshot: &ProviderSnapshot<B::Value>) -> bool {
        snapshot.belongs_to_instance(self.receiver.instance_generation())
    }

    async fn refresh(
        &self,
        refreshes: &ProviderRefreshRegistry,
        cancellation: &CancellationToken,
    ) -> ProviderSnapshot<B::Value> {
        let _ = refresh_provider_once(
            &self.publisher,
            self.backend.as_ref(),
            refreshes,
            cancellation,
        )
        .await;
        self.current()
    }

    async fn shutdown(&self) {
        shutdown_provider_backend(&self.publisher, self.backend.as_ref()).await;
    }
}

pub struct AudioProviderBackend {
    pactl: String,
}

impl AudioProviderBackend {
    pub fn new(pactl: impl Into<String>) -> Self {
        Self {
            pactl: pactl.into(),
        }
    }
}

#[async_trait]
impl ProviderBackend for AudioProviderBackend {
    type Value = AudioSnapshot;

    async fn refresh(&self) -> Result<Self::Value, String> {
        read_audio_snapshot(self.pactl.clone()).await
    }
}

#[cfg(test)]
#[derive(Debug, Clone)]
pub struct FixedAudioProviderBackend {
    snapshot: AudioSnapshot,
}

#[cfg(test)]
impl FixedAudioProviderBackend {
    pub fn new(snapshot: AudioSnapshot) -> Self {
        Self { snapshot }
    }
}

#[cfg(test)]
#[async_trait]
impl ProviderBackend for FixedAudioProviderBackend {
    type Value = AudioSnapshot;

    async fn refresh(&self) -> Result<Self::Value, String> {
        Ok(self.snapshot.clone())
    }
}

pub struct WifiProviderBackend {
    nmcli: String,
}

impl WifiProviderBackend {
    pub fn new(nmcli: impl Into<String>) -> Self {
        Self {
            nmcli: nmcli.into(),
        }
    }
}

#[async_trait]
impl ProviderBackend for WifiProviderBackend {
    type Value = WifiSnapshot;

    async fn refresh(&self) -> Result<Self::Value, String> {
        read_wifi_snapshot(self.nmcli.clone()).await
    }
}

#[cfg(test)]
#[derive(Debug, Clone)]
pub struct FixedWifiProviderBackend {
    snapshot: WifiSnapshot,
}

#[cfg(test)]
impl FixedWifiProviderBackend {
    pub fn new(snapshot: WifiSnapshot) -> Self {
        Self { snapshot }
    }
}

#[cfg(test)]
#[async_trait]
impl ProviderBackend for FixedWifiProviderBackend {
    type Value = WifiSnapshot;

    async fn refresh(&self) -> Result<Self::Value, String> {
        Ok(self.snapshot.clone())
    }
}

pub struct SystemStatsProviderBackend {
    monitor: Mutex<SysMonitor>,
}

impl SystemStatsProviderBackend {
    pub fn new() -> Self {
        Self {
            monitor: Mutex::new(SysMonitor::new()),
        }
    }
}

#[async_trait]
impl ProviderBackend for SystemStatsProviderBackend {
    type Value = SystemStatsSnapshot;

    async fn refresh(&self) -> Result<Self::Value, String> {
        let mut monitor = self.monitor.lock().await;
        monitor.update_stats();
        Ok(monitor.snapshot().clone())
    }
}

#[cfg(test)]
#[derive(Debug, Clone)]
pub struct FixedSystemStatsBackend {
    snapshot: SystemStatsSnapshot,
}

#[cfg(test)]
impl FixedSystemStatsBackend {
    pub fn new(snapshot: SystemStatsSnapshot) -> Self {
        Self { snapshot }
    }
}

#[cfg(test)]
#[async_trait]
impl ProviderBackend for FixedSystemStatsBackend {
    type Value = SystemStatsSnapshot;

    async fn refresh(&self) -> Result<Self::Value, String> {
        Ok(self.snapshot.clone())
    }
}

#[derive(Clone)]
pub struct LegacyProviderRuntime {
    pub audio: ProviderHandle<AudioProviderBackend>,
    pub network: ProviderHandle<WifiProviderBackend>,
    pub system: ProviderHandle<SystemStatsProviderBackend>,
}

impl LegacyProviderRuntime {
    pub fn new(nmcli: impl Into<String>) -> Self {
        Self {
            audio: ProviderHandle::new(
                crate::widgets::audio::provider_contract(),
                Arc::new(AudioProviderBackend::new("pactl")),
            ),
            network: ProviderHandle::new(
                crate::widgets::wifi::provider_contract(),
                Arc::new(WifiProviderBackend::new(nmcli)),
            ),
            system: ProviderHandle::new(
                crate::widgets::sysmonitor::provider_contract(),
                Arc::new(SystemStatsProviderBackend::new()),
            ),
        }
    }

    pub async fn refresh_audio(
        &self,
        selection: Option<AudioSelectionAction>,
        refreshes: ProviderRefreshRegistry,
        cancellation: CancellationToken,
    ) -> ProviderSnapshot<AudioSnapshot> {
        if let Some(selection) = selection {
            let backend = Arc::clone(&self.audio.backend);
            let _ = run_provider_operation(
                &self.audio.publisher,
                &refreshes,
                &cancellation,
                move || async move {
                    apply_audio_selection(backend.pactl.clone(), selection)
                        .await
                        .map(ProviderRefreshOutcome::Fresh)
                },
            )
            .await;
            self.audio.current()
        } else {
            self.audio.refresh(&refreshes, &cancellation).await
        }
    }

    pub async fn refresh_wifi(
        &self,
        enabled: Option<bool>,
        refreshes: ProviderRefreshRegistry,
        cancellation: CancellationToken,
    ) -> ProviderSnapshot<WifiSnapshot> {
        if let Some(enabled) = enabled {
            let backend = Arc::clone(&self.network.backend);
            let _ = run_provider_operation(
                &self.network.publisher,
                &refreshes,
                &cancellation,
                move || async move {
                    set_wifi_enabled(backend.nmcli.clone(), enabled)
                        .await
                        .map(ProviderRefreshOutcome::Fresh)
                },
            )
            .await;
            self.network.current()
        } else {
            self.network.refresh(&refreshes, &cancellation).await
        }
    }

    pub async fn refresh_system(
        &self,
        refreshes: ProviderRefreshRegistry,
        cancellation: CancellationToken,
    ) -> ProviderSnapshot<SystemStatsSnapshot> {
        self.system.refresh(&refreshes, &cancellation).await
    }

    pub async fn shutdown(&self) {
        tokio::join!(
            self.audio.shutdown(),
            self.network.shutdown(),
            self.system.shutdown(),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use deskhalloumi_core::runtime::{ProviderHealth, ProviderRefreshRegistry};

    #[tokio::test]
    async fn fixed_backends_execute_without_live_services() {
        let audio = AudioSnapshot {
            current_output: "fixture-output".to_string(),
            current_input: "fixture-input".to_string(),
            output_devices: Vec::new(),
            input_devices: Vec::new(),
        };
        assert_eq!(
            FixedAudioProviderBackend::new(audio.clone())
                .refresh()
                .await
                .unwrap(),
            audio
        );

        let wifi = WifiSnapshot {
            ssid: "fixture".to_string(),
            signal: 77,
            connected: true,
            wifi_enabled: true,
            networks: Vec::new(),
        };
        assert_eq!(
            FixedWifiProviderBackend::new(wifi.clone())
                .refresh()
                .await
                .unwrap(),
            wifi
        );

        let system = SystemStatsSnapshot::default();
        assert_eq!(
            FixedSystemStatsBackend::new(system.clone())
                .refresh()
                .await
                .unwrap(),
            system
        );
    }

    #[tokio::test]
    async fn provider_handle_uses_shared_generation_and_health_path() {
        let snapshot = WifiSnapshot {
            ssid: "fixture".to_string(),
            signal: 42,
            connected: true,
            wifi_enabled: true,
            networks: Vec::new(),
        };
        let handle = ProviderHandle::new(
            crate::widgets::wifi::provider_contract(),
            Arc::new(FixedWifiProviderBackend::new(snapshot.clone())),
        );
        let result = handle
            .refresh(&ProviderRefreshRegistry::new(2), &CancellationToken::new())
            .await;
        assert_eq!(result.health(), ProviderHealth::Fresh);
        assert_eq!(result.value(), Some(&snapshot));
    }
}
