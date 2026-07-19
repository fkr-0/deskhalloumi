#![allow(clippy::collapsible_if)]
// FIXME(T1.1/T6): main.rs still owns large tray/menu update chains; collapse or extract during the main.rs split instead of hiding this permanently.

mod action_routing;
mod app;
mod app_config;
mod cli;
mod enhanced_tray;
mod introspection;
mod menus;
mod module_loader;
mod startup;
mod subscription_manager;
mod tray;
mod update;
mod widgets;

use app::{MenuQuickSelectAction, Message, UniliiBar};
use app_config::{AppConfig, load_app_config};
use clap::Parser;
use cli::{Cli, Commands, verbose_to_level};
use deskhalloumi_core::{
    action_bus::{
        ACTION_BUS_MAX_FRAME_BYTES, ACTION_BUS_PROTOCOL_VERSION, ActionBusRequest,
        ActionBusResponse, default_action_bus_socket_path,
    },
    action_history::{ActionHistory, ActionStatus},
    bar::{default_bar_config_path, load_bar_config, starter_bar_config_toml},
    config::{Config, MenuUiConfig, load_config_with_path},
    key_import_sxhkd::import_sxhkd_config,
    keys::{
        BarDaemonAction, CommandType, KeyDryRunEvent, KeybindingDaemon, KeybindingResult,
        TrayDaemonAction, dry_run_bindings, parse_bar_action, parse_tray_action,
    },
    menu_process::{
        MenuProcessManager, parse_menu_action, prepare_runtime_dir, process_instance_status,
    },
    quick_select::{QuickSelectResolution, QuickSelectSession},
    runtime::{
        ActionCommand, ActionRunner, ProviderRefreshRegistry, ProviderSnapshot, RuntimeSupervisor,
        TaskSpawner, global_runtime_metrics, provider_channel,
    },
};
use iced::futures::SinkExt;
use iced::keyboard::{Key, Modifiers, key};
use iced::widget::{
    Space, button, column, container, image, row, scrollable, svg, text, text_input,
};
use iced::{Alignment, Element, Length, Subscription, Task, window};
use std::cell::RefCell;
use std::collections::HashMap;
use std::env;
use std::ffi::OsString;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};
use tracing::{error, info, warn};

use enhanced_tray::{EnhancedTrayState, TrayViewState};
use menus::presentation::{
    ActionItemOptions, action_item as presentation_action_item, bounded_text, confirmation_submenu,
    is_section_item, is_status_item, quickjump_hint_for_visible_index, split_label,
    strip_mnemonic_markers,
};
use menus::system::{
    PendingSystemAction, SYSTEM_MENU_APP_ID, SYSTEM_MENU_KEY, SystemDisplayPreset,
    SystemDisplaySnapshot, SystemInternalAction, SystemMenuRuntime, SystemMenuSnapshot,
    build_system_menu_model, button_label, parse_internal_action,
};
use module_loader::ModuleManager;
use startup::{build_window_settings, default_panel_config};
use subscription_manager::{
    ManagedModuleProvider, iced_subscription as module_iced_subscription,
    initialize_module_subscriptions, snapshot_matches_active_provider,
};
use update::tray_animation::apply_animation_tick;
use update::tray_favorites::toggle_favorite;
use update::tray_icon_press::{
    TrayIconOpenKind, open_tray_icon_state, open_tray_icon_state_with_menu,
    should_close_current_tray_view, to_enhanced_tray_icon,
};
use update::tray_menu_fetch::{TrayMenuFetchOutcome, apply_menu_fetch_result};
use update::tray_navigation::{navigate_left, navigate_right};
use update::tray_snapshots::{
    apply_calendar_snapshot, apply_mount_snapshot, apply_network_snapshot,
    apply_spawn_command_done, apply_spawn_command_started, mark_special_view_loading,
    network_toggle_desired_state_and_mark_loading,
};
use update::tray_text_input::set_text_input_value;
use update::tray_view::{
    enter_submenu, exit_submenu, show_aggregated, show_favorites, update_filter,
};
use widgets::{
    Audio, Power, SysMonitor, Video, Widget, WidgetMessage, Wifi,
    audio::{apply_audio_selection, parse_audio_selection_action, read_audio_snapshot},
    key_char_digit,
    power::{PowerAction, execute_power_action, read_power_snapshot},
    render_modules,
    video::{apply_video_preset, read_video_snapshot},
    wifi::{read_wifi_snapshot, set_wifi_enabled},
};

static KEYBINDING_ACTION_RECEIVER: OnceLock<
    Mutex<Option<tokio::sync::mpsc::UnboundedReceiver<KeybindingResult>>>,
> = OnceLock::new();

fn install_keybinding_action_receiver(
    receiver: tokio::sync::mpsc::UnboundedReceiver<KeybindingResult>,
) -> Result<(), String> {
    let slot = KEYBINDING_ACTION_RECEIVER.get_or_init(|| Mutex::new(None));
    let mut guard = slot
        .lock()
        .map_err(|error| format!("failed to lock keybinding action receiver: {error}"))?;
    if guard.is_some() {
        return Err("keybinding action receiver is already installed".to_string());
    }
    *guard = Some(receiver);
    Ok(())
}

fn provider_health_badge<T>(
    prefix: &str,
    snapshot: &ProviderSnapshot<T>,
) -> Element<'static, Message> {
    let age = snapshot
        .last_update_age(std::time::SystemTime::now())
        .map(|age| format!("{}s", age.as_secs()))
        .unwrap_or_else(|| "—".to_string());
    let health = snapshot.health();
    let color = match health {
        deskhalloumi_core::runtime::ProviderHealth::Fresh => {
            iced::Color::from_rgb(0.55, 0.78, 0.60)
        }
        deskhalloumi_core::runtime::ProviderHealth::Stale => {
            iced::Color::from_rgb(0.90, 0.72, 0.35)
        }
        deskhalloumi_core::runtime::ProviderHealth::Error => {
            iced::Color::from_rgb(0.92, 0.42, 0.42)
        }
        _ => iced::Color::from_rgb(0.62, 0.66, 0.73),
    };
    text(format!("{prefix}:{} {age}", health.label()))
        .size(9)
        .color(color)
        .into()
}

struct StartupState {
    config: deskhalloumi_core::config::Config,
    app_config: app_config::AppConfig,
    run_options: cli::RunOptions,
    modules: HashMap<String, module_loader::LoadedModule>,
    module_providers: HashMap<String, ManagedModuleProvider>,
    keybinding_actions_enabled: bool,
    runtime_supervisor: Arc<RuntimeSupervisor>,
    runtime_spawner: TaskSpawner,
    provider_refreshes: ProviderRefreshRegistry,
}

struct ActionBusSocketGuard(PathBuf);

impl Drop for ActionBusSocketGuard {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

async fn start_action_bus_server(
    sender: tokio::sync::mpsc::UnboundedSender<KeybindingResult>,
    spawner: &TaskSpawner,
) -> Result<(), String> {
    let path = default_action_bus_socket_path();
    let parent = path
        .parent()
        .ok_or_else(|| format!("action socket '{}' has no parent", path.display()))?;
    prepare_runtime_dir(parent)?;
    if path.exists() {
        if std::os::unix::net::UnixStream::connect(&path).is_ok() {
            return Err(format!(
                "DeskHalloumi action bus is already active at '{}'",
                path.display()
            ));
        }
        fs::remove_file(&path)
            .map_err(|error| format!("failed to remove stale action socket: {error}"))?;
    }
    let listener = UnixListener::bind(&path)
        .map_err(|error| format!("failed to bind action socket '{}': {error}", path.display()))?;
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600))
        .map_err(|error| format!("failed to secure action socket: {error}"))?;

    let server_spawner = spawner.clone();
    let cancellation = server_spawner.cancellation_token();
    spawner
        .spawn("action-bus:listener", async move {
            let _guard = ActionBusSocketGuard(path);
            loop {
                let accepted = tokio::select! {
                    _ = cancellation.cancelled() => break,
                    accepted = listener.accept() => accepted,
                };
                let (stream, _) = match accepted {
                    Ok(value) => value,
                    Err(error) => {
                        error!(%error, "DeskHalloumi action bus accept failed");
                        break;
                    }
                };
                let connection_sender = sender.clone();
                if let Err(error) = server_spawner.try_spawn("action-bus:connection", async move {
                    handle_action_bus_connection(stream, connection_sender).await;
                }) {
                    warn!(%error, "action bus connection was rejected by runtime supervisor");
                }
            }
        })
        .await
        .map_err(|error| format!("failed to supervise action bus listener: {error}"))?;
    Ok(())
}

async fn handle_action_bus_connection(
    stream: UnixStream,
    sender: tokio::sync::mpsc::UnboundedSender<KeybindingResult>,
) {
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    let response = match tokio::time::timeout(Duration::from_secs(2), reader.read_line(&mut line))
        .await
    {
        Err(_) => ActionBusResponse::error("unknown", "timed out reading action request"),
        Ok(Err(error)) => ActionBusResponse::error("unknown", format!("read failed: {error}")),
        Ok(Ok(0)) => ActionBusResponse::error("unknown", "empty action request"),
        Ok(Ok(_)) if line.len() > ACTION_BUS_MAX_FRAME_BYTES => {
            ActionBusResponse::error("unknown", "action request exceeds 64 KiB")
        }
        Ok(Ok(_)) => match serde_json::from_str::<ActionBusRequest>(line.trim()) {
            Err(error) => ActionBusResponse::error("unknown", format!("invalid request: {error}")),
            Ok(request) => match request.validate() {
                Err(error) => ActionBusResponse::error(request.request_id, error),
                Ok(()) => {
                    let route = match action_routing::route_action_bus_request(
                        request.request_id.clone(),
                        request.action,
                    ) {
                        Ok(route) => route,
                        Err(error) => {
                            let response = ActionBusResponse::error(request.request_id, error);
                            write_action_bus_response(reader.into_inner(), &response).await;
                            return;
                        }
                    };
                    match route {
                        action_routing::ActionBusRoute::Respond(response) => response,
                        action_routing::ActionBusRoute::Queue(result) => {
                            match sender.send(result) {
                                Ok(()) => ActionBusResponse::ok(request.request_id, "queued"),
                                Err(_) => ActionBusResponse::error(
                                    request.request_id,
                                    "bar action receiver is no longer available",
                                ),
                            }
                        }
                    }
                }
            },
        },
    };
    debug_assert_eq!(response.protocol_version, ACTION_BUS_PROTOCOL_VERSION);
    write_action_bus_response(reader.into_inner(), &response).await;
}

async fn write_action_bus_response(mut stream: UnixStream, response: &ActionBusResponse) {
    if let Ok(mut payload) = serde_json::to_vec(response) {
        payload.push(b'\n');
        let _ = stream.write_all(&payload).await;
        let _ = stream.shutdown().await;
    }
}

fn take_keybinding_action_receiver()
-> Option<tokio::sync::mpsc::UnboundedReceiver<KeybindingResult>> {
    KEYBINDING_ACTION_RECEIVER
        .get()
        .and_then(|slot| slot.lock().ok()?.take())
}

