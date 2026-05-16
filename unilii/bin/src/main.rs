#![allow(clippy::collapsible_if)]
// FIXME(T1.1/T6): main.rs still owns large tray/menu update chains; collapse or extract during the main.rs split instead of hiding this permanently.

mod action_runner;
mod app;
mod app_config;
mod cli;
mod enhanced_tray;
mod menus;
mod module_loader;
mod subscription_manager;
mod tray;
mod update;
mod widgets;

use app::{Message, UniliiBar};
use app_config::{AppConfig, load_app_config};
use clap::Parser;
use cli::{Cli, Commands, verbose_to_level};
use iced::futures::{SinkExt, StreamExt};
use iced::keyboard::{Key, Modifiers, key};
use iced::widget::{
    Space, button, column, container, image, row, scrollable, svg, text, text_input,
};
use iced::{Alignment, Element, Length, Subscription, Task, window};
use std::cell::RefCell;
use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Duration;
use tracing::{error, info, warn};
use unilii_core::{
    ModuleUpdate,
    config::load_config_with_path,
    key_import_sxhkd::import_sxhkd_config,
    keys::{KeyDryRunEvent, KeybindingDaemon, dry_run_bindings},
};

use enhanced_tray::{EnhancedTrayState, TrayViewState};
use update::enhanced_tray_events::apply_enhanced_tray_event;
use update::tray_navigation::{navigate_left, navigate_right};
use update::tray_view::{enter_submenu, exit_submenu, show_aggregated, show_favorites, update_filter};
use update::tray_text_input::{clear_text_input_value, set_text_input_value};
use update::tray_menu_fetch::{apply_menu_fetch_result, TrayMenuFetchOutcome};
use update::tray_favorites::toggle_favorite;
use update::tray_icon_press::{open_tray_icon_state, open_tray_icon_state_with_menu, should_close_current_tray_view, to_enhanced_tray_icon, TrayIconOpenKind};
use update::tray_snapshots::{apply_calendar_snapshot, apply_mount_snapshot, apply_network_snapshot, apply_spawn_command_done, apply_spawn_command_started, mark_special_view_loading, network_toggle_desired_state_and_mark_loading};
use module_loader::ModuleManager;
use subscription_manager::{
    get_latest_module_update, has_module_updates, initialize_global_subscriptions,
};
use widgets::{
    Audio, Power, SysMonitor, Video, Widget, WidgetMessage, Wifi, key_char_digit, render_modules,
};

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
            if let Some(task) = handle_evdev_tray_key(bar, &code, value) {
                return task;
            }
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
                    if bar.tray_quickjump_active {
                        if key_matches_named(&key, "Escape") {
                            bar.tray_quickjump_active = false;
                            bar.tray_quickjump_input.clear();
                            return Task::none();
                        }
                        if let Some(ch) = extract_key_char(&key) {
                            let alphabet =
                                quickjump_alphabet_for_view(&bar.config.menus.custom, tray_state);
                            match handle_quickjump_key(
                                &mut bar.tray_quickjump_input,
                                &alphabet,
                                get_current_menu_item_count(tray_state),
                                ch,
                            ) {
                                QuickjumpOutcome::Ignored
                                | QuickjumpOutcome::Pending
                                | QuickjumpOutcome::Reset => {
                                    return Task::none();
                                }
                                QuickjumpOutcome::Activate(index) => {
                                    bar.tray_quickjump_active = false;
                                    bar.tray_quickjump_input.clear();
                                    if let Some((app_id, action)) =
                                        get_menu_action_at_index(tray_state, index)
                                    {
                                        tray_state.animation_target = 0.0;
                                        return Task::batch(vec![
                                            Task::done(Message::TrayMenuTriggered(app_id, action)),
                                            resize_window_task(bar, false),
                                        ]);
                                    }
                                    return Task::none();
                                }
                            }
                        }
                    }
                    match key.as_str() {
                        _ if key_matches_named(&key, "Escape") => {
                            tray_state.animation_target = 0.0;
                            bar.tray_quickjump_active = false;
                            bar.tray_quickjump_input.clear();
                            return resize_window_task(bar, false);
                        }
                        _ if key_matches_named(&key, "ArrowDown")
                            || key_matches_named(&key, "Tab") =>
                        {
                            let count = get_current_menu_item_count(tray_state);
                            if count > 0 {
                                tray_state.selected_index = Some(match tray_state.selected_index {
                                    None => 0,
                                    Some(i) => (i + 1) % count,
                                });
                            }
                            return Task::none();
                        }
                        _ if key_matches_named(&key, "ArrowUp") => {
                            let count = get_current_menu_item_count(tray_state);
                            if count > 0 {
                                tray_state.selected_index = Some(match tray_state.selected_index {
                                    None => count.saturating_sub(1),
                                    Some(i) => {
                                        if i == 0 {
                                            count - 1
                                        } else {
                                            i - 1
                                        }
                                    }
                                });
                            }
                            return Task::none();
                        }
                        _ if key_matches_named(&key, "ArrowLeft") => {
                            return Task::done(Message::TrayNavigateLeft);
                        }
                        _ if key_matches_named(&key, "ArrowRight") => {
                            return Task::done(Message::TrayNavigateRight);
                        }
                        _ if key_matches_named(&key, "Enter") => {
                            if let Some(idx) = tray_state.selected_index {
                                if let Some((app_id, action)) =
                                    get_menu_action_at_index(tray_state, idx)
                                {
                                    tray_state.animation_target = 0.0;
                                    return Task::batch(vec![
                                        Task::done(Message::TrayMenuTriggered(app_id, action)),
                                        resize_window_task(bar, false),
                                    ]);
                                }
                            }
                            return Task::none();
                        }
                        _ if key_matches_char(&key, 'f') => {
                            return Task::done(Message::TrayToggleFavorite(
                                "".to_string(),
                                "".to_string(),
                            ));
                        }
                        _ if key_matches_char(&key, 'a') => {
                            return Task::done(Message::TrayShowAggregated);
                        }
                        _ if key_matches_char(&key, 'v') => {
                            return Task::done(Message::TrayShowFavorites);
                        }
                        _ if key_matches_char(&key, 'g') => {
                            if quickjump_supported_for_view(tray_state) {
                                bar.tray_quickjump_active = !bar.tray_quickjump_active;
                                bar.tray_quickjump_input.clear();
                            }
                            return Task::none();
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
                        TrayViewState::Network { app_id, .. }
                        | TrayViewState::Mount { app_id, .. }
                        | TrayViewState::Calendar { app_id, .. } => {
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
            if let Some(icon) = bar.tray_icons.iter().find(|icon| icon.key == icon_key)
                && should_close_current_tray_view(bar.enhanced_tray.as_ref(), icon)
            {
                if let Some(current) = bar.enhanced_tray.as_mut() {
                    current.animation_target = 0.0;
                }
                return resize_window_task(bar, false);
            }

            if let Some(icon) = bar.tray_icons.iter().find(|icon| icon.key == icon_key) {
                bar.tray_quickjump_active = false;
                bar.tray_quickjump_input.clear();
                // TEMPORARY: Use enhanced tray system for network icons
                if !bar.run_options.no_network_menu && tray::is_network_icon(icon) {
                    bar.enhanced_tray = Some(open_tray_icon_state(icon, TrayIconOpenKind::Network));

                    let nmcli_path = bar.run_options.nmcli_path.clone();
                    let app_id = icon.id.clone();

                    return Task::batch(vec![
                        resize_window_task(bar, true),
                        Task::perform(
                            enhanced_tray::read_network_snapshot(nmcli_path, false),
                            move |result| Message::TrayNetworkSnapshot(app_id, result),
                        ),
                    ]);
                }

                if is_mount_icon(icon) {
                    bar.enhanced_tray = Some(open_tray_icon_state(icon, TrayIconOpenKind::Mount));

                    let app_id = icon.id.clone();
                    return Task::batch(vec![
                        resize_window_task(bar, true),
                        Task::perform(
                            read_mount_snapshot(bar.config.menus.mount.clone()),
                            move |result| Message::TrayMountSnapshot(app_id, result),
                        ),
                    ]);
                }

                if is_calendar_icon(icon) {
                    bar.enhanced_tray = Some(open_tray_icon_state(icon, TrayIconOpenKind::Calendar));

                    let app_id = icon.id.clone();
                    let calendar_accounts = bar.config.menus.calendar.accounts.clone();
                    let agenda_days = bar.config.menus.calendar.agenda_days;
                    return Task::batch(vec![
                        resize_window_task(bar, true),
                        Task::perform(
                            read_calendar_snapshot(calendar_accounts, agenda_days),
                            move |result| Message::TrayCalendarSnapshot(app_id, result),
                        ),
                    ]);
                }

                if is_custom_menu_icon(icon, &bar.config.menus.custom) {
                    let enhanced_icon = to_enhanced_tray_icon(icon, false);
                    let custom_menu = build_custom_menu_items(&enhanced_icon, &bar.config.menus.custom);
                    bar.enhanced_tray = Some(open_tray_icon_state_with_menu(
                        icon,
                        TrayIconOpenKind::Regular,
                        Some(custom_menu),
                    ));
                    return resize_window_task(bar, true);
                }

                // Create enhanced tray for regular icons
                bar.enhanced_tray = Some(open_tray_icon_state(icon, TrayIconOpenKind::Regular));

                // Fetch menu if the icon has one
                if icon.has_menu && icon.menu_object_path.is_some() {
                    let fetch_icon = to_enhanced_tray_icon(icon, icon.has_menu);
                    let app_id = icon.id.clone();
                    let app_id_for_result = app_id.clone();
                    return Task::batch(vec![
                        resize_window_task(bar, true),
                        Task::perform(
                            async move {
                                enhanced_tray::dbus::fetch_dbus_menu(&fetch_icon)
                                    .await
                                    .map(|items| {
                                        enhanced_tray::dbus::convert_dbus_to_tray_menu(
                                            items, &app_id,
                                        )
                                    })
                                    .map_err(|e| e.to_string())
                            },
                            move |result| Message::TrayMenuFetched(app_id_for_result, result),
                        ),
                    ]);
                }
                return resize_window_task(bar, true);
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
                if let enhanced_tray::TrayMenuAction::DbusMenuAction { item_id, event_id } =
                    action.clone()
                {
                    let enhanced_icon = enhanced_tray::TrayIcon {
                        key: icon.key.clone(),
                        service: icon.service.clone(),
                        path: icon.path.clone(),
                        id: icon.id.clone(),
                        title: icon.title.clone(),
                        icon_name: icon.icon_name.clone(),
                        icon_pixmap: icon.icon_pixmap.clone(),
                        status: icon.status.clone(),
                        has_menu: icon.has_menu,
                        menu_object_path: icon.menu_object_path.clone(),
                    };
                    tokio::spawn(async move {
                        let _ = enhanced_tray::invoke_dbus_menu_action(
                            &enhanced_icon,
                            item_id,
                            &event_id,
                        )
                        .await;
                    });
                    if let Some(tray_state) = bar.enhanced_tray.as_mut() {
                        tray_state.animation_target = 0.0;
                    }
                    return resize_window_task(bar, false);
                }
                if let enhanced_tray::TrayMenuAction::NavigateToSubmenu { submenu_path, .. } =
                    action.clone()
                {
                    return Task::done(Message::TrayEnterSubmenu(icon.id.clone(), submenu_path));
                }
                if let enhanced_tray::TrayMenuAction::SpawnCommand(cmd) = action.clone() {
                    if cmd == "mount:refresh" {
                        return Task::done(Message::TrayMountRefresh(icon.key.clone()));
                    }
                    if cmd == "calendar:refresh" {
                        return Task::done(Message::TrayCalendarRefresh(icon.key.clone()));
                    }
                }
                let converted_action = match action {
                    enhanced_tray::TrayMenuAction::Activate => tray::TrayMenuAction::Activate,
                    enhanced_tray::TrayMenuAction::ContextMenu => tray::TrayMenuAction::ContextMenu,
                    enhanced_tray::TrayMenuAction::SecondaryActivate => {
                        tray::TrayMenuAction::SecondaryActivate
                    }
                    enhanced_tray::TrayMenuAction::SpawnCommand(cmd) => {
                        tray::TrayMenuAction::SpawnCommand(cmd)
                    }
                    // For enhanced actions that don't have legacy equivalents, use Activate as default
                    enhanced_tray::TrayMenuAction::DbusMenuAction { .. }
                    | enhanced_tray::TrayMenuAction::NavigateToApp(_)
                    | enhanced_tray::TrayMenuAction::ShowAggregated
                    | enhanced_tray::TrayMenuAction::ShowFavorites
                    | enhanced_tray::TrayMenuAction::ToggleFavorite(_)
                    | enhanced_tray::TrayMenuAction::NavigateToSubmenu { .. }
                    | enhanced_tray::TrayMenuAction::TextInputChanged { .. }
                    | enhanced_tray::TrayMenuAction::TextInputFocusGained
                    | enhanced_tray::TrayMenuAction::TextInputFocusLost
                    | enhanced_tray::TrayMenuAction::TextInputCleared => {
                        tray::TrayMenuAction::Activate
                    }
                };

                tokio::spawn(async move {
                    tray::invoke_menu_action(&icon, converted_action).await;
                });
            }

            // Close enhanced tray menu after action
            if let Some(tray_state) = bar.enhanced_tray.as_mut() {
                tray_state.animation_target = 0.0;
                return resize_window_task(bar, false);
            }
        }
        Message::TrayNetworkSnapshot(icon_key, result) => {
            apply_network_snapshot(&mut bar.enhanced_tray, &icon_key, result, |app_id| {
                bar.tray_icons
                    .iter()
                    .find(|icon| icon.id == app_id)
                    .map(|icon| icon.key.clone())
            });
        }
        Message::TrayNetworkRefresh(icon_key) => {
            mark_special_view_loading(&mut bar.enhanced_tray, &icon_key, |app_id| {
                bar.tray_icons
                    .iter()
                    .find(|icon| icon.id == app_id)
                    .map(|icon| icon.key.clone())
            });

            let nmcli_path = bar.run_options.nmcli_path.clone();
            return Task::perform(
                enhanced_tray::read_network_snapshot(nmcli_path, true),
                move |result| Message::TrayNetworkSnapshot(icon_key.clone(), result),
            );
        }
        Message::TrayNetworkToggle(icon_key) => {
            let desired_state = network_toggle_desired_state_and_mark_loading(
                &mut bar.enhanced_tray,
                &icon_key,
                |app_id| {
                    bar.tray_icons
                        .iter()
                        .find(|icon| icon.id == app_id)
                        .map(|icon| icon.key.clone())
                },
            );

            let nmcli_path = bar.run_options.nmcli_path.clone();
            return Task::perform(
                enhanced_tray::set_wifi_enabled(nmcli_path, desired_state),
                move |result| Message::TrayNetworkToggleDone(icon_key.clone(), result),
            );
        }
        Message::TrayNetworkToggleDone(icon_key, result) => {
            mark_special_view_loading(&mut bar.enhanced_tray, &icon_key, |app_id| {
                bar.tray_icons
                    .iter()
                    .find(|icon| icon.id == app_id)
                    .map(|icon| icon.key.clone())
            });
            if let Err(message) = result {
                apply_network_snapshot(&mut bar.enhanced_tray, &icon_key, Err(message), |app_id| {
                    bar.tray_icons
                        .iter()
                        .find(|icon| icon.id == app_id)
                        .map(|icon| icon.key.clone())
                });
                return Task::none();
            }

            let nmcli_path = bar.run_options.nmcli_path.clone();
            return Task::perform(
                enhanced_tray::read_network_snapshot(nmcli_path, true),
                move |result| Message::TrayNetworkSnapshot(icon_key.clone(), result),
            );
        }
        Message::TrayMountSnapshot(icon_key, result) => {
            apply_mount_snapshot(&mut bar.enhanced_tray, &icon_key, result, |app_id| {
                bar.tray_icons
                    .iter()
                    .find(|icon| icon.id == app_id)
                    .map(|icon| icon.key.clone())
            });
        }
        Message::TrayMountRefresh(icon_key) => {
            let mount_config = bar.config.menus.mount.clone();
            mark_special_view_loading(&mut bar.enhanced_tray, &icon_key, |app_id| {
                bar.tray_icons
                    .iter()
                    .find(|icon| icon.id == app_id)
                    .map(|icon| icon.key.clone())
            });
            return Task::perform(read_mount_snapshot(mount_config), move |result| {
                Message::TrayMountSnapshot(icon_key.clone(), result)
            });
        }
        Message::TrayCalendarSnapshot(icon_key, result) => {
            apply_calendar_snapshot(&mut bar.enhanced_tray, &icon_key, result, |app_id| {
                bar.tray_icons
                    .iter()
                    .find(|icon| icon.id == app_id)
                    .map(|icon| icon.key.clone())
            });
        }
        Message::TrayCalendarRefresh(icon_key) => {
            let calendar_accounts = bar.config.menus.calendar.accounts.clone();
            let agenda_days = bar.config.menus.calendar.agenda_days;

            mark_special_view_loading(&mut bar.enhanced_tray, &icon_key, |app_id| {
                bar.tray_icons
                    .iter()
                    .find(|icon| icon.id == app_id)
                    .map(|icon| icon.key.clone())
            });

            return Task::perform(
                read_calendar_snapshot(calendar_accounts, agenda_days),
                move |result| Message::TrayCalendarSnapshot(icon_key.clone(), result),
            );
        }
        Message::TraySpawnCommand(icon_key, command) => {
            apply_spawn_command_started(&mut bar.enhanced_tray, &icon_key, |app_id| {
                bar.tray_icons
                    .iter()
                    .find(|icon| icon.id == app_id)
                    .map(|icon| icon.key.clone())
            });

            return Task::perform(tray::spawn_command(command), move |result| {
                Message::TraySpawnCommandDone(icon_key.clone(), result)
            });
        }
        Message::TraySpawnCommandDone(icon_key, result) => {
            apply_spawn_command_done(&mut bar.enhanced_tray, &icon_key, result, |app_id| {
                bar.tray_icons
                    .iter()
                    .find(|icon| icon.id == app_id)
                    .map(|icon| icon.key.clone())
            });
        }
        Message::TrayAnimateTick => {
            if let Some(tray_state) = bar.enhanced_tray.as_mut() {
                tray_state.animation_progress = tray::animate_progress(
                    tray_state.animation_progress,
                    tray_state.animation_target,
                    0.12,
                );
                if tray_state.animation_progress == 0.0 && tray_state.animation_target == 0.0 {
                    bar.enhanced_tray = None;
                }
            }
        }
        Message::EnhancedTrayEvent(event) => {
            apply_enhanced_tray_event(&mut bar.enhanced_tray, event);
        }
        Message::TrayNavigateLeft => {
            navigate_left(&mut bar.enhanced_tray);
        }
        Message::TrayNavigateRight => {
            navigate_right(&mut bar.enhanced_tray);
        }
        Message::TrayShowAggregated => {
            show_aggregated(&mut bar.enhanced_tray);
        }
        Message::TrayShowFavorites => {
            show_favorites(&mut bar.enhanced_tray);
        }
        Message::TrayToggleFavorite(_app_id, item_id) => {
            toggle_favorite(&mut bar.enhanced_tray, &item_id);
        }
        Message::TrayFilterUpdate(filter_text) => {
            update_filter(&mut bar.enhanced_tray, filter_text);
        }
        Message::TrayEnterSubmenu(app_id, submenu_path) => {
            enter_submenu(&mut bar.enhanced_tray, &app_id, submenu_path);
        }
        Message::TrayExitSubmenu => {
            exit_submenu(&mut bar.enhanced_tray);
        }
        Message::TrayTextInputChanged(item_id, value) => {
            set_text_input_value(&mut bar.enhanced_tray, &item_id, &value);
        }
        Message::TrayTextInputFocusGained(item_id) => {
            info!("Text input focus gained: {}", item_id);
        }
        Message::TrayTextInputFocusLost(item_id) => {
            info!("Text input focus lost: {}", item_id);
        }
        Message::TrayTextInputCleared(item_id) => {
            clear_text_input_value(&mut bar.enhanced_tray, &item_id);
        }
        Message::TrayMenuFetched(app_id, result) => {
            match apply_menu_fetch_result(&mut bar.enhanced_tray, &app_id, result) {
                TrayMenuFetchOutcome::Populated { .. } => {
                    info!("Menu fetched and populated for app: {}", app_id);
                }
                TrayMenuFetchOutcome::KeptExistingEmptyFetch => {
                    info!("Fetched empty DBus menu for {}; keeping fallback menu", app_id);
                }
                TrayMenuFetchOutcome::FallbackPopulated { error, .. }
                | TrayMenuFetchOutcome::FetchFailedNoKnownApp { error } => {
                    info!("Failed to fetch menu for {}: {}", app_id, error);
                }
                TrayMenuFetchOutcome::NoState => {}
            }
        }
        Message::LegacyWidget(widget_message) => match widget_message.clone() {
            WidgetMessage::SysMonitor(_) => bar.sysmonitor.update(widget_message),
            WidgetMessage::Wifi(_) => bar.wifi.update(widget_message),
            WidgetMessage::Audio(_) => bar.audio.update(widget_message),
            WidgetMessage::Video(_) => bar.video.update(widget_message),
            WidgetMessage::Power(_) => bar.power.update(widget_message),
            WidgetMessage::Tray(_) => {}
        },
        Message::LegacyWidgetTick(name) => match name.as_str() {
            "sysmonitor" => bar.sysmonitor.update_stats(),
            "wifi" => bar.wifi.update_status(),
            "audio" => bar.audio.update_devices(),
            "video" => bar.video.refresh_state(),
            "power" => bar.power.update_screensaver_status(),
            _ => {}
        },
        Message::KeybindingAction(_) => {}
    }
    Task::none()
}

