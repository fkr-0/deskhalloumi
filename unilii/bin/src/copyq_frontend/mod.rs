//! Fast CopyQ clipboard history frontend.
//!
//! The design intentionally mirrors the performance property of a good shell
//! launcher: history is fetched with one `copyq eval` call, filtered in memory,
//! and activation uses at most the small set of commands needed to select or
//! merge the chosen entries.

use iced::event::{self, Event};
use iced::keyboard::{self, Key, Modifiers, key};
use iced::widget::{button, column, container, image, row, scrollable, text, text_input};
use iced::{Alignment, Element, Length, Size, Subscription, Task, Theme, window};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, ffi::OsString, fmt, time::Duration};

use deskhalloumi_core::runtime::{ActionCommand, ActionRunner};

const DEFAULT_MAX_PREVIEW_CHARS: usize = 220;
const DEFAULT_MAX_VISIBLE_ROWS: usize = 160;
const DEFAULT_COPYQ_BIN: &str = "copyq";
const DEFAULT_WINDOW_WIDTH: u32 = 880;
const DEFAULT_WINDOW_HEIGHT: u32 = 640;
const I3_WINDOW_WIDTH: u32 = 900;
const I3_WINDOW_HEIGHT: u32 = 620;
const DEFAULT_WINDOW_TITLE: &str = "DeskHalloumi CopyQ";
const DEFAULT_APPLICATION_ID: &str = "unilii-copyq";
const ANIMATION_STEP: f32 = 0.12;
const THUMBNAIL_SIZE: f32 = 76.0;
#[allow(dead_code)]
const ELLIPSIS: &str = "…";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CopyqFrontendOptions {
    pub copyq_bin: String,
    pub max_preview_chars: usize,
    pub max_visible_rows: usize,
    pub paste_on_activate: bool,
    pub window_width: u32,
    pub window_height: u32,
    pub i3_shortcut_mode: bool,
    pub close_on_unfocus: bool,
    pub window_title: String,
    pub application_id: String,
}

fn visible_row_window(
    total: usize,
    selected: usize,
    max_visible_rows: usize,
) -> std::ops::Range<usize> {
    if total == 0 || max_visible_rows == 0 {
        return 0..0;
    }

    let visible_count = total.min(max_visible_rows);
    let selected = selected.min(total - 1);
    let preferred_start = selected.saturating_sub(visible_count / 2);
    let start = preferred_start.min(total - visible_count);
    start..start + visible_count
}

impl Default for CopyqFrontendOptions {
    fn default() -> Self {
        Self {
            copyq_bin: DEFAULT_COPYQ_BIN.to_string(),
            max_preview_chars: DEFAULT_MAX_PREVIEW_CHARS,
            max_visible_rows: DEFAULT_MAX_VISIBLE_ROWS,
            paste_on_activate: true,
            window_width: DEFAULT_WINDOW_WIDTH,
            window_height: DEFAULT_WINDOW_HEIGHT,
            i3_shortcut_mode: false,
            close_on_unfocus: false,
            window_title: DEFAULT_WINDOW_TITLE.to_string(),
            application_id: DEFAULT_APPLICATION_ID.to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClipboardItem {
    pub index: usize,
    pub mime_type: String,
    #[serde(default)]
    pub mime_types: Vec<String>,
    pub preview: String,
    pub is_text: bool,
    #[serde(default)]
    pub image_mime_type: Option<String>,
    #[serde(default)]
    pub chars: usize,
    #[serde(default)]
    pub lines: usize,
}

impl ClipboardItem {
    pub fn kind_label(&self) -> &'static str {
        if self.is_image() {
            "image"
        } else if self.is_text {
            if self.mime_type == "text/html" {
                "html"
            } else {
                "text"
            }
        } else if self.mime_type.starts_with("application/") {
            "data"
        } else {
            "mime"
        }
    }

    pub fn is_image(&self) -> bool {
        self.primary_image_mime().is_some()
    }

    pub fn primary_image_mime(&self) -> Option<&str> {
        self.image_mime_type
            .as_deref()
            .or_else(|| preferred_image_mime(&self.mime_types))
            .or_else(|| {
                self.mime_type
                    .starts_with("image/")
                    .then_some(self.mime_type.as_str())
            })
    }

    pub fn compact_meta(&self) -> String {
        let mut parts = vec![format!("#{}", self.index), self.kind_label().to_string()];
        if self.lines > 1 {
            parts.push(format!("{} lines", self.lines));
        }
        if self.chars > 0 {
            parts.push(format!("{} chars", self.chars));
        }
        parts.push(self.mime_type.clone());
        parts.join(" · ")
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CopyqError {
    CommandFailed { program: String, message: String },
    Json(String),
    EmptyHistory,
}

impl fmt::Display for CopyqError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CopyqError::CommandFailed { program, message } => {
                write!(f, "{} failed: {}", program, message)
            }
            CopyqError::Json(message) => write!(f, "failed to parse CopyQ JSON: {}", message),
            CopyqError::EmptyHistory => write!(f, "CopyQ history is empty"),
        }
    }
}

impl std::error::Error for CopyqError {}

#[derive(Debug, Clone)]
pub struct CopyqClient {
    copyq_bin: String,
    max_preview_chars: usize,
}

impl CopyqClient {
    pub fn new(options: &CopyqFrontendOptions) -> Self {
        Self {
            copyq_bin: options.copyq_bin.clone(),
            max_preview_chars: options.max_preview_chars,
        }
    }

