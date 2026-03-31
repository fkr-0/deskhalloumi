mod app_config;
mod cli;
mod module_loader;
mod subscription_manager;
mod tray;
mod enhanced_tray; // New enhanced tray system

use app_config::{load_app_config, AppConfig};
use cli::{Cli, Commands, RunOptions, verbose_to_level};
use clap::Parser;
use iced::futures::{SinkExt, StreamExt};
use iced::keyboard::{key, Key, Modifiers};
use iced::widget::{button, column, container, row, text, Space};
use iced::{window, Element, Length, Subscription, Task};
use std::collections::HashMap;
use std::time::Duration;
use tracing::{error, info, warn};
use unilii_core::{config::load_config, keys::KeybindingDaemon, ModuleUpdate};

use module_loader::{LoadedModule, ModuleManager};
use subscription_manager::{initialize_global_subscriptions, get_latest_module_update, has_module_updates};

struct UniliiBar {
    modules: HashMap<String, LoadedModule>,
    config: unilii_core::config::Config,
    app_config: AppConfig,
    shift_held: bool,
    tray_icons: Vec<tray::TrayIcon>,
    enhanced_tray: Option<EnhancedTrayState>, // Enhanced tray state
    run_options: RunOptions,
}

// Enhanced tray state with hierarchical menu support
#[derive(Debug, Clone)]
struct EnhancedTrayState {
    tree: enhanced_tray::TrayMenuTree,
    current_view: TrayViewState,
    animation_progress: f32,
    animation_target: f32,
    selected_index: Option<usize>,
    filter_text: String,
}

// View modes for the enhanced tray system
#[derive(Debug, Clone)]
enum TrayViewState {
    SingleApp {
        app_id: String,
        navigation: enhanced_tray::TrayMenuNavigation,
    },
    Aggregated {
        items: Vec<enhanced_tray::TrayMenuItem>,
        filter: Option<String>,
    },
    Favorites {
        items: Vec<enhanced_tray::TrayMenuItem>,
    },
    Network {
        app_id: String,
        data: Option<crate::tray::NetworkSnapshot>,
        loading: bool,
        error: Option<String>,
    },
}

#[derive(Debug, Clone)]
enum Message {
    ModuleUpdate(String, ModuleUpdate),
    KeyboardInput {
        code: String,
        value: i32,
    },
    WindowKeyboardInput {
        key: String,
        pressed: bool,
        is_shift: bool,
    },
    // Enhanced tray events
    EnhancedTrayEvent(enhanced_tray::TrayEvent),
    TrayIconPressed(String),
    TrayMenuTriggered(String, enhanced_tray::TrayMenuAction),
    TrayNavigateLeft,
    TrayNavigateRight,
    TrayShowAggregated,
    TrayShowFavorites,
    TrayToggleFavorite(String, String), // (app_id, item_id)
    TrayFilterUpdate(String),
    TrayNetworkSnapshot(String, Result<tray::NetworkSnapshot, String>),
    TrayNetworkRefresh(String),
    TrayNetworkToggle(String),
    TrayNetworkToggleDone(String, Result<(), String>),
    TraySpawnCommand(String, String),
    TraySpawnCommandDone(String, Result<(), String>),
    TrayAnimateTick,
    
    // Legacy tray events (keep for compatibility during transition)
    TrayEvent(tray::TrayEvent),
}

