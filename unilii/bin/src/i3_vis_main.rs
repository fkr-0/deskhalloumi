//! Standalone i3 workspace tree visualization mod-release menu.

use clap::Parser;
use deskhalloumi_core::i3_vis::{
    I3VisInput, I3VisNode, I3VisNodeKind, I3VisOutcome, I3VisRow, I3VisState, handle_i3_vis_input,
    kind_icon, kind_label,
};
use iced::event::{self, Event};
use iced::keyboard::{self, Key, Modifiers, key};
use iced::widget::{button, column, container, row, scrollable, text};
use iced::{Alignment, Element, Length, Size, Subscription, Task, Theme, window};
use serde_json::Value;
use std::process::Command;

const DEFAULT_APP_ID: &str = "unilii-i3-vis";
const DEFAULT_TITLE: &str = "DeskHalloumi i3 Visualizer";

#[derive(Debug, Parser)]
#[command(name = "deskhalloumi-i3-vis")]
#[command(about = "i3 workspace tree HUD with mod-release confirm semantics")]
struct Args {
    /// i3-msg executable.
    #[arg(long, default_value = "i3-msg", value_name = "PATH")]
    i3_msg: String,

    /// Use deterministic mock i3 tree instead of reading live i3 state.
    #[arg(long)]
    mock: bool,

    /// Print deterministic text output instead of launching the GUI.
    #[arg(long)]
    dump_text: bool,

    /// Comma-separated deterministic headless actions, e.g. j,release.
    #[arg(long, value_name = "ACTIONS")]
    e2e_actions: Option<String>,

    /// Print a starter i3 config snippet and exit.
    #[arg(long, value_name = "EXECUTABLE")]
    print_i3_config: Option<String>,

    /// Modifier used in the printed i3 snippet.
    #[arg(long, default_value = "$mod", value_name = "MOD")]
    i3_modifier: String,

    /// Do not execute i3 focus command; keep status text instead.
    #[arg(long)]
    no_exec: bool,

    /// Window width in logical pixels.
    #[arg(long, default_value_t = 760, value_name = "PX")]
    width: u32,

    /// Window height in logical pixels.
    #[arg(long, default_value_t = 540, value_name = "PX")]
    height: u32,

    /// Stable legacy Linux application id used by existing i3 rules.
    #[arg(long, default_value = DEFAULT_APP_ID, value_name = "ID")]
    app_id: String,

    /// Window title.
    #[arg(long, default_value = DEFAULT_TITLE, value_name = "TITLE")]
    title: String,
}

#[derive(Debug, Clone)]
struct I3VisOptions {
    i3_msg: String,
    mock: bool,
    execute: bool,
    width: u32,
    height: u32,
    app_id: String,
    title: String,
}

#[derive(Debug, Clone)]
struct I3VisApp {
    options: I3VisOptions,
    state: I3VisState,
    status: String,
    startup_restore_window_id: Option<u64>,
}

#[derive(Debug, Clone)]
enum Message {
    Loaded(Result<Option<I3VisNode>, String>),
    KeyPressed(Key, Modifiers),
    KeyReleased(Key),
    ConfirmRow(u64),
    Refresh,
    I3CommandDone(Result<String, String>),
    OperationDone(Result<String, String>),
}

fn main() -> iced::Result {
    let args = Args::parse();
    let _ = tracing_subscriber::fmt().try_init();

    if let Some(executable) = args.print_i3_config.as_deref() {
        println!("{}", i3_config_snippet(executable, &args.i3_modifier));
        return Ok(());
    }

    let options = I3VisOptions {
        i3_msg: args.i3_msg,
        mock: args.mock,
        execute: !args.no_exec,
        width: args.width,
        height: args.height,
        app_id: args.app_id,
        title: args.title,
    };

    if args.dump_text {
        return match run_headless(&options, args.e2e_actions.as_deref()) {
            Ok(output) => {
                println!("{output}");
                Ok(())
            }
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(1);
            }
        };
    }

    // Singleton ownership protects only the interactive popup. Headless and
    // config-print operations are pure CLI paths and must remain concurrent.
    let _menu_instance =
        match deskhalloumi_core::menu_process::MenuProcessManager::register_current_process(
            "i3-vis",
        ) {
            Ok(guard) => guard,
            Err(error) => {
                eprintln!("{error}");
                return Ok(());
            }
        };

    run(options)
}