fn update(bar: &mut UniliiBar, message: Message) -> Task<Message> {
    match message {
        Message::WindowOpened(_id) => {
            info!("WindowOpened message received (single panel mode)");
        }
        Message::WindowClosed(id) => {
            info!(?id, "WindowClosed message received (single panel mode)");
            if Some(id) == bar.main_window_id {
                bar.audio_provider.shutdown();
                bar.network_provider.shutdown();
                bar.system_provider.shutdown();
                bar.runtime_spawner.cancellation_token().cancel();
                let supervisor = Arc::clone(&bar.runtime_supervisor);
                return Task::perform(
                    async move { supervisor.shutdown(Duration::from_secs(2)).await },
                    Message::RuntimeShutdownComplete,
                );
            }
        }
        Message::RuntimeShutdownComplete(result) => {
            bar.audio_provider.stopped();
            bar.network_provider.stopped();
            bar.system_provider.stopped();
            if let Err(error) = result {
                warn!(%error, "runtime supervisor required forced shutdown");
            }
            let metrics = global_runtime_metrics().snapshot();
            info!(
                active_tasks = metrics.active_tasks,
                tasks_started = metrics.tasks_started,
                tasks_completed = metrics.tasks_completed,
                tasks_cancelled = metrics.tasks_cancelled,
                tasks_panicked = metrics.tasks_panicked,
                actions_completed = metrics.actions_completed,
                action_timeouts = metrics.action_timeouts,
                truncated_outputs = metrics.truncated_outputs,
                updates_coalesced = metrics.updates_coalesced,
                updates_dropped = metrics.updates_dropped,
                "runtime shutdown metrics"
            );
            return iced::exit();
        }
        Message::ModuleUpdate(name, update) => {
            info!("module update: {name} -> {:?}", update);
            if let Some(loaded) = bar.modules.get_mut(&name) {
                if let Err(e) = loaded.module.update(update) {
                    error!("Failed to update module '{}': {}", name, e);
                }
            }
        }
        Message::ModuleProviderState(name, snapshot) => {
            let Some(active_provider) = bar.module_providers.get(&name) else {
                global_runtime_metrics().record_update_dropped();
                warn!(
                    provider = %name,
                    instance_generation = snapshot.instance_generation,
                    "discarding provider snapshot because the provider is no longer active"
                );
                return Task::none();
            };
            if !snapshot_matches_active_provider(active_provider, &snapshot) {
                global_runtime_metrics().record_update_coalesced();
                warn!(
                    provider = %name,
                    active_instance_generation = active_provider.instance_generation,
                    stale_instance_generation = snapshot.instance_generation,
                    refresh_generation = snapshot.generation,
                    "discarding queued snapshot from replaced provider instance"
                );
                return Task::none();
            }
            let age_ms = snapshot
                .last_update_age(std::time::SystemTime::now())
                .map(|age| age.as_millis());
            info!(
                provider = %name,
                instance_generation = snapshot.instance_generation,
                generation = snapshot.generation,
                health = snapshot.health().label(),
                age_ms = ?age_ms,
                error = ?snapshot.error(),
                "module provider state changed"
            );
            if let Some(update) = snapshot.value().cloned()
                && let Some(loaded) = bar.modules.get_mut(&name)
                && let Err(error) = loaded.module.update(update)
            {
                error!(module = %name, %error, "failed to apply provider value");
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
                if let Some(session) = bar.tray_quick_select.take() {
                    return match session.resolve(extract_key_char(&key)) {
                        QuickSelectResolution::Action { action, .. } => {
                            Task::done(Message::TrayMenuTriggered(action.app_id, action.action))
                        }
                        QuickSelectResolution::Aborted { key } => {
                            info!(?key, "quick-select aborted by non-mapped key");
                            Task::none()
                        }
                    };
                }
                // Enhanced menu keyboard navigation
                if let Some(tray_state) = bar.enhanced_tray.as_mut() {
                    match key.as_str() {
                        _ if key_matches_named(&key, "Escape") => {
                            if submenu_is_open(tray_state) {
                                return Task::done(Message::TrayExitSubmenu);
                            }
                            tray_state.animation_target = 0.0;
                            return resize_window_task(bar, false);
                        }
                        _ if key_matches_named(&key, "ArrowDown")
                            || key_matches_named(&key, "Tab") =>
                        {
                            move_menu_selection_with_config(&bar.config, tray_state, true);
                            return Task::none();
                        }
                        _ if key_matches_named(&key, "ArrowUp") => {
                            move_menu_selection_with_config(&bar.config, tray_state, false);
                            return Task::none();
                        }
                        _ if key_matches_named(&key, "ArrowLeft") => {
                            return Task::done(if submenu_is_open(tray_state) {
                                Message::TrayExitSubmenu
                            } else {
                                Message::TrayNavigateLeft
                            });
                        }
                        _ if key_matches_named(&key, "ArrowRight") => {
                            if let Some(idx) = tray_state.selected_index
                                && let Some((app_id, action)) = get_menu_action_at_index_with_config(
                                    &bar.config,
                                    tray_state,
                                    idx,
                                )
                                && matches!(
                                    action,
                                    enhanced_tray::TrayMenuAction::NavigateToSubmenu { .. }
                                )
                            {
                                return Task::done(Message::TrayMenuTriggered(app_id, action));
                            }
                            return Task::done(Message::TrayNavigateRight);
                        }
                        _ if key_matches_named(&key, "Enter") => {
                            if let Some(idx) = tray_state.selected_index {
                                if let Some((app_id, action)) = get_menu_action_at_index_with_config(
                                    &bar.config,
                                    tray_state,
                                    idx,
                                ) {
                                    return Task::done(Message::TrayMenuTriggered(app_id, action));
                                }
                            }
                            return Task::none();
                        }
                        _ if key_matches_char(&key, 'f') => {
                            if let Some((app_id, item_id)) = selected_favorite_target(tray_state) {
                                return Task::done(Message::TrayToggleFavorite(app_id, item_id));
                            }
                            return Task::none();
                        }
                        _ if key_matches_char(&key, 'a') => {
                            return Task::done(Message::TrayShowAggregated);
                        }
                        _ if key_matches_char(&key, 'v') => {
                            return Task::done(Message::TrayShowFavorites);
                        }
                        _ if key_matches_char(&key, 'g') => {
                            if quick_select_supported_for_view(tray_state) {
                                bar.tray_quick_select =
                                    build_menu_quick_select(&bar.config, tray_state);
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
                            let still_exists = app_id == SYSTEM_MENU_APP_ID
                                || bar.tray_icons.iter().any(|icon| icon.id == *app_id);
                            if !still_exists {
                                bar.enhanced_tray = None;
                            }
                        }
                        TrayViewState::Network { app_id, .. }
                        | TrayViewState::Mount { app_id, .. }
                        | TrayViewState::Calendar { app_id, .. } => {
                            let still_exists = app_id == SYSTEM_MENU_APP_ID
                                || bar.tray_icons.iter().any(|icon| icon.id == *app_id);
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
            if let Some(icon) = bar
                .tray_icons
                .iter()
                .find(|icon| icon.key == icon_key || icon.id == icon_key)
                && should_close_current_tray_view(bar.enhanced_tray.as_ref(), icon)
            {
                if let Some(current) = bar.enhanced_tray.as_mut() {
                    current.animation_target = 0.0;
                }
                return resize_window_task(bar, false);
            }

            if let Some(icon) = bar.tray_icons.iter().find(|icon| icon.key == icon_key) {
                bar.tray_quick_select = None;
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
                    bar.enhanced_tray =
                        Some(open_tray_icon_state(icon, TrayIconOpenKind::Calendar));

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
                    let custom_menu =
                        build_custom_menu_items(&enhanced_icon, &bar.config.menus.custom);
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
            if icon_key == SYSTEM_MENU_KEY || icon_key == SYSTEM_MENU_APP_ID {
                match action {
                    enhanced_tray::TrayMenuAction::SpawnCommand(command) => {
                        if let Some(action) = parse_internal_action(&command) {
                            return handle_system_internal_action(bar, action);
                        }
                        return run_system_shell_command(
                            bar,
                            "system-command",
                            "System command",
                            command,
                        );
                    }
                    enhanced_tray::TrayMenuAction::NavigateToSubmenu { submenu_path, .. } => {
                        return Task::done(Message::TrayEnterSubmenu(
                            SYSTEM_MENU_APP_ID.to_string(),
                            submenu_path,
                        ));
                    }
                    _ => return Task::none(),
                }
            }
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
                    if let Err(error) =
                        bar.runtime_spawner
                            .try_spawn("tray:dbus-menu-action", async move {
                                if let Err(error) = enhanced_tray::invoke_dbus_menu_action(
                                    &enhanced_icon,
                                    item_id,
                                    &event_id,
                                )
                                .await
                                {
                                    warn!(%error, "DBus tray menu action failed");
                                }
                            })
                    {
                        warn!(%error, "DBus tray menu action was not scheduled");
                    }
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
                    if cmd == "nmcli device wifi rescan" {
                        return Task::done(Message::TrayNetworkRefresh(icon.key.clone()));
                    }
                    if cmd == "nmcli radio wifi on" || cmd == "nmcli radio wifi off" {
                        return Task::done(Message::TrayNetworkToggle(icon.key.clone()));
                    }
                    let current_special_app =
                        bar.enhanced_tray
                            .as_ref()
                            .and_then(|state| match &state.current_view {
                                TrayViewState::Network { app_id, .. }
                                | TrayViewState::Mount { app_id, .. }
                                | TrayViewState::Calendar { app_id, .. } => Some(app_id.as_str()),
                                _ => None,
                            });
                    if current_special_app == Some(icon.id.as_str()) {
                        return Task::done(Message::TraySpawnCommand(icon.key.clone(), cmd));
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
                    | enhanced_tray::TrayMenuAction::TextInputCleared
                    | enhanced_tray::TrayMenuAction::Typed { .. } => tray::TrayMenuAction::Activate,
                };

                if let Err(error) = bar
                    .runtime_spawner
                    .try_spawn("tray:item-action", async move {
                        tray::invoke_menu_action(&icon, converted_action).await;
                    })
                {
                    warn!(%error, "tray item action was not scheduled");
                }
            }

            // Close enhanced tray menu after action
            if let Some(tray_state) = bar.enhanced_tray.as_mut() {
                tray_state.animation_target = 0.0;
                return resize_window_task(bar, false);
            }
        }
        Message::TrayNetworkSnapshot(icon_key, result) => {
            apply_network_snapshot(&mut bar.enhanced_tray, &icon_key, result, |app_id| {
                resolve_tray_icon_key(&bar.tray_icons, app_id)
            });
        }
        Message::TrayNetworkRefresh(icon_key) => {
            let permit = match bar
                .provider_refreshes
                .try_start(format!("tray-network:{icon_key}"))
            {
                Ok(permit) => permit,
                Err(error) => {
                    info!(%error, %icon_key, "network refresh coalesced or saturated");
                    return Task::none();
                }
            };
            mark_special_view_loading(&mut bar.enhanced_tray, &icon_key, |app_id| {
                resolve_tray_icon_key(&bar.tray_icons, app_id)
            });

            let nmcli_path = bar.run_options.nmcli_path.clone();
            return Task::perform(
                async move {
                    let _permit = permit;
                    enhanced_tray::read_network_snapshot(nmcli_path, true).await
                },
                move |result| Message::TrayNetworkSnapshot(icon_key.clone(), result),
            );
        }
        Message::TrayNetworkToggle(icon_key) => {
            let permit = match bar
                .provider_refreshes
                .try_start(format!("tray-network:{icon_key}"))
            {
                Ok(permit) => permit,
                Err(error) => {
                    info!(%error, %icon_key, "network toggle coalesced or saturated");
                    return Task::none();
                }
            };
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
                async move {
                    let _permit = permit;
                    enhanced_tray::set_wifi_enabled(nmcli_path, desired_state).await
                },
                move |result| Message::TrayNetworkToggleDone(icon_key.clone(), result),
            );
        }
        Message::TrayNetworkToggleDone(icon_key, result) => {
            mark_special_view_loading(&mut bar.enhanced_tray, &icon_key, |app_id| {
                resolve_tray_icon_key(&bar.tray_icons, app_id)
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

            return Task::done(Message::TrayNetworkRefresh(icon_key));
        }
        Message::TrayMountSnapshot(icon_key, result) => {
            apply_mount_snapshot(&mut bar.enhanced_tray, &icon_key, result, |app_id| {
                resolve_tray_icon_key(&bar.tray_icons, app_id)
            });
        }
        Message::TrayMountRefresh(icon_key) => {
            let permit = match bar
                .provider_refreshes
                .try_start(format!("tray-mount:{icon_key}"))
            {
                Ok(permit) => permit,
                Err(error) => {
                    info!(%error, %icon_key, "mount refresh coalesced or saturated");
                    return Task::none();
                }
            };
            let mount_config = bar.config.menus.mount.clone();
            mark_special_view_loading(&mut bar.enhanced_tray, &icon_key, |app_id| {
                resolve_tray_icon_key(&bar.tray_icons, app_id)
            });
            return Task::perform(
                async move {
                    let _permit = permit;
                    read_mount_snapshot(mount_config).await
                },
                move |result| Message::TrayMountSnapshot(icon_key.clone(), result),
            );
        }
        Message::TrayCalendarSnapshot(icon_key, result) => {
            apply_calendar_snapshot(&mut bar.enhanced_tray, &icon_key, result, |app_id| {
                resolve_tray_icon_key(&bar.tray_icons, app_id)
            });
        }
        Message::TrayCalendarRefresh(icon_key) => {
            let permit = match bar
                .provider_refreshes
                .try_start(format!("tray-calendar:{icon_key}"))
            {
                Ok(permit) => permit,
                Err(error) => {
                    info!(%error, %icon_key, "calendar refresh coalesced or saturated");
                    return Task::none();
                }
            };
            let calendar_accounts = bar.config.menus.calendar.accounts.clone();
            let agenda_days = bar.config.menus.calendar.agenda_days;

            mark_special_view_loading(&mut bar.enhanced_tray, &icon_key, |app_id| {
                resolve_tray_icon_key(&bar.tray_icons, app_id)
            });

            return Task::perform(
                async move {
                    let _permit = permit;
                    read_calendar_snapshot(calendar_accounts, agenda_days).await
                },
                move |result| Message::TrayCalendarSnapshot(icon_key.clone(), result),
            );
        }
        Message::TraySpawnCommand(icon_key, command) => {
            apply_spawn_command_started(&mut bar.enhanced_tray, &icon_key, |app_id| {
                resolve_tray_icon_key(&bar.tray_icons, app_id)
            });

            return Task::perform(tray::spawn_command(command), move |result| {
                Message::TraySpawnCommandDone(icon_key.clone(), result)
            });
        }
        Message::TraySpawnCommandDone(icon_key, result) => {
            let refresh = if result.is_ok() {
                bar.enhanced_tray
                    .as_ref()
                    .and_then(|state| match &state.current_view {
                        TrayViewState::Network { .. } => {
                            Some(Message::TrayNetworkRefresh(icon_key.clone()))
                        }
                        TrayViewState::Mount { .. } => {
                            Some(Message::TrayMountRefresh(icon_key.clone()))
                        }
                        TrayViewState::Calendar { .. } => {
                            Some(Message::TrayCalendarRefresh(icon_key.clone()))
                        }
                        _ => None,
                    })
            } else {
                None
            };
            apply_spawn_command_done(&mut bar.enhanced_tray, &icon_key, result, |app_id| {
                resolve_tray_icon_key(&bar.tray_icons, app_id)
            });
            if let Some(refresh) = refresh {
                return Task::done(refresh);
            }
        }
        Message::TrayAnimateTick => {
            apply_animation_tick(&mut bar.enhanced_tray);
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
        Message::TrayToggleFavorite(app_id, item_id) => {
            toggle_favorite(&mut bar.enhanced_tray, &app_id, &item_id);
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
        Message::TrayMenuFetched(app_id, result) => {
            match apply_menu_fetch_result(&mut bar.enhanced_tray, &app_id, result) {
                TrayMenuFetchOutcome::Populated { .. } => {
                    info!("Menu fetched and populated for app: {}", app_id);
                }
                TrayMenuFetchOutcome::KeptExistingEmptyFetch => {
                    info!(
                        "Fetched empty DBus menu for {}; keeping fallback menu",
                        app_id
                    );
                }
                TrayMenuFetchOutcome::FallbackPopulated { error, .. }
                | TrayMenuFetchOutcome::FetchFailedNoKnownApp { error } => {
                    info!("Failed to fetch menu for {}: {}", app_id, error);
                }
                TrayMenuFetchOutcome::NoState => {}
            }
        }
        Message::SystemMenuPressed(section) => {
            return open_system_menu(bar, &section);
        }
        Message::SystemActionDone {
            sequence,
            action_id,
            duration_ms,
            timed_out,
            result,
        } => {
            bar.system_menu.busy_action = None;
            let status = if timed_out {
                ActionStatus::TimedOut
            } else if result.is_ok() {
                ActionStatus::Succeeded
            } else {
                ActionStatus::Failed
            };
            let detail = match &result {
                Ok(message) => Some(message.clone()),
                Err(error) => Some(error.clone()),
            };
            bar.action_history.finish(
                sequence,
                status,
                Duration::from_millis(duration_ms.min(u64::MAX as u128) as u64),
                detail,
            );
            bar.system_menu.last_status = Some(match result {
                Ok(message) => message,
                Err(error) => format!("{action_id} failed: {error}"),
            });
            refresh_system_provider(bar);
            rebuild_system_menu_if_open(bar);
            return Task::batch([
                Task::done(Message::LegacyWidgetTick("wifi".to_string())),
                Task::done(Message::LegacyWidgetTick("video".to_string())),
                Task::done(Message::LegacyWidgetTick("power".to_string())),
            ]);
        }
        Message::LegacyWidget(widget_message) => match widget_message.clone() {
            WidgetMessage::SysMonitor(_) => bar.sysmonitor.update(widget_message),
            WidgetMessage::Wifi(action) => {
                if action == "toggle_menu" {
                    bar.wifi.update(widget_message);
                    if bar.wifi.menu_is_open() {
                        return start_wifi_refresh(bar, None);
                    }
                } else if action == "toggle_wifi" {
                    return start_wifi_refresh(bar, Some(bar.wifi.desired_enabled_state()));
                } else {
                    bar.wifi.update(widget_message);
                }
            }
            WidgetMessage::Audio(action) => {
                if action == "toggle_menu" {
                    bar.audio.update(widget_message);
                    if bar.audio.menu_is_open() {
                        return start_audio_refresh(bar, None);
                    }
                } else if let Some(selection) = parse_audio_selection_action(&action) {
                    return start_audio_refresh(bar, Some(selection));
                }
            }
            WidgetMessage::Video(action) => {
                if action == "toggle_menu" {
                    bar.video.update(widget_message);
                    if bar.video.menu_is_open() {
                        bar.video.reload_presets();
                        return start_video_refresh(bar, None);
                    }
                } else if action == "refresh" {
                    bar.video.reload_presets();
                    return start_video_refresh(bar, None);
                } else if let Some(key) = action.strip_prefix("preset:") {
                    if let Some(preset) = bar.video.preset(key) {
                        return start_video_refresh(bar, Some(preset));
                    }
                    warn!(preset = key, "requested unknown xrandr preset");
                }
            }
            WidgetMessage::Power(action) => {
                if action == "toggle_menu" {
                    bar.power.update(widget_message);
                    if bar.power.menu_is_open() {
                        return start_power_refresh(bar);
                    }
                } else if let Some(action) =
                    Power::requested_action(&action, bar.power.screensaver_enabled())
                {
                    return start_power_action(bar, action);
                }
            }
            WidgetMessage::Tray(_) => {}
        },
        Message::LegacyWidgetTick(name) => match name.as_str() {
            "sysmonitor" => refresh_system_provider(bar),
            "wifi" => return start_wifi_refresh(bar, None),
            "audio" => return start_audio_refresh(bar, None),
            "video" => return start_video_refresh(bar, None),
            "power" => return start_power_refresh(bar),
            _ => {}
        },
        Message::AudioRefreshDone { generation, result } => {
            if bar
                .audio_provider
                .publish_result(generation, result.clone())
            {
                match result {
                    Ok(snapshot) => bar.audio.apply_snapshot(snapshot),
                    Err(error) => warn!(%error, "audio widget refresh failed"),
                }
            } else {
                info!(generation, "ignored stale audio provider result");
            }
        }
        Message::WifiRefreshDone { generation, result } => {
            if bar
                .network_provider
                .publish_result(generation, result.clone())
            {
                match result {
                    Ok(snapshot) => bar.wifi.apply_snapshot(snapshot),
                    Err(error) => warn!(%error, "WiFi widget refresh failed"),
                }
            } else {
                info!(generation, "ignored stale network provider result");
            }
        }
        Message::VideoRefreshDone(result) => match result {
            Ok(snapshot) => bar.video.apply_snapshot(snapshot),
            Err(error) => warn!(%error, "video widget refresh failed"),
        },
        Message::PowerRefreshDone(result) => match result {
            Ok(snapshot) => bar.power.apply_snapshot(snapshot),
            Err(error) => warn!(%error, "power widget refresh failed"),
        },
        Message::PowerActionDone(result) => match result {
            Ok(Some(snapshot)) => bar.power.apply_snapshot(snapshot),
            Ok(None) => {}
            Err(error) => warn!(%error, "power action failed"),
        },
        Message::KeybindingAction(action) => {
            return handle_keybinding_action(bar, action);
        }
    }
    Task::none()
}

fn start_audio_refresh(
    bar: &mut UniliiBar,
    selection: Option<widgets::audio::AudioSelectionAction>,
) -> Task<Message> {
    let permit = match bar.provider_refreshes.try_start("legacy-audio") {
        Ok(permit) => permit,
        Err(error) => {
            info!(%error, "audio widget refresh coalesced or saturated");
            return Task::none();
        }
    };
    let generation = bar.audio_provider.begin_refresh();
    Task::perform(
        async move {
            let _permit = permit;
            match selection {
                Some(selection) => apply_audio_selection("pactl".to_string(), selection).await,
                None => read_audio_snapshot("pactl".to_string()).await,
            }
        },
        move |result| Message::AudioRefreshDone { generation, result },
    )
}

fn start_wifi_refresh(bar: &mut UniliiBar, enabled: Option<bool>) -> Task<Message> {
    let permit = match bar.provider_refreshes.try_start("legacy-wifi") {
        Ok(permit) => permit,
        Err(error) => {
            info!(%error, "WiFi widget refresh coalesced or saturated");
            return Task::none();
        }
    };
    let generation = bar.network_provider.begin_refresh();
    let nmcli = bar.run_options.nmcli_path.clone();
    Task::perform(
        async move {
            let _permit = permit;
            match enabled {
                Some(enabled) => set_wifi_enabled(nmcli, enabled).await,
                None => read_wifi_snapshot(nmcli).await,
            }
        },
        move |result| Message::WifiRefreshDone { generation, result },
    )
}

fn refresh_system_provider(bar: &mut UniliiBar) {
    let generation = bar.system_provider.begin_refresh();
    bar.sysmonitor.update_stats();
    let snapshot = bar.sysmonitor.snapshot().clone();
    let _ = bar.system_provider.publish_result(generation, Ok(snapshot));
}

fn start_video_refresh(
    bar: &mut UniliiBar,
    preset: Option<widgets::video::XrandrPreset>,
) -> Task<Message> {
    let permit = match bar.provider_refreshes.try_start("legacy-video") {
        Ok(permit) => permit,
        Err(error) => {
            info!(%error, "video widget refresh coalesced or saturated");
            return Task::none();
        }
    };
    Task::perform(
        async move {
            let _permit = permit;
            match preset {
                Some(preset) => apply_video_preset("xrandr".to_string(), preset).await,
                None => read_video_snapshot("xrandr".to_string()).await,
            }
        },
        Message::VideoRefreshDone,
    )
}

fn start_power_refresh(bar: &mut UniliiBar) -> Task<Message> {
    let permit = match bar.provider_refreshes.try_start("legacy-power") {
        Ok(permit) => permit,
        Err(error) => {
            info!(%error, "power widget refresh coalesced or saturated");
            return Task::none();
        }
    };
    Task::perform(
        async move {
            let _permit = permit;
            read_power_snapshot("xset".to_string()).await
        },
        Message::PowerRefreshDone,
    )
}

fn start_power_action(bar: &mut UniliiBar, action: PowerAction) -> Task<Message> {
    let permit = match bar.provider_refreshes.try_start("legacy-power") {
        Ok(permit) => permit,
        Err(error) => {
            info!(%error, "power action coalesced or saturated");
            return Task::none();
        }
    };
    Task::perform(
        async move {
            let _permit = permit;
            execute_power_action("xset".to_string(), "systemctl".to_string(), action).await
        },
        Message::PowerActionDone,
    )
}

fn view(bar: &UniliiBar, window_id: window::Id) -> Element<'_, Message> {
    if Some(window_id) == bar.tray_window_id {
        return if let Some(tray_state) = &bar.enhanced_tray {
            render_enhanced_tray_menu(bar, tray_state)
        } else {
            Space::new().into()
        };
    }

    let mut right_widgets: Vec<Element<'_, Message>> =
        render_modules(&bar.modules, &bar.module_providers);

    if bar.config.menus.system.enabled {
        let system_snapshot = system_menu_snapshot(bar);
        let system_open =
            bar.enhanced_tray
                .as_ref()
                .is_some_and(|state| match &state.current_view {
                    TrayViewState::SingleApp { app_id, .. }
                    | TrayViewState::Network { app_id, .. } => app_id == SYSTEM_MENU_APP_ID,
                    _ => false,
                });
        let system_buttons = bar
            .config
            .menus
            .system
            .buttons
            .iter()
            .filter(|button_config| button_config.enabled)
            .fold(
                row!().spacing(1).align_y(iced::Alignment::Center),
                |row, button_config| {
                    let label = button_label(button_config, &system_snapshot);
                    let section = button_config.section.clone();
                    let mut menu_button = button(text(label).size(11))
                        .padding([2, 7])
                        .on_press(Message::SystemMenuPressed(section));
                    menu_button = if system_open {
                        menu_button.style(button::primary)
                    } else {
                        menu_button.style(button::text)
                    };
                    row.push(menu_button)
                },
            );
        right_widgets.push(system_buttons.into());
    }

    if !bar.config.menus.system.enabled || !bar.config.menus.system.replace_legacy_widgets {
        right_widgets.push(bar.sysmonitor.view().map(Message::LegacyWidget));
        right_widgets.push(bar.wifi.view().map(Message::LegacyWidget));
        right_widgets.push(bar.video.view().map(Message::LegacyWidget));
        right_widgets.push(bar.power.view().map(Message::LegacyWidget));
    }
    right_widgets.push(bar.audio.view().map(Message::LegacyWidget));
    right_widgets.push(provider_health_badge(
        "sys",
        &bar.system_provider_state.current(),
    ));
    right_widgets.push(provider_health_badge(
        "net",
        &bar.network_provider_state.current(),
    ));
    right_widgets.push(provider_health_badge(
        "aud",
        &bar.audio_provider_state.current(),
    ));

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

    let module_subscriptions = bar
        .module_providers
        .values()
        .map(module_iced_subscription)
        .collect::<Vec<_>>();

    let keybinding_action_subscription = bar.keybinding_actions_enabled.then(|| {
        Subscription::run(|| {
            stream::channel(64, async move |mut output| {
                let Some(mut receiver) = take_keybinding_action_receiver() else {
                    warn!("embedded keybinding action subscription has no receiver");
                    return;
                };
                while let Some(action) = receiver.recv().await {
                    if output
                        .send(Message::KeybindingAction(action))
                        .await
                        .is_err()
                    {
                        break;
                    }
                }
            })
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
            let mut workers = tokio::task::JoinSet::new();
            workers.spawn(async move {
                tray::run_tray_watcher(tx, 1500).await; // Use default poll interval
            });

            loop {
                tokio::select! {
                    event = rx.recv() => {
                        let Some(event) = event else { break; };
                        if output.send(Message::TrayEvent(event)).await.is_err() {
                            break;
                        }
                    }
                    worker = workers.join_next() => {
                        match worker {
                            Some(Ok(())) | None => break,
                            Some(Err(error)) => {
                                error!(%error, "tray watcher task failed");
                                break;
                            }
                        }
                    }
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
    if let Some(subscription) = keybinding_action_subscription {
        subscriptions.push(subscription);
    }
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
            no_hotkeyd: false,
        })
        .run_options()
        .unwrap_or_default();

    let log_level = verbose_to_level(cli.verbose);
    let _ = tracing_subscriber::fmt()
        .with_max_level(log_level)
        .try_init();

    // Handle subcommands that don't run the bar
    match &cli.command {
        Some(Commands::ListModules { json }) => {
            let config = load_config_with_path(cli.config.clone());
            introspection::print_records(&introspection::active_modules(&config), *json)
                .map_err(|error| iced::Error::WindowCreationFailed(error.into()))?;
            return Ok(());
        }
        Some(Commands::RuntimeMetrics { json, socket }) => {
            let metrics = introspection::query_runtime_metrics(socket.as_deref())
                .map_err(|error| iced::Error::WindowCreationFailed(error.into()))?;
            introspection::print_runtime_metrics(&metrics, *json)
                .map_err(|error| iced::Error::WindowCreationFailed(error.into()))?;
            return Ok(());
        }
        Some(Commands::ListMenus { json }) => {
            let config = load_config_with_path(cli.config.clone());
            introspection::print_records(&introspection::menus(&config), *json)
                .map_err(|error| iced::Error::WindowCreationFailed(error.into()))?;
            return Ok(());
        }
        Some(Commands::ListActions { json }) => {
            let config = load_config_with_path(cli.config.clone());
            introspection::print_records(&introspection::actions(&config), *json)
                .map_err(|error| iced::Error::WindowCreationFailed(error.into()))?;
            return Ok(());
        }
        Some(Commands::ListHotkeys { json }) => {
            let config = load_config_with_path(cli.config.clone());
            introspection::print_records(&introspection::hotkeys(&config), *json)
                .map_err(|error| iced::Error::WindowCreationFailed(error.into()))?;
            return Ok(());
        }
        Some(Commands::InvokeAction {
            kind,
            payload,
            socket,
        }) => {
            let response =
                introspection::invoke_typed_action(*kind, payload.clone(), socket.as_deref())
                    .map_err(|error| iced::Error::WindowCreationFailed(error.into()))?;
            println!(
                "{}",
                serde_json::to_string_pretty(&response).map_err(|error| {
                    iced::Error::WindowCreationFailed(
                        format!("failed to serialize action response: {error}").into(),
                    )
                })?
            );
            if !response.ok {
                return Err(iced::Error::WindowCreationFailed(response.message.into()));
            }
            return Ok(());
        }
        Some(Commands::InitBarConfig { output, force }) => {
            if let Some(path) = output.clone().or_else(default_bar_config_path) {
                if path.exists() && !force {
                    return Err(iced::Error::WindowCreationFailed(
                        format!(
                            "bar config '{}' already exists; pass --force to overwrite",
                            path.display()
                        )
                        .into(),
                    ));
                }
                if let Some(parent) = path.parent() {
                    fs::create_dir_all(parent).map_err(|error| {
                        iced::Error::WindowCreationFailed(
                            format!(
                                "failed to create config directory '{}': {}",
                                parent.display(),
                                error
                            )
                            .into(),
                        )
                    })?;
                }
                fs::write(&path, starter_bar_config_toml()).map_err(|error| {
                    iced::Error::WindowCreationFailed(
                        format!("failed to write bar config '{}': {}", path.display(), error)
                            .into(),
                    )
                })?;
                println!("wrote bar config: {}", path.display());
            } else {
                print!("{}", starter_bar_config_toml());
            }
            return Ok(());
        }
        Some(Commands::ValidateBarConfig { config }) => {
            let path = config
                .clone()
                .or_else(default_bar_config_path)
                .ok_or_else(|| {
                    iced::Error::WindowCreationFailed(
                        "no bar config path provided and no default config path is available"
                            .into(),
                    )
                })?;
            let config = load_bar_config(&path).map_err(|error| {
                iced::Error::WindowCreationFailed(
                    format!("invalid bar config '{}': {}", path.display(), error).into(),
                )
            })?;
            println!(
                "bar config ok: {} modules, height={}px, position={:?}",
                config.modules.len(),
                config.bar.height,
                config.bar.position
            );
            return Ok(());
        }
        Some(Commands::Version) => {
            println!("DeskHalloumi {}", env!("CARGO_PKG_VERSION"));
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

    info!("DeskHalloumi startup: begin");

    // Run async initialization in a tokio runtime.
    let runtime = tokio::runtime::Runtime::new().map_err(|error| {
        iced::Error::WindowCreationFailed(
            format!("failed to create tokio runtime during startup: {error}").into(),
        )
    })?;

    let StartupState {
        config,
        app_config: loaded_app_config,
        run_options,
        modules,
        module_providers,
        keybinding_actions_enabled,
        runtime_supervisor,
        runtime_spawner,
        provider_refreshes,
    } = runtime.block_on(async {
        // Load configuration and modules at startup
        let config = load_config_with_path(cli.config.clone());
        let scan = deskhalloumi_lib::input::scan_keyboard_device_stats();
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

        let runtime_supervisor = Arc::new(RuntimeSupervisor::start("deskhalloumi-bar", 128));
        let runtime_spawner = runtime_supervisor.spawner();
        let provider_refreshes = ProviderRefreshRegistry::new(4);

        let (action_sender, action_receiver) =
            tokio::sync::mpsc::unbounded_channel::<KeybindingResult>();
        install_keybinding_action_receiver(action_receiver).map_err(std::io::Error::other)?;
        start_action_bus_server(action_sender.clone(), &runtime_spawner)
            .await
            .map_err(std::io::Error::other)?;
        let keybinding_actions_enabled = true;
        if !run_options.no_hotkeyd && !config.keybindings.is_empty() {
            if let Some(pid) = process_instance_status("hotkeyd") {
                warn!(
                    "standalone/global hotkey daemon already owns input as pid={};                      skipping bar-embedded daemon (equivalent to --no-hotkeyd)",
                    pid
                );
            } else {
                let keybindings = config.keybindings.clone();
                let embedded_action_sender = action_sender.clone();
                let cancellation = runtime_spawner.cancellation_token();
                runtime_spawner
                    .spawn("embedded-hotkeyd", async move {
                        let mut daemon = KeybindingDaemon::new(keybindings);
                        daemon.set_action_sender(embedded_action_sender);
                        tokio::select! {
                            _ = cancellation.cancelled() => {}
                            result = daemon.run() => {
                                if let Err(error) = result {
                                    error!(%error, "embedded keybinding daemon exited with error");
                                }
                            }
                        }
                    })
                    .await
                    .map_err(|error| std::io::Error::other(error.to_string()))?;
                info!(
                    "keybinding daemon started with {} bindings and embedded action channel",
                    config.keybindings.len()
                );
            }
        } else if run_options.no_hotkeyd && !config.keybindings.is_empty() {
            info!(
                "bar-embedded hotkey daemon disabled; expecting standalone deskhalloumi-hotkeyd"
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
            unsafe {
                env::set_var("DESKHALLOUMI_XRANDR_PRESETS_YAML", path);
                if env::var_os("UNILII_XRANDR_PRESETS_YAML").is_none() {
                    env::set_var("UNILII_XRANDR_PRESETS_YAML", path);
                }
            }
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

        let module_providers = if module_subscriptions.is_empty() {
            warn!("No module subscriptions available, continuing without real-time updates");
            HashMap::new()
        } else {
            let providers = initialize_module_subscriptions(module_subscriptions, &runtime_spawner)
                .map_err(std::io::Error::other)?;
            info!(providers = providers.len(), "typed module providers initialized");
            providers
        };

        Ok(StartupState {
            config,
            app_config: loaded_app_config,
            run_options,
            modules,
            module_providers,
            keybinding_actions_enabled,
            runtime_supervisor,
            runtime_spawner,
            provider_refreshes,
        })
    }).map_err(|error: Box<dyn std::error::Error>| {
        iced::Error::WindowCreationFailed(
            format!("runtime initialization failed: {error}").into(),
        )
    })?;

    // Get window settings from first panel config
    let first_panel = config
        .panels
        .first()
        .cloned()
        .unwrap_or_else(default_panel_config);
    let window_settings = build_window_settings(&first_panel, &run_options);

    #[cfg(target_os = "linux")]
    info!(
        "linux window settings: application_id=com.unilii.bar, override_redirect={}, debug_focus_mode={}",
        !run_options.debug_focus, run_options.debug_focus
    );

    info!("DeskHalloumi startup: load finished, launching iced application");

    // Wrap pre-loaded data in Rc<RefCell<>> for Fn-compatible closure
    let modules = Rc::new(RefCell::new(Some(modules)));
    let module_providers = Rc::new(RefCell::new(Some(module_providers)));
    let config = Rc::new(RefCell::new(Some(config)));
    let app_config = Rc::new(RefCell::new(Some(loaded_app_config)));
    let window_settings = Rc::new(RefCell::new(Some(window_settings)));
    let run_options = Rc::new(RefCell::new(Some(run_options)));
    let keybinding_actions_enabled = Rc::new(RefCell::new(Some(keybinding_actions_enabled)));
    let runtime_supervisor = Rc::new(RefCell::new(Some(runtime_supervisor)));
    let runtime_spawner = Rc::new(RefCell::new(Some(runtime_spawner)));
    let provider_refreshes = Rc::new(RefCell::new(Some(provider_refreshes)));

    // Create closure that can be called multiple times (Fn requirement)
    let initial_state = move || -> (UniliiBar, Task<Message>) {
        let modules = modules.borrow_mut().take().unwrap_or_default();
        let module_providers = module_providers.borrow_mut().take().unwrap_or_default();
        let config = config.borrow_mut().take().unwrap_or_default();
        let app_config = app_config.borrow_mut().take().unwrap_or_default();
        let window_settings = window_settings.borrow_mut().take().unwrap_or_default();
        let run_options = run_options.borrow_mut().take().unwrap_or_default();
        let keybinding_actions_enabled = keybinding_actions_enabled
            .borrow_mut()
            .take()
            .unwrap_or(false);
        let runtime_supervisor = runtime_supervisor
            .borrow_mut()
            .take()
            .unwrap_or_else(|| Arc::new(RuntimeSupervisor::start("deskhalloumi-bar-fallback", 32)));
        let runtime_spawner = runtime_spawner
            .borrow_mut()
            .take()
            .unwrap_or_else(|| runtime_supervisor.spawner());
        let provider_refreshes = provider_refreshes
            .borrow_mut()
            .take()
            .unwrap_or_else(|| ProviderRefreshRegistry::new(4));
        let mut sysmonitor = SysMonitor::new();
        sysmonitor.update_stats();
        let wifi = Wifi::new();
        let audio = Audio::new();
        let power = Power::new();
        let video = Video::with_preset_source(
            config
                .menus
                .system
                .xrandr_presets_yaml
                .clone()
                .or_else(|| app_config.app.xrandr_presets_yaml.clone())
                .map(PathBuf::from),
        );
        let metrics = global_runtime_metrics();
        let (audio_provider, audio_provider_state) =
            provider_channel(widgets::audio::provider_contract(), Arc::clone(&metrics));
        let (network_provider, network_provider_state) =
            provider_channel(widgets::wifi::provider_contract(), Arc::clone(&metrics));
        let (system_provider, system_provider_state) =
            provider_channel(widgets::sysmonitor::provider_contract(), metrics);
        let generation = system_provider.begin_refresh();
        let _ = system_provider.publish_result(generation, Ok(sysmonitor.snapshot().clone()));
        let (main_window_id, open_main_window) = window::open(window_settings);

        (
            UniliiBar {
                main_window_id: Some(main_window_id),
                tray_window_id: None,
                modules,
                module_providers,
                config,
                sysmonitor,
                wifi,
                audio,
                video,
                power,
                runtime_supervisor,
                runtime_spawner,
                provider_refreshes,
                audio_provider,
                audio_provider_state,
                network_provider,
                network_provider_state,
                system_provider,
                system_provider_state,
                system_menu: SystemMenuRuntime::default(),
                action_history: ActionHistory::default(),
                shift_held: false,
                tray_icons: Vec::new(),
                enhanced_tray: None,
                tray_quick_select: None,
                run_options,
                keybinding_actions_enabled,
            },
            Task::batch([
                open_main_window.map(Message::WindowOpened),
                Task::done(Message::LegacyWidgetTick("wifi".to_string())),
                Task::done(Message::LegacyWidgetTick("audio".to_string())),
                Task::done(Message::LegacyWidgetTick("video".to_string())),
                Task::done(Message::LegacyWidgetTick("power".to_string())),
            ]),
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
    fn network_view_count_includes_semantic_status_and_section_rows() {
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

        // Three controls, connection status, two section headings, two networks,
        // and the explicit empty saved-connections state.
        assert_eq!(get_current_menu_item_count(&state), 9);
        assert_eq!(current_menu_items_len(&state), 9);
        assert_eq!(selectable_menu_indices(&state), vec![0, 1, 2, 6]);
    }

    #[test]
    fn menu_selection_skips_disabled_labels_and_separators() {
        let make_item = |id: &str, enabled: bool, separator: bool| enhanced_tray::TrayMenuItem {
            id: id.to_string(),
            label: id.to_string(),
            action: enhanced_tray::TrayMenuAction::SpawnCommand(format!("echo {id}")),
            icon: None,
            submenu: Vec::new(),
            enabled,
            visible: true,
            checkable: false,
            checked: false,
            shortcut: None,
            is_separator: separator,
            app_id: SYSTEM_MENU_APP_ID.to_string(),
            full_path: id.to_string(),
            widget_type: enhanced_tray::TrayWidgetType::Button,
            default_value: None,
            placeholder: None,
        };
        let mut state = EnhancedTrayState::new();
        state.tree.update_app(system_menu_icon());
        state.tree.update_app_menu(
            SYSTEM_MENU_APP_ID,
            vec![
                make_item("status", false, false),
                make_item("separator", false, true),
                make_item("confirm", true, false),
                make_item("cancel", true, false),
            ],
        );
        state.current_view = TrayViewState::SingleApp {
            app_id: SYSTEM_MENU_APP_ID.to_string(),
            navigation: state.tree.get_app_navigation(SYSTEM_MENU_APP_ID),
            submenu_path: Vec::new(),
        };
        state.selected_index = None;

        assert_eq!(selectable_menu_indices(&state), vec![2, 3]);
        move_menu_selection(&mut state, true);
        assert_eq!(state.selected_index, Some(2));
        move_menu_selection(&mut state, false);
        assert_eq!(state.selected_index, Some(3));
        assert!(get_menu_action_at_index(&state, 0).is_none());
        assert!(get_menu_action_at_index(&state, 2).is_some());
    }

    #[test]
    fn network_view_actions_follow_canonical_semantic_row_order() {
        // unilii-audit: allow-live-session-command-reference -- this test only asserts menu action data; it does not execute commands.
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
        // Status and section-heading rows are deliberately not actionable.
        assert!(get_menu_action_at_index(&state, 3).is_none());
        assert!(get_menu_action_at_index(&state, 4).is_none());
        assert_eq!(
            get_menu_action_at_index(&state, 5).map(|(_, action)| action),
            Some(enhanced_tray::TrayMenuAction::SpawnCommand(
                "nmcli device wifi connect 'cafe'".into()
            ))
        );
    }
}

// == Enhanced Tray Helper Functions ==

/// Build the canonical ordered rows for specialized menus.
fn specialized_menu_items(
    config: &Config,
    tray_state: &EnhancedTrayState,
) -> Option<Vec<enhanced_tray::TrayMenuItem>> {
    match &tray_state.current_view {
        TrayViewState::Network {
            app_id,
            data,
            loading,
            error,
        } => Some(crate::menus::wifi::build_menu_items(
            app_id,
            data.as_ref(),
            *loading,
            error.as_deref(),
            &config.menus.wifi,
        )),
        TrayViewState::Mount {
            app_id,
            data,
            loading,
            error,
        } => Some(crate::menus::mount::build_menu_items(
            app_id,
            data.as_ref(),
            *loading,
            error.as_deref(),
            &config.menus.mount,
        )),
        TrayViewState::Calendar {
            app_id,
            data,
            loading,
            error,
        } => Some(crate::menus::calendar::build_menu_items(
            app_id,
            data.as_ref(),
            *loading,
            error.as_deref(),
            &config.menus.calendar,
        )),
        _ => None,
    }
}

fn get_current_menu_item_count_with_config(
    config: &Config,
    tray_state: &EnhancedTrayState,
) -> usize {
    match &tray_state.current_view {
        TrayViewState::SingleApp {
            app_id,
            submenu_path,
            ..
        } => resolve_current_single_app_items(tray_state, app_id, submenu_path)
            .map(|items| items.iter().filter(|item| item.visible).count())
            .unwrap_or(0),
        TrayViewState::Aggregated { items, .. } | TrayViewState::Favorites { items } => {
            items.iter().filter(|item| item.visible).count()
        }
        _ => specialized_menu_items(config, tray_state)
            .map(|items| items.into_iter().filter(|item| item.visible).count())
            .unwrap_or(0),
    }
}

#[cfg(test)]
fn get_current_menu_item_count(tray_state: &EnhancedTrayState) -> usize {
    get_current_menu_item_count_with_config(&Config::default(), tray_state)
}

fn get_menu_action_at_index_with_config(
    config: &Config,
    tray_state: &EnhancedTrayState,
    index: usize,
) -> Option<(String, enhanced_tray::TrayMenuAction)> {
    get_menu_item_at_index_with_config(config, tray_state, index)
        .map(|item| (item.app_id, item.action))
}

fn get_menu_item_at_index_with_config(
    config: &Config,
    tray_state: &EnhancedTrayState,
    index: usize,
) -> Option<enhanced_tray::TrayMenuItem> {
    match &tray_state.current_view {
        TrayViewState::SingleApp {
            app_id,
            submenu_path,
            ..
        } => resolve_current_single_app_items(tray_state, app_id, submenu_path)
            .and_then(|items| items.iter().filter(|item| item.visible).nth(index))
            .filter(|item| crate::menus::presentation::is_selectable(item))
            .cloned(),
        TrayViewState::Aggregated { items, .. } | TrayViewState::Favorites { items } => items
            .get(index)
            .filter(|item| crate::menus::presentation::is_selectable(item))
            .cloned(),
        _ => specialized_menu_items(config, tray_state)
            .and_then(|items| items.into_iter().nth(index))
            .filter(crate::menus::presentation::is_selectable),
    }
}

fn build_menu_quick_select(
    config: &Config,
    tray_state: &EnhancedTrayState,
) -> Option<QuickSelectSession<MenuQuickSelectAction>> {
    let action_limit = deskhalloumi_core::quick_select::QUICK_SELECT_ALPHABET
        .chars()
        .count();
    let actions = selectable_menu_indices_with_config(config, tray_state)
        .into_iter()
        .take(action_limit)
        .filter_map(|index| get_menu_item_at_index_with_config(config, tray_state, index))
        .map(|item| {
            let label = split_label(&item.label).0.to_string();
            (
                label,
                MenuQuickSelectAction {
                    app_id: item.app_id,
                    action: item.action,
                },
            )
        })
        .collect::<Vec<_>>();
    let session = QuickSelectSession::new(actions).ok()?;
    (!session.is_empty()).then_some(session)
}

#[cfg(test)]
fn get_menu_action_at_index(
    tray_state: &EnhancedTrayState,
    index: usize,
) -> Option<(String, enhanced_tray::TrayMenuAction)> {
    get_menu_action_at_index_with_config(&Config::default(), tray_state, index)
}

fn selected_favorite_target(tray_state: &EnhancedTrayState) -> Option<(String, String)> {
    let index = tray_state.selected_index?;
    match &tray_state.current_view {
        TrayViewState::SingleApp {
            app_id,
            submenu_path,
            ..
        } if app_id != SYSTEM_MENU_APP_ID => {
            resolve_current_single_app_items(tray_state, app_id, submenu_path)
                .and_then(|items| items.iter().filter(|item| item.visible).nth(index))
                .filter(|item| crate::menus::presentation::is_selectable(item))
                .map(|item| (app_id.clone(), item.id.clone()))
        }
        TrayViewState::Aggregated { items, .. } | TrayViewState::Favorites { items } => items
            .get(index)
            .filter(|item| crate::menus::presentation::is_selectable(item))
            .map(|item| (item.app_id.clone(), item.id.clone())),
        _ => None,
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
fn quick_select_supported_for_view(tray_state: &EnhancedTrayState) -> bool {
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
fn current_menu_items_len_with_config(config: &Config, tray_state: &EnhancedTrayState) -> usize {
    get_current_menu_item_count_with_config(config, tray_state)
}

#[cfg(test)]
fn current_menu_items_len(tray_state: &EnhancedTrayState) -> usize {
    current_menu_items_len_with_config(&Config::default(), tray_state)
}
fn tray_window_width(bar: &UniliiBar) -> f32 {
    let menu_items = bar
        .enhanced_tray
        .as_ref()
        .map(|tray| current_menu_items_len_with_config(&bar.config, tray))
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
        .map(|tray| current_menu_items_len_with_config(&bar.config, tray))
        .unwrap_or(6);
    let ui = &bar.config.menus.ui;
    let body_height = if menu_items > ui.max_visible_rows {
        ui.scroll_height as f32
    } else {
        menu_items.max(1) as f32 * 42.0
    };
    (bar_height + 112.0 + body_height).clamp(
        bar_height + 180.0,
        bar_height + ui.scroll_height as f32 + 180.0,
    )
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
    bar.tray_quick_select = None;
    if let Some(id) = bar.tray_window_id.take() {
        return window::close(id).map(move |_: ()| Message::WindowClosed(id));
    }
    Task::none()
}
fn handle_global_key_event(bar: &mut UniliiBar, code: &str, value: i32) -> Task<Message> {
    info!("keyboard event: code={code}, value={value}");
    if code == "KEY_LEFTSHIFT" || code == "KEY_RIGHTSHIFT" {
        bar.shift_held = value != 0;
        info!("shift state changed: held={}", bar.shift_held);
    }
    handle_evdev_tray_key(bar, code, value).unwrap_or_else(Task::none)
}

fn system_menu_snapshot(bar: &UniliiBar) -> SystemMenuSnapshot {
    let stats = bar.sysmonitor.snapshot();
    SystemMenuSnapshot {
        wifi_enabled: bar.wifi.wifi_enabled(),
        connected_ssid: bar.wifi.connected_ssid().map(str::to_string),
        wifi_label: bar.wifi.compact_label(),
        display_label: bar.video.compact_label(),
        displays: bar
            .video
            .displays()
            .iter()
            .map(|display| SystemDisplaySnapshot {
                name: display.name.clone(),
                mode: display.mode.clone(),
                primary: display.primary,
            })
            .collect(),
        display_status: bar.video.last_status().to_string(),
        display_presets: bar
            .video
            .preset_entries()
            .into_iter()
            .map(|(key, name, description, command)| SystemDisplayPreset {
                key,
                name,
                description,
                command,
            })
            .collect(),
        stats_label: bar.sysmonitor.compact_label(),
        cpu_percent: stats.cpu_percent,
        memory_percent: stats.memory_percent,
        load_average: stats.load_average,
        root_disk_percent: stats.root_disk_percent,
        uptime_label: crate::widgets::sysmonitor::format_uptime(stats.uptime_seconds),
        idle_sleep_enabled: bar.power.idle_sleep_enabled(),
    }
}

fn system_menu_icon() -> enhanced_tray::TrayIcon {
    enhanced_tray::TrayIcon {
        key: SYSTEM_MENU_KEY.to_string(),
        service: "com.unilii.system-menu".to_string(),
        path: "/com/unilii/SystemMenu".to_string(),
        id: SYSTEM_MENU_APP_ID.to_string(),
        title: "System menu".to_string(),
        icon_name: Some("preferences-system".to_string()),
        icon_pixmap: None,
        status: "Active".to_string(),
        has_menu: true,
        menu_object_path: None,
    }
}

fn system_menu_is_open_for(bar: &UniliiBar, section: &str) -> bool {
    if bar.tray_window_id.is_none() {
        return false;
    }
    bar.enhanced_tray
        .as_ref()
        .is_some_and(|state| match &state.current_view {
            TrayViewState::SingleApp {
                app_id,
                submenu_path,
                ..
            } if app_id == SYSTEM_MENU_APP_ID => {
                section == "root" && submenu_path.is_empty()
                    || submenu_path.first().is_some_and(|value| value == section)
            }
            TrayViewState::Network { app_id, .. } => {
                app_id == SYSTEM_MENU_APP_ID && section == "wifi"
            }
            _ => false,
        })
}

fn open_system_menu(bar: &mut UniliiBar, section: &str) -> Task<Message> {
    if system_menu_is_open_for(bar, section) {
        if let Some(state) = bar.enhanced_tray.as_mut() {
            state.hide();
        }
        return resize_window_task(bar, false);
    }
    refresh_system_provider(bar);
    let path = if section == "root" {
        Vec::new()
    } else {
        vec![section.to_string()]
    };
    rebuild_system_menu_with_path(bar, path);
    Task::batch([
        resize_window_task(bar, true),
        Task::done(Message::LegacyWidgetTick("wifi".to_string())),
        Task::done(Message::LegacyWidgetTick("video".to_string())),
        Task::done(Message::LegacyWidgetTick("power".to_string())),
    ])
}

fn rebuild_system_menu_with_path(bar: &mut UniliiBar, submenu_path: Vec<String>) {
    let icon = system_menu_icon();
    let snapshot = system_menu_snapshot(bar);
    let model = build_system_menu_model(
        &bar.config.menus.system,
        &snapshot,
        &bar.config.keybindings,
        &bar.system_menu,
        &bar.action_history,
    );
    let items = model.items;
    let initial_items = submenu_path
        .first()
        .and_then(|section| {
            items
                .iter()
                .find(|item| item.id == *section)
                .map(|item| item.submenu.as_slice())
        })
        .unwrap_or(items.as_slice());
    let initial_selection = initial_items
        .iter()
        .filter(|item| item.visible)
        .enumerate()
        .find_map(|(index, item)| (item.enabled && !item.is_separator).then_some(index));
    let mut state = EnhancedTrayState::new();
    state.tree.update_app(icon);
    state.tree.update_app_menu(SYSTEM_MENU_APP_ID, items);
    let navigation = state.tree.get_app_navigation(SYSTEM_MENU_APP_ID);
    state.current_view = TrayViewState::SingleApp {
        app_id: SYSTEM_MENU_APP_ID.to_string(),
        navigation,
        submenu_path,
    };
    state.selected_index = initial_selection;
    state.show();
    bar.enhanced_tray = Some(state);
}

fn rebuild_system_menu_if_open(bar: &mut UniliiBar) {
    let path = bar
        .enhanced_tray
        .as_ref()
        .and_then(|state| match &state.current_view {
            TrayViewState::SingleApp {
                app_id,
                submenu_path,
                ..
            } if app_id == SYSTEM_MENU_APP_ID => Some(submenu_path.clone()),
            _ => None,
        });
    if let Some(path) = path {
        rebuild_system_menu_with_path(bar, path);
    }
}

fn system_command_for(bar: &UniliiBar, id: &str) -> Option<(String, String)> {
    let config = &bar.config.menus.system;
    let result = match id {
        "wifi-settings" => (
            "Network settings".to_string(),
            bar.config.menus.wifi.settings_command.clone(),
        ),
        "stats" => ("System monitor".to_string(), config.stats_command.clone()),
        "lock" => ("Lock session".to_string(), config.lock_command.clone()),
        "suspend" => ("Suspend".to_string(), config.suspend_command.clone()),
        "logout" => ("Log out".to_string(), config.logout_command.clone()),
        "reboot" => (
            "Restart computer".to_string(),
            config.reboot_command.clone(),
        ),
        "poweroff" => (
            "Shut down computer".to_string(),
            config.poweroff_command.clone(),
        ),
        "idle-toggle" if bar.power.idle_sleep_enabled() => (
            "Disable inactivity sleep".to_string(),
            config.idle_disable_command.clone(),
        ),
        "idle-toggle" => (
            "Enable inactivity sleep".to_string(),
            config.idle_enable_command.clone(),
        ),
        _ => return None,
    };
    (!result.1.trim().is_empty()).then_some(result)
}

fn pending_system_action(bar: &UniliiBar, id: &str) -> Option<PendingSystemAction> {
    if let Some(extra_id) = id.strip_prefix("extra:") {
        let item = bar
            .config
            .menus
            .system
            .extra_items
            .iter()
            .find(|item| item.id == extra_id)?;
        return Some(PendingSystemAction {
            id: format!("extra:{extra_id}"),
            title: item.title.clone(),
            command: item.command.clone(),
            return_section: "extra".to_string(),
        });
    }
    let (title, command) = system_command_for(bar, id)?;
    Some(PendingSystemAction {
        id: id.to_string(),
        title,
        command,
        return_section: "power".to_string(),
    })
}

fn run_system_shell_command(
    bar: &mut UniliiBar,
    action_id: &str,
    title: &str,
    command: String,
) -> Task<Message> {
    if command.trim().is_empty() {
        bar.system_menu.last_status = Some(format!("{title}: no command configured"));
        bar.action_history.record(
            action_id,
            title,
            "system-menu",
            ActionStatus::Failed,
            Duration::ZERO,
            Some("no command configured".to_string()),
        );
        rebuild_system_menu_if_open(bar);
        return Task::none();
    }
    let action_id_owned = action_id.to_string();
    let completion_action_id = action_id_owned.clone();
    let title_owned = title.to_string();
    let sequence =
        bar.action_history
            .start(action_id_owned.clone(), title_owned.clone(), "system-menu");
    bar.system_menu.busy_action = Some(title_owned.clone());
    bar.system_menu.last_status = None;
    rebuild_system_menu_if_open(bar);
    let timeout = Duration::from_millis(bar.config.menus.system.command_timeout_ms.max(100));
    Task::perform(
        async move {
            let runner =
                ActionRunner::with_timeout("system-menu", action_id_owned.clone(), timeout);
            let outcome = runner
                .run_command(ActionCommand::new(
                    "sh",
                    vec![OsString::from("-lc"), OsString::from(command)],
                ))
                .await;
            tracing::info!(
                menu = %outcome.menu,
                action = %outcome.action,
                duration_ms = outcome.duration_ms,
                exit_code = ?outcome.exit_code,
                error_class = ?outcome.error_class,
                stdout_bytes = outcome.stdout_bytes,
                stderr_bytes = outcome.stderr_bytes,
                stdout_truncated = outcome.stdout_truncated,
                stderr_truncated = outcome.stderr_truncated,
                "system menu action completed"
            );
            let timed_out = outcome.error_class.as_deref() == Some("timeout");
            let duration_ms = outcome.duration_ms;
            let result = match outcome.result {
                Ok(()) => {
                    let detail = outcome.stdout.trim();
                    let truncation = if outcome.stdout_truncated {
                        format!(" [output truncated; {} bytes total]", outcome.stdout_bytes)
                    } else {
                        String::new()
                    };
                    Ok(if detail.is_empty() {
                        format!("{title_owned} completed{truncation}")
                    } else {
                        format!("{title_owned}: {detail}{truncation}")
                    })
                }
                Err(error) => {
                    let stderr = outcome.stderr.trim();
                    let truncation = if outcome.stderr_truncated {
                        format!(
                            " [error output truncated; {} bytes total]",
                            outcome.stderr_bytes
                        )
                    } else {
                        String::new()
                    };
                    Err(if stderr.is_empty() {
                        format!("{error}{truncation}")
                    } else {
                        format!("{stderr}{truncation}")
                    })
                }
            };
            (duration_ms, timed_out, result)
        },
        move |(duration_ms, timed_out, result)| Message::SystemActionDone {
            sequence,
            action_id: completion_action_id.clone(),
            duration_ms,
            timed_out,
            result,
        },
    )
}

fn handle_system_shortcut(bar: &mut UniliiBar, index: usize) -> Task<Message> {
    let Some(binding) = bar.config.keybindings.get(index).cloned() else {
        bar.system_menu.last_status = Some(format!("Shortcut index {index} no longer exists"));
        rebuild_system_menu_if_open(bar);
        return Task::none();
    };
    match binding.command_type {
        CommandType::Shell => {
            run_system_shell_command(bar, &binding.name, &binding.name, binding.command)
        }
        CommandType::Menu => match parse_menu_action(&binding.command).and_then(|action| {
            MenuProcessManager::default()
                .execute(&action)
                .map(|outcome| format!("{outcome:?}"))
        }) {
            Ok(message) => {
                bar.system_menu.last_status = Some(format!("{}: {message}", binding.name));
                rebuild_system_menu_if_open(bar);
                Task::none()
            }
            Err(error) => {
                bar.system_menu.last_status = Some(format!("{} failed: {error}", binding.name));
                rebuild_system_menu_if_open(bar);
                Task::none()
            }
        },
        CommandType::Tray => handle_tray_daemon_action(bar, parse_tray_action(&binding.command)),
        CommandType::Bar => handle_bar_daemon_action(parse_bar_action(&binding.command)),
        CommandType::Widget => {
            bar.system_menu.last_status = Some(format!(
                "{}: widget actions are not available",
                binding.name
            ));
            rebuild_system_menu_if_open(bar);
            Task::none()
        }
    }
}

fn handle_system_internal_action(
    bar: &mut UniliiBar,
    action: SystemInternalAction,
) -> Task<Message> {
    match action {
        SystemInternalAction::OpenWifi | SystemInternalAction::RefreshWifi => {
            let force_scan = matches!(action, SystemInternalAction::RefreshWifi);
            bar.enhanced_tray = Some(EnhancedTrayState::new());
            if let Some(state) = bar.enhanced_tray.as_mut() {
                state.current_view = TrayViewState::Network {
                    app_id: SYSTEM_MENU_APP_ID.to_string(),
                    data: None,
                    loading: true,
                    error: None,
                };
                state.show();
            }
            let _ = force_scan;
            Task::done(Message::TrayNetworkRefresh(SYSTEM_MENU_KEY.to_string()))
        }
        SystemInternalAction::ToggleWifi => {
            let desired = !bar.wifi.wifi_enabled();
            let title = if desired {
                "Enable Wi-Fi"
            } else {
                "Disable Wi-Fi"
            };
            let sequence = bar
                .action_history
                .start("wifi-toggle", title, "system-menu");
            bar.system_menu.busy_action = Some(title.to_string());
            let permit = match bar.provider_refreshes.try_start("legacy-wifi") {
                Ok(permit) => permit,
                Err(error) => {
                    bar.system_menu.busy_action = None;
                    bar.system_menu.last_status = Some(format!("Wi-Fi action skipped: {error}"));
                    bar.action_history.finish(
                        sequence,
                        ActionStatus::Failed,
                        Duration::ZERO,
                        Some(error.to_string()),
                    );
                    rebuild_system_menu_if_open(bar);
                    return Task::none();
                }
            };
            let nmcli_path = bar.run_options.nmcli_path.clone();
            Task::perform(
                async move {
                    let _permit = permit;
                    let started = std::time::Instant::now();
                    let result = set_wifi_enabled(nmcli_path, desired).await;
                    (started.elapsed().as_millis(), result)
                },
                move |(duration_ms, result)| Message::SystemActionDone {
                    sequence,
                    action_id: "wifi-toggle".to_string(),
                    duration_ms,
                    timed_out: false,
                    result: result.map(|_| {
                        if desired {
                            "Wi-Fi enabled".to_string()
                        } else {
                            "Wi-Fi disabled".to_string()
                        }
                    }),
                },
            )
        }
        SystemInternalAction::RefreshDisplays => {
            bar.system_menu.last_status = Some("Display state refreshed".to_string());
            rebuild_system_menu_if_open(bar);
            start_video_refresh(bar, None)
        }
        SystemInternalAction::ApplyDisplayPreset(key) => {
            let Some((_, title, _, command)) = bar
                .video
                .preset_entries()
                .into_iter()
                .find(|entry| entry.0 == key)
            else {
                bar.system_menu.last_status = Some(format!("Unknown display preset: {key}"));
                rebuild_system_menu_if_open(bar);
                return Task::none();
            };
            run_system_shell_command(bar, &format!("display-preset:{key}"), &title, command)
        }
        SystemInternalAction::RefreshStats => {
            refresh_system_provider(bar);
            bar.system_menu.last_status = Some("System statistics refreshed".to_string());
            rebuild_system_menu_if_open(bar);
            Task::none()
        }
        SystemInternalAction::RunConfigured(id) => match system_command_for(bar, &id) {
            Some((title, command)) => run_system_shell_command(bar, &id, &title, command),
            None => {
                bar.system_menu.last_status = Some(format!("No command configured for {id}"));
                rebuild_system_menu_if_open(bar);
                Task::none()
            }
        },
        SystemInternalAction::Shortcut(index) => handle_system_shortcut(bar, index),
        SystemInternalAction::Extra(id) => {
            let Some(item) = bar
                .config
                .menus
                .system
                .extra_items
                .iter()
                .find(|item| item.id == id)
                .cloned()
            else {
                bar.system_menu.last_status = Some(format!("Unknown extra action: {id}"));
                rebuild_system_menu_if_open(bar);
                return Task::none();
            };
            run_system_shell_command(bar, &format!("extra:{id}"), &item.title, item.command)
        }
        SystemInternalAction::Confirm(id) => {
            bar.system_menu.pending_confirmation = pending_system_action(bar, &id);
            rebuild_system_menu_with_path(bar, Vec::new());
            Task::none()
        }
        SystemInternalAction::ConfirmExecute => {
            let Some(pending) = bar.system_menu.pending_confirmation.take() else {
                return Task::none();
            };
            run_system_shell_command(bar, &pending.id, &pending.title, pending.command)
        }
        SystemInternalAction::ConfirmCancel => {
            let return_section = bar
                .system_menu
                .pending_confirmation
                .take()
                .map(|pending| pending.return_section)
                .unwrap_or_else(|| "power".to_string());
            bar.system_menu.last_status = Some("Action cancelled".to_string());
            rebuild_system_menu_with_path(bar, vec![return_section]);
            Task::none()
        }
    }
}

fn resolve_tray_icon_key(tray_icons: &[tray::TrayIcon], app_id: &str) -> Option<String> {
    if app_id == SYSTEM_MENU_APP_ID {
        return Some(SYSTEM_MENU_KEY.to_string());
    }
    tray_icons
        .iter()
        .find(|icon| icon.id == app_id)
        .map(|icon| icon.key.clone())
}

fn ensure_tray_state_for_global_action(bar: &mut UniliiBar) {
    if bar.enhanced_tray.is_some() {
        return;
    }
    let mut state = EnhancedTrayState::new();
    for icon in &bar.tray_icons {
        state
            .tree
            .update_app(to_enhanced_tray_icon(icon, icon.has_menu));
    }
    state.show();
    bar.enhanced_tray = Some(state);
}

fn open_existing_or_first_tray(bar: &mut UniliiBar) -> Task<Message> {
    if let Some(state) = bar.enhanced_tray.as_mut() {
        state.show();
        return resize_window_task(bar, true);
    }
    if let Some(icon) = bar.tray_icons.first() {
        return Task::done(Message::TrayIconPressed(icon.key.clone()));
    }
    warn!("tray action requested but no tray icons are available");
    Task::none()
}

fn handle_tray_daemon_action(bar: &mut UniliiBar, action: TrayDaemonAction) -> Task<Message> {
    match action {
        TrayDaemonAction::OpenMenu => open_existing_or_first_tray(bar),
        TrayDaemonAction::CloseMenu => {
            if let Some(state) = bar.enhanced_tray.as_mut() {
                state.hide();
            }
            resize_window_task(bar, false)
        }
        TrayDaemonAction::ToggleMenu => {
            if bar.tray_window_id.is_some() {
                if let Some(state) = bar.enhanced_tray.as_mut() {
                    state.hide();
                }
                resize_window_task(bar, false)
            } else {
                open_existing_or_first_tray(bar)
            }
        }
        TrayDaemonAction::ShowAggregated => {
            ensure_tray_state_for_global_action(bar);
            show_aggregated(&mut bar.enhanced_tray);
            if let Some(state) = bar.enhanced_tray.as_mut() {
                state.show();
            }
            resize_window_task(bar, true)
        }
        TrayDaemonAction::ShowFavorites => {
            ensure_tray_state_for_global_action(bar);
            show_favorites(&mut bar.enhanced_tray);
            if let Some(state) = bar.enhanced_tray.as_mut() {
                state.show();
            }
            resize_window_task(bar, true)
        }
        TrayDaemonAction::FocusNext => {
            handle_evdev_tray_key(bar, "KEY_DOWN", 1).unwrap_or_else(Task::none)
        }
        TrayDaemonAction::FocusPrevious => {
            handle_evdev_tray_key(bar, "KEY_UP", 1).unwrap_or_else(Task::none)
        }
        TrayDaemonAction::ActivateSelected => {
            handle_evdev_tray_key(bar, "KEY_ENTER", 1).unwrap_or_else(Task::none)
        }
        TrayDaemonAction::OpenIndex(index) => {
            if let Some(icon) = bar.tray_icons.get(index) {
                Task::done(Message::TrayIconPressed(icon.key.clone()))
            } else {
                warn!(
                    "tray open-index action out of range: index={} icons={}",
                    index,
                    bar.tray_icons.len()
                );
                Task::none()
            }
        }
        TrayDaemonAction::RefreshStatus => {
            let refresh = bar
                .enhanced_tray
                .as_ref()
                .and_then(|state| match &state.current_view {
                    TrayViewState::Network { app_id, .. } => {
                        Some(Message::TrayNetworkRefresh(app_id.clone()))
                    }
                    TrayViewState::Mount { app_id, .. } => {
                        Some(Message::TrayMountRefresh(app_id.clone()))
                    }
                    TrayViewState::Calendar { app_id, .. } => {
                        Some(Message::TrayCalendarRefresh(app_id.clone()))
                    }
                    _ => None,
                });
            refresh.map(Task::done).unwrap_or_else(|| {
                warn!("tray refresh-status is unavailable for the current view");
                Task::none()
            })
        }
        TrayDaemonAction::Raw(command) => {
            warn!("unsupported tray hotkey action: {command}");
            Task::none()
        }
    }
}

fn handle_bar_daemon_action(action: BarDaemonAction) -> Task<Message> {
    match action {
        BarDaemonAction::ReloadConfig => {
            warn!(
                "bar reload-config hotkey reached the embedded action bus, but live bar config reload is not implemented; restart the bar"
            );
        }
        BarDaemonAction::ToggleModule(module) => {
            warn!(
                "bar toggle-module action for '{module}' is not implemented by the current module runtime"
            );
        }
        BarDaemonAction::FocusModule(module) => {
            warn!("bar focus-module action for '{module}' is not implemented");
        }
        BarDaemonAction::Raw(command) => {
            warn!("unsupported bar hotkey action: {command}");
        }
    }
    Task::none()
}

fn handle_keybinding_action(bar: &mut UniliiBar, action: KeybindingResult) -> Task<Message> {
    match action {
        KeybindingResult::RawKeyEvent { code, value } => handle_global_key_event(bar, &code, value),
        KeybindingResult::TrayAction(command) => {
            handle_tray_daemon_action(bar, parse_tray_action(&command))
        }
        KeybindingResult::BarAction(command) => {
            handle_bar_daemon_action(parse_bar_action(&command))
        }
        KeybindingResult::WidgetAction(command) => {
            let Some((widget, action)) = command.split_once(':') else {
                warn!("widget action must use <widget>:<action>: {command}");
                return Task::none();
            };
            match widget.trim().to_ascii_lowercase().as_str() {
                "wifi" => Task::done(Message::LegacyWidget(WidgetMessage::Wifi(
                    action.to_string(),
                ))),
                "audio" => Task::done(Message::LegacyWidget(WidgetMessage::Audio(
                    action.to_string(),
                ))),
                "video" | "display" => Task::done(Message::LegacyWidget(WidgetMessage::Video(
                    action.to_string(),
                ))),
                "power" => Task::done(Message::LegacyWidget(WidgetMessage::Power(
                    action.to_string(),
                ))),
                "sysmonitor" | "system" if action == "refresh" => {
                    refresh_system_provider(bar);
                    Task::none()
                }
                unknown => {
                    warn!("unsupported widget action target '{unknown}': {command}");
                    Task::none()
                }
            }
        }
        KeybindingResult::MenuAction(command) => {
            info!("managed menu action completed in keybinding daemon: {command}");
            Task::none()
        }
        KeybindingResult::ShellCommand(command) => {
            info!("shell hotkey command started: {command}");
            Task::none()
        }
        KeybindingResult::Unknown => {
            warn!("received unknown keybinding action");
            Task::none()
        }
    }
}

fn submenu_is_open(tray_state: &EnhancedTrayState) -> bool {
    matches!(
        &tray_state.current_view,
        TrayViewState::SingleApp { submenu_path, .. } if !submenu_path.is_empty()
    )
}

fn selectable_menu_indices_with_config(
    config: &Config,
    tray_state: &EnhancedTrayState,
) -> Vec<usize> {
    match &tray_state.current_view {
        TrayViewState::SingleApp {
            app_id,
            submenu_path,
            ..
        } => resolve_current_single_app_items(tray_state, app_id, submenu_path)
            .map(crate::menus::presentation::selectable_visible_indices)
            .unwrap_or_default(),
        TrayViewState::Aggregated { items, .. } | TrayViewState::Favorites { items } => items
            .iter()
            .enumerate()
            .filter_map(|(index, item)| {
                crate::menus::presentation::is_selectable(item).then_some(index)
            })
            .collect(),
        _ => specialized_menu_items(config, tray_state)
            .map(|items| crate::menus::presentation::selectable_visible_indices(&items))
            .unwrap_or_default(),
    }
}

#[cfg(test)]
fn selectable_menu_indices(tray_state: &EnhancedTrayState) -> Vec<usize> {
    selectable_menu_indices_with_config(&Config::default(), tray_state)
}

fn move_menu_selection_with_config(
    config: &Config,
    tray_state: &mut EnhancedTrayState,
    forward: bool,
) {
    let indices = selectable_menu_indices_with_config(config, tray_state);
    if indices.is_empty() {
        tray_state.selected_index = None;
        return;
    }
    let current_position = tray_state
        .selected_index
        .and_then(|current| indices.iter().position(|index| *index == current));
    let next_position = match (current_position, forward) {
        (Some(position), true) => (position + 1) % indices.len(),
        (Some(0), false) => indices.len() - 1,
        (Some(position), false) => position - 1,
        (None, true) => 0,
        (None, false) => indices.len() - 1,
    };
    tray_state.selected_index = Some(indices[next_position]);
}

#[cfg(test)]
fn move_menu_selection(tray_state: &mut EnhancedTrayState, forward: bool) {
    move_menu_selection_with_config(&Config::default(), tray_state, forward)
}

fn handle_evdev_tray_key(bar: &mut UniliiBar, code: &str, value: i32) -> Option<Task<Message>> {
    if value == 0 {
        return None;
    }
    if let Some(tray_state) = bar.enhanced_tray.as_mut() {
        match code {
            "KEY_ESC" => {
                if submenu_is_open(tray_state) {
                    return Some(Task::done(Message::TrayExitSubmenu));
                }
                tray_state.animation_target = 0.0;
                return Some(resize_window_task(bar, false));
            }
            "KEY_DOWN" | "KEY_TAB" => {
                move_menu_selection_with_config(&bar.config, tray_state, true);
                return Some(Task::none());
            }
            "KEY_UP" => {
                move_menu_selection_with_config(&bar.config, tray_state, false);
                return Some(Task::none());
            }
            "KEY_LEFT" => {
                return Some(Task::done(if submenu_is_open(tray_state) {
                    Message::TrayExitSubmenu
                } else {
                    Message::TrayNavigateLeft
                }));
            }
            "KEY_RIGHT" => {
                if let Some(idx) = tray_state.selected_index
                    && let Some((app_id, action)) =
                        get_menu_action_at_index_with_config(&bar.config, tray_state, idx)
                    && matches!(
                        action,
                        enhanced_tray::TrayMenuAction::NavigateToSubmenu { .. }
                    )
                {
                    return Some(Task::done(Message::TrayMenuTriggered(app_id, action)));
                }
                return Some(Task::done(Message::TrayNavigateRight));
            }
            "KEY_ENTER" | "KEY_KPENTER" => {
                if let Some(idx) = tray_state.selected_index {
                    if let Some((app_id, action)) =
                        get_menu_action_at_index_with_config(&bar.config, tray_state, idx)
                    {
                        return Some(Task::done(Message::TrayMenuTriggered(app_id, action)));
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
    config: deskhalloumi_core::config::MountMenuConfig,
) -> Result<crate::menus::mount::MountMenuSnapshot, String> {
    let outcome = ActionRunner::with_timeout("mount-menu", "lsblk", Duration::from_secs(6))
        .with_output_limit(2 * 1024 * 1024)
        .run_command(ActionCommand::new(
            "lsblk",
            [
                "-P",
                "-o",
                "NAME,TYPE,FSTYPE,SIZE,MOUNTPOINT,RO,RM,LABEL,MODEL",
            ]
            .into_iter()
            .map(OsString::from)
            .collect(),
        ))
        .await;
    if let Err(error) = outcome.result {
        return Err(if outcome.stderr.trim().is_empty() {
            error
        } else {
            outcome.stderr.trim().to_string()
        });
    }
    if outcome.stdout_truncated {
        return Err(format!(
            "lsblk output exceeded limit ({} bytes)",
            outcome.stdout_bytes
        ));
    }

    let stdout = outcome.stdout;
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
        read_loop_mounts(&devices).await?
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

async fn read_loop_mounts(
    local_devices: &[crate::menus::mount::LocalDevice],
) -> Result<Vec<crate::menus::mount::LoopMount>, String> {
    let outcome = ActionRunner::with_timeout("mount-menu", "losetup", Duration::from_secs(6))
        .with_output_limit(2 * 1024 * 1024)
        .run_command(ActionCommand::new(
            "losetup",
            [
                "--list",
                "--noheadings",
                "--raw",
                "--output",
                "NAME,RO,BACK-FILE",
            ]
            .into_iter()
            .map(OsString::from)
            .collect(),
        ))
        .await;
    if outcome.result.is_err() {
        return Ok(Vec::new());
    }

    let mut mountpoints_by_name = std::collections::HashMap::new();
    for device in local_devices {
        mountpoints_by_name.insert(device.name.clone(), device.mountpoint.clone());
    }

    let mut loop_mounts = Vec::new();
    for line in outcome.stdout.lines() {
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
    accounts: Vec<deskhalloumi_core::config::CalendarAccountConfig>,
    agenda_days: u32,
) -> Result<crate::menus::calendar::CalendarMenuSnapshot, String> {
    use deskhalloumi_lib::calendar::{
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

        match provider
            .fetch_events(
                &account.id,
                &window_start_rfc3339,
                &window_end_rfc3339,
                None,
            )
            .await
        {
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
    config: &deskhalloumi_core::config::CustomMenuConfig,
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
    config: &deskhalloumi_core::config::CustomMenuConfig,
) -> Vec<enhanced_tray::TrayMenuItem> {
    let snapshot = crate::menus::custom::CustomMenuSnapshot::from_config(config);
    snapshot
        .items
        .into_iter()
        .map(|item| {
            let icon_name = item
                .icon_theme
                .or(item.icon_svg_path)
                .or(item.icon_image_path);
            if item.confirm {
                confirmation_submenu(
                    &icon.id,
                    item.id,
                    item.title,
                    item.subtitle
                        .unwrap_or_else(|| "Review this command before running it".to_string()),
                    item.action_command,
                    icon_name,
                    Some("Confirm".to_string()),
                )
            } else {
                presentation_action_item(
                    &icon.id,
                    item.id,
                    item.title,
                    enhanced_tray::TrayMenuAction::SpawnCommand(item.action_command),
                    ActionItemOptions {
                        subtitle: item.subtitle,
                        icon: icon_name,
                        shortcut: None,
                        enabled: true,
                    },
                )
            }
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
    let quick_select_active = bar.tray_quick_select.is_some();
    let quickjump_labels = bar
        .tray_quick_select
        .as_ref()
        .map(|session| {
            session
                .options()
                .iter()
                .map(|option| option.shortcut.to_string())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let content = match &tray_state.current_view {
        TrayViewState::SingleApp {
            app_id, navigation, ..
        } => render_single_app_view_with_main_messages(
            tray_state,
            app_id,
            navigation,
            quick_select_active,
            "",
            &quickjump_labels,
            &bar.config.menus.ui,
        ),
        TrayViewState::Aggregated { items, filter } => render_aggregated_view_with_main_messages(
            tray_state,
            items,
            filter,
            &bar.config.menus.ui,
        ),
        TrayViewState::Favorites { items } => {
            render_favorites_view_with_main_messages(tray_state, items, &bar.config.menus.ui)
        }
        TrayViewState::Network { .. }
        | TrayViewState::Mount { .. }
        | TrayViewState::Calendar { .. } => render_specialized_view_with_main_messages(
            bar,
            tray_state,
            quick_select_active,
            "",
            &quickjump_labels,
        ),
    };

    let opacity = tray_state.animation_progress.clamp(0.0, 1.0);

    container(content)
        .width(Length::Fill)
        .height(Length::Fill)
        .padding([10, 12])
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

fn menu_header_title<'a>(
    title: String,
    subtitle: Option<String>,
    count: Option<usize>,
) -> Element<'a, Message> {
    let mut title_column = column![text(title).size(16)].spacing(1);
    if let Some(subtitle) = subtitle.filter(|value| !value.trim().is_empty()) {
        title_column = title_column.push(
            text(subtitle)
                .size(10)
                .color(iced::Color::from_rgb(0.66, 0.69, 0.75)),
        );
    }
    let mut header = row![title_column].align_y(Alignment::Center).spacing(8);
    if let Some(count) = count {
        header = header
            .push(Space::new().width(Length::Fill))
            .push(shortcut_badge(format!("{count} items")));
    }
    header.into()
}

fn shortcut_badge(label: String) -> Element<'static, Message> {
    container(text(label).size(9))
        .padding([2, 6])
        .style(|_theme| container::Style {
            background: Some(iced::Background::Color([0.16, 0.18, 0.22, 1.0].into())),
            border: iced::Border {
                width: 1.0,
                color: [0.28, 0.31, 0.37, 1.0].into(),
                radius: 8.0.into(),
            },
            ..Default::default()
        })
        .into()
}

fn menu_mode_toolbar(show_favorites: bool) -> Element<'static, Message> {
    let all = button(text("All actions").size(10))
        .padding([3, 8])
        .style(if show_favorites {
            button::text
        } else {
            button::primary
        })
        .on_press(Message::TrayShowAggregated);
    let favorites = button(text("Favorites").size(10))
        .padding([3, 8])
        .style(if show_favorites {
            button::primary
        } else {
            button::text
        })
        .on_press(Message::TrayShowFavorites);
    row![all, favorites].spacing(2).into()
}

fn quickjump_banner(_input: &str) -> Element<'static, Message> {
    let detail = "Quick-select: press one shown key; any other key aborts".to_string();
    container(
        row![
            text("⌨").size(12),
            text(detail).size(10),
            Space::new().width(Length::Fill),
            shortcut_badge("one key".to_string()),
        ]
        .spacing(6)
        .align_y(Alignment::Center),
    )
    .padding([5, 8])
    .width(Length::Fill)
    .style(|_theme| container::Style {
        background: Some(iced::Background::Color([0.12, 0.16, 0.22, 1.0].into())),
        border: iced::Border {
            width: 1.0,
            color: [0.24, 0.38, 0.58, 1.0].into(),
            radius: 8.0.into(),
        },
        ..Default::default()
    })
    .into()
}

fn submenu_breadcrumb(state: &EnhancedTrayState, app_id: &str, path: &[String]) -> String {
    let Some(app) = state.tree.apps.get(app_id) else {
        return path.join(" / ");
    };
    let mut labels = vec![app.icon.title.clone()];
    let mut items = app.menu_items.as_slice();
    for segment in path {
        if let Some(item) = items.iter().find(|item| item.id == *segment) {
            labels.push(split_label(&item.label).0.to_string());
            items = item.submenu.as_slice();
        } else {
            labels.push(segment.clone());
            break;
        }
    }
    labels.join("  ›  ")
}

fn render_single_app_view_with_main_messages<'a>(
    state: &'a EnhancedTrayState,
    app_id: &'a str,
    navigation: &'a enhanced_tray::TrayMenuNavigation,
    quickjump_active: bool,
    quickjump_input: &str,
    quickjump_labels: &[String],
    ui: &'a MenuUiConfig,
) -> Element<'a, Message> {
    let app_menu = state.tree.apps.get(app_id);
    let submenu_path = match &state.current_view {
        TrayViewState::SingleApp { submenu_path, .. } => submenu_path.as_slice(),
        _ => &[],
    };
    let current_items = app_menu
        .and_then(|_| resolve_current_single_app_items(state, app_id, submenu_path))
        .unwrap_or(&[]);
    let actionable_count =
        crate::menus::presentation::selectable_visible_indices(current_items).len();
    let mut content = column!().spacing(8);

    let mut header = row!().spacing(6).align_y(Alignment::Center);
    if !submenu_path.is_empty() {
        header = header.push(
            button(text("←").size(13))
                .padding([3, 7])
                .style(button::text)
                .on_press(Message::TrayExitSubmenu),
        );
    }
    if navigation.can_go_left {
        header = header.push(
            button(text("‹").size(16))
                .padding([2, 6])
                .style(button::text)
                .on_press(Message::TrayNavigateLeft),
        );
    }
    if let Some(app) = app_menu {
        header = header
            .push(render_enhanced_icon_badge(&app.icon, 22.0))
            .push(menu_header_title(
                bounded_text(&app.icon.title, ui.max_label_chars),
                Some(if submenu_path.is_empty() {
                    format!("{} · {} actionable", app.icon.status, actionable_count)
                } else {
                    format!("Submenu · {} actionable", actionable_count)
                }),
                None,
            ));
    } else {
        header = header.push(menu_header_title(
            bounded_text(app_id, ui.max_label_chars),
            Some("Menu provider unavailable".to_string()),
            None,
        ));
    }
    header = header.push(Space::new().width(Length::Fill));
    if navigation.can_go_right {
        header = header.push(
            button(text("›").size(16))
                .padding([2, 6])
                .style(button::text)
                .on_press(Message::TrayNavigateRight),
        );
    }
    content = content.push(header);

    if ui.show_breadcrumbs && !submenu_path.is_empty() {
        content = content.push(
            text(submenu_breadcrumb(state, app_id, submenu_path))
                .size(10)
                .color(iced::Color::from_rgb(0.62, 0.66, 0.73)),
        );
    }
    content = content.push(menu_mode_toolbar(false));
    if quickjump_active {
        content = content.push(quickjump_banner(quickjump_input));
    }
    if current_items.iter().any(|item| item.visible) {
        content = content.push(render_menu_items_with_main_messages(
            current_items,
            state.selected_index,
            app_id,
            submenu_path,
            quickjump_active,
            quickjump_labels,
            ui,
        ));
    } else {
        content = content.push(render_empty_state(
            "No actions available",
            "This application did not expose any visible menu items.",
        ));
    }
    if ui.show_keyboard_hints {
        content = content.push(render_keyboard_hints(
            "↑/↓ select · Enter activate · ← back · g quick-select · Esc close",
        ));
    }
    content.into()
}

fn render_aggregated_view_with_main_messages<'a>(
    state: &'a EnhancedTrayState,
    items: &'a [enhanced_tray::TrayMenuItem],
    filter: &'a Option<String>,
    ui: &'a MenuUiConfig,
) -> Element<'a, Message> {
    let mut content = column!().spacing(8);
    content = content.push(menu_header_title(
        "All tray actions".to_string(),
        Some("Search across every application menu".to_string()),
        ui.show_item_counts.then_some(items.len()),
    ));
    content = content.push(menu_mode_toolbar(false));
    let search = text_input(
        "Search actions, paths, and applications…",
        filter.as_deref().unwrap_or(""),
    )
    .on_input(Message::TrayFilterUpdate)
    .size(12)
    .padding([6, 9])
    .width(Length::Fill);
    let mut search_row = row![search].spacing(4).align_y(Alignment::Center);
    if filter.as_ref().is_some_and(|value| !value.is_empty()) {
        search_row = search_row.push(
            button(text("Clear").size(10))
                .padding([5, 8])
                .style(button::text)
                .on_press(Message::TrayFilterUpdate(String::new())),
        );
    }
    content = content.push(search_row);
    if items.is_empty() {
        content = content.push(render_empty_state(
            "No matching actions",
            "Try fewer or broader search terms.",
        ));
    } else {
        content = content.push(render_action_collection(
            state,
            items,
            state.selected_index,
            false,
            ui,
        ));
    }
    if ui.show_keyboard_hints {
        content = content.push(render_keyboard_hints(
            "Type to filter · ↑/↓ select · Enter activate · f favorite · Esc close",
        ));
    }
    content.into()
}

fn render_favorites_view_with_main_messages<'a>(
    state: &'a EnhancedTrayState,
    items: &'a [enhanced_tray::TrayMenuItem],
    ui: &'a MenuUiConfig,
) -> Element<'a, Message> {
    let mut content = column!().spacing(8);
    content = content.push(menu_header_title(
        "Favorite actions".to_string(),
        Some("Pinned commands from application menus".to_string()),
        ui.show_item_counts.then_some(items.len()),
    ));
    content = content.push(menu_mode_toolbar(true));
    if items.is_empty() {
        content = content.push(render_empty_state(
            "No favorites yet",
            "Open All actions and use the star button or press f on a selected row.",
        ));
    } else {
        content = content.push(render_action_collection(
            state,
            items,
            state.selected_index,
            true,
            ui,
        ));
    }
    if ui.show_keyboard_hints {
        content = content.push(render_keyboard_hints(
            "↑/↓ select · Enter activate · f remove favorite · a all actions",
        ));
    }
    content.into()
}

fn specialized_view_metadata(tray_state: &EnhancedTrayState) -> (String, String, &'static str) {
    match &tray_state.current_view {
        TrayViewState::Network {
            data,
            loading,
            error,
            ..
        } => {
            let subtitle = if *loading {
                "Scanning wireless networks…".to_string()
            } else if let Some(error) = error {
                format!("Network data unavailable · {error}")
            } else if let Some(snapshot) = data {
                match snapshot.connected_ssid.as_deref() {
                    Some(ssid) => format!("Connected to {ssid} on {}", snapshot.interface),
                    None if snapshot.enabled => format!("{} · not connected", snapshot.interface),
                    None => "Wireless radio disabled".to_string(),
                }
            } else {
                "No network snapshot".to_string()
            };
            ("Wi-Fi".to_string(), subtitle, "network-wireless")
        }
        TrayViewState::Mount {
            data,
            loading,
            error,
            ..
        } => {
            let subtitle = if *loading {
                "Discovering storage and remote profiles…".to_string()
            } else if let Some(error) = error {
                format!("Storage refresh failed · {error}")
            } else if let Some(snapshot) = data {
                format!(
                    "{} local · {} SSHFS · {} loop · {} encrypted",
                    snapshot.local_devices.len(),
                    snapshot.sshfs_profiles.len(),
                    snapshot.loop_mounts.len(),
                    snapshot.vcvolume_profiles.len()
                )
            } else {
                "No storage snapshot".to_string()
            };
            ("Storage".to_string(), subtitle, "drive-harddisk")
        }
        TrayViewState::Calendar {
            data,
            loading,
            error,
            ..
        } => {
            let subtitle = if *loading {
                "Synchronizing calendar accounts…".to_string()
            } else if let Some(error) = error {
                format!("Calendar refresh failed · {error}")
            } else if let Some(snapshot) = data {
                format!(
                    "{} account(s) · {} upcoming event(s){}",
                    snapshot.account_ids.len(),
                    snapshot.events.len(),
                    if snapshot.stale {
                        " · partial data"
                    } else {
                        ""
                    }
                )
            } else {
                "No calendar snapshot".to_string()
            };
            ("Calendar".to_string(), subtitle, "x-office-calendar")
        }
        _ => ("Menu".to_string(), String::new(), "applications-system"),
    }
}

fn render_specialized_view_with_main_messages<'a>(
    bar: &'a UniliiBar,
    tray_state: &'a EnhancedTrayState,
    quickjump_active: bool,
    quickjump_input: &str,
    quickjump_labels: &[String],
) -> Element<'a, Message> {
    let (title, subtitle, icon_name) = specialized_view_metadata(tray_state);
    let items = specialized_menu_items(&bar.config, tray_state).unwrap_or_default();
    let app_id = match &tray_state.current_view {
        TrayViewState::Network { app_id, .. }
        | TrayViewState::Mount { app_id, .. }
        | TrayViewState::Calendar { app_id, .. } => app_id.clone(),
        _ => String::new(),
    };
    let actionable = crate::menus::presentation::selectable_visible_indices(&items).len();
    let mut content = column!().spacing(8);
    content = content.push(
        row![
            render_icon_badge(Some(icon_name), None, &title, &app_id, &app_id, 22.0),
            menu_header_title(
                bounded_text(&title, bar.config.menus.ui.max_label_chars),
                Some(bounded_text(
                    &subtitle,
                    bar.config.menus.ui.max_subtitle_chars,
                )),
                bar.config.menus.ui.show_item_counts.then_some(actionable),
            ),
        ]
        .spacing(8)
        .align_y(Alignment::Center),
    );
    if quickjump_active {
        content = content.push(quickjump_banner(quickjump_input));
    }
    content = content.push(render_owned_menu_items_with_main_messages(
        items,
        tray_state.selected_index,
        app_id,
        Vec::new(),
        quickjump_active,
        quickjump_labels.to_vec(),
        bar.config.menus.ui.clone(),
    ));
    if bar.config.menus.ui.show_keyboard_hints {
        content = content.push(render_keyboard_hints(
            "↑/↓ select · Enter activate · g quick-select · r refresh · Esc close",
        ));
    }
    content.into()
}

fn render_menu_items_with_main_messages<'a>(
    items: &'a [enhanced_tray::TrayMenuItem],
    selected_index: Option<usize>,
    app_id: &'a str,
    current_submenu_path: &[String],
    quickjump_active: bool,
    quickjump_labels: &[String],
    ui: &'a MenuUiConfig,
) -> Element<'a, Message> {
    let rendered = items
        .iter()
        .filter(|item| item.visible)
        .cloned()
        .enumerate()
        .map(|(visible_index, item)| {
            let hint = quickjump_hint_for_visible_index(items, visible_index, quickjump_labels);
            render_menu_item_owned(
                item,
                selected_index == Some(visible_index),
                app_id.to_string(),
                current_submenu_path.to_vec(),
                quickjump_active,
                hint,
                ui.clone(),
            )
        })
        .collect::<Vec<_>>();
    render_menu_body(
        rendered,
        items.iter().filter(|item| item.visible).count(),
        ui,
    )
}

fn render_owned_menu_items_with_main_messages(
    items: Vec<enhanced_tray::TrayMenuItem>,
    selected_index: Option<usize>,
    app_id: String,
    current_submenu_path: Vec<String>,
    quickjump_active: bool,
    quickjump_labels: Vec<String>,
    ui: MenuUiConfig,
) -> Element<'static, Message> {
    let visible_count = items.iter().filter(|item| item.visible).count();
    let rendered = items
        .iter()
        .filter(|item| item.visible)
        .cloned()
        .enumerate()
        .map(|(visible_index, item)| {
            let hint = quickjump_hint_for_visible_index(&items, visible_index, &quickjump_labels);
            render_menu_item_owned(
                item,
                selected_index == Some(visible_index),
                app_id.clone(),
                current_submenu_path.clone(),
                quickjump_active,
                hint,
                ui.clone(),
            )
        })
        .collect::<Vec<_>>();
    render_menu_body_owned(rendered, visible_count, ui)
}

fn render_menu_body<'a>(
    rows: Vec<Element<'a, Message>>,
    visible_count: usize,
    ui: &'a MenuUiConfig,
) -> Element<'a, Message> {
    let body = rows
        .into_iter()
        .fold(column!().spacing(2), |column, row| column.push(row));
    if visible_count > ui.max_visible_rows {
        scrollable(body)
            .height(Length::Fixed(ui.scroll_height as f32))
            .into()
    } else {
        body.into()
    }
}

fn render_menu_body_owned(
    rows: Vec<Element<'static, Message>>,
    visible_count: usize,
    ui: MenuUiConfig,
) -> Element<'static, Message> {
    let body = rows
        .into_iter()
        .fold(column!().spacing(2), |column, row| column.push(row));
    if visible_count > ui.max_visible_rows {
        scrollable(body)
            .height(Length::Fixed(ui.scroll_height as f32))
            .into()
    } else {
        body.into()
    }
}

fn render_menu_item_owned(
    item: enhanced_tray::TrayMenuItem,
    is_selected: bool,
    app_id: String,
    current_submenu_path: Vec<String>,
    quickjump_active: bool,
    quickjump_label: Option<String>,
    ui: MenuUiConfig,
) -> Element<'static, Message> {
    if item.is_separator {
        return container(Space::new().height(1).width(Length::Fill))
            .padding([4, 0])
            .style(|_theme| container::Style {
                background: Some(iced::Background::Color([0.23, 0.25, 0.29, 1.0].into())),
                ..Default::default()
            })
            .into();
    }

    let cleaned_label = strip_mnemonic_markers(&item.label);
    let (raw_title, raw_subtitle) = split_label(&cleaned_label);
    let title = bounded_text(raw_title, ui.max_label_chars);
    let subtitle = raw_subtitle.map(|value| bounded_text(value, ui.max_subtitle_chars));

    if is_section_item(&item) {
        return container(
            row![text(title).size(10), Space::new().width(Length::Fill),]
                .align_y(Alignment::Center),
        )
        .padding([6, 8])
        .width(Length::Fill)
        .style(|_theme| container::Style {
            background: Some(iced::Background::Color([0.10, 0.11, 0.14, 1.0].into())),
            border: iced::Border {
                width: 0.0,
                color: iced::Color::TRANSPARENT,
                radius: 6.0.into(),
            },
            ..Default::default()
        })
        .into();
    }

    if is_status_item(&item) || (!item.enabled && item.submenu.is_empty()) {
        let mut status = column![text(title).size(11)].spacing(1);
        if let Some(subtitle) = subtitle {
            status = status.push(
                text(subtitle)
                    .size(10)
                    .color(iced::Color::from_rgb(0.65, 0.68, 0.74)),
            );
        }
        return container(status)
            .padding([6, 9])
            .width(Length::Fill)
            .style(|_theme| container::Style {
                background: Some(iced::Background::Color([0.12, 0.13, 0.16, 1.0].into())),
                border: iced::Border {
                    width: 1.0,
                    color: [0.20, 0.22, 0.27, 1.0].into(),
                    radius: 7.0.into(),
                },
                ..Default::default()
            })
            .into();
    }

    if matches!(item.widget_type, enhanced_tray::TrayWidgetType::TextInput)
        || matches!(
            item.action,
            enhanced_tray::TrayMenuAction::TextInputChanged { .. }
        )
    {
        let item_id = item.id.clone();
        let mut input_column = column!().spacing(3);
        if !title.trim().is_empty() {
            input_column = input_column.push(text(title).size(10));
        }
        input_column = input_column.push(
            text_input(
                item.placeholder.as_deref().unwrap_or("Enter value…"),
                item.default_value.as_deref().unwrap_or(""),
            )
            .on_input(move |value| Message::TrayTextInputChanged(item_id.clone(), value))
            .size(12)
            .padding([6, 8])
            .width(Length::Fill),
        );
        return input_column.into();
    }

    let mut item_title = title;
    if quickjump_active {
        if let Some(hint) = quickjump_label {
            item_title = format!("[{hint}] {item_title}");
        }
    }
    if item.checkable {
        item_title = format!("{} {item_title}", if item.checked { "☑" } else { "☐" });
    }
    let mut labels = column![text(item_title).size(12)].spacing(1);
    if let Some(subtitle) = subtitle {
        labels = labels.push(
            text(subtitle)
                .size(10)
                .color(iced::Color::from_rgb(0.64, 0.67, 0.73)),
        );
    }
    let mut row_content = row!().spacing(8).align_y(Alignment::Center);
    row_content = row_content.push(text(if is_selected { "›" } else { " " }).size(12));
    if let Some(icon) = render_menu_item_icon(item.icon.as_deref()) {
        row_content = row_content.push(icon);
    }
    row_content = row_content
        .push(labels)
        .push(Space::new().width(Length::Fill));
    if let Some(shortcut) = item
        .shortcut
        .clone()
        .filter(|value| !value.trim().is_empty())
    {
        row_content = row_content.push(shortcut_badge(shortcut));
    }
    if !item.submenu.is_empty() {
        row_content = row_content.push(text("›").size(15));
    }

    let mut item_button = button(row_content)
        .padding([6, 8])
        .width(Length::Fill)
        .style(if is_selected {
            button::primary
        } else {
            button::text
        });
    if item.enabled {
        if item.submenu.is_empty() {
            item_button = item_button.on_press(Message::TrayMenuTriggered(app_id, item.action));
        } else {
            let mut submenu_path = current_submenu_path;
            submenu_path.push(item.id);
            item_button = item_button.on_press(Message::TrayEnterSubmenu(app_id, submenu_path));
        }
    }
    item_button.into()
}

fn render_action_collection<'a>(
    state: &'a EnhancedTrayState,
    items: &'a [enhanced_tray::TrayMenuItem],
    selected_index: Option<usize>,
    favorites_view: bool,
    ui: &'a MenuUiConfig,
) -> Element<'a, Message> {
    let rows = items
        .iter()
        .enumerate()
        .filter(|(_, item)| item.visible)
        .map(|(index, item)| {
            let cleaned_label = strip_mnemonic_markers(&item.label);
            let (title, subtitle) = split_label(&cleaned_label);
            let display_title = bounded_text(title, ui.max_label_chars);
            let path = if item.full_path.trim().is_empty() {
                subtitle.unwrap_or_default().to_string()
            } else {
                item.full_path.clone()
            };
            let mut labels = column![text(display_title).size(11)].spacing(1);
            if !path.trim().is_empty() {
                labels = labels.push(
                    text(bounded_text(&path, ui.max_subtitle_chars))
                        .size(9)
                        .color(iced::Color::from_rgb(0.62, 0.65, 0.71)),
                );
            }
            let mut action = button(labels).padding([6, 8]).width(Length::Fill).style(
                if selected_index == Some(index) {
                    button::primary
                } else {
                    button::text
                },
            );
            if item.enabled {
                action = action.on_press(Message::TrayMenuTriggered(
                    item.app_id.clone(),
                    item.action.clone(),
                ));
            }
            if favorites_view || ui.show_all_favorites_controls {
                let favorite = state.tree.is_favorite(&item.app_id, &item.id);
                let favorite_label = if favorites_view || favorite {
                    "★"
                } else {
                    "☆"
                };
                let favorite_button = button(text(favorite_label).size(13))
                    .padding([5, 7])
                    .style(button::text)
                    .on_press(Message::TrayToggleFavorite(
                        item.app_id.clone(),
                        item.id.clone(),
                    ));
                row![action, favorite_button]
                    .spacing(3)
                    .align_y(Alignment::Center)
                    .into()
            } else {
                action.into()
            }
        })
        .collect::<Vec<Element<'a, Message>>>();
    let visible_count = rows.len();
    render_menu_body(rows, visible_count, ui)
}

fn render_empty_state(title: &str, detail: &str) -> Element<'static, Message> {
    container(
        column![
            text(title.to_string()).size(12),
            text(detail.to_string())
                .size(10)
                .color(iced::Color::from_rgb(0.64, 0.67, 0.73)),
        ]
        .spacing(3),
    )
    .padding([12, 10])
    .width(Length::Fill)
    .style(|_theme| container::Style {
        background: Some(iced::Background::Color([0.11, 0.12, 0.15, 1.0].into())),
        border: iced::Border {
            width: 1.0,
            color: [0.20, 0.22, 0.27, 1.0].into(),
            radius: 9.0.into(),
        },
        ..Default::default()
    })
    .into()
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

fn render_keyboard_hints(value: &str) -> Element<'static, Message> {
    text(value.to_string())
        .size(9)
        .color(iced::Color::from_rgb(0.56, 0.60, 0.67))
        .into()
}