fn update(bar: &mut UniliiBar, message: Message) -> Task<Message> {
    match message {
        Message::ModuleUpdate(name, update) => {
            info!("module update: {name} -> {:?}", update);
            if let Some(loaded) = bar.modules.get_mut(&name) {
                if let Err(e) = loaded.module.update(update) {
                    error!("Failed to update module '{}': {}", name, e);
                }
            }
        }
        Message::KeyboardInput { code, value } => {
            info!("keyboard event: code={code}, value={value}");
            if code == "KEY_LEFTSHIFT" || code == "KEY_RIGHTSHIFT" {
                bar.shift_held = value != 0;
                info!("shift state changed: held={}", bar.shift_held);
            }
            info!("evdev key: {code} ({value})");
        }
        Message::WindowKeyboardInput {
            key,
            pressed,
            is_shift,
        } => {
            if is_shift {
                bar.shift_held = pressed;
            }

            if pressed {
                // Enhanced menu keyboard navigation
                if let Some(tray_state) = bar.enhanced_tray.as_mut() {
                    match key.as_str() {
                        "Named(Escape)" => {
                            tray_state.animation_target = 0.0;
                            return Task::none();
                        }
                        "Named(ArrowDown)" | "Named(Tab)" => {
                            let count = get_current_menu_item_count(&tray_state.current_view);
                            if count > 0 {
                                tray_state.selected_index = Some(match tray_state.selected_index {
                                    None => 0,
                                    Some(i) => (i + 1) % count,
                                });
                            }
                            return Task::none();
                        }
                        "Named(ArrowUp)" => {
                            let count = get_current_menu_item_count(&tray_state.current_view);
                            if count > 0 {
                                tray_state.selected_index = Some(match tray_state.selected_index {
                                    None => count.saturating_sub(1),
                                    Some(i) => if i == 0 { count - 1 } else { i - 1 },
                                });
                            }
                            return Task::none();
                        }
                        "Named(ArrowLeft)" => {
                            return Task::done(Message::TrayNavigateLeft);
                        }
                        "Named(ArrowRight)" => {
                            return Task::done(Message::TrayNavigateRight);
                        }
                        "Named(Enter)" => {
                            if let Some(idx) = tray_state.selected_index {
                                if let Some((app_id, action)) = get_menu_action_at_index(&tray_state.current_view, idx) {
                                    tray_state.animation_target = 0.0;
                                    return Task::done(Message::TrayMenuTriggered(app_id, action));
                                }
                            }
                            return Task::none();
                        }
                        "Character(\"f\")" => {
                            return Task::done(Message::TrayToggleFavorite("".to_string(), "".to_string()));
                        }
                        "Character(\"a\")" => {
                            return Task::done(Message::TrayShowAggregated);
                        }
                        "Character(\"v\")" => {
                            return Task::done(Message::TrayShowFavorites);
                        }
                        _ => {}
                    }
                }

                // Shift + digit: open nth tray icon
                if bar.shift_held {
                    if let Some(idx) = key_char_digit(&key) {
                        if let Some(icon) = bar.tray_icons.get(idx) {
                            let icon_key = icon.key.clone();
                            return Task::done(Message::TrayIconPressed(icon_key));
                        }
                    }
                }
            }
        }
        Message::TrayEvent(event) => match event {
            tray::TrayEvent::Icons(icons) => {
                bar.tray_icons = icons;
                if let Some(tray_state) = &bar.enhanced_tray {
                    // Check if current app still exists
                    match &tray_state.current_view {
                        TrayViewState::SingleApp { app_id, .. } => {
                            let still_exists = bar.tray_icons.iter().any(|icon| icon.id == *app_id);
                            if !still_exists {
                                bar.enhanced_tray = None;
                            }
                        }
                        TrayViewState::Network { app_id, .. } => {
                            let still_exists = bar.tray_icons.iter().any(|icon| icon.id == *app_id);
                            if !still_exists {
                                bar.enhanced_tray = None;
                            }
                        }
                        _ => {}
                    }
                }
            }
        },
        Message::TrayIconPressed(icon_key) => {
            if let Some(current) = bar.enhanced_tray.as_mut() {
                // Check if clicking on the same icon - if so, close the menu
                match &current.current_view {
                    TrayViewState::SingleApp { app_id, .. } |
                    TrayViewState::Network { app_id, .. } => {
                        if let Some(icon) = bar.tray_icons.iter().find(|icon| icon.key == icon_key) {
                            if icon.id == *app_id {
                                current.animation_target = 0.0;
                                return Task::none();
                            }
                        }
                    }
                    _ => {}
                }
            }

            if let Some(icon) = bar.tray_icons.iter().find(|icon| icon.key == icon_key) {
                // TEMPORARY: Use enhanced tray system for network icons
                if !bar.run_options.no_network_menu && tray::is_network_icon(icon) {
                    // Create enhanced tray state for network
                    let mut tree = enhanced_tray::TrayMenuTree::new();
                    let enhanced_icon = enhanced_tray::TrayIcon {
                        key: icon.key.clone(),
                        service: icon.service.clone(),
                        path: icon.path.clone(),
                        id: icon.id.clone(),
                        title: icon.title.clone(),
                        icon_name: icon.icon_name.clone(),
                        status: icon.status.clone(),
                        has_menu: icon.has_menu,
                        menu_object_path: None,
                    };
                    tree.update_app(enhanced_icon);
                    
                    bar.enhanced_tray = Some(EnhancedTrayState {
                        tree,
                        current_view: TrayViewState::Network {
                            app_id: icon.id.clone(),
                            data: None,
                            loading: true,
                            error: None,
                        },
                        animation_progress: 0.0,
                        animation_target: 1.0,
                        selected_index: Some(0),
                        filter_text: String::new(),
                    });
                    
                    let nmcli_path = bar.run_options.nmcli_path.clone();
                    let app_id = icon.id.clone();
                    return Task::perform(
                        enhanced_tray::read_network_snapshot(nmcli_path, false),
                        move |result| Message::TrayNetworkSnapshot(app_id, result),
                    );
                }

                // Create enhanced tray for regular icons
                let mut tree = enhanced_tray::TrayMenuTree::new();
                let enhanced_icon = enhanced_tray::TrayIcon {
                    key: icon.key.clone(),
                    service: icon.service.clone(),
                    path: icon.path.clone(),
                    id: icon.id.clone(),
                    title: icon.title.clone(),
                    icon_name: icon.icon_name.clone(),
                    status: icon.status.clone(),
                    has_menu: icon.has_menu,
                    menu_object_path: None,
                };
                tree.update_app(enhanced_icon);
                
                let navigation = tree.get_app_navigation(&icon.id);
                bar.enhanced_tray = Some(EnhancedTrayState {
                    tree,
                    current_view: TrayViewState::SingleApp {
                        app_id: icon.id.clone(),
                        navigation,
                    },
                    animation_progress: 0.0,
                    animation_target: 1.0,
                    selected_index: Some(0),
                    filter_text: String::new(),
                });
            }
        }
        Message::TrayMenuTriggered(icon_key, action) => {
            if let Some(icon) = bar
                .tray_icons
                .iter()
                .find(|icon| icon.key == icon_key)
                .cloned()
            {
                // Convert enhanced_tray::TrayMenuAction to tray::TrayMenuAction
                let converted_action = match action {
                    enhanced_tray::TrayMenuAction::Activate => tray::TrayMenuAction::Activate,
                    enhanced_tray::TrayMenuAction::ContextMenu => tray::TrayMenuAction::ContextMenu,
                    enhanced_tray::TrayMenuAction::SecondaryActivate => tray::TrayMenuAction::SecondaryActivate,
                    enhanced_tray::TrayMenuAction::SpawnCommand(cmd) => tray::TrayMenuAction::SpawnCommand(cmd),
                    // For enhanced actions that don't have legacy equivalents, use Activate as default
                    enhanced_tray::TrayMenuAction::DbusMenuAction { .. } |
                    enhanced_tray::TrayMenuAction::NavigateToApp(_) |
                    enhanced_tray::TrayMenuAction::ShowAggregated |
                    enhanced_tray::TrayMenuAction::ShowFavorites |
                    enhanced_tray::TrayMenuAction::ToggleFavorite(_) => tray::TrayMenuAction::Activate,
                };
                
                tokio::spawn(async move {
                    tray::invoke_menu_action(&icon, converted_action).await;
                });
            }

            // Close enhanced tray menu after action
            if let Some(tray_state) = bar.enhanced_tray.as_mut() {
                tray_state.animation_target = 0.0;
            }
        }
        Message::TrayNetworkSnapshot(icon_key, result) => {
            if let Some(tray_state) = bar.enhanced_tray.as_mut() {
                if let TrayViewState::Network { app_id, data, loading, error, .. } = &mut tray_state.current_view {
                    if let Some(icon) = bar.tray_icons.iter().find(|icon| icon.id == *app_id) {
                        if icon.key == icon_key {
                            *loading = false;
                            match result {
                                Ok(snapshot) => {
                                    *data = Some(snapshot);
                                    *error = None;
                                }
                                Err(message) => {
                                    *error = Some(message);
                                }
                            }
                        }
                    }
                }
            }
        }
        Message::TrayNetworkRefresh(icon_key) => {
            if let Some(tray_state) = bar.enhanced_tray.as_mut() {
                if let TrayViewState::Network { app_id, loading, error, .. } = &mut tray_state.current_view {
                    if let Some(icon) = bar.tray_icons.iter().find(|icon| icon.id == *app_id) {
                        if icon.key == icon_key {
                            *loading = true;
                            *error = None;
                        }
                    }
                }
            }

            let nmcli_path = bar.run_options.nmcli_path.clone();
            return Task::perform(
                enhanced_tray::read_network_snapshot(nmcli_path, true),
                move |result| Message::TrayNetworkSnapshot(icon_key.clone(), result),
            );
        }
        Message::TrayNetworkToggle(icon_key) => {
            let mut desired_state = true;
            if let Some(tray_state) = bar.enhanced_tray.as_mut() {
                if let TrayViewState::Network { app_id, data, loading, error, .. } = &mut tray_state.current_view {
                    if let Some(icon) = bar.tray_icons.iter().find(|icon| icon.id == *app_id) {
                        if icon.key == icon_key {
                            if let Some(snapshot) = data {
                                desired_state = !snapshot.enabled;
                            }
                            *loading = true;
                            *error = None;
                        }
                    }
                }
            }

            let nmcli_path = bar.run_options.nmcli_path.clone();
            return Task::perform(
                enhanced_tray::set_wifi_enabled(nmcli_path, desired_state),
                move |result| Message::TrayNetworkToggleDone(icon_key.clone(), result),
            );
        }
        Message::TrayNetworkToggleDone(icon_key, result) => {
            if let Some(tray_state) = bar.enhanced_tray.as_mut() {
                if let TrayViewState::Network { app_id, loading, error, .. } = &mut tray_state.current_view {
                    if let Some(icon) = bar.tray_icons.iter().find(|icon| icon.id == *app_id) {
                        if icon.key == icon_key {
                            *loading = true;
                            if let Err(message) = result {
                                *loading = false;
                                *error = Some(message);
                                return Task::none();
                            }
                        }
                    }
                }
            }

            let nmcli_path = bar.run_options.nmcli_path.clone();
            return Task::perform(
                enhanced_tray::read_network_snapshot(nmcli_path, true),
                move |result| Message::TrayNetworkSnapshot(icon_key.clone(), result),
            );
        }
        Message::TraySpawnCommand(icon_key, command) => {
            if let Some(tray_state) = bar.enhanced_tray.as_mut() {
                if let TrayViewState::Network { app_id, loading, error, .. } = &mut tray_state.current_view {
                    if let Some(icon) = bar.tray_icons.iter().find(|icon| icon.id == *app_id) {
                        if icon.key == icon_key {
                            *loading = true;
                            *error = None;
                        }
                    }
                }
            }

            return Task::perform(tray::spawn_command(command), move |result| {
                Message::TraySpawnCommandDone(icon_key.clone(), result)
            });
        }
        Message::TraySpawnCommandDone(icon_key, result) => {
            if let Some(tray_state) = bar.enhanced_tray.as_mut() {
                if let TrayViewState::Network { app_id, loading, error, .. } = &mut tray_state.current_view {
                    if let Some(icon) = bar.tray_icons.iter().find(|icon| icon.id == *app_id) {
                        if icon.key == icon_key {
                            *loading = false;
                            if let Err(message) = result {
                                *error = Some(message);
                            }
                        }
                    }
                }
            }
        }
        Message::TrayAnimateTick => {
            if let Some(tray_state) = bar.enhanced_tray.as_mut() {
                tray_state.animation_progress = tray::animate_progress(
                    tray_state.animation_progress, 
                    tray_state.animation_target, 
                    0.12
                );
                if tray_state.animation_progress == 0.0 && tray_state.animation_target == 0.0 {
                    bar.enhanced_tray = None;
                }
            }
        }
        Message::EnhancedTrayEvent(_event) => {
            // TODO: Handle enhanced tray events
        }
        Message::TrayNavigateLeft => {
            if let Some(tray_state) = bar.enhanced_tray.as_mut() {
                if let TrayViewState::SingleApp { app_id, navigation } = &tray_state.current_view {
                    if navigation.can_go_left && navigation.current_app_index > 0 {
                        let new_index = navigation.current_app_index - 1;
                        if let Some(new_app_id) = navigation.app_order.get(new_index) {
                            let new_navigation = tray_state.tree.get_app_navigation(new_app_id);
                            tray_state.current_view = TrayViewState::SingleApp {
                                app_id: new_app_id.clone(),
                                navigation: new_navigation,
                            };
                        }
                    }
                }
            }
        }
        Message::TrayNavigateRight => {
            if let Some(tray_state) = bar.enhanced_tray.as_mut() {
                if let TrayViewState::SingleApp { app_id, navigation } = &tray_state.current_view {
                    if navigation.can_go_right && navigation.current_app_index < navigation.app_order.len().saturating_sub(1) {
                        let new_index = navigation.current_app_index + 1;
                        if let Some(new_app_id) = navigation.app_order.get(new_index) {
                            let new_navigation = tray_state.tree.get_app_navigation(new_app_id);
                            tray_state.current_view = TrayViewState::SingleApp {
                                app_id: new_app_id.clone(),
                                navigation: new_navigation,
                            };
                        }
                    }
                }
            }
        }
        Message::TrayShowAggregated => {
            if let Some(tray_state) = bar.enhanced_tray.as_mut() {
                tray_state.current_view = TrayViewState::Aggregated {
                    items: tray_state.tree.get_aggregated_menu(None),
                    filter: None,
                };
            }
        }
        Message::TrayShowFavorites => {
            if let Some(tray_state) = bar.enhanced_tray.as_mut() {
                tray_state.current_view = TrayViewState::Favorites {
                    items: tray_state.tree.get_favorites_menu(),
                };
            }
        }
        Message::TrayToggleFavorite(app_id, item_id) => {
            if let Some(tray_state) = bar.enhanced_tray.as_mut() {
                tray_state.tree.toggle_favorite(&item_id);
                // Update current view if showing favorites
                if let TrayViewState::Favorites { items } = &mut tray_state.current_view {
                    *items = tray_state.tree.get_favorites_menu();
                }
            }
        }
        Message::TrayFilterUpdate(filter_text) => {
            if let Some(tray_state) = bar.enhanced_tray.as_mut() {
                if let TrayViewState::Aggregated { items, filter } = &mut tray_state.current_view {
                    *filter = if filter_text.is_empty() { None } else { Some(filter_text.clone()) };
                    *items = tray_state.tree.get_aggregated_menu(filter.as_deref());
                }
                tray_state.filter_text = filter_text;
            }
        }
    }
    Task::none()
}

