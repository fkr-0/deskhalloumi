use chrono::Local;
use deskhalloumi_core::{
    Module, ModuleConfig, ModuleUpdate, Result,
    runtime::{ModuleSubscription, ProviderContract, ProviderRefreshPolicy},
};
use iced::{Element, widget::text};
use std::{sync::Arc, time::Duration};

pub trait ClockSource: Send + Sync {
    fn formatted_now(&self, format: &str) -> String;
}

#[derive(Debug, Default)]
pub struct SystemClockSource;

impl ClockSource for SystemClockSource {
    fn formatted_now(&self, format: &str) -> String {
        Local::now().format(format).to_string()
    }
}

#[derive(Debug, Clone)]
pub struct FixedClockSource {
    value: String,
}

impl FixedClockSource {
    pub fn new(value: impl Into<String>) -> Self {
        Self {
            value: value.into(),
        }
    }
}

impl ClockSource for FixedClockSource {
    fn formatted_now(&self, _format: &str) -> String {
        self.value.clone()
    }
}

pub struct Clock {
    format: String,
    current_time: String,
    source: Arc<dyn ClockSource>,
}

impl Clock {
    pub fn with_source(source: Arc<dyn ClockSource>) -> Self {
        let format = "%H:%M:%S".to_string();
        let current_time = source.formatted_now(&format);
        Self {
            format,
            current_time,
            source,
        }
    }
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
        "FixedClockSource",
    )
}

#[async_trait::async_trait]
impl Module for Clock {
    async fn new(_config: &ModuleConfig) -> Result<Self> {
        Ok(Self::with_source(Arc::new(SystemClockSource)))
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
        let source = Arc::clone(&self.source);

        Ok(Some(ModuleSubscription::with_contract(
            provider_contract(),
            move |updates| async move {
                let mut interval = tokio::time::interval(Duration::from_secs(1));
                interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
                loop {
                    interval.tick().await;
                    let time_str = source.formatted_now(&format);
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
    fn fixed_clock_source_controls_initial_state_without_wall_clock_access() {
        let clock = Clock::with_source(Arc::new(FixedClockSource::new("12:34:56")));
        assert_eq!(clock.current_time, "12:34:56");
    }

    #[test]
    fn lifecycle_contract_names_executable_fixture_source() {
        let contract = provider_contract();
        assert_eq!(contract.id, "clock");
        assert_eq!(contract.refresh.interval, Duration::from_secs(1));
        assert_eq!(contract.test_backend, "FixedClockSource");
    }
}
