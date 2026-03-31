mod cli;
use cli::{Cli, Commands, RunOptions, verbose_to_level};
use iced::futures::{SinkExt, StreamExt};
use iced::keyboard::{self, key, Key, Modifiers};
use iced::widget::{button, column, container, horizontal_space, row, text};
use iced::{window, Element, Length, Subscription, Task};
use std::collections::HashMap;
use std::time::Duration;
use tracing::{error, info, Level};
use unilii_core::{config::load_config, keys::KeybindingDaemon, ModuleUpdate};

mod module_loader;
mod tray;
use module_loader::{load_modules, LoadedModule, ModuleReceiver};

struct UniliiBar {
    modules: HashMap<String, LoadedModule>,
    config: unilii_core::config::Config,
    shift_held: bool,
    module_receivers: Vec<(String, ModuleReceiver)>,
    tray_icons: Vec<tray::TrayIcon>,
    tray_menu: Option<TrayMenuState>,
    run_options: RunOptions,
}

#[derive(Debug, Clone)]
struct TrayMenuState {
    icon_key: String,
    progress: f32,
    target: f32,
    content: TrayMenuContent,
    selected_index: Option<usize>,
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
    TraySpawnCommand(String, String),
    TraySpawnCommandDone(String, Result<(), String>),
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
                // Menu keyboard navigation
                if let Some(menu) = bar.tray_menu.as_mut() {
                    match key.as_str() {
                        "Named(Escape)" => {
                            menu.target = 0.0;
                            return Task::none();
                        }
                        "Named(ArrowDown)" | "Named(Tab)" => {
                            if let TrayMenuContent::Generic { items } = &menu.content {
                                let count = items.len();
                                if count > 0 {
                                    menu.selected_index = Some(match menu.selected_index {
                                        None => 0,
                                        Some(i) => (i + 1) % count,
                                    });
                                }
                            }
                            return Task::none();
                        }
                        "Named(ArrowUp)" => {
                            if let TrayMenuContent::Generic { items } = &menu.content {
                                let count = items.len();
                                if count > 0 {
                                    menu.selected_index = Some(match menu.selected_index {
                                        None => count.saturating_sub(1),
                                        Some(i) => if i == 0 { count - 1 } else { i - 1 },
                                    });
                                }
                            }
                            return Task::none();
                        }
                        "Named(Enter)" => {
                            if let TrayMenuContent::Generic { items } = &menu.content {
                                if let Some(idx) = menu.selected_index {
                                    if let Some(item) = items.get(idx) {
                                        let action = item.action.clone();
                                        let icon_key = menu.icon_key.clone();
                                        menu.target = 0.0;
                                        return Task::done(Message::TrayMenuTriggered(icon_key, action));
                                    }
                                }
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
                        selected_index: None,
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
                        selected_index: Some(0),
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
        Message::TraySpawnCommand(icon_key, command) => {
            if let Some(menu) = bar.tray_menu.as_mut() {
                if menu.icon_key != icon_key {
                    return Task::none();
                }
                if let TrayMenuContent::Network { loading, error, .. } = &mut menu.content {
                    *loading = true;
                    *error = None;
                }
            }

            return Task::perform(tray::spawn_command(command), move |result| {
                Message::TraySpawnCommandDone(icon_key.clone(), result)
            });
        }
        Message::TraySpawnCommandDone(icon_key, result) => {
            if let Some(menu) = bar.tray_menu.as_mut() {
                if menu.icon_key != icon_key {
                    return Task::none();
                }
                if let TrayMenuContent::Network { loading, error, .. } = &mut menu.content {
                    *loading = false;
                    if let Err(message) = result {
                        *error = Some(message);
                    }
                }
            }
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
            let is_active = bar.tray_menu.as_ref().map(|m| m.icon_key == icon.key).unwrap_or(false);
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

    // Inline drop menu (animated)
    if let Some(menu) = &bar.tray_menu {
        let opacity = menu.progress.clamp(0.0, 1.0);
        let menu_widget: Element<'_, Message> = match &menu.content {
            TrayMenuContent::Generic { items } => {
                let visible = tray::visible_menu_items(items.len(), menu.progress);
                let menu_row = items.iter().enumerate().take(visible).fold(
                    row!().spacing(2).align_y(iced::Alignment::Center),
                    |acc, (i, item)| {
                        let is_sel = menu.selected_index == Some(i);
                        let btn = button(text(item.label.clone()).size(12))
                            .padding([2, 8])
                            .on_press(Message::TrayMenuTriggered(
                                menu.icon_key.clone(),
                                item.action.clone(),
                            ));
                        let btn = if is_sel { btn.style(button::primary) } else { btn.style(button::text) };
                        acc.push(btn)
                    },
                );
                container(menu_row)
                    .padding([0, 4])
                    .style(container::rounded_box)
                    .into()
            }
            TrayMenuContent::Network { data, loading, error } => {
                let wifi_label = if data.as_ref().map(|d| d.enabled).unwrap_or(false) {
                    "Disable Wi-Fi"
                } else {
                    "Enable Wi-Fi"
                };
                let mut col = column![
                    button(text(wifi_label).size(12))
                        .padding([2, 8])
                        .style(button::text)
                        .on_press(Message::TrayNetworkToggle(menu.icon_key.clone())),
                    button(text("Refresh").size(12))
                        .padding([2, 8])
                        .style(button::text)
                        .on_press(Message::TrayNetworkRefresh(menu.icon_key.clone())),
                    button(text("Settings").size(12))
                        .padding([2, 8])
                        .style(button::text)
                        .on_press(Message::TraySpawnCommand(
                            menu.icon_key.clone(),
                            "nm-connection-editor".to_string(),
                        )),
                ]
                .spacing(1);

                if let Some(snapshot) = data {
                    if snapshot.enabled && !snapshot.networks.is_empty() {
                        col = col.push(text("─────").size(10));
                        for network in snapshot.networks.iter().take(6) {
                            let connected_marker = if snapshot.state == "connected"
                                && snapshot.interface == network.ssid
                            {
                                " ●"
                            } else {
                                ""
                            };
                            col = col.push(
                                button(
                                    text(format!("{}{} {}%", network.ssid, connected_marker, network.signal))
                                        .size(11),
                                )
                                .padding([1, 8])
                                .style(button::text)
                                .on_press(Message::TraySpawnCommand(
                                    menu.icon_key.clone(),
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
        };
        let _ = opacity; // used for future fade; menu already animates via visible_menu_items
        right_widgets.push(menu_widget);
    }

    let right_row = row(right_widgets)
        .spacing(6)
        .align_y(iced::Alignment::Center);

    let bar_content = row![horizontal_space(), right_row]
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

    // Run the iced application with the loaded modules
    iced::application("unilii", update, view)
        .window(window_settings)
        .subscription(subscribe)
        .run_with(move || {
            (
                UniliiBar {
                    modules,
                    config: config.clone(),
                    shift_held: false,
                    module_receivers,
                    tray_icons: Vec::new(),
                    tray_menu: None,
                    run_options,
                },
                Task::none(),
            )
        })
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
