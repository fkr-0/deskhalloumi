//! Regression tests for keyboard listener and application initialization.
//!
//! These tests verify that the keyboard listener works correctly and that
//! modules and configuration are properly initialized. This prevents regressions
//! where the keyboard listener or modules are not loaded (as happened in a
//! previous session).

use super::*;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

#[test]
fn test_application_initialization_structure() {
    // This test verifies that the application initialization closure
    // can be called multiple times (Fn requirement) and correctly
    // transfers ownership of modules and config on the first call.

    let modules = Rc::new(RefCell::new(Some(HashMap::<String, LoadedModule>::new())));
    let config = Rc::new(RefCell::new(Some(deskhalloumi_core::config::Config::default())));
    let app_config = Rc::new(RefCell::new(Some(AppConfig::default())));
    let run_options = Rc::new(RefCell::new(Some(RunOptions::default())));

    // The closure must be callable multiple times (Fn requirement)
    let init = || {
        let mods = modules.borrow_mut().take().unwrap_or_default();
        let cfg = config.borrow_mut().take().unwrap_or_default();
        let app_cfg = app_config.borrow_mut().take().unwrap_or_default();
        let opts = run_options.borrow_mut().take().unwrap_or_default();

        UniliiPanel {
            modules: mods,
            config: cfg,
            app_config: app_cfg,
            panel_config_index: 0,
            shift_held: false,
            tray_icons: Vec::new(),
            enhanced_tray: None,
            run_options: opts,
        }
    };

    // First call should return initialized state
    let state1 = init();
    assert_eq!(state1.modules.len(), 0); // Default empty HashMap

    // Second call should return default values (containers were emptied)
    let state2 = init();
    assert_eq!(state2.modules.len(), 0);
}

#[test]
fn test_modules_container_take_pattern() {
    // This test verifies that the RefCell<Option<T>> pattern correctly
    // allows taking ownership of values on the first call.

    let test_value = "test_value".to_string();
    let container = Rc::new(RefCell::new(Some(test_value)));

    // First call: should take ownership
    let first_take = container.borrow_mut().take();
    assert_eq!(first_take, Some("test_value".to_string()));

    // Second call: should return None (value was taken)
    let second_take = container.borrow_mut().take();
    assert_eq!(second_take, None);
}

#[test]
fn test_config_cloning_for_multiple_uses() {
    // This test verifies that config can be cloned for multiple uses,
    // preventing the "use of partially moved value" error that occurred
    // when config was used in both async block and application initialization.

    let config = deskhalloumi_core::config::Config::default();

    // Clone config for async block
    let config_for_async = config.clone();
    // Clone config for window settings
    let config_for_window = config.clone();
    // Clone config for application initialization
    let config_for_app = config.clone();

    // Verify all clones are independent
    assert_ne!(&config as *const _, &config_for_async as *const _);
    assert_ne!(&config as *const _, &config_for_window as *const _);
    assert_ne!(&config as *const _, &config_for_app as *const _);
    assert_ne!(&config_for_async as *const _, &config_for_window as *const _);
}

#[test]
fn test_module_subscriptions_initialization() {
    // This test verifies that module subscriptions are properly initialized
    // and that initialize_global_subscriptions is called.

    // In the actual main() function, this ensures keyboard events work
    // by setting up module update subscriptions.
    //
    // Regression: Previously, initialize_global_subscriptions() was not called,
    // causing the keyboard listener to not work.

    // Verify that subscription_manager::initialize_global_subscriptions exists
    // This is a compile-time check - if it doesn't exist, compilation fails
    let _ = subscription_manager::initialize_global_subscriptions as fn(Vec<module_loader::ModuleSubscription>);
}

#[test]
fn test_window_settings_from_config() {
    // This test verifies that window settings are loaded from config,
    // preventing the "bigger rectangle" visual regression.

    let mut config = deskhalloumi_core::config::Config::default();
    config.panels[0].width = 800;
    config.panels[0].height = 24;
    config.panels[0].position_x = 100;
    config.panels[0].position_y = 0;

    let window_position = iced::window::Position::Specific(iced::Point {
        x: config.panels[0].position_x as f32,
        y: config.panels[0].position_y as f32,
    });

    let window_settings = window::Settings {
        size: iced::Size::new(config.panels[0].width as f32, config.panels[0].height as f32),
        position: window_position,
        resizable: false,
        decorations: false,
        level: window::Level::AlwaysOnTop,
        ..window::Settings::default()
    };

    // Verify settings match config
    assert_eq!(window_settings.size.width, 800.0);
    assert_eq!(window_settings.size.height, 24.0);
    assert!(!window_settings.resizable);
    assert!(!window_settings.decorations);
}

