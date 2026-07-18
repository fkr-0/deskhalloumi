//! Tmux pane switching widget with release-to-confirm support.

use deskhalloumi_core::{Module, ModuleConfig, ModuleUpdate, Result};
use iced::{
    Element, Length,
    widget::{button, column, container, text},
};
use std::process::Command;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;
use tracing::{error, info};

/// Represents a tmux pane
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

pub struct Tmux {
    panes: Vec<TmuxPane>,
    selected_index: Option<usize>,
    tx: Arc<Mutex<Option<mpsc::UnboundedSender<ModuleUpdate>>>>,
}

impl Tmux {
    async fn list_panes() -> Result<Vec<TmuxPane>> {
        let output = Command::new("tmux")
            .args([
                "list-panes",
                "-a",
                "-F",
                "#{pane_id} #{session_name} #{window_index} #{pane_index} #{pane_current}",
            ])
            .output()
            .map_err(|e| format!("Failed to execute tmux list-panes: {}", e))?;

        if !output.status.success() {
            return Err(format!(
                "tmux list-panes failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            )
            .into());
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        Ok(stdout.lines().filter_map(parse_tmux_pane_line).collect())
    }

    async fn switch_to_pane(pane: &TmuxPane) -> Result<()> {
        let target = format!("%{}", pane.id);
        let output = Command::new("tmux")
            .args(["select-pane", "-t", &target])
            .output()
            .map_err(|e| format!("Failed to switch to tmux pane {target}: {e}"))?;

        if !output.status.success() {
            return Err(format!(
                "tmux select-pane {target} failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            )
            .into());
        }

        info!(
            "Switched to tmux pane {}:{}:{}",
            pane.session_name, pane.window_index, pane.pane_index
        );
        Ok(())
    }
}

#[async_trait::async_trait]
impl Module for Tmux {
    async fn new(_config: &ModuleConfig) -> Result<Self> {
        let panes = Self::list_panes().await.unwrap_or_default();
        let tx = Arc::new(Mutex::new(None));

        Ok(Self {
            panes,
            selected_index: None,
            tx,
        })
    }

    fn name(&self) -> &str {
        "tmux"
    }

    fn view(&self) -> Element<'_, ModuleUpdate> {
        // Show current pane info or selection menu
        if let Some(selected) = self.selected_index {
            if selected < self.panes.len() {
                let mut buttons = Vec::new();

                for (i, p) in self.panes.iter().enumerate() {
                    let label = format!(
                        "{}:{}:{}.{}",
                        if p.current { "●" } else { "○" },
                        p.session_name,
                        p.window_index,
                        p.pane_index
                    );

                    let is_selected = i == selected;
                    let btn = button(text(label).size(12)).padding([4, 8]).on_press(
                        ModuleUpdate::Custom(format!(r#"{{"action":"select","index":{}}}"#, i)),
                    );

                    let btn = if is_selected {
                        btn.style(button::primary)
                    } else {
                        btn.style(button::text)
                    };

                    buttons.push(btn.into());
                }

                // Cancel button
                let cancel_btn = button(text("Cancel").size(12))
                    .padding([4, 8])
                    .on_press(ModuleUpdate::Custom(r#"{"action":"cancel"}"#.to_string()))
                    .style(button::text);

                buttons.push(cancel_btn.into());

                let content = column(buttons).spacing(4).width(Length::Shrink);

                container(content).padding(8).into()
            } else {
                text("No tmux panes").size(12).into()
            }
        } else {
            // Show current pane or empty state
            let current = self.panes.iter().find(|p| p.current);
            match current {
                Some(pane) => {
                    let label = format!(
                        "tmux: {}:{}.{}",
                        pane.session_name, pane.window_index, pane.pane_index
                    );
                    container(text(label).size(12)).padding(4).into()
                }
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
        let Some(action) = data.get("action").and_then(|v| v.as_str()) else {
            return Ok(());
        };

        match action {
            "select" => {
                if let Some(index) = data.get("index").and_then(|v| v.as_u64()) {
                    if let Some(pane) = self.panes.get(index as usize) {
                        let pane = pane.clone();
                        let tx = self
                            .tx
                            .lock()
                            .map_err(|_| "tmux sender lock poisoned")?
                            .clone();
                        tokio::spawn(async move {
                            if let Err(e) = Self::switch_to_pane(&pane).await {
                                error!("Failed to switch tmux pane: {}", e);
                            }
                            if let Some(tx) = tx {
                                let _ = tx.send(ModuleUpdate::Text("refresh".to_string()));
                            }
                        });
                    }
                    self.selected_index = None;
                }
            }
            "cancel" => self.selected_index = None,
            "open_menu" => {
                self.selected_index = (!self.panes.is_empty()).then_some(0);
            }
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
            "refresh" => {
                let tx = self
                    .tx
                    .lock()
                    .map_err(|_| "tmux sender lock poisoned")?
                    .clone();
                tokio::spawn(async move {
                    if let Ok(panes) = Self::list_panes().await
                        && let Some(tx) = tx
                    {
                        let json = serde_json::json!({ "action": "update_panes", "panes": panes });
                        let _ = tx.send(ModuleUpdate::Custom(json.to_string()));
                    }
                });
            }
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

    async fn subscribe(&mut self) -> Result<Option<mpsc::UnboundedReceiver<ModuleUpdate>>> {
        let (tx, rx) = mpsc::unbounded_channel();
        *self.tx.lock().map_err(|_| "tmux sender lock poisoned")? = Some(tx.clone());

        // Refresh pane list periodically
        let tx_clone = tx.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(2));
            loop {
                interval.tick().await;
                let _ = tx_clone.send(ModuleUpdate::Text("refresh".to_string()));
            }
        });

        Ok(Some(rx))
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