fn view(bar: &UniliiBar) -> Element<'_, Message> {
    // Collect module views ordered by name
    let mut module_names: Vec<_> = bar.modules.keys().collect();
    module_names.sort();

    let mut right_widgets: Vec<Element<'_, Message>> = vec![];

    for name in module_names {
        if let Some(loaded) = bar.modules.get(name) {
            let widget = loaded.module.view().map({
                let name = name.clone();
                move |update| Message::ModuleUpdate(name.clone(), update)
            });
            right_widgets.push(widget);
        }
    }

    // Tray icons — show digit hints when shift is held
    let tray_row = bar.tray_icons.iter().enumerate().fold(
        row!().spacing(1).align_y(iced::Alignment::Center),
        |acc_row, (i, icon)| {
            let label = if bar.shift_held {
                format!("{}:{}", i + 1, tray::icon_label_for(icon))
            } else {
                tray::icon_label_for(icon)
            };
            let is_active = if let Some(tray_state) = &bar.enhanced_tray {
                match &tray_state.current_view {
                    TrayViewState::SingleApp { app_id, .. } |
                    TrayViewState::Network { app_id, .. } => icon.id == *app_id,
                    _ => false,
                }
            } else {
                false
            };
            let btn = button(text(label).size(13))
                .padding([2, 7])
                .on_press(Message::TrayIconPressed(icon.key.clone()));
            let btn = if is_active {
                btn.style(button::primary)
            } else {
                btn.style(button::text)
            };
            acc_row.push(btn)
        },
    );
    right_widgets.push(tray_row.into());

    // Enhanced tray menu (animated)
    if let Some(tray_state) = &bar.enhanced_tray {
        if tray_state.animation_progress > 0.01 {
            let menu_widget = render_enhanced_tray_menu(tray_state);
            right_widgets.push(menu_widget);
        }
    }

    let right_row = row(right_widgets)
        .spacing(6)
        .align_y(iced::Alignment::Center);

    let bar_content = row![Space::new(), right_row]
        .spacing(8)
        .align_y(iced::Alignment::Center)
        .width(Length::Fill)
        .height(Length::Shrink);

    // Apply config background color when available, else fall back to dark theme
    let bg_color = bar
        .config
        .window
        .background_color
        .as_deref()
        .and_then(parse_hex_color);

    let bar_container = container(bar_content)
        .width(Length::Fill)
        .padding([3, 6]);

    if let Some(color) = bg_color {
        bar_container
            .style(move |_theme| container::Style {
                background: Some(iced::Background::Color(color)),
                ..Default::default()
            })
            .into()
    } else {
        bar_container.style(container::dark).into()
    }
}

