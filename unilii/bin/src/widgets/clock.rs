//! Clock widget implementation

use super::{Widget, WidgetMessage};
use iced::widget::text;
use iced::{Color, Element, Length};
use chrono::Local;

#[derive(Debug)]
pub struct Clock {
    format: String,
    current_time: String,
}

impl Clock {
    pub fn new(format: String) -> Self {
        let current_time = Local::now().format(&format).to_string();
        Self {
            format,
            current_time,
        }
    }

    pub fn update_time(&mut self) {
        self.current_time = Local::now().format(&self.format).to_string();
    }
}

impl Widget for Clock {
    fn name(&self) -> &str {
        "clock"
    }

    fn view(&self) -> Element<'_, WidgetMessage> {
        text(&self.current_time)
            .size(14)
            .color(Color::WHITE)
            .into()
    }

    fn update(&mut self, message: WidgetMessage) {
        if let WidgetMessage::Clock(time) = message {
            self.current_time = time;
        }
    }

    fn update_interval(&self) -> Option<u64> {
        Some(1000)
    }
}

impl Default for Clock {
    fn default() -> Self {
        Self::new("%H:%M:%S".to_string())
    }
}
