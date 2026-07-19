use chrono::Local;
use deskhalloumi_core::{
    Module, ModuleConfig, ModuleUpdate, Result,
    runtime::{ModuleSubscription, ProviderContract, ProviderRefreshPolicy},
};
use iced::{Element, widget::text};
use std::time::Duration;

pub struct Clock {
    format: String,
    current_time: String,
}

pub fn provider_contract() -> ProviderContract {
    ProviderContract::new(
        "clock",
        "Clock",
        ProviderRefreshPolicy {
            interval: Duration::from_secs(1),
            timeout: Duration::from_millis(250),
            stale_after: Duration::from_secs(3),
            refresh_on_start: true,
        },
        "TestProviderBackend<String>",
    )
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

        Ok(Some(ModuleSubscription::with_contract(
            provider_contract(),
            move |updates| async move {
                let mut interval = tokio::time::interval(Duration::from_secs(1));
                interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
                loop {
                    interval.tick().await;
                    let time_str = Local::now().format(&format).to_string();
                    if !updates.send(ModuleUpdate::Text(time_str)) {
                        break;
                    }
                }
            },
        )))
    }

    fn update_interval(&self) -> Option<u64> {
        Some(1000)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lifecycle_contract_has_hardware_free_test_backend() {
        let contract = provider_contract();
        assert_eq!(contract.id, "clock");
        assert_eq!(contract.refresh.interval, Duration::from_secs(1));
        assert!(contract.test_backend.contains("TestProviderBackend"));
    }
}