fn run_headless(options: &I3VisOptions, actions: Option<&str>) -> Result<String, String> {
    let root = load_workspace_tree_sync(options)?;
    let mut state = I3VisState::new(root);
    handle_i3_vis_input(&mut state, I3VisInput::Activate);
    let startup_restore_window_id = state.startup_focused_window_id;
    let mut status = status_for_state(&state);

    if let Some(actions) = actions {
        for action in parse_e2e_actions(actions) {
            match action.as_str() {
                "next" => {
                    handle_i3_vis_input(&mut state, I3VisInput::Next);
                    status = status_for_state(&state);
                }
                "previous" | "prev" => {
                    handle_i3_vis_input(&mut state, I3VisInput::Previous);
                    status = status_for_state(&state);
                }
                "first" => {
                    handle_i3_vis_input(&mut state, I3VisInput::First);
                    status = status_for_state(&state);
                }
                "G" | "last" => {
                    handle_i3_vis_input(&mut state, I3VisInput::Last);
                    status = status_for_state(&state);
                }
                "h" | "j" | "k" | "l" | "H" | "J" | "K" | "L" => {
                    let command = i3_vim_command_from_action(&action)
                        .ok_or_else(|| format!("unknown e2e action: {action}"))?;
                    status = run_i3_passthrough_command_headless(options, &mut state, command)?;
                }
                "focus-left" | "focus-down" | "focus-up" | "focus-right" => {
                    let command = action.replace('-', " ");
                    status = run_i3_passthrough_command_headless(options, &mut state, command)?;
                }
                "move-left" | "move-down" | "move-up" | "move-right" => {
                    let command = action.replace('-', " ");
                    status = run_i3_passthrough_command_headless(options, &mut state, command)?;
                }
                "enter" | "confirm" | "release" | "modrelease" => {
                    let outcome = handle_i3_vis_input(&mut state, I3VisInput::ModifierRelease);
                    if let I3VisOutcome::ConfirmWindow(id) = outcome {
                        let command = startup_restore_command(id);
                        status = if options.execute {
                            execute_i3_command_sync(
                                &options.i3_msg,
                                &command,
                                format!("Focused #{id}"),
                            )?
                        } else {
                            format!("dry-run: i3-msg {command}")
                        };
                    }
                }
                "esc" | "escape" | "cancel" => {
                    handle_i3_vis_input(&mut state, I3VisInput::Escape);
                    status = restore_startup_focus_headless(options, startup_restore_window_id)?;
                }
                "r" | "refresh" => {
                    let root = load_workspace_tree_sync(options)?;
                    state.set_root(root);
                    state.activate();
                    status = status_for_state(&state);
                }
                unknown => return Err(format!("unknown e2e action: {unknown}")),
            }
        }
    }

    Ok(render_i3_vis_text(&state, &status))
}

fn run_i3_passthrough_command_headless(
    options: &I3VisOptions,
    state: &mut I3VisState,
    command: String,
) -> Result<String, String> {
    let status = if options.execute {
        execute_i3_command_sync(&options.i3_msg, &command, format!("Executed i3 {command}"))?
    } else {
        format!("dry-run: i3-msg {command}")
    };
    if options.execute {
        let root = load_workspace_tree_sync(options)?;
        state.set_root(root);
        state.activate();
    }
    Ok(status)
}

fn restore_startup_focus_headless(
    options: &I3VisOptions,
    startup_restore_window_id: Option<u64>,
) -> Result<String, String> {
    let Some(id) = startup_restore_window_id else {
        return Ok("Cancelled".to_string());
    };
    let command = startup_restore_command(id);
    if options.execute {
        execute_i3_command_sync(
            &options.i3_msg,
            &command,
            format!("Restored startup focus #{id}"),
        )
    } else {
        Ok(format!("dry-run: i3-msg {command}"))
    }
}

fn startup_restore_command(id: u64) -> String {
    format!("[con_id={id}] focus")
}

fn parse_e2e_actions(actions: &str) -> Vec<String> {
    actions
        .split(',')
        .map(str::trim)
        .filter(|action| !action.is_empty())
        .map(str::to_string)
        .collect()
}

fn status_for_state(state: &I3VisState) -> String {
    let count = state.selectable_window_ids().len();
    match state.selected_label() {
        Some(label) => format!("{count} windows · selected {label}"),
        None => format!("{count} windows"),
    }
}

fn render_i3_vis_text(state: &I3VisState, status: &str) -> String {
    let mut lines = vec!["i3-vis".to_string(), status.to_string()];
    let selected = state
        .selected_label()
        .unwrap_or_else(|| "no selectable window".to_string());
    lines.push(format!("Selected: {selected}"));

    let rows = state.rows();
    if rows.is_empty() {
        lines.push("No i3 workspace tree loaded".to_string());
    } else {
        for row in rows {
            lines.push(render_i3_vis_text_row(&row));
        }
    }
    lines.join(
        "
",
    )
}

