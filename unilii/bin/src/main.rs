mod app;
mod app_config;
mod cli;
mod module_loader;
mod subscription_manager;
mod tray;
mod enhanced_tray;
mod widgets;

use app_config::{load_app_config, AppConfig};
use app::{Message, UniliiBar};
use cli::{Cli, Commands, RunOptions, verbose_to_level};
use clap::Parser;
use iced::futures::{SinkExt, StreamExt};
use iced::keyboard::{key, Key, Modifiers};
use iced::widget::{button, column, container, row, scrollable, text, text_input, Space};
use iced::{window, Alignment, Element, Length, Subscription, Task};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::time::Duration;
use tracing::{error, info, warn};
use unilii_core::{config::{load_config, load_config_with_path}, keys::KeybindingDaemon, ModuleUpdate};

use module_loader::{LoadedModule, ModuleManager, ModuleSubscription};
use subscription_manager::{initialize_global_subscriptions, get_latest_module_update, has_module_updates};
use enhanced_tray::{TrayViewState, TrayMenuAction, EnhancedTrayState};
use widgets::{key_char_digit, render_modules};

fn update(bar: &mut UniliiBar, message: Message) -> Task<Message> {
    match message {
        Message::InitializePanels => {
            info!("InitializePanels message received (single panel mode)");
        }
        Message::WindowOpened(_id) => {
            info!("WindowOpened message received (single panel mode)");
        }
        Message::WindowClosed(_id) => {
            info!("WindowClosed message received (single panel mode)");
        }
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
                    
                    bar.enhanced_tray = Some(enhanced_tray::EnhancedTrayState {
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
                bar.enhanced_tray = Some(enhanced_tray::EnhancedTrayState {
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
    let mut right_widgets: Vec<Element<'_, Message>> = render_modules(&bar.modules);

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
            let mut btn = button(text(label).size(13))
                .padding([2, 7])
                .on_press(Message::TrayIconPressed(icon.key.clone()));
            if is_active {
                btn = btn.style(button::primary);
            } else {
                btn = btn.style(button::text);
            }
            acc_row.push(btn)
        },
    );
    right_widgets.push(tray_row.into());
    tracing::info!("layout: tray icons count={}, total widgets in right_row={}", bar.tray_icons.len(), right_widgets.len());

    // Enhanced tray menu (animated)
    if let Some(tray_state) = &bar.enhanced_tray {
        if tray_state.animation_progress > 0.01 {
            let menu_widget = render_enhanced_tray_menu(tray_state);
            right_widgets.push(menu_widget);
        }
    }

    let right_row = row(right_widgets)
        .spacing(0)
        .align_y(iced::Alignment::Center)
        .width(Length::Fill)
        .height(Length::Shrink);

    right_row.into()
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

fn main() -> iced::Result {
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

    // Run async initialization in a tokio runtime
    let runtime = tokio::runtime::Runtime::new().expect("Failed to create tokio runtime");

    let (config, loaded_app_config, run_options, modules): (
        unilii_core::config::Config,
        app_config::AppConfig,
        cli::RunOptions,
        std::collections::HashMap<String, module_loader::LoadedModule>,
    ) = runtime.block_on(async {
        // Load configuration and modules at startup
        let config = load_config_with_path(cli.config.clone());
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
            "config loaded: {} panels, first panel size={}x{}, pos=({}, {})",
            config.panels.len(),
            config.panels.first().map(|p| p.width).unwrap_or(1024),
            config.panels.first().map(|p| p.height).unwrap_or(24),
            config.panels.first().map(|p| p.position_x).unwrap_or(0),
            config.panels.first().map(|p| p.position_y).unwrap_or(0)
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
        let config_path_str = cli.config.as_ref().and_then(|p| p.to_str());
        let loaded_app_config = match load_app_config(config_path_str) {
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

        let (modules, module_subscriptions) = match module_manager.load_modules(loaded_app_config.modules.clone()).await {
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

        Ok((config, loaded_app_config, run_options, modules))
    }).map_err(|e: Box<dyn std::error::Error>| {
        eprintln!("Runtime initialization error: {:?}", e);
        std::process::exit(1);
    }).unwrap();

    // Get window settings from first panel config
    let first_panel = config.panels.first().cloned().unwrap_or_else(|| {
        unilii_core::config::PanelConfig {
            name: "default".to_string(),
            width: 1024,
            height: 24,
            position_x: 0,
            position_y: 0,
            background_color: Some("#1e1e1e".to_string()),
            text_color: Some("#ffffff".to_string()),
        }
    });

    let window_position = iced::window::Position::Specific(iced::Point {
        x: first_panel.position_x as f32,
        y: first_panel.position_y as f32,
    });

    let mut window_settings = window::Settings {
        size: iced::Size::new(first_panel.width as f32, first_panel.height as f32),
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

    // Wrap pre-loaded data in Rc<RefCell<>> for Fn-compatible closure
    let modules = Rc::new(RefCell::new(Some(modules)));
    let config = Rc::new(RefCell::new(Some(config)));
    let app_config = Rc::new(RefCell::new(Some(loaded_app_config)));
    let run_options = Rc::new(RefCell::new(Some(run_options)));

    // Create closure that can be called multiple times (Fn requirement)
    let initial_state = move || -> (UniliiBar, Task<Message>) {
        let modules = modules.borrow_mut().take().unwrap_or_default();
        let config = config.borrow_mut().take().unwrap_or_default();
        let app_config = app_config.borrow_mut().take().unwrap_or_default();
        let run_options = run_options.borrow_mut().take().unwrap_or_default();

        (
            UniliiBar {
                modules,
                config,
                app_config,
                shift_held: false,
                tray_icons: Vec::new(),
                enhanced_tray: None,
                run_options,
            },
            Task::none(),
        )
    };

    // Run the iced application with the loaded modules
    iced::application(
        initial_state,
        update,
        view,
    )
    .subscription(subscribe)
    .window(window_settings)
    .run()
}

#[cfg(test)]
mod tests {
    use super::widgets::key_char_digit;

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
    use crate::enhanced_tray::rendering::*;

    if !tray_state.is_visible() {
        return Space::new().into();
    }

    let content = match &tray_state.current_view {
        TrayViewState::SingleApp { app_id, navigation } => {
            render_single_app_view_with_main_messages(tray_state, app_id, navigation)
        }
        TrayViewState::Aggregated { items, filter } => {
            render_aggregated_view_with_main_messages(tray_state, items, filter)
        }
        TrayViewState::Favorites { items } => {
            render_favorites_view_with_main_messages(tray_state, items)
        }
        TrayViewState::Network { app_id, data, loading, error } => {
            render_network_view_with_main_messages(tray_state, app_id, data, *loading, error)
        }
    };

    let opacity = tray_state.animation_progress.clamp(0.0, 1.0);

    container(content)
        .padding([4, 8])
        .style(move |theme| {
            let mut appearance: container::Style = container::Style::default();
            appearance.background = Some(iced::Background::Color(theme.palette().background));
            appearance.background = appearance.background.map(|bg| match bg {
                iced::Background::Color(mut color) => {
                    color.a = opacity;
                    iced::Background::Color(color)
                }
                other => other,
            });
            appearance
        })
        .into()
}

fn render_single_app_view_with_main_messages<'a>(
    state: &'a EnhancedTrayState,
    app_id: &'a str,
    navigation: &'a enhanced_tray::TrayMenuNavigation,
) -> Element<'a, Message> {
    let app_menu = state.tree.apps.get(app_id);

    let mut content = column!().spacing(2);

    let mut title_row = row!().spacing(4).align_y(iced::Alignment::Center);

    if navigation.can_go_left {
        title_row = title_row.push(
            button(text("◀").size(12))
                .on_press(Message::TrayNavigateLeft)
        );
    }

    if let Some(app) = app_menu {
        title_row = title_row.push(
            text(&app.icon.title)
                .size(14)
        );
    } else {
        title_row = title_row.push(text(app_id).size(14));
    }

    if navigation.can_go_right {
        title_row = title_row.push(
            button(text("▶").size(12))
                .on_press(Message::TrayNavigateRight)
        );
    }

    content = content.push(title_row);

    if let Some(app) = app_menu {
        let menu_items = render_menu_items_with_main_messages(&app.menu_items, state.selected_index, app_id);
        content = content.push(menu_items);
    } else {
        content = content.push(text("No menu available").size(12));
    }

    content = content.push(render_keyboard_hints_single());

    content.into()
}

fn render_aggregated_view_with_main_messages<'a>(
    _state: &'a EnhancedTrayState,
    items: &'a [enhanced_tray::TrayMenuItem],
    filter: &'a Option<String>,
) -> Element<'a, Message> {
    let mut content = column!().spacing(2);

    content = content.push(
        text("All Menu Items")
            .size(14)
    );

    content = content.push(
        text_input(
            "Search menu items...",
            filter.as_deref().unwrap_or("")
        )
        .on_input(Message::TrayFilterUpdate)
        .size(12)
        .padding([2, 4])
    );

    if items.is_empty() {
        content = content.push(text("No items found").size(12));
    } else {
        let items_container = render_aggregated_items_with_main_messages(items);
        content = content.push(items_container);
    }

    content = content.push(render_keyboard_hints_aggregated());

    content.into()
}

fn render_favorites_view_with_main_messages<'a>(
    _state: &'a EnhancedTrayState,
    items: &'a [enhanced_tray::TrayMenuItem],
) -> Element<'a, Message> {
    let mut content = column!().spacing(2);

    content = content.push(
        text("Favorite Items ⭐")
            .size(14)
    );

    if items.is_empty() {
        content = content.push(
            text("No favorites yet. Press 'f' on any menu item to add it here.")
                .size(12)
        );
    } else {
        let items_container = render_favorite_items_with_main_messages(items);
        content = content.push(items_container);
    }

    content = content.push(render_keyboard_hints_favorites());

    content.into()
}

fn render_network_view_with_main_messages<'a>(
    _state: &'a EnhancedTrayState,
    app_id: &'a str,
    data: &'a Option<crate::tray::NetworkSnapshot>,
    loading: bool,
    error: &'a Option<String>,
) -> Element<'a, Message> {
    let mut content = column!().spacing(2);

    content = content.push(
        text("Network Settings")
            .size(14)
    );

    if loading {
        content = content.push(
            text("⟳ Loading...")
                .size(12)
        );
    } else if let Some(err) = error {
        content = content.push(
            text(format!("⚠ Error: {}", err))
                .size(12)
        );
    }

    let controls = render_network_controls_with_main_messages(app_id, data);
    content = content.push(controls);

    if let Some(snapshot) = data {
        if snapshot.enabled && !snapshot.networks.is_empty() {
            let networks = render_network_list_with_main_messages(app_id, snapshot);
            content = content.push(networks);
        } else if !snapshot.enabled {
            content = content.push(text("Wi-Fi is disabled").size(12));
        }
    }

    content = content.push(render_keyboard_hints_network());

    content.into()
}

