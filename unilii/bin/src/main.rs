use clap::Parser;
use iced::futures::{SinkExt, StreamExt};
use iced::keyboard::{self, key, Key, Modifiers};
use iced::widget::{button, column, container, horizontal_space, row, text};
use iced::{window, Element, Length, Subscription, Task};
use std::collections::HashMap;
use std::time::Duration;
use tracing::{error, info, Level};
use unilii_core::{config::load_config, ModuleUpdate};

mod module_loader;
mod tray;
use module_loader::{load_modules, LoadedModule, ModuleReceiver};

struct UniliiBar {
    modules: HashMap<String, LoadedModule>,
    config: unilii_core::config::Config,
    last_key: Option<String>,
    shift_held: bool,
    key_display_mode: KeyDisplayMode,
    module_receivers: Vec<(String, ModuleReceiver)>,
    tray_icons: Vec<tray::TrayIcon>,
    tray_menu: Option<TrayMenuState>,
    cli: Cli,
}

#[derive(Debug, Clone)]
struct TrayMenuState {
    icon_key: String,
    progress: f32,
    target: f32,
    content: TrayMenuContent,
}

#[derive(Debug, Clone)]
enum TrayMenuContent {
    Generic {
        items: Vec<tray::TrayMenuItem>,
    },
    Network {
        data: Option<tray::NetworkSnapshot>,
        loading: bool,
        error: Option<String>,
    },
}

#[derive(Debug, Clone, Parser)]
#[command(name = "unilii", about = "unilii status bar")]
struct Cli {
    #[arg(long, default_value = "nmcli")]
    nmcli_path: String,
    #[arg(long, default_value_t = true)]
    network_menu: bool,
    #[arg(long, default_value_t = 1500)]
    tray_poll_ms: u64,
}

#[derive(Debug, Clone, Copy)]
enum KeyDisplayMode {
    Always,
    ShiftHold,
}

impl KeyDisplayMode {
    fn from_env_value(value: &str) -> Self {
        match value.to_ascii_lowercase().as_str() {
            "always" => Self::Always,
            "shift" | "shift-hold" | "shift_hold" => Self::ShiftHold,
            _ => Self::ShiftHold,
        }
    }

    fn from_env() -> Self {
        let value =
            std::env::var("UNILII_KEY_DISPLAY_MODE").unwrap_or_else(|_| "shift-hold".to_string());
        Self::from_env_value(&value)
    }
}

fn env_flag(name: &str) -> bool {
    matches!(
        std::env::var(name).ok().as_deref(),
        Some("1") | Some("true") | Some("TRUE") | Some("yes") | Some("YES")
    )
}