fn view(bar: &UniliiBar, window_id: window::Id) -> Element<'_, Message> {
    if Some(window_id) == bar.tray_window_id {
        return if let Some(tray_state) = &bar.enhanced_tray {
            render_enhanced_tray_menu(bar, tray_state)
        } else {
            Space::new().into()
        };
    }

    let mut right_widgets: Vec<Element<'_, Message>> = render_modules(&bar.modules);
    right_widgets.push(bar.sysmonitor.view().map(Message::LegacyWidget));
    right_widgets.push(bar.wifi.view().map(Message::LegacyWidget));
    right_widgets.push(bar.audio.view().map(Message::LegacyWidget));
    right_widgets.push(bar.video.view().map(Message::LegacyWidget));
    right_widgets.push(bar.power.view().map(Message::LegacyWidget));

    // Tray icons — show digit hints when shift is held
    let tray_row = bar.tray_icons.iter().enumerate().fold(
        row!().spacing(1).align_y(iced::Alignment::Center),
        |acc_row, (i, icon)| {
            let is_active = if let Some(tray_state) = &bar.enhanced_tray {
                match &tray_state.current_view {
                    TrayViewState::SingleApp { app_id, .. }
                    | TrayViewState::Network { app_id, .. }
                    | TrayViewState::Mount { app_id, .. }
                    | TrayViewState::Calendar { app_id, .. } => icon.id == *app_id,
                    _ => false,
                }
            } else {
                false
            };
            let mut btn = button(render_tray_button_content(icon, bar.shift_held, i))
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

    let right_row = row(right_widgets)
        .spacing(0)
        .align_y(iced::Alignment::Center)
        .width(Length::Fill)
        .height(Length::Shrink);
    // main bar falls through to the final right_row.into() below

    // Render tray menu inline inside the main window for now.
    // This avoids invisible/clipped popup behavior while we verify
    // tray selection and menu action dispatch end-to-end.
    if let Some(tray_state) = None::<&EnhancedTrayState> {
        let menu_widget = render_enhanced_tray_menu(bar, tray_state);
        // Render menu above bar with proper positioning
        return container(column![
            container(menu_widget)
                .width(Length::Fill)
                .style(|_theme| menu_panel_style()),
            container(right_row).width(Length::Fill)
        ])
        .width(Length::Fill)
        .into();
    }

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
                        let update = if let Some(module_update) = get_latest_module_update("clock")
                        {
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
                        let update =
                            if let Some(module_update) = get_latest_module_update("battery") {
                                module_update
                            } else {
                                // Fallback to reading battery directly
                                if let Ok(devices) =
                                    unilii_lib::sysfs::power::PowerDevice::read_all().await
                                {
                                    if let Some(battery) = devices.into_iter().find(|d| {
                                        d.kind == unilii_lib::sysfs::power::PowerDeviceKind::Battery
                                    }) {
                                        let device =
                                            unilii_lib::sysfs::power::BatteryPowerDevice(battery);
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
                        let update = if let Ok(devices) =
                            unilii_lib::sysfs::power::PowerDevice::read_all().await
                        {
                            if let Some(battery) = devices.into_iter().find(|d| {
                                d.kind == unilii_lib::sysfs::power::PowerDeviceKind::Battery
                            }) {
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

    let legacy_widget_subscriptions: Vec<Subscription<Message>> = vec![
        iced::time::every(Duration::from_millis(
            bar.sysmonitor.update_interval().unwrap_or(2_000),
        ))
        .map(|_| Message::LegacyWidgetTick("sysmonitor".to_string())),
        iced::time::every(Duration::from_millis(
            bar.wifi.update_interval().unwrap_or(5_000),
        ))
        .map(|_| Message::LegacyWidgetTick("wifi".to_string())),
        iced::time::every(Duration::from_millis(
            bar.audio.update_interval().unwrap_or(15_000),
        ))
        .map(|_| Message::LegacyWidgetTick("audio".to_string())),
        iced::time::every(Duration::from_millis(
            bar.video.update_interval().unwrap_or(15_000),
        ))
        .map(|_| Message::LegacyWidgetTick("video".to_string())),
        iced::time::every(Duration::from_millis(
            bar.power.update_interval().unwrap_or(30_000),
        ))
        .map(|_| Message::LegacyWidgetTick("power".to_string())),
    ];

    let mut subscriptions = module_subscriptions;
    subscriptions.push(keyboard_subscription);
    subscriptions.push(window_key_subscription);
    subscriptions.push(tray_subscription);
    subscriptions.push(tray_animation_subscription);
    subscriptions.extend(legacy_widget_subscriptions);
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
    let run_options = cli
        .command
        .clone()
        .unwrap_or(Commands::Run {
            no_tray: false,
            no_network_menu: false,
            nmcli_path: "nmcli".to_string(),
            tray_poll_ms: 1500,
            debug_focus: false,
        })
        .run_options()
        .unwrap_or_default();

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
        Some(Commands::KeyDryRun {
            config,
            sxhkd,
            events,
        }) => {
            let bindings = if let Some(sxhkd_path) = sxhkd {
                let content = fs::read_to_string(sxhkd_path).map_err(|error| {
                    iced::Error::WindowCreationFailed(
                        format!(
                            "failed to read sxhkd file '{}': {}",
                            sxhkd_path.display(),
                            error
                        )
                        .into(),
                    )
                })?;
                let imported = import_sxhkd_config(&content);
                if !imported.warnings.is_empty() {
                    println!("sxhkd import warnings:");
                    for warning in imported.warnings {
                        println!("  line {}: {}", warning.line, warning.message);
                    }
                }
                imported.bindings
            } else {
                let cfg = load_config_with_path(config.clone().or_else(|| cli.config.clone()));
                cfg.keybindings
            };

            let parsed_events = parse_key_dry_run_events(events).map_err(|error| {
                iced::Error::WindowCreationFailed(
                    format!("invalid --events payload: {}", error).into(),
                )
            })?;
            let steps = dry_run_bindings(&bindings, &parsed_events).map_err(|error| {
                iced::Error::WindowCreationFailed(format!("key dry-run failed: {}", error).into())
            })?;

            println!(
                "key-dry-run: bindings={} events={}",
                bindings.len(),
                parsed_events.len()
            );
            for step in steps {
                println!(
                    "event t={}ms {}:{}",
                    step.event.at_ms, step.event.key, step.event.value
                );
                if step.triggered_binding_names.is_empty() {
                    println!("  triggered: <none>");
                } else {
                    println!("  triggered: {}", step.triggered_binding_names.join(", "));
                }
                for trace in step.trace_lines {
                    println!("  trace: {}", trace);
                }
            }

            return Ok(());
        }
        _ => {}
    }

    info!("unilii startup: begin");

    // Run async initialization in a tokio runtime.
    let runtime = tokio::runtime::Runtime::new().map_err(|error| {
        iced::Error::WindowCreationFailed(
            format!("failed to create tokio runtime during startup: {error}").into(),
        )
    })?;

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

        if let Some(path) = &loaded_app_config.app.xrandr_presets_yaml {
            unsafe { env::set_var("UNILII_XRANDR_PRESETS_YAML", path); }
            info!("Configured xrandr presets YAML: {}", path);
        }

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
    }).map_err(|error: Box<dyn std::error::Error>| {
        iced::Error::WindowCreationFailed(
            format!("runtime initialization failed: {error}").into(),
        )
    })?;

    // Get window settings from first panel config
    let first_panel =
        config
            .panels
            .first()
            .cloned()
            .unwrap_or_else(|| unilii_core::config::PanelConfig {
                name: "default".to_string(),
                width: 1024,
                height: 24,
                position_x: 0,
                position_y: 0,
                background_color: Some("#1e1e1e".to_string()),
                text_color: Some("#ffffff".to_string()),
            });

    let window_position = iced::window::Position::Specific(iced::Point {
        x: first_panel.position_x as f32,
        y: first_panel.position_y as f32,
    });

    let debug_window_height = first_panel.height as f32;

    let mut window_settings = window::Settings {
        size: iced::Size::new(first_panel.width as f32, debug_window_height),
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
            !run_options.debug_focus, run_options.debug_focus
        );
    }

    info!("unilii startup: load finished, launching iced application");

    // Wrap pre-loaded data in Rc<RefCell<>> for Fn-compatible closure
    let modules = Rc::new(RefCell::new(Some(modules)));
    let config = Rc::new(RefCell::new(Some(config)));
    let app_config = Rc::new(RefCell::new(Some(loaded_app_config)));
    let window_settings = Rc::new(RefCell::new(Some(window_settings)));
    let run_options = Rc::new(RefCell::new(Some(run_options)));

    // Create closure that can be called multiple times (Fn requirement)
    let initial_state = move || -> (UniliiBar, Task<Message>) {
        let modules = modules.borrow_mut().take().unwrap_or_default();
        let config = config.borrow_mut().take().unwrap_or_default();
        let app_config = app_config.borrow_mut().take().unwrap_or_default();
        let window_settings = window_settings.borrow_mut().take().unwrap_or_default();
        let run_options = run_options.borrow_mut().take().unwrap_or_default();
        let mut sysmonitor = SysMonitor::new();
        sysmonitor.update_stats();
        let mut wifi = Wifi::new();
        wifi.update_status();
        let mut audio = Audio::new();
        audio.update_devices();
        let mut power = Power::new();
        power.update_screensaver_status();
        let video = Video::with_preset_source(
            app_config
                .app
                .xrandr_presets_yaml
                .clone()
                .map(PathBuf::from),
        );
        let (main_window_id, open_main_window) = window::open(window_settings);

        (
            UniliiBar {
                main_window_id: Some(main_window_id),
                tray_window_id: None,
                legacy_widget_window_id: None,
                active_legacy_widget: None,
                modules,
                config,
                app_config,
                sysmonitor,
                wifi,
                audio,
                video,
                power,
                shift_held: false,
                tray_icons: Vec::new(),
                enhanced_tray: None,
                tray_quickjump_active: false,
                tray_quickjump_input: String::new(),
                run_options,
            },
            open_main_window.map(Message::WindowOpened),
        )
    };

    // Run the iced daemon and open windows via tasks
    iced::daemon(initial_state, update, view)
        .subscription(subscribe)
        .run()
}

#[cfg(test)]
mod tests {
    use super::widgets::key_char_digit;
    use super::*;

    #[test]
    fn parses_key_dry_run_events_with_implicit_timestamps() {
        let events = parse_key_dry_run_events("KEY_LEFTMETA:1,KEY_ENTER:1,KEY_ENTER:0")
            .expect("events should parse");
        assert_eq!(events.len(), 3);
        assert_eq!(events[0].at_ms, 0);
        assert_eq!(events[1].at_ms, 10);
        assert_eq!(events[2].at_ms, 20);
    }

    #[test]
    fn parses_key_dry_run_events_with_explicit_timestamps() {
        let events = parse_key_dry_run_events("KEY_LEFTMETA:1@0,KEY_ENTER:1@120")
            .expect("events should parse");
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].at_ms, 0);
        assert_eq!(events[1].at_ms, 120);
    }

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
    #[test]
    fn submenu_helper_counts_nested_items() {
        let mut state = enhanced_tray::EnhancedTrayState::new();
        let icon = enhanced_tray::TrayIcon {
            key: "app".into(),
            service: "svc".into(),
            path: "/StatusNotifierItem".into(),
            id: "app".into(),
            title: "App".into(),
            icon_name: None,
            icon_pixmap: None,
            status: "Active".into(),
            has_menu: true,
            menu_object_path: None,
        };
        state.tree.update_app(icon);
        state.tree.update_app_menu(
            "app",
            vec![enhanced_tray::TrayMenuItem {
                id: "settings".into(),
                label: "Settings".into(),
                action: enhanced_tray::TrayMenuAction::NavigateToSubmenu {
                    item_id: "settings".into(),
                    submenu_path: vec!["settings".into()],
                },
                icon: None,
                submenu: vec![enhanced_tray::TrayMenuItem {
                    id: "advanced".into(),
                    label: "Advanced".into(),
                    action: enhanced_tray::TrayMenuAction::Activate,
                    icon: None,
                    submenu: vec![],
                    enabled: true,
                    visible: true,
                    checkable: false,
                    checked: false,
                    shortcut: None,
                    is_separator: false,
                    app_id: "app".into(),
                    full_path: "Settings → Advanced".into(),
                    widget_type: enhanced_tray::TrayWidgetType::Button,
                    default_value: None,
                    placeholder: None,
                }],
                enabled: true,
                visible: true,
                checkable: false,
                checked: false,
                shortcut: None,
                is_separator: false,
                app_id: "app".into(),
                full_path: "Settings".into(),
                widget_type: enhanced_tray::TrayWidgetType::SubmenuButton,
                default_value: None,
                placeholder: None,
            }],
        );
        let nav = state.tree.get_app_navigation("app");
        state.current_view = enhanced_tray::TrayViewState::SingleApp {
            app_id: "app".into(),
            navigation: nav,
            submenu_path: vec!["settings".into()],
        };
        assert_eq!(get_current_menu_item_count(&state), 1);
    }
    #[test]
    fn submenu_helper_returns_nested_action() {
        let mut state = enhanced_tray::EnhancedTrayState::new();
        let icon = enhanced_tray::TrayIcon {
            key: "app".into(),
            service: "svc".into(),
            path: "/StatusNotifierItem".into(),
            id: "app".into(),
            title: "App".into(),
            icon_name: None,
            icon_pixmap: None,
            status: "Active".into(),
            has_menu: true,
            menu_object_path: None,
        };
        state.tree.update_app(icon);
        state.tree.update_app_menu(
            "app",
            vec![enhanced_tray::TrayMenuItem {
                id: "settings".into(),
                label: "Settings".into(),
                action: enhanced_tray::TrayMenuAction::NavigateToSubmenu {
                    item_id: "settings".into(),
                    submenu_path: vec!["settings".into()],
                },
                icon: None,
                submenu: vec![enhanced_tray::TrayMenuItem {
                    id: "advanced".into(),
                    label: "Advanced".into(),
                    action: enhanced_tray::TrayMenuAction::Activate,
                    icon: None,
                    submenu: vec![],
                    enabled: true,
                    visible: true,
                    checkable: false,
                    checked: false,
                    shortcut: None,
                    is_separator: false,
                    app_id: "app".into(),
                    full_path: "Settings → Advanced".into(),
                    widget_type: enhanced_tray::TrayWidgetType::Button,
                    default_value: None,
                    placeholder: None,
                }],
                enabled: true,
                visible: true,
                checkable: false,
                checked: false,
                shortcut: None,
                is_separator: false,
                app_id: "app".into(),
                full_path: "Settings".into(),
                widget_type: enhanced_tray::TrayWidgetType::SubmenuButton,
                default_value: None,
                placeholder: None,
            }],
        );
        let nav = state.tree.get_app_navigation("app");
        state.current_view = enhanced_tray::TrayViewState::SingleApp {
            app_id: "app".into(),
            navigation: nav,
            submenu_path: vec!["settings".into()],
        };
        assert!(matches!(
            get_menu_action_at_index(&state, 0).map(|(_, action)| action),
            Some(enhanced_tray::TrayMenuAction::Activate)
        ));
    }
    #[test]
    fn network_view_count_matches_controls_plus_visible_networks() {
        let mut state = enhanced_tray::EnhancedTrayState::new();
        state.current_view = enhanced_tray::TrayViewState::Network {
            app_id: "nm-applet".into(),
            data: Some(crate::tray::NetworkSnapshot {
                interface: "wlan0".into(),
                state: "connected".into(),
                enabled: true,
                connected_ssid: Some("home".into()),
                known_networks: vec![],
                networks: vec![
                    crate::tray::WifiNetwork {
                        ssid: "home".into(),
                        signal: 80,
                        security: "wpa2".into(),
                    },
                    crate::tray::WifiNetwork {
                        ssid: "mobile".into(),
                        signal: 55,
                        security: "wpa2".into(),
                    },
                ],
            }),
            loading: false,
            error: None,
        };

        assert_eq!(get_current_menu_item_count(&state), 5);
        assert_eq!(current_menu_items_len(&state), 5);
    }

    #[test]
    fn network_view_actions_follow_control_then_network_order() {
        let mut state = enhanced_tray::EnhancedTrayState::new();
        state.current_view = enhanced_tray::TrayViewState::Network {
            app_id: "nm-applet".into(),
            data: Some(crate::tray::NetworkSnapshot {
                interface: "wlan0".into(),
                state: "connected".into(),
                enabled: true,
                connected_ssid: Some("home".into()),
                known_networks: vec![],
                networks: vec![crate::tray::WifiNetwork {
                    ssid: "cafe".into(),
                    signal: 67,
                    security: "wpa2".into(),
                }],
            }),
            loading: false,
            error: None,
        };

        assert_eq!(
            get_menu_action_at_index(&state, 0).map(|(_, action)| action),
            Some(enhanced_tray::TrayMenuAction::SpawnCommand(
                "nmcli radio wifi off".into()
            ))
        );
        assert_eq!(
            get_menu_action_at_index(&state, 1).map(|(_, action)| action),
            Some(enhanced_tray::TrayMenuAction::SpawnCommand(
                "nmcli device wifi rescan".into()
            ))
        );
        assert_eq!(
            get_menu_action_at_index(&state, 2).map(|(_, action)| action),
            Some(enhanced_tray::TrayMenuAction::SpawnCommand(
                "nm-connection-editor".into()
            ))
        );
        assert_eq!(
            get_menu_action_at_index(&state, 3).map(|(_, action)| action),
            Some(enhanced_tray::TrayMenuAction::SpawnCommand(
                "nmcli device wifi connect \"cafe\"".into()
            ))
        );
    }
}

// == Enhanced Tray Helper Functions ==

/// Get the number of menu items in the current view state
fn get_current_menu_item_count(tray_state: &EnhancedTrayState) -> usize {
    match &tray_state.current_view {
        TrayViewState::SingleApp {
            app_id,
            submenu_path,
            ..
        } => resolve_current_single_app_items(tray_state, app_id, submenu_path)
            .map(|items| items.iter().filter(|item| item.visible).count())
            .unwrap_or(0),
        TrayViewState::Aggregated { items, .. } | TrayViewState::Favorites { items } => items.len(),
        TrayViewState::Network { data, .. } => 3
            + data
                .as_ref()
                .filter(|snapshot| snapshot.enabled)
                .map(|snapshot| snapshot.networks.len().min(6))
                .unwrap_or(0),
        TrayViewState::Mount { data, .. } => {
            2 + data
                .as_ref()
                .map(|snapshot| {
                    snapshot.local_devices.len().min(8)
                        + snapshot.sshfs_profiles.len().min(6)
                        + snapshot.loop_mounts.len().min(6)
                        + snapshot.vcvolume_profiles.len().min(6)
                })
                .unwrap_or(0)
        }
        TrayViewState::Calendar { data, .. } => {
            2 + data
                .as_ref()
                .map(|snapshot| snapshot.account_ids.len().min(6) + snapshot.events.len().min(6))
                .unwrap_or(0)
        }
    }
}

/// Get the menu action at a specific index in the current view state
fn get_menu_action_at_index(
    tray_state: &EnhancedTrayState,
    index: usize,
) -> Option<(String, enhanced_tray::TrayMenuAction)> {
    match &tray_state.current_view {
        TrayViewState::SingleApp {
            app_id,
            submenu_path,
            ..
        } => resolve_current_single_app_items(tray_state, app_id, submenu_path)
            .and_then(|items| items.iter().filter(|item| item.visible).nth(index))
            .map(|item| (app_id.clone(), item.action.clone())),
        TrayViewState::Aggregated { items, .. } | TrayViewState::Favorites { items } => items
            .get(index)
            .map(|item| (item.app_id.clone(), item.action.clone())),
        TrayViewState::Network { app_id, data, .. } => {
            let is_enabled = data.as_ref().map(|snapshot| snapshot.enabled).unwrap_or(false);
            let action = match index {
                0 => enhanced_tray::TrayMenuAction::SpawnCommand(if is_enabled {
                    "nmcli radio wifi off".to_string()
                } else {
                    "nmcli radio wifi on".to_string()
                }),
                1 => {
                    enhanced_tray::TrayMenuAction::SpawnCommand("nmcli device wifi rescan".to_string())
                }
                2 => {
                    enhanced_tray::TrayMenuAction::SpawnCommand("nm-connection-editor".to_string())
                }
                _ => {
                    let network_index = index.saturating_sub(3);
                    if let Some(network) = data
                        .as_ref()
                        .filter(|snapshot| snapshot.enabled)
                        .and_then(|snapshot| snapshot.networks.get(network_index))
                    {
                        enhanced_tray::TrayMenuAction::SpawnCommand(format!(
                            "nmcli device wifi connect \"{}\"",
                            network.ssid
                        ))
                    } else {
                        enhanced_tray::TrayMenuAction::SpawnCommand("true".to_string())
                    }
                }
            };
            Some((app_id.clone(), action))
        }
        TrayViewState::Mount { app_id, data, .. } => {
            if index == 0 {
                return Some((
                    app_id.clone(),
                    enhanced_tray::TrayMenuAction::SpawnCommand("mount:refresh".to_string()),
                ));
            }
            if index == 1 {
                return Some((
                    app_id.clone(),
                    enhanced_tray::TrayMenuAction::SpawnCommand("gnome-disks".to_string()),
                ));
            }
            let Some(snapshot) = data.as_ref() else {
                return Some((
                    app_id.clone(),
                    enhanced_tray::TrayMenuAction::SpawnCommand("true".to_string()),
                ));
            };
            let mut cursor = index.saturating_sub(2);

            let local_count = snapshot.local_devices.len().min(8);
            if cursor < local_count {
                let device = &snapshot.local_devices[cursor];
                let command = if device.mountpoint.is_some() {
                    crate::menus::mount::build_unmount_command(&format!("/dev/{}", device.name))
                } else {
                    crate::menus::mount::build_mount_command(&format!("/dev/{}", device.name), None)
                };
                return Some((
                    app_id.clone(),
                    enhanced_tray::TrayMenuAction::SpawnCommand(command),
                ));
            }
            cursor = cursor.saturating_sub(local_count);

            let sshfs_count = snapshot.sshfs_profiles.len().min(6);
            if cursor < sshfs_count {
                let profile = &snapshot.sshfs_profiles[cursor];
                let command = if profile.state == crate::menus::mount::MountState::Mounted {
                    crate::menus::mount::build_sshfs_unmount_command(profile)
                } else {
                    crate::menus::mount::build_sshfs_mount_command(profile)
                };
                return Some((
                    app_id.clone(),
                    enhanced_tray::TrayMenuAction::SpawnCommand(command),
                ));
            }
            cursor = cursor.saturating_sub(sshfs_count);

            let loop_count = snapshot.loop_mounts.len().min(6);
            if cursor < loop_count {
                let loop_mount = &snapshot.loop_mounts[cursor];
                let command = if let Some(loop_device) = &loop_mount.loop_device {
                    crate::menus::mount::build_loop_detach_command(loop_device)
                } else {
                    crate::menus::mount::build_loop_attach_command(
                        &loop_mount.image_path,
                        loop_mount.read_only,
                    )
                };
                return Some((
                    app_id.clone(),
                    enhanced_tray::TrayMenuAction::SpawnCommand(command),
                ));
            }
            cursor = cursor.saturating_sub(loop_count);

            let vc_count = snapshot.vcvolume_profiles.len().min(6);
            if cursor < vc_count {
                let profile = &snapshot.vcvolume_profiles[cursor];
                let command = if profile.state == crate::menus::mount::MountState::Mounted {
                    format!("umount '{}'", profile.mountpoint.replace('\'', "'\\''"))
                } else {
                    crate::menus::mount::build_vcvolume_mount_command(profile)
                };
                return Some((
                    app_id.clone(),
                    enhanced_tray::TrayMenuAction::SpawnCommand(command),
                ));
            }

            Some((
                app_id.clone(),
                enhanced_tray::TrayMenuAction::SpawnCommand("true".to_string()),
            ))
        }
        TrayViewState::Calendar { app_id, .. } => {
            let action = match index {
                0 => enhanced_tray::TrayMenuAction::SpawnCommand("calendar:refresh".to_string()),
                _ => enhanced_tray::TrayMenuAction::SpawnCommand("gnome-calendar".to_string()),
            };
            Some((app_id.clone(), action))
        }
    }
}
fn resolve_current_single_app_items<'a>(
    tray_state: &'a EnhancedTrayState,
    app_id: &str,
    submenu_path: &[String],
) -> Option<&'a [enhanced_tray::TrayMenuItem]> {
    let app = tray_state.tree.apps.get(app_id)?;
    let mut items: &'a [enhanced_tray::TrayMenuItem] = &app.menu_items;
    for segment in submenu_path {
        let next = items.iter().find(|item| item.id == *segment)?;
        if next.submenu.is_empty() {
            return None;
        }
        items = &next.submenu;
    }
    Some(items)
}

