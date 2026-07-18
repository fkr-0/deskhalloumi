//! Standalone i3 filter-tab popup built on the DeskHalloumi filter-tab model.

#[path = "menus/filter_tab.rs"]
mod filter_tab_view;

use clap::Parser;
use deskhalloumi_core::filter_tab::{
    FilterTabMenuInput, FilterTabMenuOutcome, FilterTabMenuState, WindowMatchContext,
    default_filter_tabs, handle_filter_tab_input,
};
use filter_tab_view::{FilterTabPreview, FilterTabViewMessage, view_filter_tab_menu_with_previews};
use iced::event::{self, Event};
use iced::keyboard::{self, Key, Modifiers, key};
use iced::{Size, Subscription, Task, Theme, window};
use serde_json::Value;
use std::collections::HashMap;
use std::process::Command;

const DEFAULT_APP_ID: &str = "unilii-filter-tab";
const DEFAULT_TITLE: &str = "DeskHalloumi Filter Tab";
const DEFAULT_PREVIEW_COMMAND: &str = "timeout 0.8s import -window {window} png:-";
const DEFAULT_MAX_PREVIEW_IMAGES: usize = 8;

#[derive(Debug, Parser)]
#[command(name = "deskhalloumi-filter-tab")]
#[command(about = "i3 window filter-tab popup with mod-release confirm semantics")]
struct Args {
    /// i3-msg executable.
    #[arg(long, default_value = "i3-msg", value_name = "PATH")]
    i3_msg: String,

    /// Use deterministic mock windows instead of reading i3 tree.
    #[arg(long)]
    mock: bool,

    /// Experimentally capture and show per-window image previews.
    #[arg(long)]
    preview_images: bool,

    /// Shell command used for preview capture. Supports {window}, {x11_window}, and {con_id}.
    #[arg(long, default_value = DEFAULT_PREVIEW_COMMAND, value_name = "CMD")]
    preview_command: String,

    /// Maximum visible rows to capture previews for.
    #[arg(long, default_value_t = DEFAULT_MAX_PREVIEW_IMAGES, value_name = "N")]
    max_preview_images: usize,

    /// Print a starter i3 config snippet and exit.
    #[arg(long, value_name = "EXECUTABLE")]
    print_i3_config: Option<String>,

    /// Modifier used in the printed i3 snippet.
    #[arg(long, default_value = "$mod", value_name = "MOD")]
    i3_modifier: String,

    /// Do not execute i3 commands; keep status text instead.
    #[arg(long)]
    no_exec: bool,

    /// Window width in logical pixels.
    #[arg(long, default_value_t = 900, value_name = "PX")]
    width: u32,

    /// Window height in logical pixels.
    #[arg(long, default_value_t = 620, value_name = "PX")]
    height: u32,

    /// Stable legacy Linux application id used by existing i3 rules.
    #[arg(long, default_value = DEFAULT_APP_ID, value_name = "ID")]
    app_id: String,

    /// Window title.
    #[arg(long, default_value = DEFAULT_TITLE, value_name = "TITLE")]
    title: String,
}

#[derive(Debug, Clone)]
struct FilterTabOptions {
    i3_msg: String,
    mock: bool,
    execute: bool,
    preview_images: bool,
    preview_command: String,
    max_preview_images: usize,
    width: u32,
    height: u32,
    app_id: String,
    title: String,
}

#[derive(Debug, Clone)]
struct FilterTabApp {
    options: FilterTabOptions,
    state: FilterTabMenuState,
    status: String,
    previews: HashMap<u32, FilterTabPreview>,
}

#[derive(Debug, Clone)]
enum Message {
    Loaded(Result<Vec<WindowMatchContext>, String>),
    View(FilterTabViewMessage),
    KeyPressed(Key, Modifiers),
    KeyReleased(Key),
    PreviewLoaded(u32, Result<Vec<u8>, String>),
    OperationDone(Result<String, String>),
}

