use chrono::Local;
use unilii_core::{Module, ModuleConfig, ModuleUpdate, Result};
use iced::{Element, widget::{text, container}, Length, Alignment};

pub struct Clock {
    format: String,
    current_time: String,
}

#[async_trait::async_trait]
impl Module for Clock {
    async fn new(_config: &ModuleConfig) -> Result<Self> {
        Ok(Self {
            format: "%H:%M:%S".to_string(),
            current_time: String::new(),
        })
    }

    fn name(&self) -> &str {
        "clock"
    }

    fn view(&self) -> Element<'_, ModuleUpdate> {
        container(text(&self.current_time).size(14))
            .width(Length::Shrink)
            .padding(4)
            .align_x(Alignment::Center)
            .into()
    }

    fn update(&mut self, _message: ModuleUpdate) -> Result<()> {
        Ok(())
    }

    async fn subscribe(&mut self) -> Result<Option<tokio::sync::mpsc::UnboundedReceiver<ModuleUpdate>>> {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let format = self.format.clone();

        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(1));
            loop {
                interval.tick().await;
                let time_str = Local::now().format(&format).to_string();
                let _ = tx.send(ModuleUpdate::Text(time_str));
            }
        });

        Ok(Some(rx))
    }

    fn update_interval(&self) -> Option<u64> {
        Some(1000)
    }
}
