//! Tmux pane switching widget with release-to-confirm support.

use std::{
    ffi::OsString,
    sync::{Arc, Mutex},
    time::Duration,
};

use deskhalloumi_core::{
    Module, ModuleConfig, ModuleUpdate, Result,
    runtime::{
        ActionCommand, ActionRunner, ModuleSubscription, ProviderBackend, ProviderContract,
        ProviderRefreshOutcome, ProviderRefreshPolicy, global_runtime_metrics,
        refresh_provider_once, shutdown_provider_backend,
    },
};
use iced::{
    Element, Length,
    widget::{button, column, container, text},
};
use tokio::sync::mpsc;
use tracing::{error, info};

/// Represents a tmux pane.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct TmuxPane {
    pub id: usize,
    pub session_name: String,
    pub window_index: usize,
    pub pane_index: usize,
    pub current: bool,
}

pub fn provider_contract() -> ProviderContract {
    ProviderContract::new(
        "tmux",
        "Tmux",
        ProviderRefreshPolicy {
            interval: Duration::from_secs(2),
            timeout: Duration::from_secs(2),
            stale_after: Duration::from_secs(8),
            refresh_on_start: true,
        },
        "FixedTmuxSource",
    )
}

fn parse_tmux_pane_line(line: &str) -> Option<TmuxPane> {
    let parts = line.split_whitespace().collect::<Vec<_>>();
    if parts.len() != 5 {
        return None;
    }

    Some(TmuxPane {
        id: parts[0].strip_prefix('%')?.parse().ok()?,
        session_name: parts[1].to_string(),
        window_index: parts[2].parse().ok()?,
        pane_index: parts[3].parse().ok()?,
        current: parts[4] == "1",
    })
}

#[async_trait::async_trait]
pub trait TmuxSource: Send + Sync {
    async fn list_panes(&self) -> std::result::Result<Vec<TmuxPane>, String>;
    async fn select_pane(&self, pane: &TmuxPane) -> std::result::Result<(), String>;
}

#[derive(Debug, Default)]
pub struct SystemTmuxSource;

#[async_trait::async_trait]
impl TmuxSource for SystemTmuxSource {
    async fn list_panes(&self) -> std::result::Result<Vec<TmuxPane>, String> {
        let outcome = ActionRunner::with_timeout("tmux", "list-panes", Duration::from_secs(3))
            .run_command(ActionCommand::new(
                "tmux",
                [
                    "list-panes",
                    "-a",
                    "-F",
                    "#{pane_id} #{session_name} #{window_index} #{pane_index} #{pane_current}",
                ]
                .into_iter()
                .map(OsString::from)
                .collect(),
            ))
            .await;
        if let Err(error) = outcome.result {
            let detail = outcome.stderr.trim();
            return Err(if detail.is_empty() {
                error
            } else {
                detail.to_string()
            });
        }
        Ok(outcome
            .stdout
            .lines()
            .filter_map(parse_tmux_pane_line)
            .collect())
    }