/// Animate progress value towards target
#[allow(dead_code)]
fn animate_progress(current: f32, target: f32, rate: f32) -> f32 {
    if (current - target).abs() < 0.001 {
        target
    } else {
        current + (target - current) * rate
    }
}

fn key_matches_named(key: &str, name: &str) -> bool {
    key.contains(name)
}
fn key_matches_char(key: &str, ch: char) -> bool {
    let lower = key.to_ascii_lowercase();
    let needle = format!("character(\"{}\")", ch.to_ascii_lowercase());
    lower.contains(&needle) || lower.contains(&format!("\"{}\"", ch.to_ascii_lowercase()))
}
fn extract_key_char(key: &str) -> Option<char> {
    let lower = key.to_ascii_lowercase();
    let marker = "character(\"";
    let start = lower.find(marker)? + marker.len();
    let rest = &lower[start..];
    let end = rest.find("\")")?;
    rest[..end].chars().next()
}
fn quickjump_supported_for_view(tray_state: &EnhancedTrayState) -> bool {
    matches!(
        tray_state.current_view,
        TrayViewState::SingleApp { .. }
            | TrayViewState::Aggregated { .. }
            | TrayViewState::Favorites { .. }
            | TrayViewState::Network { .. }
            | TrayViewState::Mount { .. }
            | TrayViewState::Calendar { .. }
    )
}
fn quickjump_alphabet_for_view(
    custom_config: &unilii_core::config::CustomMenuConfig,
    tray_state: &EnhancedTrayState,
) -> String {
    if let TrayViewState::SingleApp { app_id, .. } = &tray_state.current_view {
        if custom_config
            .app_ids
            .iter()
            .any(|configured| configured.eq_ignore_ascii_case(app_id))
        {
            return custom_config.quickjump_alphabet.clone();
        }
    }
    "asdfjkl;ghqwertyuiopzxcvbnm".to_string()
}
enum QuickjumpOutcome {
    Ignored,
    Pending,
    Activate(usize),
    Reset,
}
fn handle_quickjump_key(
    input: &mut String,
    alphabet: &str,
    item_count: usize,
    ch: char,
) -> QuickjumpOutcome {
    if !alphabet.contains(ch) {
        return QuickjumpOutcome::Ignored;
    }
    input.push(ch);
    let labels = crate::menus::common::generate_quickjump_labels(item_count, alphabet);
    if let Some(index) = labels.iter().position(|label| label == input) {
        return QuickjumpOutcome::Activate(index);
    }
    if !labels.iter().any(|label| label.starts_with(input.as_str())) {
        input.clear();
        return QuickjumpOutcome::Reset;
    }
    QuickjumpOutcome::Pending
}
fn sanitize_menu_label(label: &str) -> String {
    label.replace('_', "")
}
fn quickjump_prefixed_label(
    quickjump_active: bool,
    quickjump_labels: &[String],
    index: usize,
    base: impl Into<String>,
) -> String {
    let base = base.into();
    if !quickjump_active {
        return base;
    }
    match quickjump_labels.get(index) {
        Some(label) => format!("[{}] {}", label, base),
        None => base,
    }
}
fn evdev_digit_index(code: &str) -> Option<usize> {
    match code {
        "KEY_1" => Some(0),
        "KEY_2" => Some(1),
        "KEY_3" => Some(2),
        "KEY_4" => Some(3),
        "KEY_5" => Some(4),
        "KEY_6" => Some(5),
        "KEY_7" => Some(6),
        "KEY_8" => Some(7),
        "KEY_9" => Some(8),
        _ => None,
    }
}
fn current_menu_items_len(tray_state: &EnhancedTrayState) -> usize {
    match &tray_state.current_view {
        TrayViewState::SingleApp {
            app_id,
            submenu_path,
            ..
        } => resolve_current_single_app_items(tray_state, app_id, submenu_path)
            .map(|items| items.iter().filter(|item| item.visible).count())
            .unwrap_or(0),
        TrayViewState::Aggregated { items, .. } | TrayViewState::Favorites { items } => items.len(),
        TrayViewState::Network { data, .. } => 3
            + data
                .as_ref()
                .filter(|snapshot| snapshot.enabled)
                .map(|snapshot| snapshot.networks.len().min(6))
                .unwrap_or(0),
        TrayViewState::Mount { data, .. } => {
            2 + data
                .as_ref()
                .map(|snapshot| {
                    snapshot.local_devices.len().min(8)
                        + snapshot.sshfs_profiles.len().min(6)
                        + snapshot.loop_mounts.len().min(6)
                        + snapshot.vcvolume_profiles.len().min(6)
                })
                .unwrap_or(0)
        }
        TrayViewState::Calendar { data, .. } => {
            2 + data
                .as_ref()
                .map(|d| d.account_ids.len().min(6) + d.events.len().min(6))
                .unwrap_or(0)
        }
    }
}
fn tray_window_width(bar: &UniliiBar) -> f32 {
    let menu_items = bar
        .enhanced_tray
        .as_ref()
        .map(current_menu_items_len)
        .unwrap_or(6)
        .clamp(1, 12) as f32;
    let base_width = match bar.enhanced_tray.as_ref().map(|tray| &tray.current_view) {
        Some(TrayViewState::SingleApp { .. }) => 280.0,
        Some(TrayViewState::Aggregated { .. }) | Some(TrayViewState::Favorites { .. }) => 320.0,
        Some(TrayViewState::Network { data, .. }) => {
            if data.is_some() {
                340.0
            } else {
                280.0
            }
        }
        Some(TrayViewState::Mount { .. }) => 380.0,
        Some(TrayViewState::Calendar { .. }) => 360.0,
        None => bar.config.panels.first().map(|p| p.width).unwrap_or(1024) as f32,
    };
    (base_width + (menu_items.min(8.0) - 1.0) * 10.0).clamp(240.0, 560.0)
}
fn tray_window_height(bar: &UniliiBar) -> f32 {
    let bar_height = bar.config.panels.first().map(|p| p.height).unwrap_or(24) as f32;
    let menu_items = bar
        .enhanced_tray
        .as_ref()
        .map(current_menu_items_len)
        .unwrap_or(6)
        .clamp(1, 12) as f32;
    (bar_height + 42.0 + 26.0 + (menu_items * 34.0) + 20.0)
        .clamp(bar_height + 120.0, bar_height + 460.0)
}
fn tray_window_settings(bar: &UniliiBar) -> window::Settings {
    let panel = bar.config.panels.first();
    let width = tray_window_width(bar);
    let bar_height = panel.map(|p| p.height).unwrap_or(24) as f32;
    let menu_height = tray_window_height(bar);
    let pos_x = panel.map(|p| p.position_x).unwrap_or(0) as f32;
    let pos_y = panel.map(|p| p.position_y).unwrap_or(0) as f32;
    let mut settings = window::Settings {
        size: iced::Size::new(width, menu_height),
        position: iced::window::Position::Specific(iced::Point {
            x: pos_x,
            y: pos_y + bar_height,
        }),
        resizable: false,
        decorations: false,
        level: window::Level::AlwaysOnTop,
        ..window::Settings::default()
    };
    #[cfg(target_os = "linux")]
    {
        settings.platform_specific = window::settings::PlatformSpecific {
            application_id: "com.unilii.traymenu".to_string(),
            override_redirect: !bar.run_options.debug_focus,
        };
        if bar.run_options.debug_focus {
            settings.decorations = true;
            settings.resizable = true;
            settings.level = window::Level::Normal;
        }
    }
    settings
}
fn resize_window_task(bar: &mut UniliiBar, menu_open: bool) -> Task<Message> {
    if menu_open {
        if bar.tray_window_id.is_some() {
            return Task::none();
        }
        let (id, task) = window::open(tray_window_settings(bar));
        bar.tray_window_id = Some(id);
        return task.map(Message::WindowOpened);
    }
    bar.tray_quickjump_active = false;
    bar.tray_quickjump_input.clear();
    if let Some(id) = bar.tray_window_id.take() {
        return window::close(id).map(move |_: ()| Message::WindowClosed(id));
    }
    Task::none()
}
fn handle_evdev_tray_key(bar: &mut UniliiBar, code: &str, value: i32) -> Option<Task<Message>> {
    if value == 0 {
        return None;
    }
    if let Some(tray_state) = bar.enhanced_tray.as_mut() {
        match code {
            "KEY_ESC" => {
                tray_state.animation_target = 0.0;
                return Some(resize_window_task(bar, false));
            }
            "KEY_DOWN" | "KEY_TAB" => {
                let count = get_current_menu_item_count(tray_state);
                if count > 0 {
                    tray_state.selected_index = Some(match tray_state.selected_index {
                        None => 0,
                        Some(i) => (i + 1) % count,
                    });
                }
                return Some(Task::none());
            }
            "KEY_UP" => {
                let count = get_current_menu_item_count(tray_state);
                if count > 0 {
                    tray_state.selected_index = Some(match tray_state.selected_index {
                        None => count.saturating_sub(1),
                        Some(i) => {
                            if i == 0 {
                                count - 1
                            } else {
                                i - 1
                            }
                        }
                    });
                }
                return Some(Task::none());
            }
            "KEY_LEFT" => return Some(Task::done(Message::TrayNavigateLeft)),
            "KEY_RIGHT" => return Some(Task::done(Message::TrayNavigateRight)),
            "KEY_ENTER" | "KEY_KPENTER" => {
                if let Some(idx) = tray_state.selected_index {
                    if let Some((app_id, action)) = get_menu_action_at_index(tray_state, idx) {
                        tray_state.animation_target = 0.0;
                        return Some(Task::batch(vec![
                            Task::done(Message::TrayMenuTriggered(app_id, action)),
                            resize_window_task(bar, false),
                        ]));
                    }
                }
                return Some(Task::none());
            }
            _ => {}
        }
    }
    if bar.shift_held {
        if let Some(idx) = evdev_digit_index(code) {
            if let Some(icon) = bar.tray_icons.get(idx) {
                return Some(Task::done(Message::TrayIconPressed(icon.key.clone())));
            }
        }
    }
    None
}
async fn read_mount_snapshot(
    config: unilii_core::config::MountMenuConfig,
) -> Result<crate::menus::mount::MountMenuSnapshot, String> {
    let output = tokio::process::Command::new("sh")
        .arg("-lc")
        .arg("lsblk -P -o NAME,TYPE,FSTYPE,SIZE,MOUNTPOINT,RO,RM,LABEL,MODEL")
        .output()
        .await
        .map_err(|e| format!("failed to run lsblk: {}", e))?;

    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let mut devices = crate::menus::mount::parse_lsblk_pairs(&stdout);
    if devices.len() > config.max_local_rows {
        devices.truncate(config.max_local_rows);
    }

    let mounts = read_proc_mounts().await.unwrap_or_default();
    let mountpoints: std::collections::HashSet<String> =
        mounts.iter().map(|entry| entry.target.clone()).collect();

    let sshfs_profiles = config
        .sshfs_profiles
        .iter()
        .map(|profile| {
            let mounted = mounts.iter().any(|entry| {
                entry.target == profile.mountpoint
                    && (entry.fstype.contains("sshfs")
                        || entry.source
                            == format!("{}@{}:{}", profile.user, profile.host, profile.remote_path))
            });
            crate::menus::mount::SshfsProfile {
                name: profile.name.clone(),
                user: profile.user.clone(),
                host: profile.host.clone(),
                remote_path: profile.remote_path.clone(),
                mountpoint: profile.mountpoint.clone(),
                options: profile.options.clone(),
                state: if mounted {
                    crate::menus::mount::MountState::Mounted
                } else {
                    crate::menus::mount::MountState::Unmounted
                },
            }
        })
        .collect::<Vec<_>>();

    let loop_mounts = if config.show_loop_devices {
        read_loop_mounts(&devices)?
    } else {
        Vec::new()
    };

    let vcvolume_profiles = config
        .vcvolume_profiles
        .iter()
        .map(|profile| crate::menus::mount::VcvolumeProfile {
            name: profile.name.clone(),
            volume_path: profile.volume_path.clone(),
            mountpoint: profile.mountpoint.clone(),
            command_template: profile.command_template.clone(),
            state: if mountpoints.contains(&profile.mountpoint) {
                crate::menus::mount::MountState::Mounted
            } else {
                crate::menus::mount::MountState::Unmounted
            },
        })
        .collect::<Vec<_>>();

    Ok(crate::menus::mount::MountMenuSnapshot {
        local_devices: devices,
        sshfs_profiles,
        loop_mounts,
        vcvolume_profiles,
    })
}