fn render_menu_items_with_main_messages<'a>(
    items: &'a [enhanced_tray::TrayMenuItem],
    selected_index: Option<usize>,
    app_id: &'a str,
) -> Element<'a, Message> {
    if items.is_empty() {
        return text("No menu items").size(12).into();
    }

    let mut menu_col = column!().spacing(1);

    for (index, item) in items.iter().enumerate() {
        let item_widget = render_menu_item_with_main_messages(item, selected_index == Some(index), app_id);
        menu_col = menu_col.push(item_widget);
    }

    if items.len() > 8 {
        scrollable(menu_col)
            .height(Length::Fixed(200.0))
            .into()
    } else {
        menu_col.into()
    }
}

fn render_menu_item_with_main_messages<'a>(
    item: &'a enhanced_tray::TrayMenuItem,
    _is_selected: bool,
    app_id: &'a str,
) -> Element<'a, Message> {
    if item.is_separator {
        return text("─".repeat(20))
            .size(10)
            .into();
    }

    let mut label = item.label.clone();

    if item.checkable {
        label = format!("{} {}", if item.checked { "☑" } else { "☐" }, label);
    }

    if let Some(shortcut) = &item.shortcut {
        label = format!("{} ({})", label, shortcut);
    }

    let btn = button(text(label).size(12))
        .padding([2, 8])
        .width(Length::Fill);

    if item.enabled {
        btn.on_press(Message::TrayMenuTriggered(
            app_id.to_string(),
            item.action.clone(),
        )).into()
    } else {
        btn.into()
    }
}

