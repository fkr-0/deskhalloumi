//! System statistics widget and snapshot collection.

use super::{Widget, WidgetMessage};
use iced::widget::text;
use iced::{Color, Element};
use std::fs;
use std::process::Command;

#[derive(Debug, Clone, PartialEq)]
pub struct SystemStatsSnapshot {
    pub cpu_percent: Option<f32>,
    pub memory_percent: Option<f32>,
    pub load_average: [f32; 3],
    pub uptime_seconds: u64,
    pub root_disk_percent: Option<u8>,
}

impl Default for SystemStatsSnapshot {
    fn default() -> Self {
        Self {
            cpu_percent: None,
            memory_percent: None,
            load_average: [0.0; 3],
            uptime_seconds: 0,
            root_disk_percent: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CpuCounters {
    busy: u64,
    total: u64,
}

#[derive(Debug)]
pub struct SysMonitor {
    snapshot: SystemStatsSnapshot,
    previous_cpu: Option<CpuCounters>,
}

impl SysMonitor {
    pub fn new() -> Self {
        Self {
            snapshot: SystemStatsSnapshot::default(),
            previous_cpu: None,
        }
    }

    pub fn snapshot(&self) -> &SystemStatsSnapshot {
        &self.snapshot
    }

    pub fn compact_label(&self) -> String {
        format!(
            "CPU {} RAM {}",
            percent_label(self.snapshot.cpu_percent),
            percent_label(self.snapshot.memory_percent)
        )
    }

    pub fn update_stats(&mut self) {
        if let Ok(stat) = fs::read_to_string("/proc/stat")
            && let Some(current) = parse_cpu_counters(&stat)
        {
            self.snapshot.cpu_percent = self.previous_cpu.and_then(|previous| {
                let total = current.total.saturating_sub(previous.total);
                let busy = current.busy.saturating_sub(previous.busy);
                (total > 0).then_some((busy as f32 * 100.0 / total as f32).clamp(0.0, 100.0))
            });
            self.previous_cpu = Some(current);
        }
        if let Ok(meminfo) = fs::read_to_string("/proc/meminfo") {
            self.snapshot.memory_percent = parse_memory_percent(&meminfo);
        }
        if let Ok(loadavg) = fs::read_to_string("/proc/loadavg") {
            self.snapshot.load_average = parse_load_average(&loadavg).unwrap_or([0.0; 3]);
        }
        if let Ok(uptime) = fs::read_to_string("/proc/uptime") {
            self.snapshot.uptime_seconds = parse_uptime_seconds(&uptime).unwrap_or(0);
        }
        self.snapshot.root_disk_percent = read_root_disk_percent();
    }
}

fn percent_label(value: Option<f32>) -> String {
    value
        .map(|value| format!("{value:.0}%"))
        .unwrap_or_else(|| "--".to_string())
}

fn parse_cpu_counters(input: &str) -> Option<CpuCounters> {
    let line = input.lines().find(|line| line.starts_with("cpu "))?;
    let values = line
        .split_whitespace()
        .skip(1)
        .filter_map(|value| value.parse::<u64>().ok())
        .collect::<Vec<_>>();
    if values.len() < 4 {
        return None;
    }
    let idle = values.get(3).copied().unwrap_or(0) + values.get(4).copied().unwrap_or(0);
    let total = values.iter().copied().sum::<u64>();
    Some(CpuCounters {
        busy: total.saturating_sub(idle),
        total,
    })
}

fn parse_memory_percent(input: &str) -> Option<f32> {
    let mut total = None;
    let mut available = None;
    for line in input.lines() {
        let mut parts = line.split_whitespace();
        match parts.next()? {
            "MemTotal:" => total = parts.next()?.parse::<f32>().ok(),
            "MemAvailable:" => available = parts.next()?.parse::<f32>().ok(),
            _ => {}
        }
    }
    let total = total?;
    let available = available?;
    (total > 0.0).then_some(((total - available) * 100.0 / total).clamp(0.0, 100.0))
}

fn parse_load_average(input: &str) -> Option<[f32; 3]> {
    let values = input
        .split_whitespace()
        .take(3)
        .map(str::parse::<f32>)
        .collect::<Result<Vec<_>, _>>()
        .ok()?;
    Some([*values.first()?, *values.get(1)?, *values.get(2)?])
}

fn parse_uptime_seconds(input: &str) -> Option<u64> {
    input
        .split_whitespace()
        .next()?
        .parse::<f64>()
        .ok()
        .map(|value| value as u64)
}

fn read_root_disk_percent() -> Option<u8> {
    let output = Command::new("df").args(["-P", "/"]).output().ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .nth(1)?
        .split_whitespace()
        .nth(4)?
        .trim_end_matches('%')
        .parse::<u8>()
        .ok()
}

pub fn format_uptime(seconds: u64) -> String {
    let days = seconds / 86_400;
    let hours = (seconds % 86_400) / 3_600;
    let minutes = (seconds % 3_600) / 60;
    if days > 0 {
        format!("{days}d {hours}h")
    } else if hours > 0 {
        format!("{hours}h {minutes}m")
    } else {
        format!("{minutes}m")
    }
}

impl Widget for SysMonitor {
    fn name(&self) -> &str {
        "sysmonitor"
    }

    fn view(&self) -> Element<'_, WidgetMessage> {
        text(self.compact_label())
            .size(11)
            .color(Color::WHITE)
            .into()
    }

    fn update(&mut self, message: WidgetMessage) {
        if let WidgetMessage::SysMonitor(stats) = message {
            let parts = stats.split('|').map(str::trim).collect::<Vec<_>>();
            if let Some(cpu) = parts
                .first()
                .and_then(|part| part.trim_end_matches('%').parse().ok())
            {
                self.snapshot.cpu_percent = Some(cpu);
            }
            if let Some(memory) = parts
                .get(1)
                .and_then(|part| part.trim_end_matches('%').parse().ok())
            {
                self.snapshot.memory_percent = Some(memory);
            }
        }
    }

    fn update_interval(&self) -> Option<u64> {
        Some(2_000)
    }
}

impl Default for SysMonitor {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_proc_stat_and_memory() {
        let cpu = parse_cpu_counters("cpu  10 2 5 80 3 0 0 0 0 0\n").unwrap();
        assert_eq!(cpu.total, 100);
        assert_eq!(cpu.busy, 17);
        let memory = parse_memory_percent("MemTotal: 1000 kB\nMemAvailable: 250 kB\n").unwrap();
        assert_eq!(memory, 75.0);
    }

    #[test]
    fn parses_load_and_formats_uptime() {
        assert_eq!(
            parse_load_average("1.25 0.75 0.50 1/100 42"),
            Some([1.25, 0.75, 0.5])
        );
        assert_eq!(format_uptime(90), "1m");
        assert_eq!(format_uptime(90_000), "1d 1h");
    }
}