#[derive(Debug, Clone)]
struct ProcMountEntry {
    source: String,
    target: String,
    fstype: String,
}

async fn read_proc_mounts() -> Result<Vec<ProcMountEntry>, String> {
    let contents = tokio::fs::read_to_string("/proc/mounts")
        .await
        .map_err(|error| format!("failed to read /proc/mounts: {}", error))?;

    let mut entries = Vec::new();
    for line in contents.lines() {
        let mut fields = line.split_whitespace();
        let (Some(source), Some(target), Some(fstype)) =
            (fields.next(), fields.next(), fields.next())
        else {
            continue;
        };
        entries.push(ProcMountEntry {
            source: unescape_mount_field(source),
            target: unescape_mount_field(target),
            fstype: fstype.to_string(),
        });
    }
    Ok(entries)
}

fn unescape_mount_field(value: &str) -> String {
    value
        .replace("\\040", " ")
        .replace("\\011", "\t")
        .replace("\\012", "\n")
        .replace("\\134", "\\")
}

fn read_loop_mounts(
    local_devices: &[crate::menus::mount::LocalDevice],
) -> Result<Vec<crate::menus::mount::LoopMount>, String> {
    let output = match std::process::Command::new("sh")
        .arg("-lc")
        .arg("losetup --list --noheadings --raw --output NAME,RO,BACK-FILE")
        .output()
    {
        Ok(output) => output,
        Err(_) => return Ok(Vec::new()),
    };

    if !output.status.success() {
        return Ok(Vec::new());
    }

    let mut mountpoints_by_name = std::collections::HashMap::new();
    for device in local_devices {
        mountpoints_by_name.insert(device.name.clone(), device.mountpoint.clone());
    }

    let mut loop_mounts = Vec::new();
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let Some((loop_device, read_only, image_path)) =
            crate::menus::mount::parse_losetup_list_row(trimmed)
        else {
            continue;
        };
        let loop_name = loop_device.trim_start_matches("/dev/").to_string();
        let mountpoint = mountpoints_by_name.get(&loop_name).cloned().flatten();
        let state = if mountpoint.is_some() {
            crate::menus::mount::MountState::Mounted
        } else {
            crate::menus::mount::MountState::Unmounted
        };
        loop_mounts.push(crate::menus::mount::LoopMount {
            image_path,
            loop_device: Some(loop_device),
            mountpoint,
            read_only,
            state,
        });
    }
    Ok(loop_mounts)
}