    async fn select_pane(&self, pane: &TmuxPane) -> std::result::Result<(), String> {
        let target = format!("%{}", pane.id);
        let outcome = ActionRunner::with_timeout("tmux", "select-pane", Duration::from_secs(3))
            .run_command(ActionCommand::new(
                "tmux",
                ["select-pane", "-t", target.as_str()]
                    .into_iter()
                    .map(OsString::from)
                    .collect(),
            ))
            .await;
        if let Err(error) = outcome.result {
            let detail = outcome.stderr.trim();
            return Err(if detail.is_empty() {
                error
            } else {
                detail.to_string()
            });
        }
        info!(
            pane_id = pane.id,
            session = %pane.session_name,
            window_index = pane.window_index,
            pane_index = pane.pane_index,
            "switched tmux pane"
        );
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct FixedTmuxSource {
    panes: Vec<TmuxPane>,
    selected: Arc<Mutex<Vec<usize>>>,
}

impl FixedTmuxSource {
    pub fn new(panes: Vec<TmuxPane>) -> Self {
        Self {
            panes,
            selected: Arc::new(Mutex::new(Vec::new())),
        }
    }

    pub fn selected_panes(&self) -> Vec<usize> {
        self.selected
            .lock()
            .map(|selected| selected.clone())
            .unwrap_or_default()
    }
}

#[async_trait::async_trait]
impl TmuxSource for FixedTmuxSource {
    async fn list_panes(&self) -> std::result::Result<Vec<TmuxPane>, String> {
        Ok(self.panes.clone())
    }

    async fn select_pane(&self, pane: &TmuxPane) -> std::result::Result<(), String> {
        self.selected
            .lock()
            .map_err(|error| format!("fixed tmux source lock poisoned: {error}"))?
            .push(pane.id);
        Ok(())
    }
}

fn tmux_unavailable_error(error: &str) -> bool {
    let error = error.to_ascii_lowercase();
    error.contains("no server running")
        || error.contains("failed to connect to server")
        || error.contains("connection refused")
        || error.contains("no such file or directory")
}

fn panes_update(panes: Vec<TmuxPane>) -> ModuleUpdate {
    ModuleUpdate::Custom(
        serde_json::json!({
            "action": "update_panes",
            "panes": panes,
        })
        .to_string(),
    )
}

struct TmuxProviderBackend {
    source: Arc<dyn TmuxSource>,
}

#[async_trait::async_trait]
impl ProviderBackend for TmuxProviderBackend {
    type Value = ModuleUpdate;

    async fn refresh(&self) -> std::result::Result<Self::Value, String> {
        self.source.list_panes().await.map(panes_update)
    }

    async fn refresh_outcome(
        &self,
    ) -> std::result::Result<ProviderRefreshOutcome<Self::Value>, String> {
        match self.source.list_panes().await {
            Ok(panes) => Ok(ProviderRefreshOutcome::Fresh(panes_update(panes))),
            Err(error) if tmux_unavailable_error(&error) => {
                Ok(ProviderRefreshOutcome::Disabled(error))
            }
            Err(error) => Err(error),
        }
    }
}

#[derive(Debug)]
enum TmuxCommand {
    Refresh,
    Select(TmuxPane),
}

pub struct Tmux {
    panes: Vec<TmuxPane>,
    selected_index: Option<usize>,
    control_tx: Option<mpsc::Sender<TmuxCommand>>,
    source: Arc<dyn TmuxSource>,
}

impl Tmux {
    pub async fn with_source(source: Arc<dyn TmuxSource>) -> Result<Self> {
        let panes = source.list_panes().await.unwrap_or_default();
        Ok(Self {
            panes,
            selected_index: None,
            control_tx: None,
            source,
        })
    }

    fn queue_command(&self, command: TmuxCommand, coalescible: bool) {
        let Some(sender) = &self.control_tx else {
            return;
        };
        if let Err(error) = sender.try_send(command) {
            let metrics = global_runtime_metrics();
            match error {
                mpsc::error::TrySendError::Full(_) if coalescible => {
                    metrics.record_update_coalesced();
                }
                mpsc::error::TrySendError::Full(_) | mpsc::error::TrySendError::Closed(_) => {
                    metrics.record_update_dropped();
                }
            }
        }
    }
}

#[async_trait::async_trait]
impl Module for Tmux {
    async fn new(_config: &ModuleConfig) -> Result<Self> {
        Self::with_source(Arc::new(SystemTmuxSource)).await
    }

    fn name(&self) -> &str {
        "tmux"
    }

    fn view(&self) -> Element<'_, ModuleUpdate> {
        if let Some(selected) = self.selected_index {
            if selected < self.panes.len() {
                let mut buttons = Vec::new();
                for (index, pane) in self.panes.iter().enumerate() {
                    let label = format!(
                        "{}:{}:{}.{}",
                        if pane.current { "●" } else { "○" },
                        pane.session_name,
                        pane.window_index,
                        pane.pane_index
                    );
                    let button = button(text(label).size(12)).padding([4, 8]).on_press(
                        ModuleUpdate::Custom(format!(r#"{{"action":"select","index":{index}}}"#)),
                    );
                    buttons.push(if index == selected {
                        button.style(button::primary).into()
                    } else {
                        button.style(button::text).into()
                    });
                }
                buttons.push(
                    button(text("Cancel").size(12))
                        .padding([4, 8])
                        .on_press(ModuleUpdate::Custom(r#"{"action":"cancel"}"#.to_string()))
                        .style(button::text)
                        .into(),
                );
                container(column(buttons).spacing(4).width(Length::Shrink))
                    .padding(8)
                    .into()
            } else {
                text("No tmux panes").size(12).into()
            }
        } else {
            match self.panes.iter().find(|pane| pane.current) {
                Some(pane) => container(
                    text(format!(
                        "tmux: {}:{}.{}",
                        pane.session_name, pane.window_index, pane.pane_index
                    ))
                    .size(12),
                )
                .padding(4)
                .into(),
                None => text("tmux: none").size(12).into(),
            }
        }
    }

    fn update(&mut self, message: ModuleUpdate) -> Result<()> {
        let ModuleUpdate::Custom(json) = message else {
            return Ok(());
        };
        let Ok(data) = serde_json::from_str::<serde_json::Value>(&json) else {
            return Ok(());
        };
        let Some(action) = data.get("action").and_then(|value| value.as_str()) else {
            return Ok(());
        };

        match action {
            "select" => {
                if let Some(index) = data.get("index").and_then(|value| value.as_u64()) {
                    if let Some(pane) = self.panes.get(index as usize).cloned() {
                        self.queue_command(TmuxCommand::Select(pane), false);
                    }
                    self.selected_index = None;
                }
            }
            "cancel" => self.selected_index = None,
            "open_menu" => self.selected_index = (!self.panes.is_empty()).then_some(0),
            "next" => {
                if let Some(current) = self.selected_index
                    && current < self.panes.len().saturating_sub(1)
                {
                    self.selected_index = Some(current + 1);
                }
            }
            "prev" => {
                if let Some(current) = self.selected_index
                    && current > 0
                {
                    self.selected_index = Some(current - 1);
                }
            }
            "refresh" => self.queue_command(TmuxCommand::Refresh, true),
            "update_panes" => {
                if let Some(panes) = data.get("panes").cloned()
                    && let Ok(panes) = serde_json::from_value::<Vec<TmuxPane>>(panes)
                {
                    self.panes = panes;
                    self.selected_index = self
                        .selected_index
                        .filter(|index| *index < self.panes.len());
                }
            }
            _ => {}
        }
        Ok(())
    }

    async fn subscribe(&mut self) -> Result<Option<ModuleSubscription>> {
        let (control_tx, mut control_rx) = mpsc::channel(8);
        self.control_tx = Some(control_tx);
        let backend = Arc::new(TmuxProviderBackend {
            source: Arc::clone(&self.source),
        });

        Ok(Some(ModuleSubscription::with_runtime_worker(
            provider_contract(),
            move |updates, cancellation, refreshes| async move {
                let contract = updates.contract();
                let mut interval = tokio::time::interval(contract.refresh.interval);
                interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
                interval.tick().await;
                if contract.refresh.refresh_on_start {
                    let _ = refresh_provider_once(
                        &updates,
                        backend.as_ref(),
                        &refreshes,
                        &cancellation,
                    )
                    .await;
                }

                loop {
                    let command = tokio::select! {
                        _ = cancellation.cancelled() => break,
                        _ = updates.closed() => break,
                        _ = interval.tick() => TmuxCommand::Refresh,
                        command = control_rx.recv() => {
                            let Some(command) = command else { break; };
                            command
                        }
                    };

                    if let TmuxCommand::Select(pane) = command {
                        match tokio::time::timeout(
                            contract.refresh.timeout,
                            backend.source.select_pane(&pane),
                        )
                        .await
                        {
                            Ok(Ok(())) => {}
                            Ok(Err(error)) => {
                                global_runtime_metrics().record_provider_refresh_failed();
                                updates.mark_stale(error.clone());
                                error!(%error, "failed to switch tmux pane");
                            }
                            Err(_) => {
                                global_runtime_metrics().record_provider_refresh_timed_out();
                                let error = format!(
                                    "tmux pane selection timed out after {:?}",
                                    contract.refresh.timeout
                                );
                                updates.mark_stale(error.clone());
                                error!(%error, "failed to switch tmux pane");
                            }
                        }
                    }

                    let _ = refresh_provider_once(
                        &updates,
                        backend.as_ref(),
                        &refreshes,
                        &cancellation,
                    )
                    .await;
                }

                shutdown_provider_backend(&updates, backend.as_ref()).await;
            },
        )))
    }

    fn update_interval(&self) -> Option<u64> {
        Some(2000)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_real_tmux_pane_ids_and_rejects_malformed_rows() {
        let pane = parse_tmux_pane_line("%17 work 2 1 1").expect("valid pane row");
        assert_eq!(pane.id, 17);
        assert_eq!(pane.session_name, "work");
        assert_eq!(pane.window_index, 2);
        assert_eq!(pane.pane_index, 1);
        assert!(pane.current);

        assert!(parse_tmux_pane_line("17 work 2 1 1").is_none());
        assert!(parse_tmux_pane_line("%17 missing fields").is_none());
    }

    #[tokio::test]
    async fn fixed_source_lists_and_selects_without_tmux_server() {
        let pane = TmuxPane {
            id: 17,
            session_name: "work".to_string(),
            window_index: 2,
            pane_index: 1,
            current: true,
        };
        let source = FixedTmuxSource::new(vec![pane.clone()]);
        assert_eq!(source.list_panes().await.unwrap(), vec![pane.clone()]);
        source.select_pane(&pane).await.unwrap();
        assert_eq!(source.selected_panes(), vec![17]);
    }

    #[test]
    fn lifecycle_contract_names_executable_fixture_source() {
        let contract = provider_contract();
        assert_eq!(contract.id, "tmux");
        assert_eq!(contract.test_backend, "FixedTmuxSource");
    }
}
