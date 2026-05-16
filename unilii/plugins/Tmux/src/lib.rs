//! Tmux pane switching widget with release-to-confirm support.

use iced::{
    widget::{button, column, container, text},
    Element, Length,
};
use std::process::Command;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;
use tracing::{error, info};
use unilii_core::{Module, ModuleConfig, ModuleUpdate, Result};

/// Represents a tmux pane
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TmuxPane {
    pub id: usize,
    pub session_name: String,
    pub window_index: usize,
    pub pane_index: usize,
    pub current: bool,
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
                "-F",
                "#{pane_id} #{session_name} #{window_index} #{pane_index} #{pane_current}",
            ])
            .output()
            .map_err(|e| format!("Failed to execute tmux list-panes: {}", e))?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut panes = Vec::new();

        for line in stdout.lines() {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 5 {
                if let (Ok(id), Ok(window_index), Ok(pane_index), Ok(current)) = (
                    parts[0].parse::<usize>(),
                    parts[2].parse::<usize>(),
                    parts[3].parse::<usize>(),
                    parts[4].parse::<usize>(),
                ) {
                    panes.push(TmuxPane {
                        id,
                        session_name: parts[1].to_string(),
                        window_index,
                        pane_index,
                        current: current == 1,
                    });
                }
            }
        }

        Ok(panes)
    }

    async fn switch_to_pane(pane: &TmuxPane) -> Result<()> {
        Command::new("tmux")
            .args([
                "select-pane",
                "-t",
                &format!(
                    "{}:{}.{}",
                    pane.session_name, pane.window_index, pane.pane_index
                ),
            ])
            .spawn()
            .map_err(|e| format!("Failed to switch to tmux pane: {}", e))?;

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
        if let ModuleUpdate::Custom(json) = message {
            if let Ok(data) = serde_json::from_str::<serde_json::Value>(&json) {
                if let Some(action) = data.get("action").and_then(|v| v.as_str()) {
                    match action {
                        "select" => {
                            if let Some(index) = data.get("index").and_then(|v| v.as_u64()) {
                                if let Some(pane) = self.panes.get(index as usize) {
                                    // Trigger the pane switch via background task
                                    let pane = pane.clone();
                                    let tx = self.tx.lock().unwrap().clone();
                                    tokio::spawn(async move {
                                        if let Err(e) = Self::switch_to_pane(&pane).await {
                                            error!("Failed to switch tmux pane: {}", e);
                                        }
                                        // Refresh pane list after switch
                                        if let Some(tx) = tx {
                                            let _ =
                                                tx.send(ModuleUpdate::Text("refresh".to_string()));
                                        }
                                    });
                                }
                                self.selected_index = None;
                            }
                        }
                        "cancel" => {
                            self.selected_index = None;
                        }
                        "open_menu" => {
                            // Open selection menu
                            self.selected_index = Some(0);
                        }
                        "next" => {
                            // Navigate to next pane
                            if let Some(current) = self.selected_index {
                                if current < self.panes.len().saturating_sub(1) {
                                    self.selected_index = Some(current + 1);
                                }
                            }
                        }
                        "prev" => {
                            // Navigate to previous pane
                            if let Some(current) = self.selected_index {
                                if current > 0 {
                                    self.selected_index = Some(current - 1);
                                }
                            }
                        }
                        "refresh" => {
                            // Refresh pane list via background task
                            let tx = self.tx.lock().unwrap().clone();
                            tokio::spawn(async move {
                                if let Ok(panes) = Self::list_panes().await {
                                    if let Some(tx) = tx {
                                        let json = serde_json::json!({ "action": "update_panes", "panes": panes });
                                        let _ = tx.send(ModuleUpdate::Custom(json.to_string()));
                                    }
                                }
                            });
                        }
                        "update_panes" => {
                            if let Some(panes) = data.get("panes").and_then(|v| v.as_array()) {
                                self.panes = panes
                                    .iter()
                                    .filter_map(|p| {
                                        Some(TmuxPane {
                                            id: p.get("id")?.as_u64()? as usize,
                                            session_name: p
                                                .get("session_name")?
                                                .as_str()?
                                                .to_string(),
                                            window_index: p.get("window_index")?.as_u64()? as usize,
                                            pane_index: p.get("pane_index")?.as_u64()? as usize,
                                            current: p.get("current")?.as_bool().unwrap_or(false),
                                        })
                                    })
                                    .collect();
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
        Ok(())
    }

    async fn subscribe(&mut self) -> Result<Option<mpsc::UnboundedReceiver<ModuleUpdate>>> {
        let (tx, rx) = mpsc::unbounded_channel();
        *self.tx.lock().unwrap() = Some(tx.clone());

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
