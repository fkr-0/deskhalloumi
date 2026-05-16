use super::common::{FilterableMenu, QuickjumpMenu};
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

pub fn build_connect_command(ssid: &str) -> String {
    format!(
        "nmcli device wifi connect \"{}\"",
        ssid.replace('"', "\\\"")
    )
}

pub fn build_forget_command(name: &str) -> String {
    format!("nmcli connection delete \"{}\"", name.replace('"', "\\\""))
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
}