async fn read_calendar_snapshot(
    accounts: Vec<unilii_core::config::CalendarAccountConfig>,
    agenda_days: u32,
) -> Result<crate::menus::calendar::CalendarMenuSnapshot, String> {
    use unilii_lib::calendar::{
        CalendarProvider, caldav::CalDavProvider, caldav::CalDavProviderConfig,
    };

    if accounts.is_empty() {
        return Ok(crate::menus::calendar::CalendarMenuSnapshot::from_accounts(
            Vec::new(),
        ));
    }

    let window_start = chrono::Utc::now();
    let window_end = window_start + chrono::Duration::days(agenda_days as i64);
    let window_start_rfc3339 = window_start.to_rfc3339();
    let window_end_rfc3339 = window_end.to_rfc3339();

    let mut agenda_items = Vec::new();
    let mut account_errors = Vec::new();
    let account_ids = accounts
        .iter()
        .map(|account| account.id.clone())
        .collect::<Vec<_>>();

    for account in &accounts {
        let provider = CalDavProvider::new(CalDavProviderConfig {
            account_id: account.id.clone(),
            base_url: account.base_url.clone(),
            principal_url: account.principal_url.clone(),
            calendar_url: account.calendar_url.clone(),
            username: account.username.clone(),
            secret_ref: account.secret_ref.clone(),
        });

        match provider.fetch_events(
            &account.id,
            &window_start_rfc3339,
            &window_end_rfc3339,
            None,
        ) {
            Ok((events, _sync_token)) => {
                agenda_items.extend(events.into_iter().map(|event| {
                    crate::menus::calendar::CalendarAgendaItem {
                        account_id: event.account_id,
                        title: event.title,
                        start_rfc3339: event.start_rfc3339,
                        location: event.location,
                    }
                }));
            }
            Err(error) => {
                account_errors.push(crate::menus::calendar::CalendarAccountError {
                    account_id: account.id.clone(),
                    message: error.to_string(),
                });
            }
        }
    }

    agenda_items.sort_by(|left, right| {
        left.start_rfc3339
            .cmp(&right.start_rfc3339)
            .then(left.title.cmp(&right.title))
    });
    if agenda_items.len() > 48 {
        agenda_items.truncate(48);
    }

    let status = if account_errors.is_empty() {
        format!(
            "Synced {} event(s) from {} account(s)",
            agenda_items.len(),
            account_ids.len()
        )
    } else {
        format!(
            "Partial sync: {} event(s), {} account error(s)",
            agenda_items.len(),
            account_errors.len()
        )
    };

    let stale = !account_errors.is_empty();

    Ok(crate::menus::calendar::CalendarMenuSnapshot {
        account_ids,
        events: agenda_items,
        account_errors,
        stale,
        status,
    })
}