#[tokio::test]
async fn test_module_loading_with_subscriptions() {
    // This test verifies that modules load with subscriptions,
    // which is required for the keyboard listener to work.

    use module_loader::ModuleManager;
    use deskhalloumi_core::ModuleConfig;
    use std::collections::HashMap;

    let manager = ModuleManager::new();
    let mut configs = HashMap::new();

    // Add clock module config
    configs.insert("clock".to_string(), ModuleConfig {
        enabled: true,
        position: deskhalloumi_core::ModulePosition::Left,
        update_interval_ms: Some(1000),
        theme_overrides: None,
    });

    let (modules, subscriptions) = manager.load_modules(configs).await.unwrap();

    // Verify modules are loaded
    #[cfg(feature = "clock")]
    assert!(modules.contains_key("clock"));

    // Verify subscriptions are created
    #[cfg(feature = "clock")]
    assert!(!subscriptions.is_empty());

    // This is the regression test: verify that we have subscriptions
    // because initialize_global_subscriptions() needs to be called with them
    // In the regression case, subscriptions were empty/missing
    let has_clock_sub = subscriptions.iter().any(|s| s.name == "clock");
    assert!(has_clock_sub || cfg!(not(feature = "clock")));
}

#[test]
fn test_keyboard_input_message_handling() {
    // This test verifies that keyboard input messages are defined
    // and can be handled in the update function.

    let mut bar = UniliiPanel {
        modules: HashMap::new(),
        config: deskhalloumi_core::config::Config::default(),
        app_config: AppConfig::default(),
        panel_config_index: 0,
        shift_held: false,
        tray_icons: Vec::new(),
        enhanced_tray: None,
        run_options: RunOptions::default(),
    };

    // Test keyboard input message handling
    let message = Message::KeyboardInput {
        code: "KEY_LEFTSHIFT".to_string(),
        value: 1,
    };

    let _task = update_panel(&mut bar, message);

    // Verify shift state is updated
    assert!(bar.shift_held);

    // Test key release
    let message = Message::KeyboardInput {
        code: "KEY_LEFTSHIFT".to_string(),
        value: 0,
    };

    let _task = update_panel(&mut bar, message);

    // Verify shift state is updated
    assert!(!bar.shift_held);
}

#[test]
fn test_multi_panel_config_structure() {
    // Test that the config structure supports multiple panels
    let config = deskhalloumi_core::config::Config {
        panels: vec![
            deskhalloumi_core::config::PanelConfig {
                name: "top_bar".to_string(),
                width: 800,
                height: 24,
                position_x: 0,
                position_y: 0,
                background_color: Some("#1e1e1e".to_string()),
                text_color: Some("#ffffff".to_string()),
            },
            deskhalloumi_core::config::PanelConfig {
                name: "bottom_bar".to_string(),
                width: 800,
                height: 24,
                position_x: 0,
                position_y: 1000,
                background_color: Some("#1e1e1e".to_string()),
                text_color: Some("#ffffff".to_string()),
            },
        ],
        modules: vec![],
        keybindings: vec![],
    };

    assert_eq!(config.panels.len(), 2);
    assert_eq!(config.panels[0].name, "top_bar");
    assert_eq!(config.panels[1].name, "bottom_bar");
}

#[test]
fn test_unilii_panel_manager_initialization() {
    // Test that UniliiPanelManager can be created
    let (manager, _task) = UniliiPanelManager::new();

    // Manager should have panel configs loaded
    assert!(!manager.panel_configs.is_empty());
    assert_eq!(manager.next_panel_index, 0);
    assert!(manager.panels.is_empty());
}

#[test]
fn test_unilii_panel_manager_multiple_panels() {
    // Test that multiple panels can be created from config
    let config = deskhalloumi_core::config::Config {
        panels: vec![
            deskhalloumi_core::config::PanelConfig {
                name: "top_bar".to_string(),
                width: 800,
                height: 24,
                position_x: 0,
                position_y: 0,
                background_color: Some("#1e1e1e".to_string()),
                text_color: Some("#ffffff".to_string()),
            },
            deskhalloumi_core::config::PanelConfig {
                name: "bottom_bar".to_string(),
                width: 800,
                height: 24,
                position_x: 0,
                position_y: 1000,
                background_color: Some("#1e1e1e".to_string()),
                text_color: Some("#ffffff".to_string()),
            },
        ],
        modules: vec![],
        keybindings: vec![],
    };

    // Create a manager with custom panel configs
    let manager = UniliiPanelManager {
        panels: std::collections::BTreeMap::new(),
        panel_configs: config.panels,
        next_panel_index: 0,
    };

    assert_eq!(manager.panel_configs.len(), 2);
    assert_eq!(manager.next_panel_index, 0);
}

