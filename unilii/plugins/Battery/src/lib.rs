use unilii_core::{Module, ModuleConfig, ModuleUpdate, Result};
use iced::{widget::text, Element};

pub struct Battery {
    config: ModuleConfig,
}

#[async_trait::async_trait]
impl Module for Battery {
    async fn new(config: &ModuleConfig) -> Result<Self>
    where
        Self: Sized,
    {
        Ok(Battery {
            config: config.clone(),
        })
    }

    fn name(&self) -> &str {
        "battery"
    }

    fn view(&self) -> Element<'_, ModuleUpdate> {
        text("BAT: 100%").into()
    }

    fn update(&mut self, _message: ModuleUpdate) -> Result<()> {
        Ok(())
    }
}
