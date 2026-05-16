//! System monitor widget implementation

use super::{Widget, WidgetMessage};
use iced::widget::text;
use iced::{Color, Element};
use std::process::Command;

#[derive(Debug)]
pub struct SysMonitor {
    cpu_usage: String,
    memory_usage: String,
}

impl SysMonitor {
    pub fn new() -> Self {
        Self {
            cpu_usage: "CPU: --%".to_string(),
            memory_usage: "RAM: --%".to_string(),
        }
    }

    pub fn update_stats(&mut self) {
        // Simple CPU usage estimation from /proc/stat
        if let Ok(output) = Command::new("sh")
            .args(["-c", "grep 'cpu ' /proc/stat | awk '{usage=($2+$4)*100/($2+$4+$5)} END {print usage\"%\"}'"])
            .output()
        {
            let cpu = String::from_utf8_lossy(&output.stdout).trim().to_string();
            self.cpu_usage = format!("CPU: {}", cpu);
        }

        // Memory usage from /proc/meminfo
        if let Ok(output) = Command::new("sh")
            .args([
                "-c",
                "free | grep Mem | awk '{printf \"RAM: %.0f\\n\", ($3/$2) * 100.0}'",
            ])
            .output()
        {
            let mem = String::from_utf8_lossy(&output.stdout).trim().to_string();
            self.memory_usage = format!("{}%", mem);
        }
    }
}

impl Widget for SysMonitor {
    fn name(&self) -> &str {
        "sysmonitor"
    }

    fn view(&self) -> Element<'_, WidgetMessage> {
        text(format!("{} | {}", self.cpu_usage, self.memory_usage))
            .size(11)
            .color(Color::WHITE)
            .into()
    }

    fn update(&mut self, message: WidgetMessage) {
        if let WidgetMessage::SysMonitor(stats) = message {
            let parts: Vec<&str> = stats.split('|').collect();
            if parts.len() >= 2 {
                self.cpu_usage = parts[0].trim().to_string();
                self.memory_usage = parts[1].trim().to_string();
            }
        }
    }

    fn update_interval(&self) -> Option<u64> {
        Some(2000)
    }
}

impl Default for SysMonitor {
    fn default() -> Self {
        Self::new()
    }
}