    pub async fn list_items(&self) -> Result<Vec<ClipboardItem>, CopyqError> {
        parse_clipboard_items_json(
            &self
                .eval(&build_history_eval_script(self.max_preview_chars))
                .await?,
        )
    }

    pub async fn select_and_paste(&self, index: usize, paste: bool) -> Result<(), CopyqError> {
        self.run_command(["select", &index.to_string()]).await?;
        if paste {
            self.run_command(["paste"]).await?;
        }
        Ok(())
    }

    pub async fn merge_select_and_paste(
        &self,
        indices: &[usize],
        paste: bool,
    ) -> Result<(), CopyqError> {
        if indices.is_empty() {
            return Err(CopyqError::EmptyHistory);
        }
        if indices.len() == 1 {
            return self.select_and_paste(indices[0], paste).await;
        }

        let selected_text = self
            .eval(&build_selected_items_eval_script(indices))
            .await?;
        self.run_command(["add", selected_text.as_str()]).await?;
        self.run_command(["select", "0"]).await?;
        if paste {
            self.run_command(["paste"]).await?;
        }
        Ok(())
    }

    pub async fn read_image_bytes(
        &self,
        index: usize,
        mime_type: &str,
    ) -> Result<Vec<u8>, CopyqError> {
        let outcome = ActionRunner::with_timeout("copyq", "read-image", Duration::from_secs(5))
            .with_output_limit(16 * 1024 * 1024)
            .run_command_bytes(ActionCommand::new(
                &self.copyq_bin,
                vec![
                    OsString::from("read"),
                    OsString::from(mime_type),
                    OsString::from(index.to_string()),
                ],
            ))
            .await;
        if let Err(error) = outcome.result {
            return Err(CopyqError::CommandFailed {
                program: self.copyq_bin.clone(),
                message: command_error(error, &outcome.stderr),
            });
        }
        if outcome.stdout_truncated {
            return Err(CopyqError::CommandFailed {
                program: self.copyq_bin.clone(),
                message: format!(
                    "image exceeded the 16 MiB preview limit ({} bytes)",
                    outcome.stdout_bytes
                ),
            });
        }

        Ok(outcome.stdout)
    }

    async fn eval(&self, script: &str) -> Result<String, CopyqError> {
        let outcome = ActionRunner::with_timeout("copyq", "eval", Duration::from_secs(5))
            .with_output_limit(4 * 1024 * 1024)
            .run_command(ActionCommand::new(
                &self.copyq_bin,
                vec![
                    OsString::from("eval"),
                    OsString::from("--"),
                    OsString::from(script),
                ],
            ))
            .await;
        if let Err(error) = outcome.result {
            return Err(CopyqError::CommandFailed {
                program: self.copyq_bin.clone(),
                message: command_error(error, &outcome.stderr),
            });
        }
        if outcome.stdout_truncated {
            return Err(CopyqError::CommandFailed {
                program: self.copyq_bin.clone(),
                message: format!(
                    "CopyQ output exceeded limit ({} bytes)",
                    outcome.stdout_bytes
                ),
            });
        }

        Ok(outcome.stdout.trim().to_string())
    }

