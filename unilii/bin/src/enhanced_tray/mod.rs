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

// Re-export core types for convenience
pub use core::{
    EnhancedTrayState, TrayEvent, TrayIcon, TrayMenuAction, TrayMenuItem, TrayMenuNavigation,
    TrayMenuTree, TrayViewState, TrayWidgetType,
};

// Re-export from legacy tray for compatibility
pub use crate::tray::{is_network_icon, read_network_snapshot, set_wifi_enabled, spawn_command};

// Re-export dbus functions for menu fetching and invocation
pub use dbus::{convert_dbus_to_tray_menu, fetch_dbus_menu, invoke_dbus_menu_action};
