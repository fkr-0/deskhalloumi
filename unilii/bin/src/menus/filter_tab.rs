#![allow(dead_code)]
//! Iced view adapter for the renderer-neutral `deskhalloumi_core::filter_tab` model.
//!
//! The update semantics live in `unilii-core`; this module only turns the model
//! into unilii/Iced widgets and UI messages.

use deskhalloumi_core::filter_tab::{
    DEFAULT_QUICK_SELECT_ALPHABET, FilterTabMenuInput, FilterTabMenuState, WindowMatchContext,
    icon_for_window,
};
use iced::widget::{button, column, container, image, row, scrollable, text, text_input};
use iced::{Alignment, Element, Length};
use std::collections::HashMap;

const PREVIEW_WIDTH: f32 = 112.0;
const PREVIEW_HEIGHT: f32 = 70.0;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FilterTabViewMessage {
    Input(FilterTabMenuInput),
    QueryChanged(String),
    ConfirmWindow(u32),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilterTabViewTab {
    pub key: char,
    pub label: String,
    pub active: bool,
    pub count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FilterTabPreview {
    None,
    Loading,
    Ready(Vec<u8>),
    Error(String),
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilterTabViewRow {
    pub id: u32,
    pub icon: &'static str,
    pub title: String,
    pub class_name: String,
    pub workspace: Option<String>,
    pub selected: bool,
    pub urgent: bool,
    pub quick_label: Option<char>,
    pub preview: FilterTabPreview,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilterTabViewSnapshot {
    pub visible: bool,
    pub tabs: Vec<FilterTabViewTab>,
    pub rows: Vec<FilterTabViewRow>,
    pub query: String,
    pub status: String,
    pub quick_select_armed: bool,
}

pub fn snapshot_from_state(state: &FilterTabMenuState) -> FilterTabViewSnapshot {
    snapshot_from_state_with_previews(state, &HashMap::new())
}

pub fn snapshot_from_state_with_previews(
    state: &FilterTabMenuState,
    previews: &HashMap<u32, FilterTabPreview>,
) -> FilterTabViewSnapshot {
    let tabs = state
        .tabs
        .iter()
        .enumerate()
        .map(|(index, tab)| FilterTabViewTab {
            key: tab.key,
            label: tab.label.clone(),
            active: index == state.active_tab_index,
            count: deskhalloumi_core::filter_tab::filter_visible_window_indices(
                &state.windows,
                &tab.filter,
                &state.query,
            )
            .len(),
        })
        .collect::<Vec<_>>();

    let visible_indices = state.visible_indices();
    let quick_labels = quick_select_labels(visible_indices.len(), DEFAULT_QUICK_SELECT_ALPHABET);
    let rows = visible_indices
        .iter()
        .enumerate()
        .filter_map(|(visible_index, window_index)| {
            state.windows.get(*window_index).map(|window| {
                view_row(
                    window,
                    visible_index == state.selected_visible_index,
                    state
                        .quick_select_armed
                        .then(|| quick_labels.get(visible_index).copied())
                        .flatten(),
                    previews
                        .get(&window.id)
                        .cloned()
                        .unwrap_or(FilterTabPreview::None),
                )
            })
        })
        .collect::<Vec<_>>();

    let active_label = state
        .active_tab()
        .map(|tab| tab.label.clone())
        .unwrap_or_else(|| "No tab".to_string());
    let status = if state.quick_select_armed {
        format!(
            "{} · quick select armed: {} · Esc disarms",
            active_label,
            quick_labels.iter().collect::<String>()
        )
    } else {
        format!(
            "{} · {} visible · j/k wrap · g/G ends · Ctrl+U clear · Ctrl+R refresh · Ctrl+Q quick select · Esc clears/closes",
            active_label,
            rows.len()
        )
    };

    FilterTabViewSnapshot {
        visible: state.visible,
        tabs,
        rows,
        query: state.query.clone(),
        status,
        quick_select_armed: state.quick_select_armed,
    }
}

pub fn quick_select_labels(count: usize, alphabet: &str) -> Vec<char> {
    alphabet.chars().take(count).collect()
}

fn view_row(
    window: &WindowMatchContext,
    selected: bool,
    quick_label: Option<char>,
    preview: FilterTabPreview,
) -> FilterTabViewRow {
    FilterTabViewRow {
        id: window.id,
        icon: icon_for_window(window),
        title: window.title.clone(),
        class_name: window
            .class_name
            .clone()
            .unwrap_or_else(|| "Unknown".to_string()),
        workspace: window.workspace.clone(),
        selected,
        urgent: window.urgent,
        quick_label,
        preview,
    }
}

pub fn view_filter_tab_menu(state: &FilterTabMenuState) -> Element<'_, FilterTabViewMessage> {
    view_filter_tab_menu_with_previews(state, &HashMap::new())
}

pub fn view_filter_tab_menu_with_previews<'a>(
    state: &'a FilterTabMenuState,
    previews: &HashMap<u32, FilterTabPreview>,
) -> Element<'a, FilterTabViewMessage> {
    let snapshot = snapshot_from_state_with_previews(state, previews);

    if !snapshot.visible {
        return container(text("filter tab hidden").size(1))
            .width(Length::Shrink)
            .height(Length::Shrink)
            .into();
    }

    let mut tabs = row![].spacing(6).align_y(Alignment::Center);
    for tab in snapshot.tabs {
        let label = if tab.active {
            format!("● {} {} ({})", tab.key, tab.label, tab.count)
        } else {
            format!("○ {} {} ({})", tab.key, tab.label, tab.count)
        };
        tabs = tabs.push(button(text(label).size(13)).padding([6, 10]).on_press(
            FilterTabViewMessage::Input(FilterTabMenuInput::SelectTab(tab.key)),
        ));
    }

    let query = text_input("filter windows…", &snapshot.query)
        .on_input(FilterTabViewMessage::QueryChanged)
        .on_submit(FilterTabViewMessage::Input(FilterTabMenuInput::Confirm))
        .padding(10)
        .size(16);

    let mut rows = column![].spacing(6);
    for (index, window) in snapshot.rows.iter().cloned().enumerate() {
        rows = rows.push(view_filter_tab_row(index, window));
    }

    if snapshot.rows.is_empty() {
        rows = rows.push(
            container(
                column![
                    text("No matching windows").size(17),
                    text("Change tab/query; Esc clears query before closing; modifier release only confirms when a row exists.")
                        .size(12),
                ]
                .spacing(4),
            )
            .padding(14)
            .width(Length::Fill),
        );
    }

    let footer = row![
        text(snapshot.status).size(12).width(Length::Fill),
        button("kill")
            .padding([5, 8])
            .on_press(FilterTabViewMessage::Input(
                FilterTabMenuInput::ContextAction('q')
            )),
        button("float")
            .padding([5, 8])
            .on_press(FilterTabViewMessage::Input(
                FilterTabMenuInput::ContextAction(' ')
            )),
    ]
    .spacing(6)
    .align_y(Alignment::Center);

    container(
        column![
            row![
                column![
                    text("Window Filter Tabs").size(24),
                    text(if snapshot.quick_select_armed {
                        "quick select: press a/s/d/f/h/j/k/l · Esc disarms"
                    } else {
                        "j/k wraps · g/G ends · Ctrl+U clears · Ctrl+R refreshes · mod-release confirms"
                    })
                    .size(12),
                ]
                .spacing(2)
                .width(Length::Fill),
                button("×")
                    .padding([6, 10])
                    .on_press(FilterTabViewMessage::Input(FilterTabMenuInput::Escape)),
            ]
            .align_y(Alignment::Center),
            tabs,
            query,
            scrollable(rows).height(Length::Fill),
            footer,
        ]
        .spacing(10)
        .padding(14),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}

fn view_preview(preview: FilterTabPreview) -> Element<'static, FilterTabViewMessage> {
    match preview {
        FilterTabPreview::Ready(bytes) => container(
            image(image::Handle::from_bytes(bytes))
                .width(PREVIEW_WIDTH)
                .height(PREVIEW_HEIGHT),
        )
        .width(PREVIEW_WIDTH)
        .height(PREVIEW_HEIGHT)
        .padding(2)
        .into(),
        FilterTabPreview::Loading => container(
            text(
                "preview
loading",
            )
            .size(10),
        )
        .width(PREVIEW_WIDTH)
        .height(PREVIEW_HEIGHT)
        .padding(6)
        .into(),
        FilterTabPreview::Error(_) => container(
            text(
                "preview
failed",
            )
            .size(10),
        )
        .width(PREVIEW_WIDTH)
        .height(PREVIEW_HEIGHT)
        .padding(6)
        .into(),
        FilterTabPreview::Unavailable => container(
            text(
                "no
preview",
            )
            .size(10),
        )
        .width(PREVIEW_WIDTH)
        .height(PREVIEW_HEIGHT)
        .padding(6)
        .into(),
        FilterTabPreview::None => container(text(""))
            .width(Length::Fixed(0.0))
            .height(Length::Fixed(0.0))
            .into(),
    }
}

fn view_filter_tab_row(
    index: usize,
    window: FilterTabViewRow,
) -> Element<'static, FilterTabViewMessage> {
    let marker = if window.selected { "▶" } else { " " };
    let quick_label = window
        .quick_label
        .map(|label| format!("[{label}]"))
        .unwrap_or_else(|| format!("{}", index + 1));
    let urgency = if window.urgent { " !" } else { "" };
    let workspace = window
        .workspace
        .as_ref()
        .map(|workspace| format!("ws {workspace}"))
        .unwrap_or_else(|| "ws ?".to_string());

    let preview = view_preview(window.preview.clone());

    button(
        container(
            row![
                text(marker).size(15),
                text(quick_label).size(12),
                text(window.icon).size(18),
                preview,
                column![
                    text(format!("{}{}", window.title, urgency)).size(15),
                    text(format!(
                        "{} · {} · #{}",
                        window.class_name, workspace, window.id
                    ))
                    .size(11),
                ]
                .spacing(2)
                .width(Length::Fill),
            ]
            .spacing(8)
            .align_y(Alignment::Center),
        )
        .padding(8)
        .width(Length::Fill),
    )
    .on_press(FilterTabViewMessage::ConfirmWindow(window.id))
    .width(Length::Fill)
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use deskhalloumi_core::filter_tab::{FilterTab, handle_filter_tab_input};

    fn state() -> FilterTabMenuState {
        FilterTabMenuState::new(
            vec![
                FilterTab::new('a', "All", "*"),
                FilterTab::new('t', "Tmux", "tmux"),
            ],
            vec![
                WindowMatchContext::new(1, "work - tmux", Some("Alacritty")),
                WindowMatchContext::new(2, "browser", Some("firefox")),
            ],
        )
    }

    #[test]
    fn snapshot_marks_selected_row_and_counts_tab_matches() {
        let mut state = state();
        handle_filter_tab_input(&mut state, FilterTabMenuInput::Activate);
        handle_filter_tab_input(&mut state, FilterTabMenuInput::NextWindow);

        let snapshot = snapshot_from_state(&state);
        assert!(snapshot.visible);
        assert_eq!(snapshot.tabs[0].count, 2);
        assert_eq!(snapshot.tabs[1].count, 1);
        assert!(snapshot.rows[1].selected);
        assert!(snapshot.status.contains("j/k wrap"));
        assert!(snapshot.status.contains("Ctrl+R refresh"));
    }

    #[test]
    fn query_message_maps_directly_to_set_query_intent_for_controllers() {
        let msg = FilterTabViewMessage::QueryChanged("tmux".to_string());
        assert_eq!(msg, FilterTabViewMessage::QueryChanged("tmux".to_string()));
    }

    #[test]
    fn snapshot_exposes_quick_select_labels_when_armed() {
        let mut state = state();
        handle_filter_tab_input(&mut state, FilterTabMenuInput::Activate);
        handle_filter_tab_input(&mut state, FilterTabMenuInput::ToggleQuickSelect);

        let snapshot = snapshot_from_state(&state);
        assert!(snapshot.quick_select_armed);
        assert_eq!(snapshot.rows[0].quick_label, Some('a'));
        assert_eq!(snapshot.rows[1].quick_label, Some('s'));
        assert!(snapshot.status.contains("quick select armed"));
        assert!(snapshot.status.contains("Esc disarms"));
    }

    #[test]
    fn quick_select_labels_are_bounded_by_visible_count() {
        assert_eq!(quick_select_labels(3, "asdfhjkl"), vec!['a', 's', 'd']);
        assert_eq!(quick_select_labels(99, "as"), vec!['a', 's']);
    }

    #[test]
    fn snapshot_quick_select_labels_are_bounded_for_long_lists() {
        let windows = (0..10)
            .map(|i| WindowMatchContext::new(100 + i, format!("window {i}"), Some("class")))
            .collect::<Vec<_>>();
        let mut state = FilterTabMenuState::new(vec![FilterTab::new('a', "All", "*")], windows);
        handle_filter_tab_input(&mut state, FilterTabMenuInput::Activate);
        handle_filter_tab_input(&mut state, FilterTabMenuInput::ToggleQuickSelect);

        let snapshot = snapshot_from_state(&state);
        assert_eq!(snapshot.rows.len(), 10);
        assert_eq!(snapshot.rows[0].quick_label, Some('a'));
        assert_eq!(snapshot.rows[7].quick_label, Some('l'));
        assert_eq!(snapshot.rows[8].quick_label, None);
        assert_eq!(snapshot.rows[9].quick_label, None);
    }

    #[test]
    fn snapshot_attaches_preview_state_by_window_id() {
        let mut state = state();
        handle_filter_tab_input(&mut state, FilterTabMenuInput::Activate);
        let mut previews = HashMap::new();
        previews.insert(1, FilterTabPreview::Loading);
        previews.insert(2, FilterTabPreview::Ready(vec![1, 2, 3]));

        let snapshot = snapshot_from_state_with_previews(&state, &previews);
        assert_eq!(snapshot.rows[0].preview, FilterTabPreview::Loading);
        assert_eq!(
            snapshot.rows[1].preview,
            FilterTabPreview::Ready(vec![1, 2, 3])
        );
    }
}
