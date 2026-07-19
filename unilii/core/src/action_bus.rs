//! Versioned cross-process action protocol shared by hotkeys and the bar.

use crate::menu_process::default_runtime_dir;
use serde::{Deserialize, Serialize};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::time::Duration;

pub const ACTION_BUS_PROTOCOL_VERSION: u16 = 1;
pub const ACTION_BUS_MAX_FRAME_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "command", rename_all = "snake_case")]
pub enum DesktopAction {
    Shell(String),
    Menu(String),
    Bar(String),
    Tray(String),
    Widget(String),
}

impl DesktopAction {
    pub fn validate(&self) -> Result<(), String> {
        let command = match self {
            Self::Shell(command)
            | Self::Menu(command)
            | Self::Bar(command)
            | Self::Tray(command)
            | Self::Widget(command) => command,
        };
        if command.trim().is_empty() {
            return Err("action command cannot be empty".to_string());
        }
        if command.len() > 16 * 1024 {
            return Err("action command exceeds 16 KiB".to_string());
        }
        if command.contains('\0') {
            return Err("action command contains NUL".to_string());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActionBusRequest {
    pub protocol_version: u16,
    pub request_id: String,
    pub action: DesktopAction,
}

impl ActionBusRequest {
    pub fn new(request_id: impl Into<String>, action: DesktopAction) -> Self {
        Self {
            protocol_version: ACTION_BUS_PROTOCOL_VERSION,
            request_id: request_id.into(),
            action,
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.protocol_version != ACTION_BUS_PROTOCOL_VERSION {
            return Err(format!(
                "unsupported action protocol version {}; supported={}",
                self.protocol_version, ACTION_BUS_PROTOCOL_VERSION
            ));
        }
        if self.request_id.trim().is_empty() || self.request_id.len() > 256 {
            return Err("request_id must contain 1..=256 bytes".to_string());
        }
        self.action.validate()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActionBusResponse {
    pub protocol_version: u16,
    pub request_id: String,
    pub ok: bool,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

impl ActionBusResponse {
    pub fn ok(request_id: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            protocol_version: ACTION_BUS_PROTOCOL_VERSION,
            request_id: request_id.into(),
            ok: true,
            message: message.into(),
            data: None,
        }
    }

    pub fn ok_with_data(
        request_id: impl Into<String>,
        message: impl Into<String>,
        data: serde_json::Value,
    ) -> Self {
        Self {
            protocol_version: ACTION_BUS_PROTOCOL_VERSION,
            request_id: request_id.into(),
            ok: true,
            message: message.into(),
            data: Some(data),
        }
    }

    pub fn error(request_id: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            protocol_version: ACTION_BUS_PROTOCOL_VERSION,
            request_id: request_id.into(),
            ok: false,
            message: message.into(),
            data: None,
        }
    }
}

pub fn default_action_bus_socket_path() -> PathBuf {
    default_runtime_dir().join("action.sock")
}

pub fn send_action_request(
    socket_path: impl AsRef<Path>,
    request: &ActionBusRequest,
) -> Result<ActionBusResponse, String> {
    request.validate()?;
    let socket_path = socket_path.as_ref();
    let mut stream = UnixStream::connect(socket_path).map_err(|error| {
        format!(
            "desktop action receiver unavailable at '{}': {error}",
            socket_path.display()
        )
    })?;
    let timeout = Some(Duration::from_secs(3));
    stream
        .set_read_timeout(timeout)
        .map_err(|error| error.to_string())?;
    stream
        .set_write_timeout(timeout)
        .map_err(|error| error.to_string())?;
    let mut payload = serde_json::to_vec(request).map_err(|error| error.to_string())?;
    if payload.len() > ACTION_BUS_MAX_FRAME_BYTES {
        return Err("action request exceeds 64 KiB".to_string());
    }
    payload.push(b'\n');
    stream
        .write_all(&payload)
        .map_err(|error| format!("failed to write desktop action request: {error}"))?;
    stream.flush().map_err(|error| error.to_string())?;

    let mut line = String::new();
    BufReader::new(stream)
        .read_line(&mut line)
        .map_err(|error| format!("failed to read desktop action response: {error}"))?;
    if line.trim().is_empty() {
        return Err("desktop action receiver returned an empty response".to_string());
    }
    let response: ActionBusResponse = serde_json::from_str(line.trim())
        .map_err(|error| format!("invalid desktop action response: {error}"))?;
    if response.protocol_version != ACTION_BUS_PROTOCOL_VERSION {
        return Err(format!(
            "desktop action response protocol mismatch: received={}, supported={}",
            response.protocol_version, ACTION_BUS_PROTOCOL_VERSION
        ));
    }
    if response.request_id != request.request_id {
        return Err("desktop action response request_id mismatch".to_string());
    }
    Ok(response)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::net::UnixListener;
    use std::thread;

    #[test]
    fn action_client_round_trips_versioned_request() {
        let temp = tempfile::tempdir().unwrap();
        let socket = temp.path().join("action.sock");
        let listener = UnixListener::bind(&socket).unwrap();
        let server = thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut line = String::new();
            BufReader::new(stream.try_clone().unwrap())
                .read_line(&mut line)
                .unwrap();
            let request: ActionBusRequest = serde_json::from_str(line.trim()).unwrap();
            request.validate().unwrap();
            assert_eq!(request.action, DesktopAction::Tray("open-menu".into()));
            let mut writer = stream;
            writeln!(
                writer,
                "{}",
                serde_json::to_string(&ActionBusResponse::ok(request.request_id, "queued"))
                    .unwrap()
            )
            .unwrap();
        });
        let request = ActionBusRequest::new("test-1", DesktopAction::Tray("open-menu".into()));
        let response = send_action_request(&socket, &request).unwrap();
        assert!(response.ok);
        server.join().unwrap();
    }

    #[test]
    fn protocol_mismatch_is_rejected() {
        let mut request = ActionBusRequest::new("test-2", DesktopAction::Bar("reload".into()));
        request.protocol_version += 1;
        assert!(request.validate().unwrap_err().contains("unsupported"));
    }

    #[test]
    fn missing_receiver_returns_a_bounded_connection_error() {
        let temp = tempfile::tempdir().unwrap();
        let request = ActionBusRequest::new("test-3", DesktopAction::Bar("reload".into()));
        let error = send_action_request(temp.path().join("missing.sock"), &request).unwrap_err();
        assert!(error.contains("receiver unavailable"));
    }

    #[test]
    fn response_can_carry_structured_diagnostic_data() {
        let response = ActionBusResponse::ok_with_data(
            "metrics-1",
            "runtime metrics",
            serde_json::json!({"active_tasks": 3}),
        );
        let encoded = serde_json::to_string(&response).unwrap();
        let decoded: ActionBusResponse = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded.data.unwrap()["active_tasks"], 3);
    }
}