impl Default for UniliiBar {
    fn default() -> Self {
        tracing_subscriber::fmt().with_max_level(Level::INFO).init();
        info!("Starting unilii status bar");

        // Load configuration
        let config = load_config();
        info!(
            "Loaded window config: {}x{}",
            config.window.width, config.window.height
        );

        // Modules will be loaded in run()
        UniliiBar {
            modules: HashMap::new(),
            config,
            last_key: None,
            shift_held: false,
            key_display_mode: KeyDisplayMode::from_env(),
            module_receivers: Vec::new(),
            tray_icons: Vec::new(),
            tray_menu: None,
            cli: Cli {
                nmcli_path: "nmcli".to_string(),
                network_menu: true,
                tray_poll_ms: 1500,
            },
        }
    }
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
    TrayEvent(tray::TrayEvent),
    TrayIconPressed(String),
    TrayMenuTriggered(String, tray::TrayMenuAction),
    TrayNetworkSnapshot(String, Result<tray::NetworkSnapshot, String>),
    TrayNetworkRefresh(String),
    TrayNetworkToggle(String),
    TrayNetworkToggleDone(String, Result<(), String>),
    TrayAnimateTick,
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
            bar.last_key = Some(format!("{code} ({value})"));
        }
        Message::WindowKeyboardInput {
            key,
            pressed,
            is_shift,
        } => {
            info!(
                "window keyboard event: key={}, pressed={}, is_shift={}",
                key, pressed, is_shift
            );
            if is_shift {
                bar.shift_held = pressed;
                info!(
                    "shift state changed from window event: held={}",
                    bar.shift_held
                );
            }
            let state = if pressed { 1 } else { 0 };
            bar.last_key = Some(format!("WIN:{key} ({state})"));
        }
        Message::TrayEvent(event) => match event {
            tray::TrayEvent::Icons(icons) => {
                bar.tray_icons = icons;
                if let Some(menu) = &bar.tray_menu {
                    let still_exists = bar.tray_icons.iter().any(|icon| icon.key == menu.icon_key);
                    if !still_exists {
                        bar.tray_menu = None;
                    }
                }
            }
        },
        Message::TrayIconPressed(icon_key) => {
            if let Some(current) = bar.tray_menu.as_mut() {
                if current.icon_key == icon_key {
                    current.target = 0.0;
                    return Task::none();
                }
            }

            if let Some(icon) = bar.tray_icons.iter().find(|icon| icon.key == icon_key) {
                if bar.cli.network_menu && tray::is_network_icon(icon) {
                    let icon_key_clone = icon.key.clone();
                    bar.tray_menu = Some(TrayMenuState {
                        icon_key: icon_key_clone.clone(),
                        progress: 0.0,
                        target: 1.0,
                        content: TrayMenuContent::Network {
                            data: None,
                            loading: true,
                            error: None,
                        },
                    });
                    let nmcli_path = bar.cli.nmcli_path.clone();
                    return Task::perform(
                        tray::read_network_snapshot(nmcli_path, false),
                        move |result| Message::TrayNetworkSnapshot(icon_key_clone.clone(), result),
                    );
                }

                let items = tray::build_menu_items(icon);
                if items.is_empty() {
                    bar.tray_menu = None;
                } else {
                    bar.tray_menu = Some(TrayMenuState {
                        icon_key: icon.key.clone(),
                        progress: 0.0,
                        target: 1.0,
                        content: TrayMenuContent::Generic { items },
                    });
                }
            }
        }
        Message::TrayMenuTriggered(icon_key, action) => {
            if let Some(icon) = bar
                .tray_icons
                .iter()
                .find(|icon| icon.key == icon_key)
                .cloned()
            {
                tokio::spawn(async move {
                    tray::invoke_menu_action(&icon, action).await;
                });
            }

            if let Some(menu) = bar.tray_menu.as_mut() {
                menu.target = 0.0;
            }
        }
        Message::TrayNetworkSnapshot(icon_key, result) => {
            if let Some(menu) = bar.tray_menu.as_mut() {
                if menu.icon_key != icon_key {
                    return Task::none();
                }
                if let TrayMenuContent::Network {
                    data,
                    loading,
                    error,
                } = &mut menu.content
                {
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
        Message::TrayNetworkRefresh(icon_key) => {
            if let Some(menu) = bar.tray_menu.as_mut() {
                if menu.icon_key != icon_key {
                    return Task::none();
                }
                if let TrayMenuContent::Network { loading, error, .. } = &mut menu.content {
                    *loading = true;
                    *error = None;
                }
            }

            let nmcli_path = bar.cli.nmcli_path.clone();
            return Task::perform(
                tray::read_network_snapshot(nmcli_path, true),
                move |result| Message::TrayNetworkSnapshot(icon_key.clone(), result),
            );
        }
        Message::TrayNetworkToggle(icon_key) => {
            let mut desired_state = true;
            if let Some(menu) = bar.tray_menu.as_mut() {
                if menu.icon_key != icon_key {
                    return Task::none();
                }
                if let TrayMenuContent::Network {
                    data,
                    loading,
                    error,
                } = &mut menu.content
                {
                    if let Some(snapshot) = data {
                        desired_state = !snapshot.enabled;
                    }
                    *loading = true;
                    *error = None;
                }
            }

            let nmcli_path = bar.cli.nmcli_path.clone();
            return Task::perform(
                tray::set_wifi_enabled(nmcli_path, desired_state),
                move |result| Message::TrayNetworkToggleDone(icon_key.clone(), result),
            );
        }
        Message::TrayNetworkToggleDone(icon_key, result) => {
            if let Some(menu) = bar.tray_menu.as_mut() {
                if menu.icon_key != icon_key {
                    return Task::none();
                }
                if let TrayMenuContent::Network { loading, error, .. } = &mut menu.content {
                    *loading = true;
                    if let Err(message) = result {
                        *loading = false;
                        *error = Some(message);
                        return Task::none();
                    }
                }
            }

            let nmcli_path = bar.cli.nmcli_path.clone();
            return Task::perform(
                tray::read_network_snapshot(nmcli_path, true),
                move |result| Message::TrayNetworkSnapshot(icon_key.clone(), result),
            );
        }
        Message::TrayAnimateTick => {
            if let Some(menu) = bar.tray_menu.as_mut() {
                menu.progress = tray::animate_progress(menu.progress, menu.target, 0.12);
                if menu.progress == 0.0 && menu.target == 0.0 {
                    bar.tray_menu = None;
                }
            }
        }
    }
    Task::none()
}

fn view(bar: &UniliiBar) -> Element<'_, Message> {
    // Collect module views ordered by name
    let mut module_names: Vec<_> = bar.modules.keys().collect();
    module_names.sort();

    let mut right_widgets = vec![];

    for name in module_names {
        if let Some(loaded) = bar.modules.get(name) {
            let view = loaded.module.view();
            // Map module's internal ModuleUpdate messages to our Message
            let widget = view.map(move |update| Message::ModuleUpdate(name.clone(), update));

            right_widgets.push(widget);
        }
    }

    let tray_buttons = bar.tray_icons.iter().fold(
        row!().spacing(2).align_y(iced::Alignment::Center),
        |row, icon| {
            row.push(
                button(text(tray::icon_label_for(icon)).size(14))
                    .padding([1, 5])
                    .on_press(Message::TrayIconPressed(icon.key.clone())),
            )
        },
    );

    right_widgets.push(tray_buttons.into());

    // Right section (clock, battery, etc.)
    let mut right_row = row(right_widgets)
        .spacing(4)
        .align_y(iced::Alignment::Center);

    let show_key = should_show_key(bar.key_display_mode, bar.shift_held, bar.last_key.as_ref());

    if show_key {
        if let Some(last_key) = &bar.last_key {
            right_row = right_row.push(text(format!("Key: {last_key}")).size(12));
        }
    }

    if let Some(menu) = &bar.tray_menu {
        let dynamic_padding = ((1.0 - menu.progress) * 8.0).round().clamp(0.0, 8.0) as u16;
        match &menu.content {
            TrayMenuContent::Generic { items } => {
                let visible = tray::visible_menu_items(items.len(), menu.progress);
                let menu_row = items.iter().take(visible).fold(
                    row!().spacing(2).align_y(iced::Alignment::Center),
                    |row, item| {
                        row.push(
                            button(text(item.label.clone()).size(12))
                                .padding([1, 6])
                                .on_press(Message::TrayMenuTriggered(
                                    menu.icon_key.clone(),
                                    item.action.clone(),
                                )),
                        )
                    },
                );

                right_row = right_row.push(
                    container(menu_row)
                        .padding([0, dynamic_padding])
                        .style(container::rounded_box),
                );
            }
            TrayMenuContent::Network {
                data,
                loading,
                error,
            } => {
                let mut network_menu = column![
                    text("Network").size(12),
                    button(text(if data.as_ref().map(|d| d.enabled).unwrap_or(false) {
                        "Disable Wi-Fi"
                    } else {
                        "Enable Wi-Fi"
                    }))
                    .on_press(Message::TrayNetworkToggle(menu.icon_key.clone())),
                    button(text("Refresh Networks"))
                        .on_press(Message::TrayNetworkRefresh(menu.icon_key.clone()))
                ]
                .spacing(3);

                if let Some(snapshot) = data {
                    network_menu = network_menu
                        .push(text(format!("{}: {}", snapshot.interface, snapshot.state)));
                    if snapshot.enabled {
                        if snapshot.networks.is_empty() {
                            network_menu = network_menu.push(text("No networks found").size(11));
                        } else {
                            for network in &snapshot.networks {
                                network_menu = network_menu.push(text(format!(
                                    "{} ({}%) {}",
                                    network.ssid, network.signal, network.security
                                )));
                            }
                        }
                    } else {
                        network_menu = network_menu.push(text("Wi-Fi is disabled").size(11));
                    }
                }

                if *loading {
                    network_menu = network_menu.push(text("Loading...").size(11));
                }
                if let Some(message) = error {
                    network_menu = network_menu.push(text(format!("Error: {message}")).size(11));
                }

                right_row = right_row.push(
                    container(network_menu)
                        .padding([2, dynamic_padding])
                        .style(container::rounded_box),
                );
            }
        }
    }

    // Create the status bar layout
    let bar_content = row![horizontal_space(), right_row]
        .spacing(8)
        .align_y(iced::Alignment::Center)
        .width(Length::Fill)
        .height(Length::Shrink);

    // Apply background color from config if available
    let container_builder = container(bar_content)
        .style(container::dark)
        .width(Length::Fill)
        .padding(4);

    if let Some(_bg_color) = &bar.config.window.background_color {
        // TODO: Parse hex color and apply to container
        // For now, we'll use the default iced theme
    }

    container_builder.into()
}