/// Returns the 0-based tray index if the key is a digit 1-9 (Character("1") etc.)
fn key_char_digit(key: &str) -> Option<usize> {
    // iced Key::Character(SmolStr) formats as: Character("1")
    if let Some(inner) = key.strip_prefix("Character(\"").and_then(|s| s.strip_suffix("\")")) {
        if inner.len() == 1 {
            if let Some(d) = inner.chars().next().and_then(|c| c.to_digit(10)) {
                if d >= 1 {
                    return Some(d as usize - 1);
                }
            }
        }
    }
    None
}

/// Parse a "#rrggbb" hex string into an iced Color.
fn parse_hex_color(hex: &str) -> Option<iced::Color> {
    let hex = hex.trim_start_matches('#');
    if hex.len() == 6 {
        let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
        let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
        let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
        Some(iced::Color::from_rgb8(r, g, b))
    } else {
        None
    }
}

fn subscribe(bar: &UniliiBar) -> Subscription<Message> {
    use iced::stream;
    let _tray_poll_ms = bar.run_options.tray_poll_ms;

    // Create real module subscriptions that coordinate with actual module data
    let module_subscriptions: Vec<Subscription<Message>> = {
        let mut subs = Vec::new();
        
        // Clock module subscription - checks for real module updates
        if bar.modules.contains_key("clock") && has_module_updates("clock") {
            subs.push(Subscription::run(|| {
                stream::channel(64, async move |mut output| {
                    let mut interval = tokio::time::interval(Duration::from_secs(1));
                    loop {
                        interval.tick().await;
                        
                        // Try to get real module update first
                        let update = if let Some(module_update) = get_latest_module_update("clock") {
                            module_update
                        } else {
                            // Fallback to generating time if no module update available
                            let time_str = chrono::Local::now().format("%H:%M:%S").to_string();
                            ModuleUpdate::Text(time_str)
                        };
                        
                        let message = Message::ModuleUpdate("clock".to_string(), update);
                        if output.send(message).await.is_err() {
                            break;
                        }
                    }
                })
            }));
        } else if bar.modules.contains_key("clock") {
            // Fallback for when clock module exists but no updates yet
            subs.push(Subscription::run(|| {
                stream::channel(64, async move |mut output| {
                    let mut interval = tokio::time::interval(Duration::from_secs(1));
                    loop {
                        interval.tick().await;
                        let time_str = chrono::Local::now().format("%H:%M:%S").to_string();
                        let message = Message::ModuleUpdate(
                            "clock".to_string(),
                            ModuleUpdate::Text(time_str),
                        );
                        if output.send(message).await.is_err() {
                            break;
                        }
                    }
                })
            }));
        }
        
        // Battery module subscription - checks for real module updates
        if bar.modules.contains_key("battery") && has_module_updates("battery") {
            subs.push(Subscription::run(|| {
                stream::channel(64, async move |mut output| {
                    let mut interval = tokio::time::interval(Duration::from_secs(5));
                    loop {
                        interval.tick().await;
                        
                        // Try to get real module update first
                        let update = if let Some(module_update) = get_latest_module_update("battery") {
                            module_update
                        } else {
                            // Fallback to reading battery directly
                            if let Ok(devices) = unilii_lib::sysfs::power::PowerDevice::read_all().await {
                                if let Some(battery) = devices.into_iter()
                                    .find(|d| d.kind == unilii_lib::sysfs::power::PowerDeviceKind::Battery) {
                                    let device = unilii_lib::sysfs::power::BatteryPowerDevice(battery);
                                    if let Ok(charge) = device.read_charge().await {
                                        let percentage = (charge * 100.0) as i32;
                                        ModuleUpdate::Text(format!("🔋 {}%", percentage))
                                    } else {
                                        ModuleUpdate::Text("🔋 --".to_string())
                                    }
                                } else {
                                    ModuleUpdate::Text("🔋 --".to_string())
                                }
                            } else {
                                ModuleUpdate::Text("🔋 --".to_string())
                            }
                        };
                        
                        let message = Message::ModuleUpdate("battery".to_string(), update);
                        if output.send(message).await.is_err() {
                            break;
                        }
                    }
                })
            }));
        } else if bar.modules.contains_key("battery") {
            // Fallback for when battery module exists but no updates yet
            subs.push(Subscription::run(|| {
                stream::channel(64, async move |mut output| {
                    let mut interval = tokio::time::interval(Duration::from_secs(5));
                    loop {
                        interval.tick().await;
                        // Try to read battery info
                        let update = if let Ok(devices) = unilii_lib::sysfs::power::PowerDevice::read_all().await {
                            if let Some(battery) = devices.into_iter()
                                .find(|d| d.kind == unilii_lib::sysfs::power::PowerDeviceKind::Battery) {
                                let device = unilii_lib::sysfs::power::BatteryPowerDevice(battery);
                                if let Ok(charge) = device.read_charge().await {
                                    let percentage = (charge * 100.0) as i32;
                                    ModuleUpdate::Text(format!("🔋 {}%", percentage))
                                } else {
                                    ModuleUpdate::Text("🔋 --".to_string())
                                }
                            } else {
                                ModuleUpdate::Text("🔋 --".to_string())
                            }
                        } else {
                            ModuleUpdate::Text("🔋 --".to_string())
                        };

                        let message = Message::ModuleUpdate("battery".to_string(), update);
                        if output.send(message).await.is_err() {
                            break;
                        }
                    }
                })
            }));
        }
        
        subs
    };

    let keyboard_subscription = Subscription::run(|| {
        stream::channel(64, async move |mut output| {
            let listener = match unilii_lib::input::listen_keyboard_events_experimental() {
                Ok(stream) => {
                    info!("keyboard listener initialized: experimental tokio-udev path");
                    Ok(stream)
                }
                Err(e) => {
                    error!("experimental keyboard listener failed, falling back: {}", e);
                    unilii_lib::input::listen_keyboard_events()
                }
            };

            match listener {
                Ok(mut stream) => {
                    while let Some(event) = stream.next().await {
                        info!(
                            "keyboard stream event received: code={:?}, value={}",
                            event.code, event.value
                        );
                        if output
                            .send(Message::KeyboardInput {
                                code: format!("{:?}", event.code),
                                value: event.value,
                            })
                            .await
                            .is_err()
                        {
                            break;
                        }
                    }
                }
                Err(e) => {
                    error!("Failed to initialize keyboard listener: {}", e);
                }
            }
        })
    });

    let window_key_subscription = iced::event::listen_with(|event, _status, _id| {
        use iced::event::Event;
        use iced::keyboard::Event as KeyEvent;
        match event {
            Event::Keyboard(KeyEvent::KeyPressed { key, modifiers, .. }) => {
                map_window_key_press(key, modifiers)
            }
            Event::Keyboard(KeyEvent::KeyReleased { key, modifiers, .. }) => {
                map_window_key_release(key, modifiers)
            }
            _ => None,
        }
    });
    let tray_subscription = Subscription::run(|| {
        stream::channel(64, async move |mut output| {
            let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
            tokio::spawn(async move {
                tray::run_tray_watcher(tx, 1500).await; // Use default poll interval
            });

            while let Some(event) = rx.recv().await {
                if output.send(Message::TrayEvent(event)).await.is_err() {
                    break;
                }
            }
        })
    });
    let tray_animation_subscription =
        iced::time::every(Duration::from_millis(16)).map(|_| Message::TrayAnimateTick);

    let mut subscriptions = module_subscriptions;
    subscriptions.push(keyboard_subscription);
    subscriptions.push(window_key_subscription);
    subscriptions.push(tray_subscription);
    subscriptions.push(tray_animation_subscription);
    Subscription::batch(subscriptions)
}