fn main() -> iced::Result {
    let _menu_instance =
        match deskhalloumi_core::menu_process::MenuProcessManager::register_current_process(
            "filter-tab",
        ) {
            Ok(guard) => guard,
            Err(error) => {
                eprintln!("{error}");
                return Ok(());
            }
        };

    let args = Args::parse();
    let _ = tracing_subscriber::fmt().try_init();

    if let Some(executable) = args.print_i3_config.as_deref() {
        println!("{}", i3_config_snippet(executable, &args.i3_modifier));
        return Ok(());
    }

    run(FilterTabOptions {
        i3_msg: args.i3_msg,
        mock: args.mock,
        execute: !args.no_exec,
        preview_images: args.preview_images,
        preview_command: args.preview_command,
        max_preview_images: args.max_preview_images,
        width: args.width,
        height: args.height,
        app_id: args.app_id,
        title: args.title,
    })
}

fn run(options: FilterTabOptions) -> iced::Result {
    let settings = popup_window_settings(&options);
    let title = options.title.clone();
    iced::application(
        move || FilterTabApp::new(options.clone()),
        FilterTabApp::update,
        FilterTabApp::view,
    )
    .title(move |_state: &FilterTabApp| title.clone())
    .window(settings)
    .subscription(FilterTabApp::subscription)
    .theme(Theme::Dark)
    .run()
}

fn popup_window_settings(options: &FilterTabOptions) -> window::Settings {
    let mut settings = window::Settings {
        size: Size::new(options.width as f32, options.height as f32),
        min_size: Some(Size::new(520.0, 320.0)),
        position: window::Position::Centered,
        resizable: false,
        minimizable: false,
        decorations: false,
        transparent: true,
        blur: true,
        level: window::Level::AlwaysOnTop,
        exit_on_close_request: false,
        ..Default::default()
    };

    #[cfg(target_os = "linux")]
    {
        settings.platform_specific.application_id = options.app_id.clone();
        settings.platform_specific.override_redirect = false;
    }

    settings
}

