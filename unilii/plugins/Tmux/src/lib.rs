//! Tmux pane switching widget with release-to-confirm support.

use std::{ffi::OsString, time::Duration};

use deskhalloumi_core::{
    Module, ModuleConfig, ModuleUpdate, Result,
    runtime::{ActionCommand, ActionRunner, ModuleSubscription, global_runtime_metrics},
};
use iced::{
    Element, Length,
    widget::{button, column, container, text},
};
use tokio::sync::mpsc;
use tracing::{error, info, warn};

/// Represents a tmux pane.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TmuxPane {
    pub id: usize,
    pub session_name: String,
    pub window_index: usize,
    pub pane_index: usize,
    pub current: bool,
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

#[derive(Debug)]
enum TmuxCommand {
    Refresh,
    Select(TmuxPane),
}

pub struct Tmux {
    panes: Vec<TmuxPane>,
    selected_index: Option<usize>,
    control_tx: Option<mpsc::Sender<TmuxCommand>>,
}

impl Tmux {
    async fn list_panes() -> Result<Vec<TmuxPane>> {
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
            }
            .into());
        }

        Ok(outcome
            .stdout
            .lines()
            .filter_map(parse_tmux_pane_line)
            .collect())
    }

    async fn switch_to_pane(pane: &TmuxPane) -> Result<()> {
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
            }
            .into());
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
        let panes = Self::list_panes().await.unwrap_or_default();
        Ok(Self {
            panes,
            selected_index: None,
            control_tx: None,
        })
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

        Ok(Some(ModuleSubscription::new(move |updates| async move {
            let mut interval = tokio::time::interval(Duration::from_secs(2));
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                let command = tokio::select! {
                    _ = interval.tick() => TmuxCommand::Refresh,
                    command = control_rx.recv() => {
                        let Some(command) = command else { break; };
                        command
                    }
                };

                match command {
                    TmuxCommand::Refresh => {}
                    TmuxCommand::Select(pane) => {
                        if let Err(error) = Self::switch_to_pane(&pane).await {
                            error!(%error, "failed to switch tmux pane");
                        }
                    }
                }

                match Self::list_panes().await {
                    Ok(panes) => {
                        let json = serde_json::json!({
                            "action": "update_panes",
                            "panes": panes,
                        });
                        if !updates.send(ModuleUpdate::Custom(json.to_string())) {
                            break;
                        }
                    }
                    Err(error) => warn!(%error, "failed to refresh tmux panes"),
                }
            }
        })))
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
}