fn map_window_key_press(key: Key, _modifiers: Modifiers) -> Option<Message> {
    Some(Message::WindowKeyboardInput {
        key: format!("{:?}", key),
        pressed: true,
        is_shift: matches!(key, Key::Named(key::Named::Shift)),
    })
}

fn map_window_key_release(key: Key, _modifiers: Modifiers) -> Option<Message> {
    Some(Message::WindowKeyboardInput {
        key: format!("{:?}", key),
        pressed: false,
        is_shift: matches!(key, Key::Named(key::Named::Shift)),
    })
}

#[tokio::main]
async fn main() -> iced::Result {
    let cli = Cli::parse();
    let run_options = cli.command.clone().unwrap_or(Commands::Run {
        no_tray: false,
        no_network_menu: false,
        nmcli_path: "nmcli".to_string(),
        tray_poll_ms: 1500,
        debug_focus: false,
    }).run_options().unwrap_or_default();

    let log_level = verbose_to_level(cli.verbose);
    let _ = tracing_subscriber::fmt()
        .with_max_level(log_level)
        .try_init();

    // Handle subcommands that don't run the bar
    match &cli.command {
        Some(Commands::ListModules) => {
            println!("Available modules:");
            println!("  - clock    : Display current time");
            println!("  - battery  : Display battery status");
            return Ok(());
        }
        Some(Commands::Version) => {
            println!("unilii {}", env!("CARGO_PKG_VERSION"));
            return Ok(());
        }
        _ => {}
    }

    info!("unilii startup: begin");

    // Load configuration and modules at startup
    let config = load_config();
    let scan = unilii_lib::input::scan_keyboard_device_stats();
    if scan.total_devices == 0 {
        error!(
            "keyboard diagnostics: /dev/input appears inaccessible (total_devices=0). \
             Keyboard events will not work until device access is available."
        );
    } else {
        info!(
            "keyboard diagnostics: total_devices={}, keyboard_candidates={}",
            scan.total_devices, scan.keyboard_candidates
        );
    }
    info!(
        "config loaded: size={}x{}, pos=({}, {})",
        config.window.width,
        config.window.height,
        config.window.position_x,
        config.window.position_y
    );

    if !config.keybindings.is_empty() {
        let keybindings = config.keybindings.clone();
        tokio::spawn(async move {
            let daemon = KeybindingDaemon::new(keybindings);
            if let Err(error) = daemon.run().await {
                error!("keybinding daemon exited with error: {}", error);
            }
        });
        info!(
            "keybinding daemon started with {} bindings",
            config.keybindings.len()
        );
    }

    // Load application configuration with fallback to defaults
    let app_config = match load_app_config(None) {
        config if config.modules.is_empty() => {
            warn!("Loaded configuration has no modules, using defaults");
            AppConfig::default()
        }
        config => {
            info!("Loaded application configuration with {} module configs", config.modules.len());
            config
        }
    };
    
    // Initialize module manager and load modules with comprehensive error handling
    let module_manager = ModuleManager::new();
    
    let (modules, module_subscriptions) = match module_manager.load_modules(app_config.modules.clone()).await {
        Ok((modules, subs)) => {
            info!("Successfully loaded {} modules with {} subscriptions", modules.len(), subs.len());
            (modules, subs)
        }
        Err(e) => {
            error!("Module loading failed: {}", e);
            warn!("Falling back to empty module set - application will continue with limited functionality");
            (HashMap::new(), Vec::new())
        }
    };
    
    // Initialize the global subscription manager with error isolation
    if !module_subscriptions.is_empty() {
        initialize_global_subscriptions(module_subscriptions);
        info!("Subscription system initialized successfully");
    } else {
        warn!("No module subscriptions available, continuing without real-time updates");
    }

    // Get window settings from config
    let window_position = iced::window::Position::Specific(iced::Point {
        x: config.window.position_x as f32,
        y: config.window.position_y as f32,
    });

    let mut window_settings = window::Settings {
        size: iced::Size::new(config.window.width as f32, config.window.height as f32),
        position: window_position,
        resizable: false,
        decorations: false,
        level: window::Level::AlwaysOnTop,
        ..window::Settings::default()
    };

    #[cfg(target_os = "linux")]
    {
        window_settings.platform_specific = window::settings::PlatformSpecific {
            application_id: "com.unilii.bar".to_string(),
            override_redirect: !run_options.debug_focus,
        };
        if run_options.debug_focus {
            window_settings.decorations = true;
            window_settings.resizable = true;
            window_settings.level = window::Level::Normal;
        }
        info!(
            "linux window settings: application_id=com.unilii.bar, override_redirect={}, debug_focus_mode={}",
            !run_options.debug_focus,
            run_options.debug_focus
        );
    }

    info!("unilii startup: load finished, launching iced application");

    let _initial_state = UniliiBar {
        modules,
        config: config.clone(),
        app_config: app_config.clone(),
        shift_held: false,
        tray_icons: Vec::new(),
        enhanced_tray: None,
        run_options: run_options.clone(),
    };
    
    // Run the iced application with the loaded modules
    iced::application(
        || {
            (
                UniliiBar {
                    modules: HashMap::new(), // Start with empty, modules will be loaded later
                    config: unilii_core::config::Config::default(),
                    app_config: AppConfig::default(),
                    shift_held: false,
                    tray_icons: Vec::new(),
                    enhanced_tray: None,
                    run_options: RunOptions::default(),
                },
                Task::none(),
            )
        },
        update,
        view,
    )
    .subscription(subscribe)
    .window(window_settings)
    .run()
}

