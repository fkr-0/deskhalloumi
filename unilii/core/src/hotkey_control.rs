//! User-scoped control protocol for the standalone hotkey daemon.
//!
//! The protocol is deliberately small and line-delimited JSON. The Unix socket
//! lives below the same private runtime directory used by menu and singleton
//! records, so control commands are restricted to the current desktop user.

use crate::menu_process::{MenuStatus, default_runtime_dir};
use serde::{Deserialize, Serialize};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::time::Duration;

pub const HOTKEY_CONTROL_PROTOCOL_VERSION: u16 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "command", rename_all = "snake_case")]
pub enum HotkeyControlRequest {
    Ping,
    Status,
    Reload,
    Shutdown,
    Menu { action: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HotkeyRuntimeStatus {
    pub protocol_version: u16,
    pub pid: u32,
    pub backend: String,
    pub generation: u64,
    pub binding_count: usize,
    pub managed_menu_count: usize,
    pub shadow: bool,
    pub grab: bool,
    pub started_at_unix_ms: u128,
    pub loaded_at_unix_ms: u128,
    pub config_sources: Vec<String>,
    pub last_reload_error: Option<String>,
    pub menus: Vec<MenuStatus>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HotkeyControlResponse {
    pub ok: bool,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<HotkeyRuntimeStatus>,
}

impl HotkeyControlResponse {
    pub fn ok(message: impl Into<String>) -> Self {
        Self {
            ok: true,
            message: message.into(),
            status: None,
        }
    }

    pub fn error(message: impl Into<String>) -> Self {
        Self {
            ok: false,
            message: message.into(),
            status: None,
        }
    }

    pub fn with_status(mut self, status: HotkeyRuntimeStatus) -> Self {
        self.status = Some(status);
        self
    }
}

pub fn default_control_socket_path() -> PathBuf {
    default_runtime_dir().join("hotkeyd.sock")
}

pub fn send_control_request(
    socket_path: impl AsRef<Path>,
    request: &HotkeyControlRequest,
) -> Result<HotkeyControlResponse, String> {
    let socket_path = socket_path.as_ref();
    let mut stream = UnixStream::connect(socket_path).map_err(|error| {
        format!(
            "failed to connect to hotkeyd control socket '{}': {error}",
            socket_path.display()
        )
    })?;
    let timeout = Some(Duration::from_secs(10));
    stream
        .set_read_timeout(timeout)
        .map_err(|error| error.to_string())?;
    stream
        .set_write_timeout(timeout)
        .map_err(|error| error.to_string())?;

    let mut payload = serde_json::to_vec(request).map_err(|error| error.to_string())?;
    payload.push(b'\n');
    stream.write_all(&payload).map_err(|error| {
        format!(
            "failed to write hotkeyd request to '{}': {error}",
            socket_path.display()
        )
    })?;
    stream.flush().map_err(|error| error.to_string())?;

    let mut line = String::new();
    BufReader::new(stream)
        .read_line(&mut line)
        .map_err(|error| format!("failed to read hotkeyd response: {error}"))?;
    if line.trim().is_empty() {
        return Err("hotkeyd returned an empty control response".to_string());
    }
    serde_json::from_str(line.trim())
        .map_err(|error| format!("invalid hotkeyd control response: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::net::UnixListener;
    use std::thread;

    #[test]
    fn control_client_round_trips_json_line_protocol() {
        let temp = tempfile::tempdir().unwrap();
        let socket = temp.path().join("control.sock");
        let listener = UnixListener::bind(&socket).unwrap();
        let server = thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            let request: HotkeyControlRequest = serde_json::from_str(line.trim()).unwrap();
            assert_eq!(request, HotkeyControlRequest::Ping);
            let response = HotkeyControlResponse::ok("pong");
            let mut writer = stream;
            writeln!(writer, "{}", serde_json::to_string(&response).unwrap()).unwrap();
        });

        let response = send_control_request(&socket, &HotkeyControlRequest::Ping).unwrap();
        assert!(response.ok);
        assert_eq!(response.message, "pong");
        server.join().unwrap();
    }
}
