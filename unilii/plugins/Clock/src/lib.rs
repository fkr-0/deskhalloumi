use chrono::Local;
use deskhalloumi_core::{Module, ModuleConfig, ModuleUpdate, Result, runtime::ModuleSubscription};
use iced::{Element, widget::text};

pub struct Clock {
    format: String,
    current_time: String,
}

#[async_trait::async_trait]
impl Module for Clock {
    async fn new(_config: &ModuleConfig) -> Result<Self> {
        let format = "%H:%M:%S".to_string();
        let current_time = Local::now().format(&format).to_string();
        Ok(Self {
            format,
            current_time,
        })
    }

    fn name(&self) -> &str {
        "clock"
    }

    fn view(&self) -> Element<'_, ModuleUpdate> {
        text(&self.current_time)
            .size(14)
            .color(iced::Color::WHITE)
            .into()
    }

    fn update(&mut self, message: ModuleUpdate) -> Result<()> {
        if let ModuleUpdate::Text(time) = message {
            self.current_time = time;
        }
        Ok(())
    }

    async fn subscribe(&mut self) -> Result<Option<ModuleSubscription>> {
        let format = self.format.clone();

        Ok(Some(ModuleSubscription::new(move |updates| async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(1));
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                interval.tick().await;
                let time_str = Local::now().format(&format).to_string();
                if !updates.send(ModuleUpdate::Text(time_str)) {
                    break;
                }
            }
        })))
    }

    fn update_interval(&self) -> Option<u64> {
        Some(1000)
    }
}