#[cfg(test)]
mod tests {
    use super::key_char_digit;

    #[test]
    fn digit_1_maps_to_index_0() {
        assert_eq!(key_char_digit("Character(\"1\")"), Some(0));
    }

    #[test]
    fn digit_9_maps_to_index_8() {
        assert_eq!(key_char_digit("Character(\"9\")"), Some(8));
    }

    #[test]
    fn digit_0_is_not_a_tray_shortcut() {
        assert_eq!(key_char_digit("Character(\"0\")"), None);
    }

    #[test]
    fn non_digit_key_returns_none() {
        assert_eq!(key_char_digit("Named(Escape)"), None);
        assert_eq!(key_char_digit("Character(\"a\")"), None);
    }
}

// == Enhanced Tray Helper Functions ==

/// Get the number of menu items in the current view state
fn get_current_menu_item_count(view_state: &TrayViewState) -> usize {
    match view_state {
        TrayViewState::SingleApp { .. } => {
            // This would need more context to get the actual count from the tree
            3 // Fallback default
        }
        TrayViewState::Aggregated { items, .. } |
        TrayViewState::Favorites { items } => items.len(),
        TrayViewState::Network { data, .. } => {
            if data.is_some() { 4 } else { 2 } // Basic network menu items
        }
    }
}