fn render_aggregated_items_with_main_messages<'a>(items: &'a [enhanced_tray::TrayMenuItem]) -> Element<'a, Message> {
    let mut items_col = column!().spacing(1);

    for item in items.iter().take(10) {
        let item_row = row![
            text("⭐").size(10),
            text(&item.full_path).size(11),
            Space::new(),
            button(text("★").size(10))
                .on_press(Message::TrayToggleFavorite(
                    item.app_id.clone(),
                    item.id.clone()
                )),
        ]
        .spacing(4)
        .align_y(Alignment::Center);

        let item_btn = button(item_row)
            .padding([2, 4])
            .width(Length::Fill)
            .on_press(Message::TrayMenuTriggered(
                item.app_id.clone(),
                item.action.clone(),
            ));

        items_col = items_col.push(item_btn);
    }

    if items.len() > 10 {
        items_col = items_col.push(
            text(format!("... and {} more items", items.len() - 10))
                .size(10)
        );
    }

    scrollable(items_col)
        .height(Length::Fixed(200.0))
        .into()
}

fn render_favorite_items_with_main_messages<'a>(items: &'a [enhanced_tray::TrayMenuItem]) -> Element<'a, Message> {
    let mut items_col = column!().spacing(1);

    for item in items {
        let item_row = row![
            text("⭐").size(10),
            text(&item.full_path).size(11),
            button(text("✗").size(10))
                .on_press(Message::TrayToggleFavorite(
                    item.app_id.clone(),
                    item.id.clone()
                )),
        ]
        .spacing(4)
        .align_y(Alignment::Center);

        let item_btn = button(item_row)
            .padding([2, 4])
            .width(Length::Fill)
            .on_press(Message::TrayMenuTriggered(
                item.app_id.clone(),
                item.action.clone(),
            ));

        items_col = items_col.push(item_btn);
    }

    scrollable(items_col)
        .height(Length::Fixed(200.0))
        .into()
}

