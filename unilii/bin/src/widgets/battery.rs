//! Battery widget implementation

use super::{Widget, WidgetMessage};
use iced::widget::{row, text};
use iced::{Alignment, Color, Element, Length};

#[derive(Debug)]
pub struct Battery {
    percentage: f32,
    is_charging: bool,
}

impl Battery {
    pub fn new(percentage: f32, is_charging: bool) -> Self {
        Self {
            percentage,
            is_charging,
        }
    }

    fn battery_status_label(percentage: f32, is_charging: bool) -> String {
        let icon = if is_charging { "\u{26A1}" } else { "\u{1F50B}" };
        format!("{icon} {}%", percentage as i32)
    }
}

impl Widget for Battery {
    fn name(&self) -> &str {
        "battery"
    }

    fn view(&self) -> Element<'_, WidgetMessage> {
        let label = Self::battery_status_label(self.percentage, self.is_charging);
        let text_elem = text(label).size(12).color(Color::WHITE);

        row![text_elem]
            .spacing(8)
            .align_y(Alignment::Center)
            .into()
    }

    fn update(&mut self, message: WidgetMessage) {
        if let WidgetMessage::Battery(percentage, is_charging) = message {
            self.percentage = percentage;
            self.is_charging = is_charging;
        }
    }

    fn update_interval(&self) -> Option<u64> {
        Some(5000)
    }
}

impl Default for Battery {
    fn default() -> Self {
        Self::new(100.0, false)
    }
}
