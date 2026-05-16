use std::fs;
use std::process::Command;

use chrono::{DateTime, Utc};

use super::{CalendarEvent, CalendarProvider, provider::CalendarError};

#[derive(Debug, Clone)]
pub struct CalDavProviderConfig {
    pub account_id: String,
    pub base_url: String,
    pub principal_url: String,
    pub calendar_url: String,
    pub username: String,
    pub secret_ref: String,
}

#[derive(Debug, Clone)]
pub struct CalDavProvider {
    pub config: CalDavProviderConfig,
}

impl CalDavProvider {
    pub fn new(config: CalDavProviderConfig) -> Self {
        Self { config }
    }
}

impl CalendarProvider for CalDavProvider {
    fn fetch_events(
        &self,
        _account_id: &str,
        window_start_rfc3339: &str,
        window_end_rfc3339: &str,
        _sync_token: Option<&str>,
    ) -> Result<(Vec<CalendarEvent>, Option<String>), CalendarError> {
        let secret = resolve_secret(&self.config.secret_ref)?;
        let query_body = build_calendar_query_report_body(window_start_rfc3339, window_end_rfc3339)
            .ok_or_else(|| {
                CalendarError::Backend(format!(
                    "invalid sync window for {}",
                    self.config.account_id
                ))
            })?;

        let output = Command::new("curl")
            .arg("-fsSL")
            .arg("--connect-timeout")
            .arg("12")
            .arg("--max-time")
            .arg("20")
            .arg("-X")
            .arg("REPORT")
            .arg("-H")
            .arg("Depth: 1")
            .arg("-H")
            .arg("Content-Type: application/xml; charset=utf-8")
            .arg("-u")
            .arg(format!("{}:{}", self.config.username, secret))
            .arg("--data")
            .arg(query_body)
            .arg(&self.config.calendar_url)
            .output()
            .map_err(|error| CalendarError::Backend(format!("failed to run curl: {}", error)))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            return Err(CalendarError::Backend(format!(
                "curl failed for {}: {}",
                self.config.calendar_url, stderr
            )));
        }

        let body = String::from_utf8_lossy(&output.stdout).to_string();

        let mut events = if body.contains("BEGIN:VCALENDAR") {
            parse_ics_events(&body, &self.config.account_id)
        } else {
            // CalDAV REPORT responses often embed iCalendar chunks in calendar-data tags.
            extract_calendar_data_blocks(&body)
                .into_iter()
                .flat_map(|ics| parse_ics_events(&ics, &self.config.account_id))
                .collect::<Vec<_>>()
        };

        events.sort_by(|left, right| {
            left.start_rfc3339
                .cmp(&right.start_rfc3339)
                .then(left.title.cmp(&right.title))
        });

        Ok((events, None))
    }
}

fn resolve_secret(secret_ref: &str) -> Result<String, CalendarError> {
    if let Some(env_name) = secret_ref.strip_prefix("env:") {
        return std::env::var(env_name).map_err(|error| {
            CalendarError::Backend(format!("missing secret env {}: {}", env_name, error))
        });
    }
    if let Some(path) = secret_ref.strip_prefix("file:") {
        return fs::read_to_string(path)
            .map(|value| value.trim().to_string())
            .map_err(|error| {
                CalendarError::Backend(format!("failed to read secret file {}: {}", path, error))
            });
    }
    Ok(secret_ref.to_string())
}

fn build_calendar_query_report_body(
    window_start_rfc3339: &str,
    window_end_rfc3339: &str,
) -> Option<String> {
    let start = rfc3339_to_caldav_utc(window_start_rfc3339)?;
    let end = rfc3339_to_caldav_utc(window_end_rfc3339)?;
    Some(format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<C:calendar-query xmlns:D="DAV:" xmlns:C="urn:ietf:params:xml:ns:caldav">
  <D:prop>
    <D:getetag />
    <C:calendar-data />
  </D:prop>
  <C:filter>
    <C:comp-filter name="VCALENDAR">
      <C:comp-filter name="VEVENT">
        <C:time-range start="{start}" end="{end}" />
      </C:comp-filter>
    </C:comp-filter>
  </C:filter>
</C:calendar-query>"#,
    ))
}

fn rfc3339_to_caldav_utc(value: &str) -> Option<String> {
    let parsed = DateTime::parse_from_rfc3339(value).ok()?;
    Some(
        parsed
            .with_timezone(&Utc)
            .format("%Y%m%dT%H%M%SZ")
            .to_string(),
    )
}

fn extract_calendar_data_blocks(xml: &str) -> Vec<String> {
    let mut blocks = Vec::new();
    let mut rest = xml;

    loop {
        let Some(start) = rest.find("<calendar-data") else {
            break;
        };
        let Some(tag_end_rel) = rest[start..].find('>') else {
            break;
        };
        let content_start = start + tag_end_rel + 1;
        let close_tag = "</calendar-data>";
        let Some(content_end_rel) = rest[content_start..].find(close_tag) else {
            break;
        };
        let content_end = content_start + content_end_rel;
        blocks.push(html_unescape(&rest[content_start..content_end]));
        rest = &rest[content_end + close_tag.len()..];
    }

    blocks
}