fn render_i3_vis_text_row(row_data: &I3VisRow) -> String {
    let connector = if row_data.depth == 0 {
        ""
    } else if row_data.is_last {
        "└─"
    } else {
        "├─"
    };
    let indent = "   ".repeat(row_data.depth);
    let marker = if row_data.selected { "▶" } else { " " };
    let focus = if row_data.startup_focused { "★" } else { " " };
    let structural = if row_data.selectable {
        ""
    } else {
        kind_label(row_data.kind)
    };
    if structural.is_empty() {
        format!(
            "{marker} {indent}{connector} {} {} {}",
            kind_icon(row_data.kind),
            focus,
            row_data.label
        )
    } else {
        format!(
            "{marker} {indent}{connector} {} {} {} · {}",
            kind_icon(row_data.kind),
            focus,
            row_data.label,
            structural
        )
    }
}

fn run(options: I3VisOptions) -> iced::Result {
    let settings = popup_window_settings(&options);
    let title = options.title.clone();
    iced::application(
        move || I3VisApp::new(options.clone()),
        I3VisApp::update,
        I3VisApp::view,
    )
    .title(move |_state: &I3VisApp| title.clone())
    .window(settings)
    .subscription(I3VisApp::subscription)
    .theme(Theme::Dark)
    .run()
}

