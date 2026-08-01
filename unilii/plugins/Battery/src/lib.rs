use deskhalloumi_core::{
    Module, ModuleConfig, ModuleUpdate, Result,
    runtime::{ModuleSubscription, ProviderContract, ProviderRefreshPolicy},
};
use deskhalloumi_lib::sysfs::power::{BatteryPowerDevice, PowerDevice, PowerDeviceKind};
use iced::{
    Alignment, Element, Length,
    widget::{container, row, text},
};
use std::{sync::Arc, time::Duration};

#[async_trait::async_trait]
pub trait BatterySource: Send + Sync {
    async fn read_charge(&self) -> std::result::Result<f32, String>;
}

#[derive(Clone)]
pub struct SysfsBatterySource {
    device: BatteryPowerDevice,
}

impl SysfsBatterySource {
    pub async fn discover() -> std::result::Result<Self, String> {
        let devices = PowerDevice::read_all()
            .await
            .map_err(|error| error.to_string())?;
        let device = devices
            .into_iter()
            .find(|device| device.kind == PowerDeviceKind::Battery)
            .ok_or_else(|| "No battery device found".to_string())?;
        Ok(Self {
            device: BatteryPowerDevice(device),
        })
    }
}

#[async_trait::async_trait]
impl BatterySource for SysfsBatterySource {
    async fn read_charge(&self) -> std::result::Result<f32, String> {
        self.device
            .read_charge()
            .await
            .map(|charge| charge as f32)
            .map_err(|error| error.to_string())
    }
}

#[derive(Debug, Clone)]
pub struct FixedBatterySource {
    charge: f32,
}

impl FixedBatterySource {
    pub fn new(charge: f32) -> Self {
        Self {
            charge: charge.clamp(0.0, 1.0),
        }
    }
}

#[async_trait::async_trait]
impl BatterySource for FixedBatterySource {
    async fn read_charge(&self) -> std::result::Result<f32, String> {
        Ok(self.charge)
    }
}

pub struct Battery {
    percentage: f32,
    is_charging: bool,
    name: String,
    source: Arc<dyn BatterySource>,
}

impl Battery {
    pub async fn with_source(source: Arc<dyn BatterySource>) -> Result<Self> {
        let charge = source.read_charge().await?;
        Ok(Self {
            percentage: charge.clamp(0.0, 1.0) * 100.0,
            is_charging: false,
            name: "battery".to_string(),
            source,
        })
    }
}

pub fn provider_contract() -> ProviderContract {
    ProviderContract::new(
        "battery",
        "Battery",
        ProviderRefreshPolicy {
            interval: Duration::from_secs(5),
            timeout: Duration::from_secs(2),
            stale_after: Duration::from_secs(20),
            refresh_on_start: true,
        },
        "FixedBatterySource",
    )
}

fn battery_status_label(percentage: f32, is_charging: bool) -> String {
    let icon = if is_charging { "\u{26A1}" } else { "\u{1F50B}" };
    format!("{icon} {}%", percentage as i32)
}

#[async_trait::async_trait]
impl Module for Battery {
    async fn new(_config: &ModuleConfig) -> Result<Self>
    where
        Self: Sized,
    {
        Self::with_source(Arc::new(SysfsBatterySource::discover().await?)).await
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn view(&self) -> Element<'_, ModuleUpdate> {
        let label = battery_status_label(self.percentage, self.is_charging);
        let text_elem = text(label).size(12).color(iced::Color::WHITE);

        container(row![text_elem].spacing(8).align_y(Alignment::Center))
            .width(Length::Shrink)
            .padding(4)
            .align_x(Alignment::Center)
            .into()
    }

    fn update(&mut self, message: ModuleUpdate) -> Result<()> {
        match message {
            ModuleUpdate::Text(text) => {
                if let Some(pct_str) = text.strip_suffix('%')
                    && let Ok(pct) = pct_str.parse::<f32>()
                {
                    self.percentage = pct;
                }
            }
            ModuleUpdate::ProgressBar(value) => {
                self.percentage = value.clamp(0.0, 1.0) * 100.0;
            }
            ModuleUpdate::Icon(icon) => {
                self.is_charging = icon == "charging";
            }
            _ => {}
        }
        Ok(())
    }

    async fn subscribe(&mut self) -> Result<Option<ModuleSubscription>> {
        let source = Arc::clone(&self.source);
        Ok(Some(ModuleSubscription::with_contract(
            provider_contract(),
            move |updates| async move {
                let mut interval = tokio::time::interval(Duration::from_secs(5));
                interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
                loop {
                    interval.tick().await;
                    match source.read_charge().await {
                        Ok(charge) => {
                            if !updates.send(ModuleUpdate::ProgressBar(charge.clamp(0.0, 1.0))) {
                                break;
                            }
                        }
                        Err(error) => tracing::warn!(%error, "failed to refresh battery charge"),
                    }
                }
            },
        )))
    }

    fn update_interval(&self) -> Option<u64> {
        Some(5000)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn battery_label_discharging_compact() {
        assert_eq!(battery_status_label(73.9, false), "🔋 73%");
    }

    #[test]
    fn battery_label_charging_compact() {
        assert_eq!(battery_status_label(12.2, true), "⚡ 12%");
    }

    #[tokio::test]
    async fn fixed_source_constructs_without_sysfs_hardware() {
        let battery = Battery::with_source(Arc::new(FixedBatterySource::new(0.42)))
            .await
            .unwrap();
        assert_eq!(battery.percentage, 42.0);
    }

    #[test]
    fn lifecycle_contract_names_executable_fixture_source() {
        let contract = provider_contract();
        assert_eq!(contract.id, "battery");
        assert_eq!(contract.test_backend, "FixedBatterySource");
    }
}