fn is_mount_icon(icon: &tray::TrayIcon) -> bool {
    let blob = format!(
        "{} {} {}",
        icon.title.to_ascii_lowercase(),
        icon.id.to_ascii_lowercase(),
        icon.icon_name
            .as_deref()
            .unwrap_or_default()
            .to_ascii_lowercase()
    );
    blob.contains("drive")
        || blob.contains("disk")
        || blob.contains("mount")
        || blob.contains("usb")
}

fn is_calendar_icon(icon: &tray::TrayIcon) -> bool {
    let blob = format!(
        "{} {} {}",
        icon.title.to_ascii_lowercase(),
        icon.id.to_ascii_lowercase(),
        icon.icon_name
            .as_deref()
            .unwrap_or_default()
            .to_ascii_lowercase()
    );
    blob.contains("calendar") || blob.contains("date") || blob.contains("caldav")
}

fn is_custom_menu_icon(
    icon: &tray::TrayIcon,
    config: &unilii_core::config::CustomMenuConfig,
) -> bool {
    if !config.enabled || config.items.is_empty() {
        return false;
    }
    if config
        .app_ids
        .iter()
        .any(|app_id| app_id.eq_ignore_ascii_case(&icon.id))
    {
        return true;
    }
    let icon_name = icon
        .icon_name
        .as_deref()
        .unwrap_or_default()
        .to_ascii_lowercase();
    config
        .icon_name_patterns
        .iter()
        .any(|pattern| icon_name.contains(&pattern.to_ascii_lowercase()))
}

fn build_custom_menu_items(
    icon: &enhanced_tray::TrayIcon,
    config: &unilii_core::config::CustomMenuConfig,
) -> Vec<enhanced_tray::TrayMenuItem> {
    let snapshot = crate::menus::custom::CustomMenuSnapshot::from_config(config);
    snapshot
        .items
        .into_iter()
        .map(|item| enhanced_tray::TrayMenuItem {
            id: item.id.clone(),
            label: item.title,
            action: enhanced_tray::TrayMenuAction::SpawnCommand(item.action_command),
            icon: item
                .icon_theme
                .or(item.icon_svg_path)
                .or(item.icon_image_path),
            submenu: Vec::new(),
            enabled: true,
            visible: true,
            checkable: false,
            checked: false,
            shortcut: None,
            is_separator: false,
            app_id: icon.id.clone(),
            full_path: item.id,
            widget_type: enhanced_tray::TrayWidgetType::Button,
            default_value: None,
            placeholder: None,
        })
        .collect()
}