fn should_show_key(mode: KeyDisplayMode, shift_held: bool, last_key: Option<&String>) -> bool {
    if last_key.is_none() {
        return false;
    }

    match mode {
        KeyDisplayMode::Always => true,
        KeyDisplayMode::ShiftHold => shift_held,
    }
}

fn subscribe(bar: &UniliiBar) -> Subscription<Message> {
    use iced::stream;
    let tray_poll_ms = bar.cli.tray_poll_ms;

    let module_subscriptions: Vec<Subscription<Message>> = bar
        .module_receivers
        .iter()
        .cloned()
        .map(|(name, receiver)| {
            let sub_id = name.clone();
            let stream = stream::channel(64, async move |mut output| {
                let mut receiver = receiver.lock().await;
                while let Some(update) = receiver.recv().await {
                    if output
                        .send(Message::ModuleUpdate(name.clone(), update))
                        .await
                        .is_err()
                    {
                        break;
                    }
                }
            });
            Subscription::run_with_id(("module_updates", sub_id), stream)
        })
        .collect();

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

    let window_key_press_subscription = keyboard::on_key_press(map_window_key_press);
    let window_key_release_subscription = keyboard::on_key_release(map_window_key_release);
    let tray_stream = stream::channel(64, async move |mut output| {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        tokio::spawn(async move {
            tray::run_tray_watcher(tx, tray_poll_ms).await;
        });

        while let Some(event) = rx.recv().await {
            if output.send(Message::TrayEvent(event)).await.is_err() {
                break;
            }
        }
    });
    let tray_subscription = Subscription::run_with_id(("tray_updates", tray_poll_ms), tray_stream);
    let tray_animation_subscription =
        iced::time::every(Duration::from_millis(16)).map(|_| Message::TrayAnimateTick);

    let mut subscriptions = module_subscriptions;
    subscriptions.push(keyboard_subscription);
    subscriptions.push(window_key_press_subscription);
    subscriptions.push(window_key_release_subscription);
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
    let _ = tracing_subscriber::fmt()
        .with_max_level(Level::INFO)
        .try_init();
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
    let (modules, module_receivers) = load_modules().await.unwrap_or_else(|e| {
        error!("Failed to load modules: {}", e);
        (HashMap::new(), Vec::new())
    });

    info!("Loaded {} modules", modules.len());
    info!("Loaded {} module receiver streams", module_receivers.len());

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
        let debug_focus_mode = env_flag("UNILII_WINDOW_DEBUG_FOCUS");
        window_settings.platform_specific = window::settings::PlatformSpecific {
            application_id: "com.unilii.bar".to_string(),
            override_redirect: !debug_focus_mode,
        };
        if debug_focus_mode {
            window_settings.decorations = true;
            window_settings.resizable = true;
            window_settings.level = window::Level::Normal;
        }
        info!(
            "linux window settings: application_id=com.unilii.bar, override_redirect={}, debug_focus_mode={}",
            !debug_focus_mode,
            debug_focus_mode
        );
    }

    info!("unilii startup: load finished, launching iced application");

    // Run the iced application with the loaded modules
    iced::application("unilii", update, view)
        .window(window_settings)
        .subscription(subscribe)
        .run_with(move || {
            (
                UniliiBar {
                    modules,
                    config: config.clone(),
                    last_key: None,
                    shift_held: false,
                    key_display_mode: KeyDisplayMode::from_env(),
                    module_receivers,
                    tray_icons: Vec::new(),
                    tray_menu: None,
                    cli,
                },
                Task::none(),
            )
        })
}

#[cfg(test)]
mod tests {
    use super::{should_show_key, KeyDisplayMode};

    #[test]
    fn key_mode_uses_shift_hold_for_unknown_values() {
        std::env::set_var("UNILII_KEY_DISPLAY_MODE", "unknown-mode");
        assert!(matches!(
            KeyDisplayMode::from_env(),
            KeyDisplayMode::ShiftHold
        ));
    }

    #[test]
    fn key_mode_accepts_shift_alias() {
        std::env::set_var("UNILII_KEY_DISPLAY_MODE", "shift");
        assert!(matches!(
            KeyDisplayMode::from_env(),
            KeyDisplayMode::ShiftHold
        ));
    }

    #[test]
    fn key_visibility_in_shift_mode_requires_shift_pressed() {
        let key = "KEY_A (1)".to_string();
        assert!(!should_show_key(
            KeyDisplayMode::ShiftHold,
            false,
            Some(&key)
        ));
        assert!(should_show_key(KeyDisplayMode::ShiftHold, true, Some(&key)));
    }
}