fn parse_ics_events(ics: &str, account_id: &str) -> Vec<CalendarEvent> {
    let normalized = unfold_ics_lines(ics);
    let mut events = Vec::new();
    let mut current: Vec<&str> = Vec::new();
    let mut in_event = false;

    for line in normalized.lines() {
        if line.trim() == "BEGIN:VEVENT" {
            in_event = true;
            current.clear();
            continue;
        }
        if line.trim() == "END:VEVENT" {
            if let Some(event) = build_event_from_lines(&current, account_id) {
                events.push(event);
            }
            in_event = false;
            current.clear();
            continue;
        }
        if in_event {
            current.push(line);
        }
    }

    events
}

fn unfold_ics_lines(input: &str) -> String {
    let mut out = String::new();
    for line in input.replace("\r\n", "\n").lines() {
        if line.starts_with(' ') || line.starts_with('\t') {
            out.push_str(line.trim_start());
        } else {
            if !out.is_empty() {
                out.push('\n');
            }
            out.push_str(line);
        }
    }
    out
}

fn build_event_from_lines(lines: &[&str], account_id: &str) -> Option<CalendarEvent> {
    let uid = field_value(lines, "UID")?;
    let summary = field_value(lines, "SUMMARY").unwrap_or_else(|| "(untitled)".to_string());
    let start = field_value(lines, "DTSTART")?;
    let end = field_value(lines, "DTEND").unwrap_or_else(|| start.clone());
    let location = field_value(lines, "LOCATION");
    let all_day = start.len() == 8 && start.chars().all(|c| c.is_ascii_digit());

    Some(CalendarEvent {
        id: uid,
        account_id: account_id.to_string(),
        title: summary,
        start_rfc3339: ics_datetime_to_rfc3339(&start),
        end_rfc3339: ics_datetime_to_rfc3339(&end),
        location,
        all_day,
    })
}

fn field_value(lines: &[&str], key: &str) -> Option<String> {
    for line in lines {
        if !line.starts_with(key) {
            continue;
        }
        let (_, value) = line.split_once(':')?;
        return Some(html_unescape(value.trim()));
    }
    None
}

fn ics_datetime_to_rfc3339(raw: &str) -> String {
    let value = raw.trim();
    if value.len() == 8 && value.chars().all(|c| c.is_ascii_digit()) {
        return format!(
            "{}-{}-{}T00:00:00Z",
            &value[0..4],
            &value[4..6],
            &value[6..8]
        );
    }
    if value.len() >= 15 && value.chars().nth(8) == Some('T') {
        let date = &value[0..8];
        let time = &value[9..15];
        return format!(
            "{}-{}-{}T{}:{}:{}Z",
            &date[0..4],
            &date[4..6],
            &date[6..8],
            &time[0..2],
            &time[2..4],
            &time[4..6]
        );
    }
    value.to_string()
}

fn html_unescape(input: &str) -> String {
    input
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
}

#[cfg(test)]
mod tests {
    use super::{
        build_calendar_query_report_body, extract_calendar_data_blocks, ics_datetime_to_rfc3339,
        parse_ics_events, rfc3339_to_caldav_utc,
    };

    #[test]
    fn parses_basic_ics_event() {
        let ics = "BEGIN:VCALENDAR\nBEGIN:VEVENT\nUID:ev-1\nSUMMARY:Standup\nDTSTART:20260416T090000Z\nDTEND:20260416T093000Z\nLOCATION:Room 1\nEND:VEVENT\nEND:VCALENDAR\n";
        let events = parse_ics_events(ics, "acc");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].id, "ev-1");
        assert_eq!(events[0].title, "Standup");
        assert_eq!(events[0].start_rfc3339, "2026-04-16T09:00:00Z");
    }

    #[test]
    fn normalizes_ics_datetime_values_to_utc_shape() {
        assert_eq!(
            ics_datetime_to_rfc3339("20260416T090000Z"),
            "2026-04-16T09:00:00Z"
        );
        assert_eq!(
            ics_datetime_to_rfc3339("20260416T090000"),
            "2026-04-16T09:00:00Z"
        );
        assert_eq!(ics_datetime_to_rfc3339("20260416"), "2026-04-16T00:00:00Z");
    }

    #[test]
    fn extracts_calendar_data_from_xml() {
        let xml = "<multistatus><response><calendar-data>BEGIN:VCALENDAR\nBEGIN:VEVENT\nUID:1\nSUMMARY:A\nDTSTART:20260416T100000Z\nEND:VEVENT\nEND:VCALENDAR</calendar-data></response></multistatus>";
        let blocks = extract_calendar_data_blocks(xml);
        assert_eq!(blocks.len(), 1);
        let events = parse_ics_events(&blocks[0], "acc");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].title, "A");
    }

    #[test]
    fn converts_rfc3339_to_caldav_utc() {
        assert_eq!(
            rfc3339_to_caldav_utc("2026-04-17T10:12:13+00:00").as_deref(),
            Some("20260417T101213Z")
        );
    }

    #[test]
    fn report_body_contains_time_range() {
        let body = build_calendar_query_report_body(
            "2026-04-17T10:12:13+00:00",
            "2026-04-20T01:02:03+00:00",
        )
        .expect("query body should build");

        assert!(body.contains("calendar-query"));
        assert!(body.contains("start=\"20260417T101213Z\""));
        assert!(body.contains("end=\"20260420T010203Z\""));
    }
}
