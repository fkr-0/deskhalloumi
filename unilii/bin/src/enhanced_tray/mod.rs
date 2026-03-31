//! Enhanced tray system with hierarchical menus and DBus integration
//! 
//! This module provides a comprehensive tray menu system that follows idiomatic Iced 0.14 patterns:
//! - Declarative UI rendering based on state
//! - Single message enum for all events  
//! - Composable widget functions
//! - Proper state management

pub mod core;
pub mod dbus;
pub mod rendering;
pub mod state;

pub use core::*;
pub use dbus::*;
pub use rendering::*;
pub use state::*;

// Re-export from legacy tray for compatibility
pub use crate::tray::{
    read_network_snapshot, set_wifi_enabled, 
    spawn_command, is_network_icon,
    TrayIcon as LegacyTrayIcon
};