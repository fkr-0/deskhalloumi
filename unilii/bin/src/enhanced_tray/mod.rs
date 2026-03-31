//! Enhanced tray system with hierarchical menus and DBus integration
//!
//! This module provides a completely refactored tray system following idiomatic
//! Iced 0.14 patterns for better maintainability and testing.

pub mod core;
pub mod dbus; 
pub mod rendering;
pub mod state;

// Include comprehensive DBus tests
#[cfg(test)]
mod dbus_tests;

// Re-export key types for convenience
pub use core::{
    TrayIcon, TrayMenuAction, TrayMenuItem, TrayApp, TrayMenuTree, 
    TrayViewState, TrayMenuNavigation, EnhancedTrayState, TrayEvent
};

// Re-export from legacy tray for compatibility
pub use crate::tray::{
    read_network_snapshot, set_wifi_enabled, 
    spawn_command, is_network_icon
};