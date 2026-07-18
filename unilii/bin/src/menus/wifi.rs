#![allow(dead_code)]
// FIXME(T6): WiFi menu model is planned toolbar/menu integration surface pending canonical MenuModel wiring.

use super::common::{FilterableMenu, QuickjumpMenu};
use super::presentation::{ActionItemOptions, action_item, section_item, status_item};
use crate::enhanced_tray::{TrayMenuAction, TrayMenuItem};
use crate::tray::NetworkSnapshot;

#[derive(Debug, Clone)]
pub struct WifiMenuConfig {
    pub max_network_rows: usize,
    pub show_known_networks: bool,
    pub max_known_rows: usize,
    pub settings_command: String,
}

impl Default for WifiMenuConfig {
    fn default() -> Self {
        Self {
            max_network_rows: 12,
            show_known_networks: true,
            max_known_rows: 8,
            settings_command: "nm-connection-editor".to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WifiNetworkRow {
    pub ssid: String,
    pub signal: u8,
    pub security: String,
    pub connected: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnownNetworkRow {
    pub name: String,
    pub autoconnect: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct WifiMenuViewModel {
    pub interface: Option<String>,
    pub enabled: bool,
    pub status_text: String,
    pub available: Vec<WifiNetworkRow>,
    pub known: Vec<KnownNetworkRow>,
}

impl FilterableMenu for WifiMenuViewModel {
    type ItemId = String;

    fn filter_tokens_for(&self, item_id: &Self::ItemId) -> Vec<String> {
        if let Some(network) = self.available.iter().find(|row| &row.ssid == item_id) {
            return vec![
                network.ssid.clone(),
                network.security.clone(),
                network.signal.to_string(),
            ];
        }
        if let Some(known) = self.known.iter().find(|row| &row.name == item_id) {
            return vec![known.name.clone(), "known".to_string()];
        }
        Vec::new()
    }
}

impl QuickjumpMenu for WifiMenuViewModel {
    type ItemId = String;

    fn quickjump_targets(&self) -> Vec<Self::ItemId> {
        self.available
            .iter()
            .map(|row| row.ssid.clone())
            .chain(self.known.iter().map(|row| row.name.clone()))
            .collect()
    }
}

pub fn build_view_model(
    snapshot: Option<&NetworkSnapshot>,
    loading: bool,
    error: Option<&str>,
    config: &WifiMenuConfig,
) -> WifiMenuViewModel {
    if loading {
        return WifiMenuViewModel {
            status_text: "Loading…".to_string(),
            ..WifiMenuViewModel::default()
        };
    }

    if let Some(err) = error {
        return WifiMenuViewModel {
            status_text: format!("Error: {}", err),
            ..WifiMenuViewModel::default()
        };
    }

    let Some(snapshot) = snapshot else {
        return WifiMenuViewModel {
            status_text: "No network snapshot".to_string(),
            ..WifiMenuViewModel::default()
        };
    };

    let mut available = snapshot
        .networks
        .iter()
        .map(|network| WifiNetworkRow {
            ssid: network.ssid.clone(),
            signal: network.signal,
            security: network.security.clone(),
            connected: snapshot
                .connected_ssid
                .as_ref()
                .is_some_and(|ssid| ssid == &network.ssid),
        })
        .collect::<Vec<_>>();
    available.sort_by(|left, right| {
        right
            .connected
            .cmp(&left.connected)
            .then(right.signal.cmp(&left.signal))
            .then(left.ssid.cmp(&right.ssid))
    });
    available.truncate(config.max_network_rows);

    let mut known = if config.show_known_networks {
        snapshot
            .known_networks
            .iter()
            .map(|network| KnownNetworkRow {
                name: network.name.clone(),
                autoconnect: network.autoconnect,
            })
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };
    known.sort_by(|left, right| {
        right
            .autoconnect
            .cmp(&left.autoconnect)
            .then(left.name.cmp(&right.name))
    });
    known.truncate(config.max_known_rows);

    let status_text = if !snapshot.enabled {
        "Wi-Fi disabled".to_string()
    } else if let Some(ssid) = &snapshot.connected_ssid {
        format!("Connected to {}", ssid)
    } else {
        format!("State: {}", snapshot.state)
    };

    WifiMenuViewModel {
        interface: Some(snapshot.interface.clone()),
        enabled: snapshot.enabled,
        status_text,
        available,
        known,
    }
}

impl From<&deskhalloumi_core::config::WifiMenuConfig> for WifiMenuConfig {
    fn from(config: &deskhalloumi_core::config::WifiMenuConfig) -> Self {
        Self {
            max_network_rows: config.max_network_rows,
            show_known_networks: config.show_known_networks,
            max_known_rows: config.max_network_rows,
            settings_command: config.settings_command.clone(),
        }
    }
}

pub fn signal_glyph(signal: u8) -> &'static str {
    match signal {
        75..=u8::MAX => "▂▄▆█",
        50..=74 => "▂▄▆·",
        25..=49 => "▂▄··",
        _ => "▂···",
    }
}

pub fn build_menu_items(
    app_id: &str,
    snapshot: Option<&NetworkSnapshot>,
    loading: bool,
    error: Option<&str>,
    config: &deskhalloumi_core::config::WifiMenuConfig,
) -> Vec<TrayMenuItem> {
    let view_config = WifiMenuConfig::from(config);
    let view = build_view_model(snapshot, loading, error, &view_config);
    let mut items = vec![
        action_item(
            app_id,
            "wifi-toggle",
            if view.enabled {
                "Disable Wi-Fi"
            } else {
                "Enable Wi-Fi"
            },
            TrayMenuAction::SpawnCommand(if view.enabled {
                "nmcli radio wifi off".to_string()
            } else {
                "nmcli radio wifi on".to_string()
            }),
            ActionItemOptions {
                subtitle: Some("Turn the wireless radio on or off".to_string()),
                icon: Some(
                    if view.enabled {
                        "network-wireless-disabled"
                    } else {
                        "network-wireless"
                    }
                    .to_string(),
                ),
                shortcut: None,
                enabled: true,
            },
        ),
        action_item(
            app_id,
            "wifi-refresh",
            "Rescan networks",
            TrayMenuAction::SpawnCommand("nmcli device wifi rescan".to_string()),
            ActionItemOptions {
                subtitle: Some("Refresh access points and signal levels".to_string()),
                icon: Some("view-refresh".to_string()),
                shortcut: Some("R".to_string()),
                enabled: view.enabled,
            },
        ),
        action_item(
            app_id,
            "wifi-settings",
            "Network settings",
            TrayMenuAction::SpawnCommand(config.settings_command.clone()),
            ActionItemOptions {
                subtitle: Some("Open the configured connection editor".to_string()),
                icon: Some("preferences-system-network".to_string()),
                shortcut: None,
                enabled: !config.settings_command.trim().is_empty(),
            },
        ),
        status_item(
            app_id,
            "wifi-state",
            view.status_text.clone(),
            view.interface
                .as_ref()
                .map(|interface| format!("Interface {interface}")),
        ),
    ];

    if loading || error.is_some() || snapshot.is_none() || !view.enabled {
        return items;
    }

    items.push(section_item(
        app_id,
        "wifi-available",
        "Available networks",
        Some(view.available.len()),
    ));
    if view.available.is_empty() {
        items.push(status_item(
            app_id,
            "wifi-empty",
            "No access points found",
            Some("Rescan or move within range of a wireless network".to_string()),
        ));
    } else {
        for network in &view.available {
            let security = if network.security.trim().is_empty() || network.security == "--" {
                "Open network".to_string()
            } else {
                network.security.clone()
            };
            let subtitle = format!(
                "{} · {}% · {}{}",
                signal_glyph(network.signal),
                network.signal,
                security,
                if network.connected {
                    " · Connected"
                } else {
                    ""
                }
            );
            items.push(action_item(
                app_id,
                format!("wifi-network:{}", network.ssid),
                network.ssid.clone(),
                TrayMenuAction::SpawnCommand(build_connect_command(&network.ssid)),
                ActionItemOptions {
                    subtitle: Some(subtitle),
                    icon: Some(
                        if network.connected {
                            "network-wireless-connected"
                        } else {
                            "network-wireless"
                        }
                        .to_string(),
                    ),
                    shortcut: network.connected.then(|| "Connected".to_string()),
                    enabled: !network.connected,
                },
            ));
        }
    }

    if config.show_known_networks {
        items.push(section_item(
            app_id,
            "wifi-known",
            "Saved connections",
            Some(view.known.len()),
        ));
        if view.known.is_empty() {
            items.push(status_item(
                app_id,
                "wifi-known-empty",
                "No saved Wi-Fi connections",
                None,
            ));
        }
        for known in &view.known {
            items.push(action_item(
                app_id,
                format!("wifi-known-connect:{}", known.name),
                known.name.clone(),
                TrayMenuAction::SpawnCommand(build_known_connect_command(&known.name)),
                ActionItemOptions {
                    subtitle: Some(if known.autoconnect {
                        "Saved profile · connects automatically".to_string()
                    } else {
                        "Saved profile".to_string()
                    }),
                    icon: Some("network-wireless".to_string()),
                    shortcut: Some("Connect".to_string()),
                    enabled: true,
                },
            ));
            if config.allow_forget {
                items.push(action_item(
                    app_id,
                    format!("wifi-known-forget:{}", known.name),
                    format!("Forget {}", known.name),
                    TrayMenuAction::SpawnCommand(build_forget_command(&known.name)),
                    ActionItemOptions {
                        subtitle: Some("Remove the saved NetworkManager profile".to_string()),
                        icon: Some("edit-delete".to_string()),
                        shortcut: Some("Forget".to_string()),
                        enabled: true,
                    },
                ));
            }
        }
    }
    items
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

pub fn build_connect_command(ssid: &str) -> String {
    format!("nmcli device wifi connect {}", shell_quote(ssid))
}

pub fn build_known_connect_command(name: &str) -> String {
    format!("nmcli connection up id {}", shell_quote(name))
}

pub fn build_forget_command(name: &str) -> String {
    format!("nmcli connection delete {}", shell_quote(name))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tray::{KnownNetwork, NetworkSnapshot, WifiNetwork};

    #[test]
    fn view_model_marks_connected_network_and_sorts() {
        let snapshot = NetworkSnapshot {
            interface: "wlp0s20f3".to_string(),
            state: "connected".to_string(),
            enabled: true,
            connected_ssid: Some("Home".to_string()),
            known_networks: vec![
                KnownNetwork {
                    name: "Cafe".to_string(),
                    autoconnect: false,
                },
                KnownNetwork {
                    name: "Home".to_string(),
                    autoconnect: true,
                },
            ],
            networks: vec![
                WifiNetwork {
                    ssid: "Cafe".to_string(),
                    signal: 90,
                    security: "WPA2".to_string(),
                },
                WifiNetwork {
                    ssid: "Home".to_string(),
                    signal: 40,
                    security: "WPA2".to_string(),
                },
            ],
        };

        let vm = build_view_model(Some(&snapshot), false, None, &WifiMenuConfig::default());
        assert_eq!(
            vm.available.first().map(|row| row.ssid.as_str()),
            Some("Home")
        );
        assert_eq!(vm.known.first().map(|row| row.name.as_str()), Some("Home"));
    }

    #[test]
    fn command_builders_shell_quote_untrusted_names() {
        let command = build_connect_command("Cafe '$(touch /tmp/no)'");
        assert!(command.starts_with("nmcli device wifi connect '"));
        assert!(command.contains("'\\''"));
        assert!(command.ends_with('\''));
        assert_eq!(
            build_known_connect_command("Office WiFi"),
            "nmcli connection up id 'Office WiFi'"
        );
    }
    #[test]
    fn built_items_keep_controls_sections_and_actions_in_one_order() {
        let snapshot = NetworkSnapshot {
            interface: "wlan0".into(),
            state: "connected".into(),
            enabled: true,
            connected_ssid: Some("Home".into()),
            known_networks: vec![KnownNetwork {
                name: "Home".into(),
                autoconnect: true,
            }],
            networks: vec![WifiNetwork {
                ssid: "Home".into(),
                signal: 81,
                security: "WPA2".into(),
            }],
        };
        let items = build_menu_items(
            "network",
            Some(&snapshot),
            false,
            None,
            &deskhalloumi_core::config::WifiMenuConfig::default(),
        );
        assert_eq!(items[0].id, "wifi-toggle");
        assert!(items.iter().any(|item| item.id == "section:wifi-available"));
        assert!(
            items
                .iter()
                .any(|item| item.id == "wifi-known-connect:Home")
        );
        assert!(items.iter().any(|item| item.id == "wifi-known-forget:Home"));
    }
}