/// Get the menu action at a specific index in the current view state
fn get_menu_action_at_index(view_state: &TrayViewState, index: usize) -> Option<(String, enhanced_tray::TrayMenuAction)> {
    match view_state {
        TrayViewState::SingleApp { app_id, .. } => {
            // Return basic action for the app
            Some((app_id.clone(), enhanced_tray::TrayMenuAction::Activate))
        }
        TrayViewState::Aggregated { items, .. } |
        TrayViewState::Favorites { items } => {
            items.get(index).map(|item| (item.app_id.clone(), item.action.clone()))
        }
        TrayViewState::Network { app_id, .. } => {
            // Basic network actions based on index
            let action = match index {
                0 => enhanced_tray::TrayMenuAction::SpawnCommand("nmcli radio wifi off".to_string()),
                1 => enhanced_tray::TrayMenuAction::SpawnCommand("nmcli device wifi rescan".to_string()),
                2 => enhanced_tray::TrayMenuAction::SpawnCommand("nm-connection-editor".to_string()),
                _ => enhanced_tray::TrayMenuAction::Activate,
            };
            Some((app_id.clone(), action))
        }
    }
}

/// Animate progress value towards target
fn animate_progress(current: f32, target: f32, rate: f32) -> f32 {
    if (current - target).abs() < 0.001 {
        target
    } else {
        current + (target - current) * rate
    }
}