impl FilterTabApp {
    fn new(options: FilterTabOptions) -> (Self, Task<Message>) {
        let mut state = FilterTabMenuState::new(default_filter_tabs(), Vec::new());
        handle_filter_tab_input(&mut state, FilterTabMenuInput::Activate);
        (
            Self {
                options: options.clone(),
                state,
                status: "Loading i3 windows…".to_string(),
                previews: HashMap::new(),
            },
            Task::perform(load_windows(options), Message::Loaded),
        )
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Loaded(Ok(windows)) => {
                self.state.windows = windows;
                self.state.clamp_selection();
                self.status = if self.options.preview_images {
                    format!(
                        "{} windows loaded · capturing previews…",
                        self.state.windows.len()
                    )
                } else {
                    format!("{} windows loaded", self.state.windows.len())
                };
                self.request_preview_loads()
            }
            Message::Loaded(Err(error)) => {
                self.status = error;
                Task::none()
            }
            Message::View(FilterTabViewMessage::Input(input)) => self.apply_model_input(input),
            Message::View(FilterTabViewMessage::QueryChanged(query)) => {
                self.state.query = query;
                self.state.reset_selection();
                self.request_preview_loads()
            }
            Message::View(FilterTabViewMessage::ConfirmWindow(id)) => {
                self.apply_outcome(FilterTabMenuOutcome::ConfirmWindow(id))
            }
            Message::KeyPressed(key, modifiers) => self.handle_key_press(key, modifiers),
            Message::KeyReleased(key) => {
                if is_modifier_release_key(&key) {
                    self.apply_model_input(FilterTabMenuInput::ModifierRelease)
                } else {
                    Task::none()
                }
            }
            Message::PreviewLoaded(con_id, Ok(bytes)) => {
                self.previews.insert(con_id, FilterTabPreview::Ready(bytes));
                Task::none()
            }
            Message::PreviewLoaded(con_id, Err(error)) => {
                self.previews.insert(con_id, FilterTabPreview::Error(error));
                Task::none()
            }
            Message::OperationDone(Ok(message)) => {
                self.status = message;
                window::latest().and_then(window::close)
            }
            Message::OperationDone(Err(error)) => {
                self.status = error;
                Task::none()
            }
        }
    }

    fn view(&self) -> iced::Element<'_, Message> {
        let view =
            view_filter_tab_menu_with_previews(&self.state, &self.previews).map(Message::View);
        iced::widget::container(
            iced::widget::column![view, iced::widget::text(&self.status).size(12)]
                .spacing(6)
                .padding(8),
        )
        .width(iced::Length::Fill)
        .height(iced::Length::Fill)
        .into()
    }

    fn subscription(&self) -> Subscription<Message> {
        event::listen_with(|event, _status, _id| match event {
            Event::Keyboard(keyboard::Event::KeyPressed { key, modifiers, .. }) => {
                Some(Message::KeyPressed(key, modifiers))
            }
            Event::Keyboard(keyboard::Event::KeyReleased { key, .. }) => {
                Some(Message::KeyReleased(key))
            }
            Event::Window(window::Event::CloseRequested) => Some(Message::View(
                FilterTabViewMessage::Input(FilterTabMenuInput::Escape),
            )),
            _ => None,
        })
    }

    fn handle_key_press(&mut self, key: Key, modifiers: Modifiers) -> Task<Message> {
        if self.state.quick_select_armed
            && let Some(label) = quick_select_label_from_key(&key, modifiers)
        {
            return self.apply_model_input(FilterTabMenuInput::QuickSelect(label));
        }

        match key {
            Key::Named(key::Named::Escape) => self.apply_model_input(FilterTabMenuInput::Escape),
            Key::Named(key::Named::Enter) => self.apply_model_input(FilterTabMenuInput::Confirm),
            Key::Named(key::Named::ArrowDown) => {
                self.apply_model_input(FilterTabMenuInput::NextWindow)
            }
            Key::Named(key::Named::ArrowUp) => {
                self.apply_model_input(FilterTabMenuInput::PreviousWindow)
            }
            Key::Named(key::Named::PageDown) => {
                self.apply_model_input(FilterTabMenuInput::PageDown)
            }
            Key::Named(key::Named::PageUp) => self.apply_model_input(FilterTabMenuInput::PageUp),
            Key::Named(key::Named::Home) => self.apply_model_input(FilterTabMenuInput::FirstWindow),
            Key::Named(key::Named::End) => self.apply_model_input(FilterTabMenuInput::LastWindow),
            Key::Named(key::Named::Tab) if modifiers.shift() => {
                self.apply_model_input(FilterTabMenuInput::PreviousTab)
            }
            Key::Named(key::Named::Tab) => self.apply_model_input(FilterTabMenuInput::NextTab),
            Key::Named(key::Named::Backspace) if modifiers.control() => self.clear_query(),
            Key::Character(value) if modifiers.control() && value.eq_ignore_ascii_case("u") => {
                self.clear_query()
            }
            Key::Character(value) if modifiers.control() && value.eq_ignore_ascii_case("r") => {
                self.refresh_windows()
            }
            Key::Character(value) if modifiers.control() && value.eq_ignore_ascii_case("j") => {
                self.apply_model_input(FilterTabMenuInput::NextWindow)
            }
            Key::Character(value) if modifiers.control() && value.eq_ignore_ascii_case("k") => {
                self.apply_model_input(FilterTabMenuInput::PreviousWindow)
            }
            Key::Character(value)
                if vim_list_key_enabled(&self.state.query, modifiers) && value == "g" =>
            {
                self.apply_model_input(FilterTabMenuInput::FirstWindow)
            }
            Key::Character(value)
                if vim_list_key_enabled(&self.state.query, modifiers) && value == "G" =>
            {
                self.apply_model_input(FilterTabMenuInput::LastWindow)
            }
            Key::Character(value)
                if vim_list_key_enabled(&self.state.query, modifiers) && value == "j" =>
            {
                self.apply_model_input(FilterTabMenuInput::NextWindow)
            }
            Key::Character(value)
                if vim_list_key_enabled(&self.state.query, modifiers) && value == "k" =>
            {
                self.apply_model_input(FilterTabMenuInput::PreviousWindow)
            }
            Key::Character(value) if modifiers.control() && value.eq_ignore_ascii_case("n") => {
                self.apply_model_input(FilterTabMenuInput::NextWindow)
            }
            Key::Character(value) if modifiers.control() && value.eq_ignore_ascii_case("p") => {
                self.apply_model_input(FilterTabMenuInput::PreviousWindow)
            }
            Key::Character(value) if modifiers.control() && value.eq_ignore_ascii_case("q") => {
                self.apply_model_input(FilterTabMenuInput::ToggleQuickSelect)
            }
            Key::Character(value) if modifiers.control() && value.eq_ignore_ascii_case("m") => {
                self.apply_model_input(FilterTabMenuInput::Confirm)
            }
            Key::Character(value) if modifiers.alt() && value.chars().count() == 1 => {
                let ch = value.chars().next().unwrap_or_default();
                self.apply_model_input(FilterTabMenuInput::SelectTab(ch))
            }
            _ => Task::none(),
        }
    }

    fn apply_model_input(&mut self, input: FilterTabMenuInput) -> Task<Message> {
        let outcome = handle_filter_tab_input(&mut self.state, input);
        match outcome {
            FilterTabMenuOutcome::None => self.request_preview_loads(),
            other => self.apply_outcome(other),
        }
    }

    fn apply_outcome(&mut self, outcome: FilterTabMenuOutcome) -> Task<Message> {
        match outcome {
            FilterTabMenuOutcome::None => Task::none(),
            FilterTabMenuOutcome::Cancelled => window::latest().and_then(window::close),
            FilterTabMenuOutcome::ConfirmWindow(id) => {
                let command = format!("[con_id={id}] focus");
                self.run_i3_command(command, format!("Focused window #{id}"))
            }
            FilterTabMenuOutcome::ExecuteContextCommand(command) => {
                self.run_i3_command(command, "Executed window action".to_string())
            }
            FilterTabMenuOutcome::NeedsRefresh => {
                Task::perform(load_windows(self.options.clone()), Message::Loaded)
            }
        }
    }

    fn clear_query(&mut self) -> Task<Message> {
        self.state.query.clear();
        self.state.clear_quick_select();
        self.state.reset_selection();
        self.request_preview_loads()
    }

    fn refresh_windows(&mut self) -> Task<Message> {
        self.previews.clear();
        self.status = "Refreshing i3 windows…".to_string();
        Task::perform(load_windows(self.options.clone()), Message::Loaded)
    }

    fn request_preview_loads(&mut self) -> Task<Message> {
        if !self.options.preview_images {
            return Task::none();
        }

        let visible_indices = self.state.visible_indices();
        let mut tasks = Vec::new();
        for window_index in visible_indices
            .into_iter()
            .take(self.options.max_preview_images)
        {
            let Some(window) = self.state.windows.get(window_index) else {
                continue;
            };
            if self.previews.contains_key(&window.id) {
                continue;
            }
            let Some(native_window_id) = window.native_window_id else {
                self.previews
                    .insert(window.id, FilterTabPreview::Unavailable);
                continue;
            };
            self.previews.insert(window.id, FilterTabPreview::Loading);
            let request = PreviewRequest {
                con_id: window.id,
                native_window_id,
                command_template: self.options.preview_command.clone(),
            };
            tasks.push(Task::perform(
                capture_preview_image(request),
                |(con_id, result)| Message::PreviewLoaded(con_id, result),
            ));
        }
        Task::batch(tasks)
    }

    fn run_i3_command(&mut self, command: String, success_message: String) -> Task<Message> {
        if !self.options.execute {
            self.status = format!("dry-run: i3-msg {command}");
            return Task::none();
        }

        self.status = format!("i3-msg {command}");
        Task::perform(
            execute_i3_command(self.options.i3_msg.clone(), command, success_message),
            Message::OperationDone,
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PreviewRequest {
    con_id: u32,
    native_window_id: u64,
    command_template: String,
}

async fn capture_preview_image(request: PreviewRequest) -> (u32, Result<Vec<u8>, String>) {
    let command = render_preview_command(
        &request.command_template,
        request.con_id,
        request.native_window_id,
    );
    let result = Command::new("sh")
        .arg("-c")
        .arg(&command)
        .output()
        .map_err(|error| format!("failed to run preview command: {error}"))
        .and_then(|output| {
            if output.status.success() && !output.stdout.is_empty() {
                Ok(output.stdout)
            } else if output.status.success() {
                Err("preview command returned no image bytes".to_string())
            } else {
                Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
            }
        });
    (request.con_id, result)
}

fn render_preview_command(template: &str, con_id: u32, native_window_id: u64) -> String {
    template
        .replace("{con_id}", &con_id.to_string())
        .replace("{window}", &native_window_id.to_string())
        .replace("{x11_window}", &native_window_id.to_string())
}

async fn load_windows(options: FilterTabOptions) -> Result<Vec<WindowMatchContext>, String> {
    if options.mock {
        return Ok(mock_windows());
    }

    let output = Command::new(&options.i3_msg)
        .args(["-t", "get_tree"])
        .output()
        .map_err(|error| format!("failed to run {}: {}", options.i3_msg, error))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    let value = serde_json::from_slice::<Value>(&output.stdout)
        .map_err(|error| format!("failed to parse i3 tree JSON: {error}"))?;
    Ok(collect_windows_from_i3_tree(&value))
}

async fn execute_i3_command(
    i3_msg: String,
    command: String,
    success_message: String,
) -> Result<String, String> {
    let output = Command::new(&i3_msg)
        .arg(&command)
        .output()
        .map_err(|error| format!("failed to run {i3_msg}: {error}"))?;
    if output.status.success() {
        Ok(success_message)
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
    }
}

fn collect_windows_from_i3_tree(value: &Value) -> Vec<WindowMatchContext> {
    let mut windows = Vec::new();
    collect_windows_rec(value, None, &mut windows);
    windows
}

fn collect_windows_rec(
    value: &Value,
    workspace: Option<String>,
    windows: &mut Vec<WindowMatchContext>,
) {
    let node_type = value.get("type").and_then(Value::as_str);
    let workspace = if node_type == Some("workspace") {
        value
            .get("name")
            .and_then(Value::as_str)
            .map(str::to_string)
            .or(workspace)
    } else {
        workspace
    };

    let has_window = value.get("window").is_some_and(|window| !window.is_null());
    if has_window && let Some(id) = value.get("id").and_then(Value::as_u64) {
        let title = value
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("Untitled")
            .to_string();
        let class_name = value
            .pointer("/window_properties/class")
            .and_then(Value::as_str)
            .map(str::to_string)
            .or_else(|| {
                value
                    .pointer("/window_properties/instance")
                    .and_then(Value::as_str)
                    .map(str::to_string)
            });
        let urgent = value
            .get("urgent")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        if !is_filter_tab_popup_window(&title, class_name.as_deref(), value) {
            windows.push(WindowMatchContext {
                id: id as u32,
                title,
                class_name,
                workspace: workspace.clone(),
                urgent,
                native_window_id: value.get("window").and_then(Value::as_u64),
            });
        }
    }

    for key in ["nodes", "floating_nodes"] {
        if let Some(children) = value.get(key).and_then(Value::as_array) {
            for child in children {
                collect_windows_rec(child, workspace.clone(), windows);
            }
        }
    }
}

fn is_filter_tab_popup_window(title: &str, class_name: Option<&str>, node: &Value) -> bool {
    let app_id = node
        .get("app_id")
        .and_then(Value::as_str)
        .or_else(|| {
            node.pointer("/window_properties/class")
                .and_then(Value::as_str)
        })
        .or_else(|| {
            node.pointer("/window_properties/instance")
                .and_then(Value::as_str)
        });

    title == DEFAULT_TITLE
        || app_id.is_some_and(|value| value.eq_ignore_ascii_case(DEFAULT_APP_ID))
        || class_name.is_some_and(|value| value.eq_ignore_ascii_case(DEFAULT_APP_ID))
}

fn mock_windows() -> Vec<WindowMatchContext> {
    vec![
        WindowMatchContext::new(1001, "work - tmux", Some("Alacritty"))
            .with_workspace("1")
            .with_native_window_id(2001),
        WindowMatchContext::new(1002, "ChatGPT - Firefox", Some("firefox"))
            .with_workspace("2")
            .with_native_window_id(2002),
        WindowMatchContext::new(1003, "unilii - Emacs", Some("Emacs"))
            .with_workspace("3")
            .with_native_window_id(2003),
        WindowMatchContext::new(1004, "urgent mail - firefox", Some("firefox"))
            .with_workspace("4")
            .with_native_window_id(2004)
            .urgent(true),
    ]
}

fn is_modifier_release_key(key: &Key) -> bool {
    matches!(
        key,
        Key::Named(key::Named::Alt)
            | Key::Named(key::Named::AltGraph)
            | Key::Named(key::Named::Control)
            | Key::Named(key::Named::Shift)
            | Key::Named(key::Named::Meta)
            | Key::Named(key::Named::Super)
    )
}

fn quick_select_label_from_key(key: &Key, modifiers: Modifiers) -> Option<char> {
    if modifiers.control() || modifiers.alt() || modifiers.logo() {
        return None;
    }

    match key {
        Key::Character(value) if value.chars().count() == 1 => value.chars().next(),
        _ => None,
    }
}

fn vim_list_key_enabled(query: &str, modifiers: Modifiers) -> bool {
    query.is_empty() && !modifiers.control() && !modifiers.alt() && !modifiers.logo()
}

fn i3_config_snippet(executable: &str, modifier: &str) -> String {
    format!(
        r#"for_window [app_id="unilii-filter-tab"] floating enable, border pixel 2, move position center, sticky enable
bindsym {modifier}+u exec --no-startup-id {executable}"#,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced::keyboard::key;

    fn mock_options(preview_images: bool) -> FilterTabOptions {
        FilterTabOptions {
            // unilii-audit: allow-live-session-command-reference -- mock=true and execute=false prevent command execution.
            i3_msg: "i3-msg".into(),
            mock: true,
            execute: false,
            preview_images,
            preview_command: DEFAULT_PREVIEW_COMMAND.into(),
            max_preview_images: DEFAULT_MAX_PREVIEW_IMAGES,
            width: 900,
            height: 620,
            app_id: DEFAULT_APP_ID.into(),
            title: DEFAULT_TITLE.into(),
        }
    }

    #[test]
    fn parses_i3_tree_windows_with_workspace_and_class() {
        let tree = serde_json::json!({
            "type": "root",
            "nodes": [{
                "type": "workspace",
                "name": "2:web",
                "nodes": [{
                    "id": 42,
                    "type": "con",
                    "name": "ChatGPT - Firefox",
                    "window": 123,
                    "urgent": true,
                    "window_properties": { "class": "firefox", "instance": "Navigator" },
                    "nodes": [],
                    "floating_nodes": []
                }],
                "floating_nodes": []
            }],
            "floating_nodes": []
        });
        let windows = collect_windows_from_i3_tree(&tree);
        assert_eq!(windows.len(), 1);
        assert_eq!(windows[0].id, 42);
        assert_eq!(windows[0].workspace.as_deref(), Some("2:web"));
        assert_eq!(windows[0].class_name.as_deref(), Some("firefox"));
        assert_eq!(windows[0].native_window_id, Some(123));
        assert!(windows[0].urgent);
    }

    #[test]
    fn modifier_release_key_detection_covers_super_alt_ctrl_shift() {
        assert!(is_modifier_release_key(&Key::Named(key::Named::Super)));
        assert!(is_modifier_release_key(&Key::Named(key::Named::Alt)));
        assert!(is_modifier_release_key(&Key::Named(key::Named::Control)));
        assert!(is_modifier_release_key(&Key::Named(key::Named::Shift)));
        assert!(!is_modifier_release_key(&Key::Character("u".into())));
    }

    #[test]
    fn i3_snippet_uses_stable_app_id_and_release_binding() {
        let snippet = i3_config_snippet("/usr/local/bin/unilii-filter-tab", "$mod");
        assert!(snippet.contains(r#"app_id="unilii-filter-tab""#));
        assert!(snippet.contains("bindsym $mod+u"));
        assert!(snippet.contains("/usr/local/bin/unilii-filter-tab"));
    }

    #[test]
    fn popup_window_settings_are_launcher_like() {
        let options = mock_options(false);
        let settings = popup_window_settings(&options);
        assert_eq!(settings.level, window::Level::AlwaysOnTop);
        assert!(!settings.decorations);
        assert!(!settings.resizable);
        assert!(!settings.exit_on_close_request);
    }

    #[test]
    fn quick_select_label_from_key_accepts_plain_home_row_only() {
        assert_eq!(
            quick_select_label_from_key(&Key::Character("a".into()), Modifiers::empty()),
            Some('a')
        );
        assert_eq!(
            quick_select_label_from_key(&Key::Character("a".into()), Modifiers::CTRL),
            None
        );
        assert_eq!(
            quick_select_label_from_key(&Key::Named(key::Named::Enter), Modifiers::empty()),
            None
        );
    }

    #[test]
    fn ctrl_q_toggles_quick_select_in_app_state() {
        let options = mock_options(false);
        let (mut app, _) = FilterTabApp::new(options);
        assert!(!app.state.quick_select_armed);
        let _ = app.handle_key_press(Key::Character("q".into()), Modifiers::CTRL);
        assert!(app.state.quick_select_armed);
    }

    #[test]
    fn i3_tree_parser_skips_filter_tab_popup_itself() {
        let tree = serde_json::json!({
            "type": "root",
            "nodes": [{
                "type": "workspace",
                "name": "1",
                "nodes": [
                    {
                        "id": 1,
                        "type": "con",
                        "name": "unilii Filter Tab",
                        "window": 11,
                        "window_properties": { "class": "unilii-filter-tab" },
                        "nodes": [],
                        "floating_nodes": []
                    },
                    {
                        "id": 2,
                        "type": "con",
                        "name": "work - tmux",
                        "window": 22,
                        "window_properties": { "class": "Alacritty" },
                        "nodes": [],
                        "floating_nodes": []
                    }
                ],
                "floating_nodes": []
            }],
            "floating_nodes": []
        });
        let windows = collect_windows_from_i3_tree(&tree);
        assert_eq!(windows.len(), 1);
        assert_eq!(windows[0].id, 2);
    }

    #[test]
    fn preview_command_template_renders_con_and_native_ids() {
        assert_eq!(
            render_preview_command("shot {window} {x11_window} {con_id}", 10, 4242),
            "shot 4242 4242 10"
        );
    }

    #[test]
    fn request_preview_loads_marks_unavailable_without_native_window_id() {
        let options = mock_options(true);
        let (mut app, _) = FilterTabApp::new(options);
        app.state.windows = vec![WindowMatchContext::new(7, "no native", Some("Class"))];
        let _ = app.request_preview_loads();
        assert_eq!(app.previews.get(&7), Some(&FilterTabPreview::Unavailable));
    }

    #[test]
    fn bare_jk_move_selection_only_when_query_is_empty() {
        let options = mock_options(false);
        let (mut app, _) = FilterTabApp::new(options);
        app.state.windows = mock_windows();
        assert_eq!(app.state.selected_visible_index, 0);
        let _ = app.handle_key_press(Key::Character("j".into()), Modifiers::empty());
        assert_eq!(app.state.selected_visible_index, 1);
        let _ = app.handle_key_press(Key::Character("k".into()), Modifiers::empty());
        assert_eq!(app.state.selected_visible_index, 0);
        let _ = app.handle_key_press(Key::Character("k".into()), Modifiers::empty());
        assert_eq!(app.state.selected_visible_index, 3);
        let _ = app.handle_key_press(Key::Character("j".into()), Modifiers::empty());
        assert_eq!(app.state.selected_visible_index, 0);

        app.state.query = "fire".into();
        let _ = app.handle_key_press(Key::Character("j".into()), Modifiers::empty());
        assert_eq!(app.state.selected_visible_index, 0);
        let _ = app.handle_key_press(Key::Character("j".into()), Modifiers::CTRL);
        assert_eq!(app.state.selected_visible_index, 1);
    }

    #[test]
    fn escape_in_app_disarms_then_clears_then_closes() {
        let options = mock_options(false);
        let (mut app, _) = FilterTabApp::new(options);
        app.state.query = "fire".into();
        app.state.quick_select_armed = true;

        let _ = app.handle_key_press(Key::Named(key::Named::Escape), Modifiers::empty());
        assert!(app.state.visible);
        assert!(!app.state.quick_select_armed);
        assert_eq!(app.state.query, "fire");

        let _ = app.handle_key_press(Key::Named(key::Named::Escape), Modifiers::empty());
        assert!(app.state.visible);
        assert!(app.state.query.is_empty());
    }

    #[test]
    fn vim_list_key_guard_protects_query_typing() {
        assert!(vim_list_key_enabled("", Modifiers::empty()));
        assert!(!vim_list_key_enabled("fire", Modifiers::empty()));
        assert!(vim_list_key_enabled("", Modifiers::SHIFT));
        assert!(!vim_list_key_enabled("", Modifiers::CTRL));
    }

    #[test]
    fn bare_g_and_shift_g_jump_when_query_is_empty() {
        let options = mock_options(false);
        let (mut app, _) = FilterTabApp::new(options);
        app.state.windows = mock_windows();
        app.state.selected_visible_index = 2;
        let _ = app.handle_key_press(Key::Character("g".into()), Modifiers::empty());
        assert_eq!(app.state.selected_visible_index, 0);
        let _ = app.handle_key_press(Key::Character("G".into()), Modifiers::SHIFT);
        assert_eq!(app.state.selected_visible_index, 3);

        app.state.query = "g".into();
        let _ = app.handle_key_press(Key::Character("g".into()), Modifiers::empty());
        assert_eq!(app.state.selected_visible_index, 3);
    }

    #[test]
    fn ctrl_u_clears_query_and_disarms_quick_select() {
        let options = mock_options(false);
        let (mut app, _) = FilterTabApp::new(options);
        app.state.query = "fire".into();
        app.state.quick_select_armed = true;
        app.state.selected_visible_index = 2;
        let _ = app.handle_key_press(Key::Character("u".into()), Modifiers::CTRL);
        assert!(app.state.query.is_empty());
        assert!(!app.state.quick_select_armed);
        assert_eq!(app.state.selected_visible_index, 0);
    }

    #[test]
    fn ctrl_r_refreshes_status_and_clears_preview_cache() {
        let options = mock_options(true);
        let (mut app, _) = FilterTabApp::new(options);
        app.previews.insert(1, FilterTabPreview::Unavailable);
        let _ = app.handle_key_press(Key::Character("r".into()), Modifiers::CTRL);
        assert!(app.previews.is_empty());
        assert_eq!(app.status, "Refreshing i3 windows…");
    }
}