fn popup_window_settings(options: &I3VisOptions) -> window::Settings {
    let mut settings = window::Settings {
        size: Size::new(options.width as f32, options.height as f32),
        min_size: Some(Size::new(420.0, 260.0)),
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

impl I3VisApp {
    fn new(options: I3VisOptions) -> (Self, Task<Message>) {
        let mut state = I3VisState::new(None);
        handle_i3_vis_input(&mut state, I3VisInput::Activate);
        (
            Self {
                options: options.clone(),
                state,
                status: "Loading i3 tree…".to_string(),
                startup_restore_window_id: None,
            },
            Task::perform(load_workspace_tree(options), Message::Loaded),
        )
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Loaded(Ok(root)) => {
                self.state.set_root(root);
                self.state.activate();
                if self.startup_restore_window_id.is_none() {
                    self.startup_restore_window_id = self.state.startup_focused_window_id;
                }
                self.status = status_for_state(&self.state);
                Task::none()
            }
            Message::Loaded(Err(error)) => {
                self.status = error;
                Task::none()
            }
            Message::KeyPressed(key, modifiers) => self.handle_key_press(key, modifiers),
            Message::KeyReleased(key) => {
                if is_modifier_release_key(&key) {
                    self.apply_input(I3VisInput::ModifierRelease)
                } else {
                    Task::none()
                }
            }
            Message::ConfirmRow(id) => self.apply_outcome(I3VisOutcome::ConfirmWindow(id)),
            Message::Refresh => self.refresh_tree(),
            Message::I3CommandDone(Ok(message)) => {
                self.status = message;
                self.refresh_tree()
            }
            Message::I3CommandDone(Err(error)) => {
                self.status = error;
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

    fn view(&self) -> Element<'_, Message> {
        let rows = self.state.rows();
        let mut graph = column![].spacing(4);
        if rows.is_empty() {
            graph = graph.push(
                container(
                    column![
                        text("No i3 workspace tree loaded").size(18),
                        text("Press r to refresh or Esc to close.").size(12),
                    ]
                    .spacing(4),
                )
                .padding(12)
                .width(Length::Fill),
            );
        } else {
            for row in rows {
                graph = graph.push(view_graph_row(row));
            }
        }

        let selected = self
            .state
            .selected_label()
            .unwrap_or_else(|| "no selectable window".to_string());
        container(
            column![
                row![
                    column![
                        text("i3-vis").size(26),
                        text("workspace tree · release modifier to confirm selected window")
                            .size(12),
                    ]
                    .spacing(2)
                    .width(Length::Fill),
                    button("×").padding([6, 10]).on_press(Message::KeyPressed(
                        Key::Named(key::Named::Escape),
                        Modifiers::empty()
                    )),
                ]
                .align_y(Alignment::Center),
                text(format!("Selected: {selected}")).size(13),
                scrollable(graph).height(Length::Fill),
                row![
                    text(&self.status).size(12).width(Length::Fill),
                    button("refresh").padding([5, 8]).on_press(Message::Refresh),
                ]
                .spacing(8)
                .align_y(Alignment::Center),
                text("h/j/k/l i3 focus · Shift+H/J/K/L i3 move · arrows/Tab select · Shift+Tab previous · Enter/release confirms")
                    .size(11),
            ]
            .spacing(10)
            .padding(14),
        )
        .width(Length::Fill)
        .height(Length::Fill)
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
            Event::Window(window::Event::CloseRequested) => Some(Message::KeyPressed(
                Key::Named(key::Named::Escape),
                Modifiers::empty(),
            )),
            _ => None,
        })
    }

    fn handle_key_press(&mut self, key: Key, modifiers: Modifiers) -> Task<Message> {
        match key {
            Key::Named(key::Named::Escape) => self.apply_input(I3VisInput::Escape),
            Key::Named(key::Named::Enter) => self.apply_input(I3VisInput::Confirm),
            Key::Named(key::Named::Tab) if modifiers.shift() => {
                self.apply_input(I3VisInput::Previous)
            }
            Key::Named(key::Named::ArrowDown) | Key::Named(key::Named::Tab) => {
                self.apply_input(I3VisInput::Next)
            }
            Key::Named(key::Named::ArrowUp) => self.apply_input(I3VisInput::Previous),
            Key::Named(key::Named::Home) => self.apply_input(I3VisInput::First),
            Key::Named(key::Named::End) => self.apply_input(I3VisInput::Last),
            Key::Character(value) if modifiers.control() && value.eq_ignore_ascii_case("r") => {
                self.refresh_tree()
            }
            Key::Character(value) if value == "r" && no_command_modifier(modifiers) => {
                self.refresh_tree()
            }
            Key::Character(value) if i3_vim_command_from_key(&value, modifiers).is_some() => {
                let command = i3_vim_command_from_key(&value, modifiers).unwrap();
                self.run_i3_passthrough_command(command)
            }
            Key::Character(value) if value == "g" && no_command_modifier(modifiers) => {
                self.apply_input(I3VisInput::First)
            }
            Key::Character(value) if value == "G" && no_command_modifier(modifiers) => {
                self.apply_input(I3VisInput::Last)
            }
            _ => Task::none(),
        }
    }

    fn apply_input(&mut self, input: I3VisInput) -> Task<Message> {
        let outcome = handle_i3_vis_input(&mut self.state, input);
        self.apply_outcome(outcome)
    }

    fn apply_outcome(&mut self, outcome: I3VisOutcome) -> Task<Message> {
        match outcome {
            I3VisOutcome::None => Task::none(),
            I3VisOutcome::Cancelled => self.restore_startup_focus_and_close(),
            I3VisOutcome::ConfirmWindow(id) => self.focus_window(id),
        }
    }

    fn restore_startup_focus_and_close(&mut self) -> Task<Message> {
        let Some(id) = self.startup_restore_window_id else {
            self.status = "Cancelled".to_string();
            return window::latest().and_then(window::close);
        };
        let command = startup_restore_command(id);
        if !self.options.execute {
            self.status = format!("dry-run: i3-msg {command}");
            return window::latest().and_then(window::close);
        }
        self.status = format!("i3-msg {command}");
        Task::perform(
            execute_i3_command(
                self.options.i3_msg.clone(),
                command,
                format!("Restored startup focus #{id}"),
            ),
            Message::OperationDone,
        )
    }

    fn focus_window(&mut self, id: u64) -> Task<Message> {
        let command = startup_restore_command(id);
        if !self.options.execute {
            self.status = format!("dry-run: i3-msg {command}");
            return Task::none();
        }
        self.status = format!("i3-msg {command}");
        Task::perform(
            execute_i3_command(
                self.options.i3_msg.clone(),
                command,
                format!("Focused #{id}"),
            ),
            Message::OperationDone,
        )
    }

    fn run_i3_passthrough_command(&mut self, command: String) -> Task<Message> {
        if !self.options.execute {
            self.status = format!("dry-run: i3-msg {command}");
            return Task::none();
        }
        self.status = format!("i3-msg {command}");
        Task::perform(
            execute_i3_command(
                self.options.i3_msg.clone(),
                command.clone(),
                format!("Executed i3 {command}"),
            ),
            Message::I3CommandDone,
        )
    }

    fn refresh_tree(&mut self) -> Task<Message> {
        self.status = "Refreshing i3 tree…".to_string();
        Task::perform(load_workspace_tree(self.options.clone()), Message::Loaded)
    }
}

fn view_graph_row(row_data: I3VisRow) -> Element<'static, Message> {
    let connector = if row_data.depth == 0 {
        "".to_string()
    } else if row_data.is_last {
        "└─".to_string()
    } else {
        "├─".to_string()
    };
    let indent = "   ".repeat(row_data.depth);
    let marker = if row_data.selected { "▶" } else { " " };
    let focus = if row_data.startup_focused { "★" } else { " " };
    let structural = if row_data.selectable {
        ""
    } else {
        kind_label(row_data.kind)
    };
    let label = if structural.is_empty() {
        format!(
            "{indent}{connector} {} {} {}",
            kind_icon(row_data.kind),
            focus,
            row_data.label
        )
    } else {
        format!(
            "{indent}{connector} {} {} {} · {}",
            kind_icon(row_data.kind),
            focus,
            row_data.label,
            structural
        )
    };
    let content = row![
        text(marker).size(15),
        text(label).size(14).width(Length::Fill)
    ]
    .spacing(8)
    .align_y(Alignment::Center);
    let cell = container(content).padding([5, 8]).width(Length::Fill);
    if row_data.selectable {
        button(cell)
            .style(button::text)
            .on_press(Message::ConfirmRow(row_data.id))
            .width(Length::Fill)
            .into()
    } else {
        cell.into()
    }
}

