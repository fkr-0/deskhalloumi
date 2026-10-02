//! Renderer-neutral window identity, state, capability, and control contracts.
//!
//! Window titles and application identifiers are presentation metadata only.
//! Control operations must route through an opaque provider-issued id and an
//! explicit capability check. Compositor/WM adapters own the translation
//! between this contract and i3/X11, wlroots foreign-toplevel, or other native
//! control protocols.

use serde::{Deserialize, Serialize};
use std::{error::Error, fmt};

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct OpaqueWindowId(String);

impl OpaqueWindowId {
    pub fn new(value: impl Into<String>) -> Result<Self, WindowControlError> {
        let value = value.into();
        if value.is_empty() {
            return Err(WindowControlError::EmptyOpaqueId);
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for OpaqueWindowId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WindowCapabilities {
    pub activate: bool,
    pub fullscreen: bool,
    pub close: bool,
}

impl WindowCapabilities {
    pub const fn read_only() -> Self {
        Self {
            activate: false,
            fullscreen: false,
            close: false,
        }
    }

    pub const fn full_control() -> Self {
        Self {
            activate: true,
            fullscreen: true,
            close: true,
        }
    }

    pub const fn allows(self, action: WindowControlAction) -> bool {
        match action {
            WindowControlAction::Activate => self.activate,
            WindowControlAction::SetFullscreen(_) => self.fullscreen,
            WindowControlAction::Close => self.close,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WindowState {
    pub focused: bool,
    pub fullscreen: bool,
    pub urgent: bool,
    pub floating: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WindowEntry {
    pub id: OpaqueWindowId,
    pub title: String,
    pub app_id: Option<String>,
    pub state: WindowState,
    pub capabilities: WindowCapabilities,
}

impl WindowEntry {
    pub fn request(
        &self,
        action: WindowControlAction,
    ) -> Result<WindowControlRequest, WindowControlError> {
        if !self.capabilities.allows(action) {
            return Err(WindowControlError::UnsupportedCapability { action });
        }
        Ok(WindowControlRequest {
            id: self.id.clone(),
            action,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WindowControlAction {
    Activate,
    SetFullscreen(bool),
    Close,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WindowControlRequest {
    pub id: OpaqueWindowId,
    pub action: WindowControlAction,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowControlError {
    EmptyOpaqueId,
    UnsupportedCapability { action: WindowControlAction },
}

impl fmt::Display for WindowControlError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyOpaqueId => f.write_str("window control id must not be empty"),
            Self::UnsupportedCapability { action } => {
                write!(f, "window does not advertise capability for {action:?}")
            }
        }
    }
}

impl Error for WindowControlError {}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(id: &str, title: &str, app_id: &str, capabilities: WindowCapabilities) -> WindowEntry {
        WindowEntry {
            id: OpaqueWindowId::new(id).unwrap(),
            title: title.into(),
            app_id: Some(app_id.into()),
            state: WindowState::default(),
            capabilities,
        }
    }

    #[test]
    fn titles_and_app_ids_are_metadata_not_control_identity() {
        let mut window = entry(
            "i3:con:412",
            "Editor",
            "org.example.Editor",
            WindowCapabilities::full_control(),
        );
        let before = window.request(WindowControlAction::Activate).unwrap();

        window.title = "Renamed document".into();
        window.app_id = Some("org.example.Editor.Next".into());
        let after = window.request(WindowControlAction::Activate).unwrap();

        assert_eq!(before.id.as_str(), "i3:con:412");
        assert_eq!(before.id, after.id);
    }

    #[test]
    fn duplicate_app_ids_remain_independently_addressable() {
        let first = entry(
            "i3:con:41",
            "Terminal A",
            "org.example.Terminal",
            WindowCapabilities::full_control(),
        );
        let second = entry(
            "i3:con:42",
            "Terminal B",
            "org.example.Terminal",
            WindowCapabilities::full_control(),
        );

        assert_eq!(first.app_id, second.app_id);
        assert_ne!(first.id, second.id);
    }

    #[test]
    fn read_only_entries_fail_closed_for_control() {
        let window = entry(
            "ext:handle:7",
            "Observed window",
            "org.example.App",
            WindowCapabilities::read_only(),
        );

        assert_eq!(
            window.request(WindowControlAction::Close),
            Err(WindowControlError::UnsupportedCapability {
                action: WindowControlAction::Close,
            })
        );
    }

    #[test]
    fn fullscreen_requests_are_explicit_not_implicit_toggles() {
        let window = entry(
            "wlr:handle:9",
            "Player",
            "org.example.Player",
            WindowCapabilities::full_control(),
        );

        assert_eq!(
            window
                .request(WindowControlAction::SetFullscreen(true))
                .unwrap()
                .action,
            WindowControlAction::SetFullscreen(true)
        );
    }

    #[test]
    fn empty_provider_identity_is_rejected() {
        assert_eq!(
            OpaqueWindowId::new(""),
            Err(WindowControlError::EmptyOpaqueId)
        );
    }
}