/// Render the enhanced tray menu view
fn render_enhanced_tray_menu(tray_state: &EnhancedTrayState) -> Element<'_, Message> {
    let opacity = tray_state.animation_progress.clamp(0.0, 1.0);
    
    match &tray_state.current_view {
        TrayViewState::SingleApp { app_id, .. } => {
            // Get the app's menu items
            if let Some(app) = tray_state.tree.apps.get(app_id) {
                let visible_count = (app.menu_items.len() as f32 * tray_state.animation_progress).ceil() as usize;
                let menu_row = app.menu_items.iter().enumerate().take(visible_count).fold(
                    row!().spacing(2).align_y(iced::Alignment::Center),
                    |mut acc: iced::widget::Row<'_, Message>, (i, item)| {
                        let is_sel = tray_state.selected_index == Some(i);
                        let btn = button(text(item.label.clone()).size(12))
                            .padding([2, 8])
                            .on_press(Message::TrayMenuTriggered(
                                app_id.clone(),
                                item.action.clone(),
                            ));
                        let btn = if is_sel { btn.style(button::primary) } else { btn.style(button::text) };
                        acc.push(btn)
                    },
                );
                
                // Add navigation hints
                let mut nav_row = row![menu_row];
                
                if tray_state.current_view.get_navigation().map_or(false, |nav| nav.can_go_left) {
                    nav_row = nav_row.push(text("◀").size(10));
                }
                if tray_state.current_view.get_navigation().map_or(false, |nav| nav.can_go_right) {
                    nav_row = nav_row.push(text("▶").size(10));
                }
                
                container(nav_row)
                    .padding([0, 4])
                    .style(container::rounded_box)
                    .into()
            } else {
                text("No menu items").into()
            }
        }
        TrayViewState::Aggregated { items, .. } => {
            let visible_count = (items.len() as f32 * tray_state.animation_progress).ceil() as usize;
            let menu_col = items.iter().enumerate().take(visible_count).take(8).fold( // Limit to 8 items
                column!().spacing(1),
                |mut acc: iced::widget::Column<'_, Message>, (i, item)| {
                    let is_sel = tray_state.selected_index == Some(i);
                    let btn = button(text(format!("{} → {}", item.app_id, item.label)).size(11))
                        .padding([1, 6])
                        .on_press(Message::TrayMenuTriggered(
                            item.app_id.clone(),
                            item.action.clone(),
                        ));
                    let btn = if is_sel { btn.style(button::primary) } else { btn.style(button::text) };
                    acc.push(btn)
                },
            );
            
            let header = row![
                text("All Items").size(11),
                Space::new(),
                text("[a]gg [v]favs [f]fav").size(9)
            ];
            
            container(column![header, menu_col])
                .padding([3, 4])
                .style(container::rounded_box)
                .into()
        }
        TrayViewState::Favorites { items } => {
            let visible_count = (items.len() as f32 * tray_state.animation_progress).ceil() as usize;
            let menu_col = items.iter().enumerate().take(visible_count).fold(
                column!().spacing(1),
                |mut acc: iced::widget::Column<'_, Message>, (i, item)| {
                    let is_sel = tray_state.selected_index == Some(i);
                    let btn = button(text(format!("⭐ {} → {}", item.app_id, item.label)).size(11))
                        .padding([1, 6])
                        .on_press(Message::TrayMenuTriggered(
                            item.app_id.clone(),
                            item.action.clone(),
                        ));
                    let btn = if is_sel { btn.style(button::primary) } else { btn.style(button::text) };
                    acc.push(btn)
                },
            );
            
            let header = text("Favorites").size(11);
            
            container(column![header, menu_col])
                .padding([3, 4])
                .style(container::rounded_box)
                .into()
        }
        TrayViewState::Network { data, loading, error, .. } => {
            let mut col = column![
                // Basic network options
                button(text(if data.as_ref().map(|d| d.enabled).unwrap_or(false) { "Disable Wi-Fi" } else { "Enable Wi-Fi" }).size(12))
                    .padding([2, 8])
                    .style(button::text)
                    .on_press(Message::TrayNetworkToggle("default".to_string())),
                button(text("Refresh").size(12))
                    .padding([2, 8])
                    .style(button::text)
                    .on_press(Message::TrayNetworkRefresh("default".to_string())),
                button(text("Settings").size(12))
                    .padding([2, 8])
                    .style(button::text)
                    .on_press(Message::TraySpawnCommand(
                        "default".to_string(),
                        "nm-connection-editor".to_string(),
                    )),
            ].spacing(1);

            if let Some(snapshot) = data {
                if snapshot.enabled && !snapshot.networks.is_empty() {
                    col = col.push(text("─────").size(10));
                    for network in snapshot.networks.iter().take(6) {
                        let connected_marker = if snapshot.state == "connected" { " ●" } else { "" };
                        col = col.push(
                            button(
                                text(format!("{}{} {}%", network.ssid, connected_marker, network.signal))
                                    .size(11),
                            )
                            .padding([1, 8])
                            .style(button::text)
                            .on_press(Message::TraySpawnCommand(
                                "default".to_string(),
                                format!("nmcli device wifi connect \"{}\"", network.ssid),
                            )),
                        );
                    }
                } else if !snapshot.enabled {
                    col = col.push(text("  Wi-Fi off").size(11));
                }
            }
            
            if *loading {
                col = col.push(text("  …").size(11));
            }
            if let Some(msg) = error {
                col = col.push(text(format!("  ⚠ {msg}")).size(11));
            }

            container(col)
                .padding([4, 6])
                .style(container::rounded_box)
                .into()
        }
    }
}

impl TrayViewState {
    fn get_navigation(&self) -> Option<&enhanced_tray::TrayMenuNavigation> {
        match self {
            TrayViewState::SingleApp { navigation, .. } => Some(navigation),
            _ => None,
        }
    }
}
