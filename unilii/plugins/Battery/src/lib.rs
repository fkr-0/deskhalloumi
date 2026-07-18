use futures::StreamExt;

use deskhalloumi_core::{Module, ModuleConfig, ModuleUpdate, Result};
use deskhalloumi_lib::sysfs::power::{BatteryPowerDevice, PowerDevice, PowerDeviceKind};
use iced::{
    Alignment, Element, Length,
    widget::{container, row, text},
};

pub struct Battery {
    percentage: f32,
    is_charging: bool,
    name: String,
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
        // Find the first battery device
        let devices = PowerDevice::read_all().await?;
        let battery_device = devices
            .into_iter()
            .find(|d| d.kind == PowerDeviceKind::Battery)
            .ok_or("No battery device found")?;

        let device = BatteryPowerDevice(battery_device);

        // Read initial state
        let charge = device.read_charge().await.unwrap_or(1.0);
        let percentage = (charge * 100.0) as f32;

        Ok(Battery {
            percentage,
            is_charging: false,
            name: "battery".to_string(),
        })
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
                // Parse percentage from text
                if let Some(pct_str) = text.strip_suffix('%')
                    && let Ok(pct) = pct_str.parse::<f32>()
                {
                    self.percentage = pct;
                }
            }
            ModuleUpdate::ProgressBar(value) => {
                self.percentage = value * 100.0;
            }
            ModuleUpdate::Icon(icon) => {
                self.is_charging = icon == "charging";
            }
            _ => {}
        }
        Ok(())
    }

    async fn subscribe(
        &mut self,
    ) -> Result<Option<tokio::sync::mpsc::UnboundedReceiver<ModuleUpdate>>> {
        // Find the battery device again for the subscription
        let devices = PowerDevice::read_all().await?;
        let battery_device = devices
            .into_iter()
            .find(|d| d.kind == PowerDeviceKind::Battery)
            .ok_or("No battery device found")?;

        let device = BatteryPowerDevice(battery_device);

        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();

        tokio::spawn(async move {
            let stream = device.listen_charge(std::time::Duration::from_secs(5));

            futures::pin_mut!(stream);

            while let Some(charge) = StreamExt::next(&mut stream).await {
                let percentage = (charge * 100.0) as f32;
                let _ = tx.send(ModuleUpdate::ProgressBar(charge as f32));
                let _ = tx.send(ModuleUpdate::Text(format!("{}%", percentage as i32)));
            }
        });

        Ok(Some(rx))
    }

    fn update_interval(&self) -> Option<u64> {
        Some(5000)
    }
}

#[cfg(test)]
mod tests {
    use super::battery_status_label;

    #[test]
    fn battery_label_discharging_compact() {
        assert_eq!(battery_status_label(73.9, false), "🔋 73%");
    }

    #[test]
    fn battery_label_charging_compact() {
        assert_eq!(battery_status_label(12.2, true), "⚡ 12%");
    }
}