async fn load_workspace_tree(options: I3VisOptions) -> Result<Option<I3VisNode>, String> {
    load_workspace_tree_sync(&options)
}

fn load_workspace_tree_sync(options: &I3VisOptions) -> Result<Option<I3VisNode>, String> {
    if options.mock {
        return Ok(Some(mock_tree()));
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
    Ok(workspace_tree_from_i3_tree(&value))
}

async fn execute_i3_command(
    i3_msg: String,
    command: String,
    success_message: String,
) -> Result<String, String> {
    execute_i3_command_sync(&i3_msg, &command, success_message)
}

fn execute_i3_command_sync(
    i3_msg: &str,
    command: &str,
    success_message: String,
) -> Result<String, String> {
    let output = Command::new(i3_msg)
        .arg(command)
        .output()
        .map_err(|error| format!("failed to run {i3_msg}: {error}"))?;
    if output.status.success() {
        Ok(success_message)
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
    }
}

fn workspace_tree_from_i3_tree(value: &Value) -> Option<I3VisNode> {
    let focused_workspace_id = focused_workspace_id(value);
    if let Some(id) = focused_workspace_id {
        find_workspace_node(value, id).and_then(build_workspace_graph)
    } else {
        first_workspace_node(value).and_then(build_workspace_graph)
    }
}

fn focused_workspace_id(value: &Value) -> Option<u64> {
    if value.get("type").and_then(Value::as_str) == Some("workspace") && subtree_has_focused(value)
    {
        return value.get("id").and_then(Value::as_u64);
    }
    child_arrays(value)
        .into_iter()
        .flatten()
        .find_map(focused_workspace_id)
}

fn subtree_has_focused(value: &Value) -> bool {
    value
        .get("focused")
        .and_then(Value::as_bool)
        .unwrap_or(false)
        || child_arrays(value)
            .into_iter()
            .flatten()
            .any(subtree_has_focused)
}

fn find_workspace_node(value: &Value, id: u64) -> Option<&Value> {
    if value.get("type").and_then(Value::as_str) == Some("workspace")
        && value.get("id").and_then(Value::as_u64) == Some(id)
    {
        return Some(value);
    }
    child_arrays(value)
        .into_iter()
        .flatten()
        .find_map(|child| find_workspace_node(child, id))
}

fn first_workspace_node(value: &Value) -> Option<&Value> {
    if value.get("type").and_then(Value::as_str) == Some("workspace") {
        return Some(value);
    }
    child_arrays(value)
        .into_iter()
        .flatten()
        .find_map(first_workspace_node)
}

fn build_workspace_graph(value: &Value) -> Option<I3VisNode> {
    let id = value.get("id").and_then(Value::as_u64)?;
    let name = value
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or("workspace")
        .to_string();
    let children = graph_children(value);
    Some(
        I3VisNode::new(id, format!("workspace {name}"), I3VisNodeKind::Workspace)
            .with_children(children),
    )
}

fn build_graph_node(value: &Value) -> Option<I3VisNode> {
    let id = value.get("id").and_then(Value::as_u64)?;
    if is_i3_vis_popup_node(value) {
        return None;
    }
    if value.get("window").is_some_and(|window| !window.is_null()) {
        let label = window_label(value);
        let focused = value
            .get("focused")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        return Some(I3VisNode::window(id, label).with_startup_focused(focused));
    }

    let children = graph_children(value);
    if children.is_empty() {
        return None;
    }
    let kind = layout_kind(value);
    let label = layout_label(value, children.len());
    Some(I3VisNode::new(id, label, kind).with_children(children))
}

fn graph_children(value: &Value) -> Vec<I3VisNode> {
    let mut children = Vec::new();
    for key in ["nodes", "floating_nodes"] {
        if let Some(values) = value.get(key).and_then(Value::as_array) {
            for child in values {
                if let Some(node) = build_graph_node(child) {
                    children.push(node);
                }
            }
        }
    }
    children
}

fn child_arrays(value: &Value) -> Vec<&Vec<Value>> {
    ["nodes", "floating_nodes"]
        .iter()
        .filter_map(|key| value.get(*key).and_then(Value::as_array))
        .collect()
}

fn layout_kind(value: &Value) -> I3VisNodeKind {
    match value
        .get("layout")
        .and_then(Value::as_str)
        .unwrap_or_default()
    {
        "splitv" => I3VisNodeKind::SplitVertical,
        "splith" => I3VisNodeKind::SplitHorizontal,
        "tabbed" => I3VisNodeKind::Tabbed,
        "stacked" => I3VisNodeKind::Stacked,
        _ if value.get("type").and_then(Value::as_str) == Some("floating_con") => {
            I3VisNodeKind::Floating
        }
        _ => I3VisNodeKind::Container,
    }
}

fn layout_label(value: &Value, child_count: usize) -> String {
    let child_count = child_count.max(1);
    match layout_kind(value) {
        I3VisNodeKind::SplitVertical => format!("{child_count} vertical"),
        I3VisNodeKind::SplitHorizontal => format!("{child_count} horizontal"),
        I3VisNodeKind::Tabbed => format!("{child_count} tabbed"),
        I3VisNodeKind::Stacked => format!("{child_count} stacked"),
        I3VisNodeKind::Floating => format!("{child_count} floating"),
        _ => value
            .get("name")
            .and_then(Value::as_str)
            .filter(|name| !name.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| format!("{child_count} container")),
    }
}

fn window_label(value: &Value) -> String {
    let title = value
        .get("name")
        .and_then(Value::as_str)
        .filter(|name| !name.is_empty())
        .unwrap_or("Untitled");
    let class = value
        .pointer("/window_properties/class")
        .and_then(Value::as_str)
        .or_else(|| {
            value
                .pointer("/window_properties/instance")
                .and_then(Value::as_str)
        });
    class
        .map(|class| format!("{class} · {title}"))
        .unwrap_or_else(|| title.to_string())
}

fn is_i3_vis_popup_node(value: &Value) -> bool {
    let title = value
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let app_id = value
        .get("app_id")
        .and_then(Value::as_str)
        .or_else(|| {
            value
                .pointer("/window_properties/class")
                .and_then(Value::as_str)
        })
        .or_else(|| {
            value
                .pointer("/window_properties/instance")
                .and_then(Value::as_str)
        });
    title == DEFAULT_TITLE || app_id.is_some_and(|value| value.eq_ignore_ascii_case(DEFAULT_APP_ID))
}

fn mock_tree() -> I3VisNode {
    I3VisNode::new(2, "workspace 2", I3VisNodeKind::Workspace).with_children(vec![
        I3VisNode::new(20, "2 vertical", I3VisNodeKind::SplitVertical).with_children(vec![
            I3VisNode::window(21, "Emacs · unilii/src/i3_vis.rs"),
            I3VisNode::new(22, "2 horizontal", I3VisNodeKind::SplitHorizontal).with_children(vec![
                I3VisNode::window(23, "Firefox · ChatGPT").with_startup_focused(true),
                I3VisNode::window(24, "XTerm · logs"),
            ]),
        ]),
    ])
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

fn i3_vim_command_from_key(value: &str, modifiers: Modifiers) -> Option<String> {
    if modifiers.control() || modifiers.alt() || modifiers.logo() || value.chars().count() != 1 {
        return None;
    }
    let ch = value.chars().next()?;
    let direction = i3_direction_for_vim_char(ch)?;
    let verb = if modifiers.shift() || ch.is_ascii_uppercase() {
        "move"
    } else {
        "focus"
    };
    Some(format!("{verb} {direction}"))
}

fn i3_vim_command_from_action(action: &str) -> Option<String> {
    let ch = action.chars().next()?;
    if action.chars().count() != 1 {
        return None;
    }
    let direction = i3_direction_for_vim_char(ch)?;
    let verb = if ch.is_ascii_uppercase() {
        "move"
    } else {
        "focus"
    };
    Some(format!("{verb} {direction}"))
}

fn i3_direction_for_vim_char(ch: char) -> Option<&'static str> {
    match ch.to_ascii_lowercase() {
        'h' => Some("left"),
        'j' => Some("down"),
        'k' => Some("up"),
        'l' => Some("right"),
        _ => None,
    }
}

fn no_command_modifier(modifiers: Modifiers) -> bool {
    !modifiers.control() && !modifiers.alt() && !modifiers.logo()
}

fn i3_config_snippet(executable: &str, modifier: &str) -> String {
    format!(
        r#"for_window [app_id="unilii-i3-vis"] floating enable, border pixel 0, move position center, sticky enable
for_window [app_id="unilii-i3-vis"] opacity 0.90
bindsym {modifier}+i exec --no-startup-id {executable}"#,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn options() -> I3VisOptions {
        I3VisOptions {
            i3_msg: "i3-msg".into(),
            mock: true,
            execute: false,
            width: 760,
            height: 540,
            app_id: DEFAULT_APP_ID.into(),
            title: DEFAULT_TITLE.into(),
        }
    }

    #[test]
    fn parses_workspace_tree_containing_focused_window() {
        let tree = serde_json::json!({
            "type": "root",
            "nodes": [{
                "id": 1,
                "type": "workspace",
                "name": "1",
                "layout": "splith",
                "nodes": [{
                    "id": 11,
                    "type": "con",
                    "layout": "splith",
                    "name": "ignored",
                    "nodes": [{
                        "id": 12,
                        "type": "con",
                        "name": "other",
                        "window": 999,
                        "focused": false,
                        "window_properties": {"class": "Other"},
                        "nodes": [],
                        "floating_nodes": []
                    }],
                    "floating_nodes": []
                }],
                "floating_nodes": []
            }, {
                "id": 2,
                "type": "workspace",
                "name": "2",
                "layout": "splitv",
                "nodes": [{
                    "id": 20,
                    "type": "con",
                    "layout": "splitv",
                    "nodes": [{
                        "id": 21,
                        "type": "con",
                        "name": "main.rs - Emacs",
                        "window": 101,
                        "focused": false,
                        "window_properties": {"class": "Emacs"},
                        "nodes": [],
                        "floating_nodes": []
                    }, {
                        "id": 22,
                        "type": "con",
                        "layout": "splith",
                        "nodes": [{
                            "id": 23,
                            "type": "con",
                            "name": "ChatGPT",
                            "window": 102,
                            "focused": true,
                            "window_properties": {"class": "firefox"},
                            "nodes": [],
                            "floating_nodes": []
                        }, {
                            "id": 24,
                            "type": "con",
                            "name": "shell",
                            "window": 103,
                            "focused": false,
                            "window_properties": {"class": "XTerm"},
                            "nodes": [],
                            "floating_nodes": []
                        }],
                        "floating_nodes": []
                    }],
                    "floating_nodes": []
                }],
                "floating_nodes": []
            }],
            "floating_nodes": []
        });

        let root = workspace_tree_from_i3_tree(&tree).expect("workspace graph");
        assert_eq!(root.label, "workspace 2");
        let mut state = I3VisState::new(Some(root));
        state.activate();
        assert_eq!(state.startup_focused_window_id, Some(23));
        assert_eq!(state.selected_window_id, Some(23));
        assert_eq!(state.selectable_window_ids(), vec![21, 23, 24]);
        assert!(state.rows().iter().any(|row| row.label == "2 horizontal"));
    }

    #[test]
    fn skips_i3_vis_popup_itself() {
        let tree = serde_json::json!({
            "type": "root",
            "nodes": [{
                "id": 2,
                "type": "workspace",
                "name": "2",
                "layout": "splith",
                "nodes": [{
                    "id": 88,
                    "type": "con",
                    "name": "i3-vis",
                    "window": 200,
                    "focused": false,
                    "window_properties": {"class": "unilii-i3-vis"},
                    "nodes": [],
                    "floating_nodes": []
                }, {
                    "id": 23,
                    "type": "con",
                    "name": "ChatGPT",
                    "window": 102,
                    "focused": true,
                    "window_properties": {"class": "firefox"},
                    "nodes": [],
                    "floating_nodes": []
                }],
                "floating_nodes": []
            }],
            "floating_nodes": []
        });
        let root = workspace_tree_from_i3_tree(&tree).expect("workspace graph");
        let state = I3VisState::new(Some(root));
        assert_eq!(state.selectable_window_ids(), vec![23]);
    }

    #[test]
    fn modifier_release_key_detection_covers_common_modifiers() {
        assert!(is_modifier_release_key(&Key::Named(key::Named::Super)));
        assert!(is_modifier_release_key(&Key::Named(key::Named::Alt)));
        assert!(is_modifier_release_key(&Key::Named(key::Named::Control)));
        assert!(is_modifier_release_key(&Key::Named(key::Named::Shift)));
        assert!(!is_modifier_release_key(&Key::Character("i".into())));
    }

    #[test]
    fn i3_snippet_uses_stable_app_id_and_binding() {
        let snippet = i3_config_snippet("/usr/local/bin/unilii-i3-vis", "$mod");
        assert!(snippet.contains(r#"app_id="unilii-i3-vis""#));
        assert!(snippet.contains("bindsym $mod+i"));
        assert!(snippet.contains("/usr/local/bin/unilii-i3-vis"));
    }

    #[test]
    fn popup_window_settings_are_hud_like() {
        let settings = popup_window_settings(&options());
        assert_eq!(settings.level, window::Level::AlwaysOnTop);
        assert!(!settings.decorations);
        assert!(!settings.resizable);
        assert!(!settings.exit_on_close_request);
        assert!(settings.transparent);
    }

    #[test]
    fn key_navigation_and_modrelease_confirm_current_selection() {
        let (mut app, _) = I3VisApp::new(options());
        app.state.set_root(Some(mock_tree()));
        app.state.activate();
        assert_eq!(app.state.selected_window_id, Some(23));
        let _ = app.handle_key_press(Key::Named(key::Named::ArrowDown), Modifiers::empty());
        assert_eq!(app.state.selected_window_id, Some(24));
        let _ = app.handle_key_press(Key::Named(key::Named::ArrowUp), Modifiers::empty());
        assert_eq!(app.state.selected_window_id, Some(23));
        let _ = app.handle_key_press(Key::Named(key::Named::Tab), Modifiers::empty());
        assert_eq!(app.state.selected_window_id, Some(24));
        let _ = app.handle_key_press(Key::Named(key::Named::Tab), Modifiers::SHIFT);
        assert_eq!(app.state.selected_window_id, Some(23));
        let _ = app.handle_key_press(Key::Character("g".into()), Modifiers::empty());
        assert_eq!(app.state.selected_window_id, Some(21));
    }

    #[test]
    fn vim_hjkl_keys_map_to_i3_focus_and_move_commands() {
        assert_eq!(
            i3_vim_command_from_key("h", Modifiers::empty()),
            Some("focus left".to_string())
        );
        assert_eq!(
            i3_vim_command_from_key("j", Modifiers::empty()),
            Some("focus down".to_string())
        );
        assert_eq!(
            i3_vim_command_from_key("K", Modifiers::empty()),
            Some("move up".to_string())
        );
        assert_eq!(
            i3_vim_command_from_key("l", Modifiers::SHIFT),
            Some("move right".to_string())
        );
        assert_eq!(i3_vim_command_from_key("j", Modifiers::CTRL), None);
    }

    #[test]
    fn render_text_contains_tree_focus_and_selection_markers() {
        let mut state = I3VisState::new(Some(mock_tree()));
        state.activate();
        let text = render_i3_vis_text(&state, &status_for_state(&state));
        assert!(text.contains("i3-vis"));
        assert!(text.contains("3 windows · selected Firefox · ChatGPT"));
        assert!(text.contains("◇   workspace 2 · workspace"));
        assert!(text.contains("↕   2 vertical · vertical"));
        assert!(text.contains("↔   2 horizontal · horizontal"));
        assert!(text.contains("▶"));
        assert!(text.contains("★ Firefox · ChatGPT"));
    }

    #[test]
    fn e2e_action_parser_ignores_empty_segments() {
        assert_eq!(parse_e2e_actions("j,, release, "), vec!["j", "release"]);
    }

    #[test]
    fn escape_restore_command_targets_startup_focused_window() {
        assert_eq!(startup_restore_command(23), "[con_id=23] focus");
        let mut state = I3VisState::new(Some(mock_tree()));
        handle_i3_vis_input(&mut state, I3VisInput::Activate);
        let startup_restore_window_id = state.startup_focused_window_id;
        handle_i3_vis_input(&mut state, I3VisInput::Next);
        assert_eq!(state.selected_window_id, Some(24));
        assert_eq!(startup_restore_window_id, Some(23));
    }
}
