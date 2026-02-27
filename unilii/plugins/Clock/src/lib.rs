use unilii_core::{Module, ModuleConfig, ModuleUpdate, Result};
use iced::{widget::text, Element};
use chrono::Local;

pub struct Clock {
    config: ModuleConfig,
}

#[async_trait::async_trait]
impl Module for Clock {
    async fn new(config: &ModuleConfig) -> Result<Self>
    where
        Self: Sized,
    {
        Ok(Clock {
            config: config.clone(),
        })
    }

    fn name(&self) -> &str {
        "clock"
    }

    fn view(&self) -> Element<'_, ModuleUpdate> {
        text(Local::now().format("%H:%M:%S").to_string()).into()
    }

    fn update(&mut self, _message: ModuleUpdate) -> Result<()> {
        Ok(())
    }
}