fn parse_key_dry_run_events(input: &str) -> std::result::Result<Vec<KeyDryRunEvent>, String> {
    let mut events = Vec::new();
    let mut fallback_ms = 0u64;

    for token in input
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        let (core, at_ms) = if let Some((left, right)) = token.rsplit_once('@') {
            let parsed = right
                .parse::<u64>()
                .map_err(|error| format!("invalid timestamp '{}': {}", right, error))?;
            (left.trim(), parsed)
        } else {
            let current = fallback_ms;
            fallback_ms = fallback_ms.saturating_add(10);
            (token, current)
        };

        let (key, value_raw) = core
            .split_once(':')
            .ok_or_else(|| format!("event '{}' must be KEY:VALUE", token))?;
        let value = value_raw
            .trim()
            .parse::<i32>()
            .map_err(|error| format!("invalid value '{}' in '{}': {}", value_raw, token, error))?;

        events.push(KeyDryRunEvent {
            key: key.trim().to_string(),
            value,
            at_ms,
        });
    }

    if events.is_empty() {
        return Err("no events provided".to_string());
    }

    Ok(events)
}
enum ThemeIconAsset {
    Raster(PathBuf),
    Svg(PathBuf),
}
fn icon_search_roots() -> Vec<PathBuf> {
    let mut roots = vec![
        PathBuf::from("/usr/share/icons"),
        PathBuf::from("/usr/local/share/icons"),
        PathBuf::from("/usr/share/pixmaps"),
    ];
    if let Ok(home) = env::var("HOME") {
        roots.insert(0, PathBuf::from(format!("{home}/.local/share/icons")));
        roots.insert(1, PathBuf::from(format!("{home}/.icons")));
    }
    roots
}
fn icon_file_candidate(path: PathBuf) -> Option<ThemeIconAsset> {
    if path.is_file() {
        match path
            .extension()
            .and_then(|e| e.to_str())
            .map(|s| s.to_ascii_lowercase())
        {
            Some(ext) if ext == "svg" => Some(ThemeIconAsset::Svg(path)),
            Some(ext) if ext == "png" || ext == "jpg" || ext == "jpeg" || ext == "webp" => {
                Some(ThemeIconAsset::Raster(path))
            }
            _ => None,
        }
    } else {
        None
    }
}
fn find_theme_icon(icon_name: &str) -> Option<ThemeIconAsset> {
    let base = Path::new(icon_name);
    if (icon_name.contains('/') || icon_name.contains('.')) && base.is_file() {
        return icon_file_candidate(base.to_path_buf());
    }
    let names = [
        icon_name.to_string(),
        icon_name.replace("-symbolic", ""),
        format!("{icon_name}-symbolic"),
    ];
    let themes = [
        "Papirus-Dark",
        "Papirus",
        "Adwaita",
        "hicolor",
        "breeze",
        "default",
    ];
    let sizes = [
        "16x16", "18x18", "20x20", "22x22", "24x24", "32x32", "scalable", "symbolic",
    ];
    let cats = [
        "status",
        "apps",
        "actions",
        "devices",
        "places",
        "categories",
        "panel",
        "mimetypes",
    ];
    for root in icon_search_roots() {
        for name in &names {
            if let Some(found) = icon_file_candidate(root.join(format!("{name}.png"))) {
                return Some(found);
            }
            if let Some(found) = icon_file_candidate(root.join(format!("{name}.svg"))) {
                return Some(found);
            }
            for theme in &themes {
                for size in &sizes {
                    for cat in &cats {
                        if let Some(found) = icon_file_candidate(
                            root.join(theme)
                                .join(size)
                                .join(cat)
                                .join(format!("{name}.png")),
                        ) {
                            return Some(found);
                        }
                        if let Some(found) = icon_file_candidate(
                            root.join(theme)
                                .join(size)
                                .join(cat)
                                .join(format!("{name}.svg")),
                        ) {
                            return Some(found);
                        }
                    }
                }
            }
        }
    }
    None
}
fn icon_badge_label(icon_name: Option<&str>, title: &str, id: &str, service: &str) -> String {
    if let Some(name) = icon_name {
        let symbolic = tray::icon_label_for_name(name);
        if symbolic != "◉" {
            return symbolic;
        }
    }
    tray::icon_label_for(&tray::TrayIcon {
        key: String::new(),
        service: service.to_string(),
        path: String::new(),
        id: id.to_string(),
        title: title.to_string(),
        icon_name: icon_name.map(|s| s.to_string()),
        icon_pixmap: None,
        status: String::new(),
        has_menu: false,
        menu_object_path: None,
    })
}
fn pixmap_handle(pixmap: Option<&tray::TrayIconPixmap>) -> Option<image::Handle> {
    let pixmap = pixmap?;
    if pixmap.width <= 0 || pixmap.height <= 0 {
        return None;
    }
    let mut rgba = Vec::with_capacity(pixmap.data.len());
    for chunk in pixmap.data.chunks_exact(4) {
        rgba.push(chunk[1]);
        rgba.push(chunk[2]);
        rgba.push(chunk[3]);
        rgba.push(chunk[0]);
    }
    Some(image::Handle::from_rgba(
        pixmap.width as u32,
        pixmap.height as u32,
        rgba,
    ))
}
fn menu_panel_style() -> container::Style {
    container::Style {
        background: Some(iced::Background::Color([0.08, 0.09, 0.11, 0.97].into())),
        border: iced::Border {
            width: 1.0,
            color: [0.24, 0.26, 0.30, 1.0].into(),
            radius: 12.0.into(),
        },
        ..Default::default()
    }
}
#[allow(dead_code)]
fn tray_window_background_style() -> container::Style {
    container::Style {
        background: Some(iced::Background::Color([0.06, 0.07, 0.09, 0.97].into())),
        ..Default::default()
    }
}
fn render_icon_badge(
    icon_name: Option<&str>,
    icon_pixmap: Option<&tray::TrayIconPixmap>,
    title: &str,
    id: &str,
    service: &str,
    size: f32,
) -> Element<'static, Message> {
    if let Some(handle) = pixmap_handle(icon_pixmap) {
        return image(handle).width(size).height(size).into();
    }
    if let Some(name) = icon_name {
        if let Some(asset) = find_theme_icon(name) {
            return match asset {
                ThemeIconAsset::Svg(path) => svg(svg::Handle::from_path(path))
                    .width(size)
                    .height(size)
                    .into(),
                ThemeIconAsset::Raster(path) => image(image::Handle::from_path(path))
                    .width(size)
                    .height(size)
                    .into(),
            };
        }
    }
    container(text(icon_badge_label(icon_name, title, id, service)).size(size - 2.0))
        .padding([2, 4])
        .style(|_theme| container::Style {
            background: Some(iced::Background::Color([0.18, 0.18, 0.22, 1.0].into())),
            border: iced::Border {
                width: 1.0,
                color: [0.28, 0.28, 0.34, 1.0].into(),
                radius: 8.0.into(),
            },
            ..Default::default()
        })
        .into()
}
fn render_tray_button_content(
    icon: &tray::TrayIcon,
    shift_held: bool,
    index: usize,
) -> Element<'static, Message> {
    let mut content = row!().spacing(6).align_y(iced::Alignment::Center);
    if shift_held {
        content = content.push(text(format!("{}", index + 1)).size(11));
    }
    content = content.push(render_icon_badge(
        icon.icon_name.as_deref(),
        icon.icon_pixmap.as_ref(),
        icon.title.as_str(),
        icon.id.as_str(),
        icon.service.as_str(),
        16.0,
    ));
    content.into()
}
fn render_enhanced_icon_badge(
    icon: &enhanced_tray::TrayIcon,
    size: f32,
) -> Element<'static, Message> {
    render_icon_badge(
        icon.icon_name.as_deref(),
        icon.icon_pixmap.as_ref(),
        icon.title.as_str(),
        icon.id.as_str(),
        icon.service.as_str(),
        size,
    )
}
/// Render the enhanced tray menu view
fn render_enhanced_tray_menu<'a>(
    bar: &'a UniliiBar,
    tray_state: &'a EnhancedTrayState,
) -> Element<'a, Message> {
    let quickjump_labels = if bar.tray_quickjump_active && quickjump_supported_for_view(tray_state)
    {
        crate::menus::common::generate_quickjump_labels(
            get_current_menu_item_count(tray_state),
            &quickjump_alphabet_for_view(&bar.config.menus.custom, tray_state),
        )
    } else {
        Vec::new()
    };

    let content = match &tray_state.current_view {
        TrayViewState::SingleApp {
            app_id, navigation, ..
        } => render_single_app_view_with_main_messages(
            tray_state,
            app_id,
            navigation,
            bar.tray_quickjump_active,
            &bar.tray_quickjump_input,
            &quickjump_labels,
        ),
        TrayViewState::Aggregated { items, filter } => {
            render_aggregated_view_with_main_messages(tray_state, items, filter)
        }
        TrayViewState::Favorites { items } => {
            render_favorites_view_with_main_messages(tray_state, items)
        }
        TrayViewState::Network {
            app_id,
            data,
            loading,
            error,
        } => render_network_view_with_main_messages(
            tray_state,
            app_id,
            data,
            *loading,
            error,
            bar.tray_quickjump_active,
            &quickjump_labels,
        ),
        TrayViewState::Mount {
            app_id,
            data,
            loading,
            error,
        } => render_mount_view_with_main_messages(
            tray_state,
            app_id,
            data,
            *loading,
            error,
            bar.tray_quickjump_active,
            &quickjump_labels,
        ),
        TrayViewState::Calendar {
            app_id,
            data,
            loading,
            error,
        } => render_calendar_view_with_main_messages(
            tray_state,
            app_id,
            data,
            *loading,
            error,
            bar.tray_quickjump_active,
            &quickjump_labels,
        ),
    };

    let opacity = tray_state.animation_progress.clamp(0.0, 1.0);

    container(content)
        .width(Length::Fill)
        .height(Length::Fill)
        .padding([4, 8])
        .style(move |_theme| {
            let mut appearance = menu_panel_style();

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
    quickjump_active: bool,
    quickjump_input: &str,
    quickjump_labels: &[String],
) -> Element<'a, Message> {
    let app_menu = state.tree.apps.get(app_id);

    let mut content = column!().spacing(6);

    let mut title_row = row!().spacing(8).align_y(iced::Alignment::Center);
    if matches!(&state.current_view, TrayViewState::SingleApp { submenu_path, .. } if !submenu_path.is_empty())
    {
        title_row = title_row.push(button(text("↩").size(12)).on_press(Message::TrayExitSubmenu));
    }

    if navigation.can_go_left {
        title_row = title_row.push(button(text("◀").size(12)).on_press(Message::TrayNavigateLeft));
    }

    if let Some(app) = app_menu {
        title_row = title_row.push(render_enhanced_icon_badge(&app.icon, 18.0));
        title_row = title_row.push(text(&app.icon.title).size(14));
    } else {
        title_row = title_row.push(text(app_id).size(14));
    }

    if navigation.can_go_right {
        title_row = title_row.push(button(text("▶").size(12)).on_press(Message::TrayNavigateRight));
    }

    content = content.push(title_row);

    if quickjump_active {
        content = content.push(
            text(format!(
                "Quickjump [{}] — type label (Esc to cancel)",
                quickjump_input
            ))
            .size(10),
        );
    }

    if let Some(app) = app_menu {
        let menu_items = render_menu_items_with_main_messages(
            resolve_current_single_app_items(
                state,
                app_id,
                match &state.current_view {
                    TrayViewState::SingleApp { submenu_path, .. } => submenu_path.as_slice(),
                    _ => &[],
                },
            )
            .unwrap_or(&app.menu_items),
            state.selected_index,
            app_id,
            match &state.current_view {
                TrayViewState::SingleApp { submenu_path, .. } => submenu_path.as_slice(),
                _ => &[],
            },
            quickjump_active,
            quickjump_labels,
        );
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

    content = content.push(text("All Menu Items").size(14));

    content = content.push(
        text_input("Search menu items...", filter.as_deref().unwrap_or(""))
            .on_input(Message::TrayFilterUpdate)
            .size(12)
            .padding([2, 4]),
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

    content = content.push(text("Favorite Items ⭐").size(14));

    if items.is_empty() {
        content = content
            .push(text("No favorites yet. Press 'f' on any menu item to add it here.").size(12));
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
    quickjump_active: bool,
    quickjump_labels: &[String],
) -> Element<'a, Message> {
    let mut content = column!().spacing(2);

    content = content.push(text("Network Settings").size(14));
    if quickjump_active {
        content = content.push(text("Quickjump active").size(10));
    }

    if loading {
        content = content.push(text("⟳ Loading...").size(12));
    } else if let Some(err) = error {
        content = content.push(text(format!("⚠ Error: {}", err)).size(12));
    }

    let controls = render_network_controls_with_main_messages(
        app_id,
        data,
        quickjump_active,
        quickjump_labels,
    );
    content = content.push(controls);

    if let Some(snapshot) = data {
        if snapshot.enabled && !snapshot.networks.is_empty() {
            let networks =
                render_network_list_with_main_messages(app_id, snapshot, quickjump_active, quickjump_labels, 3);
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
    current_submenu_path: &[String],
    quickjump_active: bool,
    quickjump_labels: &[String],
) -> Element<'a, Message> {
    if !items.iter().any(|item| item.visible) {
        return text("No menu items").size(12).into();
    }

    let mut menu_col = column!().spacing(1);

    for (index, item) in items.iter().filter(|item| item.visible).enumerate() {
        let item_widget = render_menu_item_with_main_messages(
            item,
            selected_index == Some(index),
            app_id,
            current_submenu_path,
            quickjump_active,
            quickjump_labels.get(index).cloned(),
        );
        menu_col = menu_col.push(item_widget);
    }

    if items.iter().filter(|item| item.visible).count() > 8 {
        scrollable(menu_col).height(Length::Fixed(200.0)).into()
    } else {
        menu_col.into()
    }
}

fn render_menu_item_with_main_messages<'a>(
    item: &'a enhanced_tray::TrayMenuItem,
    is_selected: bool,
    app_id: &'a str,
    current_submenu_path: &[String],
    quickjump_active: bool,
    quickjump_label: Option<String>,
) -> Element<'a, Message> {
    if item.is_separator {
        return text("─".repeat(20)).size(10).into();
    }

    if matches!(
        item.widget_type,
        enhanced_tray::core::TrayWidgetType::TextInput
    ) || matches!(
        &item.action,
        enhanced_tray::TrayMenuAction::TextInputChanged { .. }
    ) {
        return text_input(
            item.placeholder.as_deref().unwrap_or("Enter value..."),
            item.default_value.as_deref().unwrap_or(""),
        )
        .on_input(move |value| Message::TrayTextInputChanged(item.id.clone(), value))
        .size(12)
        .padding([2, 4])
        .width(Length::Fixed(200.0))
        .into();
    }
    let mut label = sanitize_menu_label(&item.label);
    if quickjump_active {
        if let Some(hint) = quickjump_label {
            label = format!("[{}] {}", hint, label);
        }
    }

    if item.checkable {
        label = format!("{} {}", if item.checked { "☑" } else { "☐" }, label);
    }

    let shortcut_hint = item.shortcut.clone();
    let submenu_hint = if !item.submenu.is_empty() {
        Some("›".to_string())
    } else {
        None
    };

    let mut row_content = row!().spacing(8).align_y(iced::Alignment::Center);
    if let Some(icon) = render_menu_item_icon(item.icon.as_deref()) {
        row_content = row_content.push(icon);
    }
    row_content = row_content
        .push(text(label).size(12))
        .push(Space::new())
        .push(text(submenu_hint.or(shortcut_hint).unwrap_or_default()).size(11));

    let mut btn = button(row_content).padding([6, 10]).width(Length::Fill);
    btn = if !item.enabled {
        btn.style(button::text)
    } else if is_selected {
        btn.style(button::primary)
    } else {
        btn.style(button::secondary)
    };

    if item.enabled {
        if item.submenu.is_empty() {
            btn.on_press(Message::TrayMenuTriggered(
                app_id.to_string(),
                item.action.clone(),
            ))
            .into()
        } else {
            let mut submenu_path = current_submenu_path.to_vec();
            submenu_path.push(item.id.clone());
            btn.on_press(Message::TrayEnterSubmenu(app_id.to_string(), submenu_path))
                .into()
        }
    } else {
        btn.into()
    }
}

fn render_menu_item_icon(icon: Option<&str>) -> Option<Element<'static, Message>> {
    let icon = icon?;
    if icon.trim().is_empty() {
        return None;
    }
    if let Some(asset) = find_theme_icon(icon) {
        return Some(match asset {
            ThemeIconAsset::Svg(path) => svg(svg::Handle::from_path(path))
                .width(16.0)
                .height(16.0)
                .into(),
            ThemeIconAsset::Raster(path) => image(image::Handle::from_path(path))
                .width(16.0)
                .height(16.0)
                .into(),
        });
    }
    Some(text("•").size(12).into())
}