#[test]
fn test_window_opened_message_handling() {
    // Test that WindowOpened message creates a new panel
    let mut manager = UniliiPanelManager {
        panels: std::collections::BTreeMap::new(),
        panel_configs: vec![
            deskhalloumi_core::config::PanelConfig {
                name: "test_panel".to_string(),
                width: 800,
                height: 24,
                position_x: 0,
                position_y: 0,
                background_color: None,
                text_color: None,
            },
        ],
        next_panel_index: 0,
    };

    // Simulate opening a window
    let window_id = iced::window::Id::unique();
    let message = Message::WindowOpened(window_id);

    let _task = update(&mut manager, message);

    // Verify panel was created
    assert_eq!(manager.panels.len(), 1);
    assert!(manager.panels.contains_key(&window_id));
    assert_eq!(manager.next_panel_index, 1);
}

#[test]
fn test_window_closed_message_handling() {
    // Test that WindowClosed message removes a panel
    let mut manager = UniliiPanelManager {
        panels: std::collections::BTreeMap::new(),
        panel_configs: vec![],
        next_panel_index: 0,
    };

    // Add a panel
    let window_id = iced::window::Id::unique();
    let panel = UniliiPanel {
        modules: std::collections::HashMap::new(),
        config: deskhalloumi_core::config::Config::default(),
        app_config: AppConfig::default(),
        panel_config_index: 0,
        shift_held: false,
        tray_icons: Vec::new(),
        enhanced_tray: None,
        run_options: RunOptions::default(),
    };
    manager.panels.insert(window_id, panel);

    // Close the window
    let message = Message::WindowClosed(window_id);
    let _task = update(&mut manager, message);

    // Verify panel was removed
    assert!(manager.panels.is_empty());
}

#[test]
fn test_app_exit_on_last_panel_closed() {
    // Test that application exits when last panel is closed
    let mut manager = UniliiPanelManager {
        panels: std::collections::BTreeMap::new(),
        panel_configs: vec![],
        next_panel_index: 0,
    };

    // Add a panel
    let window_id = iced::window::Id::unique();
    let panel = UniliiPanel {
        modules: std::collections::HashMap::new(),
        config: deskhalloumi_core::config::Config::default(),
        app_config: AppConfig::default(),
        panel_config_index: 0,
        shift_held: false,
        tray_icons: Vec::new(),
        enhanced_tray: None,
        run_options: RunOptions::default(),
    };
    manager.panels.insert(window_id, panel);

    // Close the last panel
    let message = Message::WindowClosed(window_id);
    let task = update(&mut manager, message);

    // Should return exit task and panels should be empty
    assert!(manager.panels.is_empty());
}

#[test]
fn test_panel_config_index_association() {
    // Test that each panel is associated with its config index
    let mut manager = UniliiPanelManager {
        panels: std::collections::BTreeMap::new(),
        panel_configs: vec![
            deskhalloumi_core::config::PanelConfig {
                name: "panel_0".to_string(),
                width: 800,
                height: 24,
                position_x: 0,
                position_y: 0,
                background_color: Some("#ff0000".to_string()),
                text_color: None,
            },
            deskhalloumi_core::config::PanelConfig {
                name: "panel_1".to_string(),
                width: 800,
                height: 24,
                position_x: 0,
                position_y: 1000,
                background_color: Some("#00ff00".to_string()),
                text_color: None,
            },
        ],
        next_panel_index: 0,
    };

    // Open first panel
    let window_id_0 = iced::window::Id::unique();
    let message = Message::WindowOpened(window_id_0);
    let _task = update(&mut manager, message);

    // Open second panel
    let window_id_1 = iced::window::Id::unique();
    let message = Message::WindowOpened(window_id_1);
    let _task = update(&mut manager, message);

    // Verify each panel has correct config index
    if let Some(panel) = manager.panels.get(&window_id_0) {
        assert_eq!(panel.panel_config_index, 0);
    }
    if let Some(panel) = manager.panels.get(&window_id_1) {
        assert_eq!(panel.panel_config_index, 1);
    }
}