fn render_network_controls_with_main_messages<'a>(
    app_id: &'a str,
    data: &'a Option<crate::tray::NetworkSnapshot>,
) -> Element<'a, Message> {
    let is_enabled = data.as_ref().map(|d| d.enabled).unwrap_or(false);

    row![
        button(text(if is_enabled { "Disable Wi-Fi" } else { "Enable Wi-Fi" }).size(12))
            .padding([2, 6])
            .on_press(Message::TrayNetworkToggle(app_id.to_string())),

        button(text("Settings").size(12))
            .padding([2, 6])
            .on_press(Message::TraySpawnCommand(
                app_id.to_string(),
                "nm-connection-editor".to_string()
            )),
    ]
    .spacing(4)
    .into()
}

fn render_network_list_with_main_messages<'a>(
    app_id: &'a str,
    snapshot: &'a crate::tray::NetworkSnapshot,
) -> Element<'a, Message> {
    let mut networks_col = column!().spacing(1);

    networks_col = networks_col.push(text("Available Networks:").size(12));

    for network in snapshot.networks.iter().take(6) {
        let mut label = format!("{} ({}%)", network.ssid, network.signal);

        if snapshot.state == "connected" && snapshot.interface == network.ssid {
            label = format!("● {}", label);
        }

        let network_btn = button(text(label).size(11))
            .padding([1, 4])
            .width(Length::Fill)
            .on_press(Message::TraySpawnCommand(
                app_id.to_string(),
                format!("nmcli device wifi connect \"{}\"", network.ssid)
            ));

        networks_col = networks_col.push(network_btn);
    }

    scrollable(networks_col)
        .height(Length::Fixed(150.0))
        .into()
}

fn render_keyboard_hints_single() -> Element<'static, Message> {
    text("◀/▶: Navigate apps • a: All items • v: Favorites")
        .size(10)
        .into()
}

fn render_keyboard_hints_aggregated() -> Element<'static, Message> {
    text("Type: Filter • f: Toggle favorite • v: Favorites only")
        .size(10)
        .into()
}

fn render_keyboard_hints_favorites() -> Element<'static, Message> {
    text("a: All items • f: Remove favorite")
        .size(10)
        .into()
}

fn render_keyboard_hints_network() -> Element<'static, Message> {
    text("Click to connect/control • a: All items")
        .size(10)
        .into()
}