fn render_aggregated_items_with_main_messages<'a>(
    items: &'a [enhanced_tray::TrayMenuItem],
) -> Element<'a, Message> {
    let mut items_col = column!().spacing(1);

    for item in items.iter().take(10) {
        let item_row = row![
            text("⭐").size(10),
            text(&item.full_path).size(11),
            Space::new(),
            button(text("★").size(10)).on_press(Message::TrayToggleFavorite(
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
        items_col =
            items_col.push(text(format!("... and {} more items", items.len() - 10)).size(10));
    }

    scrollable(items_col).height(Length::Fixed(200.0)).into()
}

fn render_favorite_items_with_main_messages<'a>(
    items: &'a [enhanced_tray::TrayMenuItem],
) -> Element<'a, Message> {
    let mut items_col = column!().spacing(1);

    for item in items {
        let item_row = row![
            text("⭐").size(10),
            text(&item.full_path).size(11),
            button(text("✗").size(10)).on_press(Message::TrayToggleFavorite(
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

    scrollable(items_col).height(Length::Fixed(200.0)).into()
}

fn render_network_controls_with_main_messages<'a>(
    app_id: &'a str,
    data: &'a Option<crate::tray::NetworkSnapshot>,
    quickjump_active: bool,
    quickjump_labels: &[String],
) -> Element<'a, Message> {
    let is_enabled = data.as_ref().map(|d| d.enabled).unwrap_or(false);

    row![
        button(
            text(quickjump_prefixed_label(quickjump_active, quickjump_labels, 0, if is_enabled {
                "Disable Wi-Fi"
            } else {
                "Enable Wi-Fi"
            }))
            .size(12)
        )
        .padding([2, 6])
        .on_press(Message::TrayNetworkToggle(app_id.to_string())),
        button(text(quickjump_prefixed_label(
            quickjump_active,
            quickjump_labels,
            1,
            "Rescan",
        ))
        .size(12))
        .padding([2, 6])
        .on_press(Message::TraySpawnCommand(
            app_id.to_string(),
            "nmcli device wifi rescan".to_string()
        )),
        button(text(quickjump_prefixed_label(
            quickjump_active,
            quickjump_labels,
            2,
            "Settings",
        ))
        .size(12))
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
    quickjump_active: bool,
    quickjump_labels: &[String],
    start_index: usize,
) -> Element<'a, Message> {
    let mut networks_col = column!().spacing(1);

    networks_col = networks_col.push(text("Available Networks:").size(12));

    for (offset, network) in snapshot.networks.iter().take(6).enumerate() {
        let mut label = format!("{} ({}%)", network.ssid, network.signal);

        if snapshot.state == "connected" && snapshot.interface == network.ssid {
            label = format!("● {}", label);
        }
        label = quickjump_prefixed_label(
            quickjump_active,
            quickjump_labels,
            start_index + offset,
            label,
        );

        let network_btn = button(text(label).size(11))
            .padding([1, 4])
            .width(Length::Fill)
            .on_press(Message::TraySpawnCommand(
                app_id.to_string(),
                format!("nmcli device wifi connect \"{}\"", network.ssid),
            ));

        networks_col = networks_col.push(network_btn);
    }

    scrollable(networks_col).height(Length::Fixed(150.0)).into()
}

fn render_mount_view_with_main_messages<'a>(
    _state: &'a EnhancedTrayState,
    app_id: &'a str,
    data: &'a Option<crate::menus::mount::MountMenuSnapshot>,
    loading: bool,
    error: &'a Option<String>,
    quickjump_active: bool,
    quickjump_labels: &[String],
) -> Element<'a, Message> {
    let mut content = column!().spacing(2);
    content = content.push(text("Mount / SSHFS / Loop / VCVolume").size(14));
    if quickjump_active {
        content = content.push(text("Quickjump active").size(10));
    }

    let mut quickjump_index = 0usize;

    if loading {
        content = content.push(text("⟳ Loading storage snapshot...").size(12));
    } else if let Some(err) = error {
        content = content.push(text(format!("⚠ Error: {}", err)).size(12));
    }

    content = content.push(
        row![
            button(text(quickjump_prefixed_label(
                quickjump_active,
                quickjump_labels,
                quickjump_index,
                "Refresh",
            ))
            .size(12))
            .padding([2, 6])
            .on_press(Message::TrayMountRefresh(app_id.to_string())),
            button(text(quickjump_prefixed_label(
                quickjump_active,
                quickjump_labels,
                quickjump_index + 1,
                "Disks",
            ))
            .size(12))
            .padding([2, 6])
            .on_press(Message::TraySpawnCommand(
                app_id.to_string(),
                "gnome-disks".to_string()
            )),
        ]
        .spacing(4),
    );
    quickjump_index += 2;
    if let Some(snapshot) = data {
        if !snapshot.local_devices.is_empty() {
            content = content.push(text("Local Devices:").size(12));
            for device in snapshot.local_devices.iter().take(8) {
                let mounted = device.mountpoint.is_some();
                let prefix = if mounted { "●" } else { "○" };
                let label = format!(
                    "{} {} {}",
                    prefix,
                    device.name,
                    device
                        .mountpoint
                        .clone()
                        .unwrap_or_else(|| "(unmounted)".to_string())
                );
                let command = if mounted {
                    crate::menus::mount::build_unmount_command(&format!("/dev/{}", device.name))
                } else {
                    crate::menus::mount::build_mount_command(&format!("/dev/{}", device.name), None)
                };
                let label = quickjump_prefixed_label(
                    quickjump_active,
                    quickjump_labels,
                    quickjump_index,
                    label,
                );
                content = content.push(
                    button(text(label).size(11))
                        .padding([1, 4])
                        .width(Length::Fill)
                        .on_press(Message::TraySpawnCommand(app_id.to_string(), command)),
                );
                quickjump_index += 1;
            }
        }

        if !snapshot.sshfs_profiles.is_empty() {
            content = content.push(text("SSHFS Profiles:").size(12));
            for profile in snapshot.sshfs_profiles.iter().take(6) {
                let mounted = profile.state == crate::menus::mount::MountState::Mounted;
                let label = if mounted {
                    format!("● {} ({})", profile.name, profile.mountpoint)
                } else {
                    format!("○ {} ({})", profile.name, profile.host)
                };
                let command = if mounted {
                    crate::menus::mount::build_sshfs_unmount_command(profile)
                } else {
                    crate::menus::mount::build_sshfs_mount_command(profile)
                };
                let label = quickjump_prefixed_label(
                    quickjump_active,
                    quickjump_labels,
                    quickjump_index,
                    label,
                );
                content = content.push(
                    button(text(label).size(11))
                        .padding([1, 4])
                        .width(Length::Fill)
                        .on_press(Message::TraySpawnCommand(app_id.to_string(), command)),
                );
                quickjump_index += 1;
            }
        }

        if !snapshot.loop_mounts.is_empty() {
            content = content.push(text("Loop Devices:").size(12));
            for loop_mount in snapshot.loop_mounts.iter().take(6) {
                let attached = loop_mount.loop_device.is_some();
                let ro_label = if loop_mount.read_only { "[RO]" } else { "[RW]" };
                let label = format!(
                    "{} {} {} {}",
                    if attached { "●" } else { "○" },
                    loop_mount.loop_device.as_deref().unwrap_or("(detached)"),
                    ro_label,
                    loop_mount.image_path
                );
                let command = if let Some(loop_device) = &loop_mount.loop_device {
                    crate::menus::mount::build_loop_detach_command(loop_device)
                } else {
                    crate::menus::mount::build_loop_attach_command(
                        &loop_mount.image_path,
                        loop_mount.read_only,
                    )
                };
                let label = quickjump_prefixed_label(
                    quickjump_active,
                    quickjump_labels,
                    quickjump_index,
                    label,
                );
                content = content.push(
                    button(text(label).size(11))
                        .padding([1, 4])
                        .width(Length::Fill)
                        .on_press(Message::TraySpawnCommand(app_id.to_string(), command)),
                );
                quickjump_index += 1;
            }
        }

        if !snapshot.vcvolume_profiles.is_empty() {
            content = content.push(text("VCVolume Profiles:").size(12));
            for profile in snapshot.vcvolume_profiles.iter().take(6) {
                let mounted = profile.state == crate::menus::mount::MountState::Mounted;
                let label = format!(
                    "{} {} ({})",
                    if mounted { "●" } else { "○" },
                    profile.name,
                    profile.mountpoint
                );
                let command = if mounted {
                    format!("umount '{}'", profile.mountpoint.replace('\'', "'\\''"))
                } else {
                    crate::menus::mount::build_vcvolume_mount_command(profile)
                };
                let label = quickjump_prefixed_label(
                    quickjump_active,
                    quickjump_labels,
                    quickjump_index,
                    label,
                );
                content = content.push(
                    button(text(label).size(11))
                        .padding([1, 4])
                        .width(Length::Fill)
                        .on_press(Message::TraySpawnCommand(app_id.to_string(), command)),
                );
                quickjump_index += 1;
            }
        }

        if snapshot.local_devices.is_empty()
            && snapshot.sshfs_profiles.is_empty()
            && snapshot.loop_mounts.is_empty()
            && snapshot.vcvolume_profiles.is_empty()
        {
            content = content.push(text("No mountable targets configured or discovered").size(12));
        }
    }

    content = content.push(text("Enter triggers selected action").size(10));
    content.into()
}

fn render_calendar_view_with_main_messages<'a>(
    _state: &'a EnhancedTrayState,
    app_id: &'a str,
    data: &'a Option<crate::menus::calendar::CalendarMenuSnapshot>,
    loading: bool,
    error: &'a Option<String>,
    quickjump_active: bool,
    quickjump_labels: &[String],
) -> Element<'a, Message> {
    let mut content = column!().spacing(2);
    content = content.push(text("Calendar / CalDAV").size(14));
    if quickjump_active {
        content = content.push(text("Quickjump active").size(10));
    }

    let mut quickjump_index = 0usize;

    if loading {
        content = content.push(text("⟳ Loading calendar snapshot...").size(12));
    } else if let Some(err) = error {
        content = content.push(text(format!("⚠ Error: {}", err)).size(12));
    }

    content = content.push(
        row![
            button(text(quickjump_prefixed_label(
                quickjump_active,
                quickjump_labels,
                quickjump_index,
                "Refresh",
            ))
            .size(12))
            .padding([2, 6])
            .on_press(Message::TrayCalendarRefresh(app_id.to_string())),
            button(text(quickjump_prefixed_label(
                quickjump_active,
                quickjump_labels,
                quickjump_index + 1,
                "Calendar",
            ))
            .size(12))
            .padding([2, 6])
            .on_press(Message::TraySpawnCommand(
                app_id.to_string(),
                "gnome-calendar".to_string()
            )),
        ]
        .spacing(4),
    );
    quickjump_index += 2;

    if let Some(snapshot) = data {
        content = content.push(text(snapshot.status.clone()).size(11));

        if snapshot.account_ids.is_empty() {
            content = content
                .push(text("No accounts configured under [menus.calendar.accounts]").size(12));
        } else {
            content = content.push(text("Accounts:").size(12));
            for account in snapshot.account_ids.iter().take(6) {
                let label = quickjump_prefixed_label(
                    quickjump_active,
                    quickjump_labels,
                    quickjump_index,
                    format!("• {}", account),
                );
                content = content.push(
                    button(text(label).size(11))
                        .padding([1, 4])
                        .width(Length::Fill)
                        .on_press(Message::TraySpawnCommand(
                            app_id.to_string(),
                            "gnome-calendar".to_string(),
                        )),
                );
                quickjump_index += 1;
            }
        }

        if snapshot.events.is_empty() {
            content = content.push(text("No upcoming events in the sync window").size(11));
        } else {
            content = content.push(text("Upcoming:").size(12));
            for event in snapshot.events.iter().take(6) {
                let location = event
                    .location
                    .as_deref()
                    .map(|value| format!(" ({})", value))
                    .unwrap_or_default();
                let label = format!(
                    "• [{}] {} @ {}{}",
                    event.account_id, event.title, event.start_rfc3339, location
                );
                let label = quickjump_prefixed_label(
                    quickjump_active,
                    quickjump_labels,
                    quickjump_index,
                    label,
                );
                content = content.push(
                    button(text(label).size(11))
                        .padding([1, 4])
                        .width(Length::Fill)
                        .on_press(Message::TraySpawnCommand(
                            app_id.to_string(),
                            "gnome-calendar".to_string(),
                        )),
                );
                quickjump_index += 1;
            }
        }

        if !snapshot.account_errors.is_empty() {
            content = content.push(text("Account Errors:").size(12));
            for account_error in snapshot.account_errors.iter().take(6) {
                let label = format!("⚠ [{}] {}", account_error.account_id, account_error.message);
                content = content.push(text(label).size(10));
            }
        } else if snapshot.stale {
            content = content.push(text("Some accounts failed; showing partial data").size(10));
        }
    }

    content = content.push(text("Enter on rows opens calendar app").size(10));
    content.into()
}

fn render_keyboard_hints_single() -> Element<'static, Message> {
    text("◀/▶: Navigate apps • g: Quickjump • a: All items • v: Favorites")
        .size(10)
        .into()
}

fn render_keyboard_hints_aggregated() -> Element<'static, Message> {
    text("Type: Filter • f: Toggle favorite • v: Favorites only")
        .size(10)
        .into()
}

fn render_keyboard_hints_favorites() -> Element<'static, Message> {
    text("a: All items • f: Remove favorite").size(10).into()
}

fn render_keyboard_hints_network() -> Element<'static, Message> {
    text("g: Quickjump • Click to connect/control • a: All items")
        .size(10)
        .into()
}