    async fn run_command<const N: usize>(&self, args: [&str; N]) -> Result<(), CopyqError> {
        let outcome = ActionRunner::with_timeout("copyq", "command", Duration::from_secs(5))
            .run_command(ActionCommand::new(
                &self.copyq_bin,
                args.into_iter().map(OsString::from).collect(),
            ))
            .await;
        if let Err(error) = outcome.result {
            return Err(CopyqError::CommandFailed {
                program: self.copyq_bin.clone(),
                message: command_error(error, &outcome.stderr),
            });
        }

        Ok(())
    }
}

fn command_error(error: String, stderr: &str) -> String {
    if stderr.trim().is_empty() {
        error
    } else {
        stderr.trim().to_string()
    }
}

pub fn run(options: CopyqFrontendOptions) -> iced::Result {
    let window_settings = build_window_settings(&options);
    let window_title = options.window_title.clone();

    iced::application(
        move || CopyqApp::new(options.clone()),
        CopyqApp::update,
        CopyqApp::view,
    )
    .title(move |_state: &CopyqApp| window_title.clone())
    .window(window_settings)
    .subscription(CopyqApp::subscription)
    .theme(Theme::Dark)
    .run()
}

pub fn apply_i3_shortcut_defaults(mut options: CopyqFrontendOptions) -> CopyqFrontendOptions {
    options.i3_shortcut_mode = true;
    options.close_on_unfocus = true;
    if options.window_width == DEFAULT_WINDOW_WIDTH {
        options.window_width = I3_WINDOW_WIDTH;
    }
    if options.window_height == DEFAULT_WINDOW_HEIGHT {
        options.window_height = I3_WINDOW_HEIGHT;
    }
    options
}

pub fn build_window_settings(options: &CopyqFrontendOptions) -> window::Settings {
    let mut settings = window::Settings {
        size: Size::new(options.window_width as f32, options.window_height as f32),
        min_size: Some(Size::new(520.0, 320.0)),
        position: window::Position::Centered,
        resizable: !options.i3_shortcut_mode,
        minimizable: !options.i3_shortcut_mode,
        decorations: !options.i3_shortcut_mode,
        transparent: options.i3_shortcut_mode,
        blur: options.i3_shortcut_mode,
        level: if options.i3_shortcut_mode {
            window::Level::AlwaysOnTop
        } else {
            window::Level::Normal
        },
        exit_on_close_request: false,
        ..Default::default()
    };

    #[cfg(target_os = "linux")]
    {
        settings.platform_specific.application_id = options.application_id.clone();
        settings.platform_specific.override_redirect = false;
    }

    settings
}

#[derive(Debug, Clone)]
struct CopyqApp {
    options: CopyqFrontendOptions,
    query: String,
    items: Vec<ClipboardItem>,
    selected_row: usize,
    show_help: bool,
    status: String,
    loading: bool,
    image_previews: HashMap<usize, ImagePreviewState>,
    visibility: VisibilityPhase,
    animation: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum VisibilityPhase {
    Opening,
    Open,
    Closing,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ImagePreviewState {
    Loading,
    Ready(Vec<u8>),
    Error(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ImageLoadRequest {
    pub index: usize,
    pub mime_type: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SelectionMove {
    Up(usize),
    Down(usize),
    First,
    Last,
}

#[derive(Debug, Clone)]
enum Message {
    LoadRequested,
    Loaded(Result<Vec<ClipboardItem>, String>),
    ImageLoaded(usize, Result<Vec<u8>, String>),
    AnimationTick,
    QueryChanged(String),
    RowSelected(usize),
    ActivateSelected,
    PasteIndex(usize),
    MergeVisibleSelection,
    ToggleHelp,
    OperationDone(Result<(), String>),
    CloseRequested,
    WindowUnfocused,
    KeyPressed(Key, Modifiers),
}

impl CopyqApp {
    fn new(options: CopyqFrontendOptions) -> (Self, Task<Message>) {
        (
            Self {
                options: options.clone(),
                query: String::new(),
                items: Vec::new(),
                selected_row: 0,
                show_help: false,
                status: "Loading CopyQ history…".to_string(),
                loading: true,
                image_previews: HashMap::new(),
                visibility: VisibilityPhase::Opening,
                animation: 0.0,
            },
            Task::perform(load_items(options), Message::Loaded),
        )
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::LoadRequested => {
                self.loading = true;
                self.image_previews.clear();
                self.status = "Refreshing CopyQ history…".to_string();
                Task::perform(load_items(self.options.clone()), Message::Loaded)
            }
            Message::Loaded(Ok(items)) => {
                let count = items.len();
                self.items = items;
                self.loading = false;
                self.selected_row = self
                    .selected_row
                    .min(self.filtered_indices().len().saturating_sub(1));
                self.status = format!("{} clipboard entries · single CopyQ eval", count);
                self.schedule_visible_image_loads()
            }
            Message::Loaded(Err(error)) => {
                self.loading = false;
                self.status = error;
                Task::none()
            }
            Message::QueryChanged(query) => {
                self.query = query;
                self.selected_row = 0;
                self.schedule_visible_image_loads()
            }
            Message::RowSelected(row) => {
                self.selected_row = row;
                Task::none()
            }
            Message::ActivateSelected => {
                if let Some(index) = self.selected_item_index() {
                    return self.start_paste(vec![index]);
                }
                Task::none()
            }
            Message::PasteIndex(index) => self.start_paste(vec![index]),
            Message::MergeVisibleSelection => {
                let filtered = self.filtered_indices();
                let visible = visible_row_window(
                    filtered.len(),
                    self.selected_row,
                    self.options.max_visible_rows,
                );
                let indices = filtered[visible]
                    .iter()
                    .copied()
                    .map(|item_index| self.items[item_index].index)
                    .collect::<Vec<_>>();
                self.start_paste(indices)
            }
            Message::ToggleHelp => {
                self.show_help = !self.show_help;
                Task::none()
            }
            Message::OperationDone(Ok(())) => {
                self.status = "Clipboard entry activated".to_string();
                if self.options.paste_on_activate {
                    self.begin_close();
                }
                Task::none()
            }
            Message::OperationDone(Err(error)) => {
                self.status = error;
                Task::none()
            }
            Message::CloseRequested => {
                self.begin_close();
                Task::none()
            }
            Message::WindowUnfocused => {
                if self.options.close_on_unfocus && !self.loading {
                    self.begin_close();
                }
                Task::none()
            }
            Message::ImageLoaded(index, Ok(bytes)) => {
                if !bytes.is_empty() {
                    self.image_previews
                        .insert(index, ImagePreviewState::Ready(bytes));
                } else {
                    self.image_previews.insert(
                        index,
                        ImagePreviewState::Error("CopyQ returned empty image data".to_string()),
                    );
                }
                Task::none()
            }
            Message::ImageLoaded(index, Err(error)) => {
                self.image_previews
                    .insert(index, ImagePreviewState::Error(error));
                Task::none()
            }
            Message::AnimationTick => self.apply_animation_tick(),
            Message::KeyPressed(key, modifiers) => self.handle_key(key, modifiers),
        }
    }

    fn view(&self) -> Element<'_, Message> {
        let filtered = self.filtered_indices();
        let visible = visible_row_window(
            filtered.len(),
            self.selected_row,
            self.options.max_visible_rows,
        );
        let visible_start = visible.start;
        let visible_end = visible.end;
        let visible_count = visible_end.saturating_sub(visible_start);
        let progress = eased_progress(self.animation);

        let header = row![
            column![
                text("Clipboard").size(28),
                text("CopyQ history · fast fuzzy access · multi-entry merge · image previews")
                    .size(13),
            ]
            .spacing(2)
            .width(Length::Fill),
            button("refresh")
                .padding([8, 14])
                .on_press(Message::LoadRequested),
            button("merge visible")
                .padding([8, 14])
                .on_press(Message::MergeVisibleSelection),
            button(if self.show_help {
                "hide shortcuts"
            } else {
                "shortcuts"
            })
            .padding([8, 14])
            .on_press(Message::ToggleHelp),
            button("×")
                .padding([8, 12])
                .on_press(Message::CloseRequested),
        ]
        .spacing(10)
        .align_y(Alignment::Center);

        let search = text_input("search clipboard history…", &self.query)
            .on_input(Message::QueryChanged)
            .on_submit(Message::ActivateSelected)
            .padding(12)
            .size(18);

        let mut list = column![].spacing((4.0 + 4.0 * progress).round());
        for (row_index, item_index) in filtered
            .iter()
            .copied()
            .enumerate()
            .skip(visible_start)
            .take(visible_count)
        {
            let item = &self.items[item_index];
            list = list.push(self.view_item(row_index, item));
        }

        if visible_count == 0 && !self.loading {
            list = list.push(
                container(
                    column![
                        text("No matching clipboard entries").size(18),
                        text("Try a broader query, or press refresh if CopyQ changed.").size(13),
                    ]
                    .spacing(6),
                )
                .padding(18)
                .width(Length::Fill)
                .style(|_| muted_panel_style()),
            );
        }

        let selection_status = if filtered.is_empty() {
            "no selection".to_string()
        } else {
            format!(
                "selected {} / {} · rows {}–{}",
                self.selected_row.min(filtered.len() - 1) + 1,
                filtered.len(),
                visible_start + 1,
                visible_end
            )
        };
        let status = row![
            text(if self.loading { "◌" } else { "●" }).size(16),
            text(format!(
                "{} · {} · {} total · F1 shortcuts",
                self.status,
                selection_status,
                self.items.len()
            ))
            .size(12)
        ]
        .spacing(8)
        .align_y(Alignment::Center);

        let mut content = column![header, search].spacing(14);
        if self.show_help {
            content = content.push(
                container(
                    column![
                        text("Keyboard shortcuts").size(17),
                        text("↑/↓ or Tab navigate · PgUp/PgDn jump · Home/End go to bounds")
                            .size(12),
                        text("Enter pastes selected · Ctrl+M merges the rendered rows").size(12),
                        text("Ctrl+R refreshes · Ctrl+U clears search · F1 hides this guide")
                            .size(12),
                        text("Esc hides this guide first, then closes the popup").size(12),
                    ]
                    .spacing(5),
                )
                .padding(14)
                .width(Length::Fill)
                .style(|_| muted_panel_style()),
            );
        }
        content = content
            .push(scrollable(list).height(Length::Fill))
            .push(status);

        container(content.padding((12.0 + 6.0 * progress).round() as u16))
            .width(Length::Fill)
            .height(Length::Fill)
            .style(move |_| app_background_style(progress))
            .into()
    }

    fn subscription(&self) -> Subscription<Message> {
        Subscription::batch(vec![
            event::listen_with(|event, _status, _id| match event {
                Event::Keyboard(keyboard::Event::KeyPressed { key, modifiers, .. }) => {
                    Some(Message::KeyPressed(key, modifiers))
                }
                Event::Window(window::Event::CloseRequested) => Some(Message::CloseRequested),
                Event::Window(window::Event::Unfocused) => Some(Message::WindowUnfocused),
                _ => None,
            }),
            iced::time::every(Duration::from_millis(16)).map(|_| Message::AnimationTick),
        ])
    }

    fn view_item<'a>(&'a self, row_index: usize, item: &'a ClipboardItem) -> Element<'a, Message> {
        let selected = row_index == self.selected_row;
        let preview = if item.preview.is_empty() {
            "<empty>".to_string()
        } else {
            item.preview.clone()
        };

        let badge = container(text(item.kind_label()).size(12))
            .padding([4, 8])
            .style(|_| badge_style());

        let body = column![text(preview).size(15), text(item.compact_meta()).size(11)].spacing(5);
        let thumbnail = self.view_image_preview(item);

        let row_content = row![
            text(format!("{:>2}", row_index + 1)).size(13),
            badge,
            thumbnail,
            body.width(Length::Fill),
            button("paste")
                .padding([6, 10])
                .on_press(Message::PasteIndex(item.index)),
        ]
        .spacing(10)
        .align_y(Alignment::Center);

        let progress = eased_progress(self.animation);
        button(
            container(row_content)
                .width(Length::Fill)
                .padding((8.0 + 2.0 * progress).round() as u16)
                .style(move |_| item_panel_style(selected, progress)),
        )
        .style(button::text)
        .on_press(Message::RowSelected(row_index))
        .width(Length::Fill)
        .into()
    }

    fn view_image_preview<'a>(&'a self, item: &'a ClipboardItem) -> Element<'a, Message> {
        if !item.is_image() {
            return container(text(""))
                .width(Length::Fixed(0.0))
                .height(Length::Fixed(0.0))
                .into();
        }

        match self.image_previews.get(&item.index) {
            Some(ImagePreviewState::Ready(bytes)) => container(
                image(image::Handle::from_bytes(bytes.clone()))
                    .width(THUMBNAIL_SIZE)
                    .height(THUMBNAIL_SIZE),
            )
            .padding(2)
            .style(|_| thumbnail_frame_style(true))
            .into(),
            Some(ImagePreviewState::Error(_)) => container(
                text(
                    "image
preview
failed",
                )
                .size(11),
            )
            .width(THUMBNAIL_SIZE)
            .height(THUMBNAIL_SIZE)
            .padding(6)
            .style(|_| thumbnail_frame_style(false))
            .into(),
            Some(ImagePreviewState::Loading) => container(
                text(
                    "loading
image…",
                )
                .size(11),
            )
            .width(THUMBNAIL_SIZE)
            .height(THUMBNAIL_SIZE)
            .padding(6)
            .style(|_| thumbnail_frame_style(false))
            .into(),
            None => container(text("image").size(12))
                .width(THUMBNAIL_SIZE)
                .height(THUMBNAIL_SIZE)
                .padding(6)
                .style(|_| thumbnail_frame_style(false))
                .into(),
        }
    }

    fn handle_key(&mut self, key: Key, modifiers: Modifiers) -> Task<Message> {
        let len = self.filtered_indices().len();
        match key {
            Key::Named(key::Named::Escape) => {
                if self.show_help {
                    self.show_help = false;
                } else {
                    self.begin_close();
                }
                Task::none()
            }
            Key::Named(key::Named::F1) => self.update(Message::ToggleHelp),
            Key::Named(key::Named::ArrowDown) | Key::Named(key::Named::Tab) => {
                self.move_selection(SelectionMove::Down(1), len)
            }
            Key::Named(key::Named::ArrowUp) => self.move_selection(SelectionMove::Up(1), len),
            Key::Named(key::Named::PageDown) => self.move_selection(SelectionMove::Down(10), len),
            Key::Named(key::Named::PageUp) => self.move_selection(SelectionMove::Up(10), len),
            Key::Named(key::Named::Home) => self.move_selection(SelectionMove::First, len),
            Key::Named(key::Named::End) => self.move_selection(SelectionMove::Last, len),
            Key::Named(key::Named::Enter) => self.update(Message::ActivateSelected),
            Key::Character(value) if modifiers.control() && value.eq_ignore_ascii_case("r") => {
                self.update(Message::LoadRequested)
            }
            Key::Character(value) if modifiers.control() && value.eq_ignore_ascii_case("u") => {
                self.query.clear();
                self.selected_row = 0;
                Task::none()
            }
            Key::Character(value) if modifiers.control() && value.eq_ignore_ascii_case("m") => {
                self.update(Message::MergeVisibleSelection)
            }
            _ => Task::none(),
        }
    }

    fn move_selection(&mut self, movement: SelectionMove, len: usize) -> Task<Message> {
        self.selected_row = next_selected_row(self.selected_row, len, movement);
        self.schedule_visible_image_loads()
    }

    fn start_paste(&mut self, indices: Vec<usize>) -> Task<Message> {
        if indices.is_empty() {
            self.status = "No clipboard entry selected".to_string();
            return Task::none();
        }
        self.status = if indices.len() == 1 {
            format!("Activating clipboard #{}…", indices[0])
        } else {
            format!("Merging {} visible clipboard entries…", indices.len())
        };
        Task::perform(
            activate_items(self.options.clone(), indices),
            Message::OperationDone,
        )
    }

    fn schedule_visible_image_loads(&mut self) -> Task<Message> {
        let filtered = self.filtered_indices();
        let visible = visible_row_window(
            filtered.len(),
            self.selected_row,
            self.options.max_visible_rows,
        );
        let requests = image_load_requests_for_visible_items(
            &self.items,
            &filtered[visible],
            &self.image_previews,
        );

        if requests.is_empty() {
            return Task::none();
        }

        let tasks = requests
            .into_iter()
            .map(|request| {
                self.image_previews
                    .insert(request.index, ImagePreviewState::Loading);
                Task::perform(
                    load_image(self.options.clone(), request.index, request.mime_type),
                    move |result| Message::ImageLoaded(request.index, result),
                )
            })
            .collect::<Vec<_>>();

        Task::batch(tasks)
    }

    fn begin_close(&mut self) {
        self.visibility = VisibilityPhase::Closing;
        self.status = "Closing…".to_string();
    }

    fn apply_animation_tick(&mut self) -> Task<Message> {
        match self.visibility {
            VisibilityPhase::Opening => {
                self.animation = (self.animation + ANIMATION_STEP).min(1.0);
                if self.animation >= 1.0 {
                    self.visibility = VisibilityPhase::Open;
                }
                Task::none()
            }
            VisibilityPhase::Open => Task::none(),
            VisibilityPhase::Closing => {
                self.animation = (self.animation - ANIMATION_STEP).max(0.0);
                if self.animation <= 0.0 {
                    window::latest().and_then(window::close)
                } else {
                    Task::none()
                }
            }
        }
    }

    fn selected_item_index(&self) -> Option<usize> {
        self.filtered_indices()
            .get(self.selected_row)
            .map(|item_index| self.items[*item_index].index)
    }

    fn filtered_indices(&self) -> Vec<usize> {
        filter_item_indices(&self.items, &self.query)
    }
}

async fn load_items(options: CopyqFrontendOptions) -> Result<Vec<ClipboardItem>, String> {
    CopyqClient::new(&options)
        .list_items()
        .await
        .map_err(|error| error.to_string())
}

async fn activate_items(options: CopyqFrontendOptions, indices: Vec<usize>) -> Result<(), String> {
    CopyqClient::new(&options)
        .merge_select_and_paste(&indices, options.paste_on_activate)
        .await
        .map_err(|error| error.to_string())
}

async fn load_image(
    options: CopyqFrontendOptions,
    index: usize,
    mime_type: String,
) -> Result<Vec<u8>, String> {
    CopyqClient::new(&options)
        .read_image_bytes(index, &mime_type)
        .await
        .map_err(|error| error.to_string())
}

pub fn preferred_image_mime(mime_types: &[String]) -> Option<&str> {
    const PREFERRED: &[&str] = &[
        "image/png",
        "image/jpeg",
        "image/jpg",
        "image/webp",
        "image/gif",
        "image/bmp",
        "image/tiff",
    ];

    PREFERRED
        .iter()
        .find_map(|candidate| {
            mime_types
                .iter()
                .any(|mime| mime == candidate)
                .then_some(*candidate)
        })
        .or_else(|| {
            mime_types
                .iter()
                .find(|mime| mime.starts_with("image/"))
                .map(String::as_str)
        })
}

fn image_load_requests_for_visible_items(
    items: &[ClipboardItem],
    filtered_indices: &[usize],
    image_previews: &HashMap<usize, ImagePreviewState>,
) -> Vec<ImageLoadRequest> {
    filtered_indices
        .iter()
        .filter_map(|item_index| items.get(*item_index))
        .filter_map(|item| {
            let mime_type = item.primary_image_mime()?;
            (!image_previews.contains_key(&item.index)).then(|| ImageLoadRequest {
                index: item.index,
                mime_type: mime_type.to_string(),
            })
        })
        .collect()
}

fn eased_progress(progress: f32) -> f32 {
    let p = progress.clamp(0.0, 1.0);
    1.0 - (1.0 - p).powi(3)
}

fn next_selected_row(current: usize, len: usize, movement: SelectionMove) -> usize {
    if len == 0 {
        return 0;
    }

    match movement {
        SelectionMove::Up(amount) => current.saturating_sub(amount),
        SelectionMove::Down(amount) => (current + amount).min(len.saturating_sub(1)),
        SelectionMove::First => 0,
        SelectionMove::Last => len.saturating_sub(1),
    }
}

pub fn i3_config_snippet(executable: &str, modifier: &str) -> String {
    format!(
        r#"for_window [app_id="unilii-copyq"] floating enable, border pixel 2, move position center, sticky enable
bindsym {modifier}+v exec --no-startup-id {executable} --i3-shortcut"#,
    )
}

pub fn filter_item_indices(items: &[ClipboardItem], query: &str) -> Vec<usize> {
    let tokens = query
        .split_whitespace()
        .map(|token| token.to_lowercase())
        .collect::<Vec<_>>();

    if tokens.is_empty() {
        return (0..items.len()).collect();
    }

    items
        .iter()
        .enumerate()
        .filter_map(|(index, item)| {
            let haystack = format!(
                "{} {} {} {}",
                item.index,
                item.mime_type,
                item.kind_label(),
                item.preview
            )
            .to_lowercase();

            tokens
                .iter()
                .all(|token| haystack.contains(token))
                .then_some(index)
        })
        .collect()
}

pub fn parse_clipboard_items_json(payload: &str) -> Result<Vec<ClipboardItem>, CopyqError> {
    serde_json::from_str::<Vec<ClipboardItem>>(payload.trim())
        .map_err(|error| CopyqError::Json(error.to_string()))
}

#[allow(dead_code)]
pub fn compact_preview(value: &str, max_chars: usize) -> String {
    let normalized = value
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .replace('\n', "↵");
    truncate_middle(&normalized, max_chars)
}

#[allow(dead_code)]
pub fn truncate_middle(value: &str, max_chars: usize) -> String {
    let len = value.chars().count();
    if len <= max_chars {
        return value.to_string();
    }
    if max_chars <= ELLIPSIS.chars().count() + 2 {
        return ELLIPSIS.to_string();
    }

    let keep = max_chars - ELLIPSIS.chars().count();
    let head = keep / 2;
    let tail = keep - head;
    let prefix = value.chars().take(head).collect::<String>();
    let suffix = value.chars().skip(len - tail).collect::<String>();
    format!("{}{}{}", prefix, ELLIPSIS, suffix)
}

pub fn build_history_eval_script(max_preview_chars: usize) -> String {
    format!(
        r#"
var MAX_LENGTH = {max_preview_chars};
var ELLIPSIS = "…";

function truncateMiddle(value) {{
  if (value.length <= MAX_LENGTH) {{
    return value;
  }}
  if (MAX_LENGTH <= ELLIPSIS.length + 2) {{
    return ELLIPSIS;
  }}
  var keep = MAX_LENGTH - ELLIPSIS.length;
  var head = Math.floor(keep / 2);
  var tail = keep - head;
  return value.substring(0, head) + ELLIPSIS + value.substring(value.length - tail);
}}

function compactPreview(value) {{
  return truncateMiddle(String(value).replace(/\r\n|\r|\n/g, "↵"));
}}

function nonEmpty(value) {{ return value.length > 0; }}
function hasMime(mimeTypes, target) {{ return mimeTypes.indexOf(target) !== -1; }}
function preferredImageMime(mimeTypes) {{
  var preferred = ["image/png", "image/jpeg", "image/jpg", "image/webp", "image/gif", "image/bmp", "image/tiff"];
  for (var p = 0; p < preferred.length; p++) {{
    if (hasMime(mimeTypes, preferred[p])) {{ return preferred[p]; }}
  }}
  for (var m = 0; m < mimeTypes.length; m++) {{
    if (mimeTypes[m].indexOf("image/") === 0) {{ return mimeTypes[m]; }}
  }}
  return null;
}}

var items = [];
for (var i = 0; i < count(); ++i) {{
  var mimeTypes = str(read("?", i)).split("\n").filter(nonEmpty);
  var mimeType = mimeTypes.length > 0 ? mimeTypes[0] : "application/octet-stream";
  var isText = hasMime(mimeTypes, "text/plain") || hasMime(mimeTypes, "text/html") ||
               mimeType === "text/plain" || mimeType === "text/html";
  var imageMime = preferredImageMime(mimeTypes);
  var raw = isText ? str(read(i)) : (imageMime ? "<image: " + imageMime + ">" : "<" + mimeType + ">");
  var lines = raw.length === 0 ? 0 : raw.split(/\r\n|\r|\n/).length;
  items.push({{
    index: i,
    mime_type: mimeType,
    mime_types: mimeTypes,
    preview: compactPreview(raw),
    is_text: isText,
    image_mime_type: imageMime,
    chars: raw.length,
    lines: lines
  }});
}}
print(JSON.stringify(items));
"#
    )
}

pub fn build_selected_items_eval_script(indices: &[usize]) -> String {
    let indices = indices
        .iter()
        .map(|index| index.to_string())
        .collect::<Vec<_>>()
        .join(", ");

    format!(
        r#"
var indices = [{indices}];
for (var i = 0; i < indices.length; i++) {{
  var j = indices[i];
  var mimeTypes = str(read("?", j)).split("\n");
  var mimeType = mimeTypes.length > 0 ? mimeTypes[0] : "application/octet-stream";
  if (mimeTypes.indexOf("text/plain") !== -1 || mimeTypes.indexOf("text/html") !== -1 ||
      mimeType === "text/plain" || mimeType === "text/html") {{
    print(str(read(j)));
  }} else {{
    print("<" + mimeType + ">");
  }}
  print("\n");
}}
"#
    )
}

fn app_background_style(progress: f32) -> container::Style {
    container::Style {
        background: Some(iced::Background::Color(
            [0.050, 0.055, 0.070, (0.84 + 0.14 * progress).min(0.98)].into(),
        )),
        text_color: Some([0.90, 0.92, 0.96, 1.0].into()),
        ..Default::default()
    }
}

fn muted_panel_style() -> container::Style {
    container::Style {
        background: Some(iced::Background::Color([0.10, 0.11, 0.14, 0.90].into())),
        text_color: Some([0.78, 0.80, 0.86, 1.0].into()),
        border: iced::Border {
            width: 1.0,
            color: [0.22, 0.24, 0.30, 1.0].into(),
            radius: 14.0.into(),
        },
        ..Default::default()
    }
}

fn item_panel_style(selected: bool, progress: f32) -> container::Style {
    let bg = if selected {
        [0.16, 0.19, 0.28, 0.98]
    } else {
        [0.09, 0.10, 0.13, 0.94]
    };
    let border = if selected {
        [0.42, 0.55, 0.95, 1.0]
    } else {
        [0.20, 0.22, 0.28, 1.0]
    };
    container::Style {
        background: Some(iced::Background::Color(
            [bg[0], bg[1], bg[2], bg[3] * progress.max(0.25)].into(),
        )),
        border: iced::Border {
            width: 1.0,
            color: border.into(),
            radius: 12.0.into(),
        },
        ..Default::default()
    }
}

fn badge_style() -> container::Style {
    container::Style {
        background: Some(iced::Background::Color([0.17, 0.19, 0.24, 1.0].into())),
        text_color: Some([0.80, 0.86, 1.0, 1.0].into()),
        border: iced::Border {
            width: 1.0,
            color: [0.28, 0.32, 0.44, 1.0].into(),
            radius: 999.0.into(),
        },
        ..Default::default()
    }
}

fn thumbnail_frame_style(loaded: bool) -> container::Style {
    let color = if loaded {
        [0.34, 0.42, 0.62, 1.0]
    } else {
        [0.24, 0.26, 0.32, 1.0]
    };
    container::Style {
        background: Some(iced::Background::Color([0.07, 0.08, 0.10, 0.92].into())),
        text_color: Some([0.70, 0.75, 0.86, 1.0].into()),
        border: iced::Border {
            width: 1.0,
            color: color.into(),
            radius: 10.0.into(),
        },
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(index: usize, mime_type: &str, preview: &str) -> ClipboardItem {
        ClipboardItem {
            index,
            mime_type: mime_type.to_string(),
            mime_types: vec![mime_type.to_string()],
            preview: preview.to_string(),
            is_text: mime_type.starts_with("text/"),
            image_mime_type: preferred_image_mime(&[mime_type.to_string()]).map(str::to_string),
            chars: preview.len(),
            lines: 1,
        }
    }

    #[test]
    fn compact_preview_replaces_newlines_and_truncates_middle() {
        let preview = compact_preview("alpha\nbeta\ngamma\ndelta", 14);
        assert!(preview.contains('↵'));
        assert!(preview.contains('…'));
        assert!(preview.starts_with("alpha"));
        assert!(preview.ends_with("delta"));
    }

    #[test]
    fn parse_clipboard_items_json_accepts_copyq_schema() {
        let payload = r#"[{"index":0,"mime_type":"text/plain","mime_types":["text/plain"],"preview":"hello","is_text":true,"chars":5,"lines":1}]"#;
        let items = parse_clipboard_items_json(payload).expect("json parses");
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].index, 0);
        assert_eq!(items[0].kind_label(), "text");
    }

    #[test]
    fn filter_matches_all_query_tokens_and_keeps_history_order_for_equal_scores() {
        let items = vec![
            item(0, "text/plain", "cargo build workspace"),
            item(1, "text/plain", "workspace cargo test"),
            item(2, "image/png", "<image/png>"),
        ];
        let matches = filter_item_indices(&items, "cargo workspace");
        assert_eq!(matches, vec![0, 1]);
        assert!(filter_item_indices(&items, "cargo image").is_empty());
    }

    #[test]
    fn build_history_eval_script_uses_single_count_loop_and_json() {
        let script = build_history_eval_script(200);
        assert!(script.contains("for (var i = 0; i < count(); ++i)"));
        assert!(script.contains("JSON.stringify(items)"));
        assert!(script.contains("preferredImageMime"));
        assert!(script.contains("image_mime_type"));
        assert!(!script.contains("copyq read"));
    }

    #[test]
    fn selected_items_script_contains_only_numeric_indices() {
        let script = build_selected_items_eval_script(&[0, 2, 13]);
        assert!(script.contains("var indices = [0, 2, 13];"));
        assert!(script.contains("print(str(read(j)))"));
    }

    #[test]
    fn image_mime_prefers_png_and_detects_image_items() {
        let item = ClipboardItem {
            index: 3,
            mime_type: "image/bmp".to_string(),
            mime_types: vec![
                "text/uri-list".to_string(),
                "image/png".to_string(),
                "image/bmp".to_string(),
            ],
            preview: "<image: image/png>".to_string(),
            is_text: false,
            image_mime_type: Some("image/png".to_string()),
            chars: 18,
            lines: 1,
        };
        assert!(item.is_image());
        assert_eq!(item.kind_label(), "image");
        assert_eq!(item.primary_image_mime(), Some("image/png"));
    }

    #[test]
    fn image_load_requests_skip_cached_or_non_visible_items() {
        let items = vec![
            item(0, "text/plain", "hello"),
            item(1, "image/png", "<image: image/png>"),
            item(2, "image/jpeg", "<image: image/jpeg>"),
        ];
        let mut cache = HashMap::new();
        cache.insert(1, ImagePreviewState::Loading);
        let requests = image_load_requests_for_visible_items(&items, &[0, 1, 2], &cache);
        assert_eq!(
            requests,
            vec![ImageLoadRequest {
                index: 2,
                mime_type: "image/jpeg".to_string()
            }]
        );
    }

    #[test]
    fn easing_is_monotonic_and_reaches_one() {
        assert_eq!(eased_progress(0.0), 0.0);
        assert_eq!(eased_progress(1.0), 1.0);
        assert!(eased_progress(0.75) > eased_progress(0.25));
    }

    #[test]
    fn i3_defaults_make_popup_shortcut_friendly() {
        let options = apply_i3_shortcut_defaults(CopyqFrontendOptions::default());
        assert!(options.i3_shortcut_mode);
        assert!(options.close_on_unfocus);
        assert_eq!(options.window_width, I3_WINDOW_WIDTH);
        assert_eq!(options.window_height, I3_WINDOW_HEIGHT);

        let settings = build_window_settings(&options);
        assert!(!settings.decorations);
        assert!(!settings.resizable);
        assert!(settings.transparent);
        assert_eq!(settings.level, window::Level::AlwaysOnTop);
        assert!(!settings.exit_on_close_request);
    }

    #[test]
    fn selection_navigation_clamps_to_history_bounds() {
        assert_eq!(next_selected_row(0, 0, SelectionMove::Down(1)), 0);
        assert_eq!(next_selected_row(0, 3, SelectionMove::Up(1)), 0);
        assert_eq!(next_selected_row(0, 3, SelectionMove::Down(10)), 2);
        assert_eq!(next_selected_row(2, 3, SelectionMove::Last), 2);
        assert_eq!(next_selected_row(2, 3, SelectionMove::First), 0);
        assert_eq!(next_selected_row(2, 10, SelectionMove::Up(2)), 0);
    }

    #[test]
    fn visible_window_keeps_the_selected_row_on_screen() {
        assert_eq!(visible_row_window(0, 0, 5), 0..0);
        assert_eq!(visible_row_window(3, 2, 5), 0..3);
        assert_eq!(visible_row_window(20, 0, 5), 0..5);
        assert_eq!(visible_row_window(20, 10, 5), 8..13);
        assert_eq!(visible_row_window(20, 19, 5), 15..20);
    }

    #[test]
    fn help_escape_is_contextual_before_closing() {
        let (mut app, _) = CopyqApp::new(CopyqFrontendOptions::default());
        let _ = app.update(Message::ToggleHelp);
        assert!(app.show_help);

        let _ = app.handle_key(Key::Named(key::Named::Escape), Modifiers::default());
        assert!(!app.show_help);
        assert_eq!(app.visibility, VisibilityPhase::Opening);

        let _ = app.handle_key(Key::Named(key::Named::Escape), Modifiers::default());
        assert_eq!(app.visibility, VisibilityPhase::Closing);
    }

    #[test]
    fn i3_config_snippet_uses_stable_app_id_and_shortcut_mode() {
        let snippet = i3_config_snippet("/usr/local/bin/unilii-copyq", "$mod");
        assert!(snippet.contains(r#"app_id="unilii-copyq""#));
        assert!(snippet.contains("floating enable"));
        assert!(snippet.contains("bindsym $mod+v"));
        assert!(snippet.contains("--i3-shortcut"));
    }
}
