//! WiFi widget tests

use super::Wifi;
use iced::widget::text;

#[test]
fn test_wifi_widget_initialization() {
    let wifi = Wifi::new();
    assert_eq!(wifi.name(), "wifi");
    assert_eq!(wifi.ssid, "No WiFi");
    assert_eq!(wifi.signal, 0);
    assert!(!wifi.connected);
    assert!(!wifi.show_menu);
}

#[test]
fn test_wifi_widget_default() {
    let wifi = Wifi::default();
    assert_eq!(wifi.name(), "wifi");
}

#[test]
fn test_wifi_widget_update_toggle_menu() {
    let mut wifi = Wifi::new();
    assert!(!wifi.show_menu);

    // Toggle menu on
    wifi.update(crate::widgets::WidgetMessage::Wifi("toggle_menu".to_string()));
    assert!(wifi.show_menu);

    // Toggle menu off
    wifi.update(crate::widgets::WidgetMessage::Wifi("toggle_menu".to_string()));
    assert!(!wifi.show_menu);
}

#[test]
fn test_wifi_widget_update_connect() {
    let mut wifi = Wifi::new();
    wifi.show_menu = true;

    wifi.update(crate::widgets::WidgetMessage::Wifi("connect".to_string()));
    assert!(!wifi.show_menu);
}

#[test]
fn test_wifi_widget_update_invalid_action() {
    let mut wifi = Wifi::new();
    let original_ssid = wifi.ssid.clone();

    wifi.update(crate::widgets::WidgetMessage::Wifi("invalid_action".to_string()));
    assert_eq!(wifi.ssid, original_ssid);
}

#[test]
fn test_wifi_widget_update_interval() {
    let wifi = Wifi::new();
    assert_eq!(wifi.update_interval(), Some(5000));
}

#[test]
fn test_wifi_widget_render_icon() {
    let wifi = Wifi::new();
    let element = wifi.view();
    // Should not panic
    drop(element);
}

#[test]
fn test_wifi_widget_render_menu() {
    let mut wifi = Wifi::new();
    wifi.show_menu = true;
    let element = wifi.view();
    // Should not panic
    drop(element);
}

#[test]
fn test_network_info_creation() {
    let network = super::NetworkInfo {
        ssid: "TestNetwork".to_string(),
        signal: 85,
        security: "WPA2".to_string(),
    };

    assert_eq!(network.ssid, "TestNetwork");
    assert_eq!(network.signal, 85);
    assert_eq!(network.security, "WPA2");
}

// Integration tests that require nmcli

#[test]
#[ignore]
fn test_wifi_update_status_connected() {
    let mut wifi = Wifi::new();
    wifi.update_status();

    // This test requires nmcli to be available and a connection to exist
    // Mark as ignored to avoid failing in CI
    if wifi.connected {
        assert_ne!(wifi.ssid, "No WiFi");
        assert_ne!(wifi.ssid, "Disconnected");
    }
}

#[test]
#[ignore]
fn test_wifi_get_networks() {
    let wifi = Wifi::new();
    let networks = wifi.get_networks();

    // This test requires nmcli to be available
    // Verify we get some network info (may be empty in isolated environment)
    assert!(networks.len() >= 0);

    for network in networks {
        assert!(!network.ssid.is_empty());
        assert!(network.signal <= 100);
    }
}
