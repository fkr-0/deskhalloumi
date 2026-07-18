//! Renderer-neutral filter-tab menu model inspired by modrelease-2.
//!
//! This module intentionally ports interaction semantics instead of the old
//! Cairo/X11 renderer: tabs are filters, visible rows are computed from a
//! stable window snapshot, and modifier-release confirms the current selection
//! exactly like an explicit confirm key.

pub const DEFAULT_QUICK_SELECT_ALPHABET: &str = "asdfhjkl";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowMatchContext {
    pub id: u32,
    pub title: String,
    pub class_name: Option<String>,
    pub workspace: Option<String>,
    pub urgent: bool,
    pub native_window_id: Option<u64>,
}

impl WindowMatchContext {
    pub fn new(id: u32, title: impl Into<String>, class_name: Option<impl Into<String>>) -> Self {
        Self {
            id,
            title: title.into(),
            class_name: class_name.map(Into::into),
            workspace: None,
            urgent: false,
            native_window_id: None,
        }
    }

    pub fn with_workspace(mut self, workspace: impl Into<String>) -> Self {
        self.workspace = Some(workspace.into());
        self
    }

    pub fn with_native_window_id(mut self, native_window_id: u64) -> Self {
        self.native_window_id = Some(native_window_id);
        self
    }

    pub fn urgent(mut self, urgent: bool) -> Self {
        self.urgent = urgent;
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilterTab {
    pub key: char,
    pub label: String,
    pub filter: String,
}

impl FilterTab {
    pub fn new(key: char, label: impl Into<String>, filter: impl Into<String>) -> Self {
        Self {
            key,
            label: label.into(),
            filter: filter.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PopupSizing {
    pub width: u16,
    pub height: u16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContextWindowAction {
    KillWindow(u32),
    MoveToNextWorkspace(u32),
    SendToScratchWorkspace(u32),
    ToggleFloating(u32),
    MoveProgramInstancesToNextWorkspace { class_name: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionDescriptor {
    pub key: char,
    pub title: &'static str,
    pub description: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilterTabMenuInput {
    Activate,
    SetQuery,
    PushQueryChar(char),
    Backspace,
    SelectTab(char),
    NextTab,
    PreviousTab,
    NextWindow,
    PreviousWindow,
    PageDown,
    PageUp,
    FirstWindow,
    LastWindow,
    Confirm,
    ModifierRelease,
    ToggleQuickSelect,
    QuickSelect(char),
    Escape,
    ContextAction(char),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FilterTabMenuOutcome {
    None,
    Cancelled,
    ConfirmWindow(u32),
    ExecuteContextCommand(String),
    NeedsRefresh,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilterTabMenuState {
    pub visible: bool,
    pub tabs: Vec<FilterTab>,
    pub windows: Vec<WindowMatchContext>,
    pub active_tab_index: usize,
    pub selected_visible_index: usize,
    pub query: String,
    pub quick_select_armed: bool,
}

impl FilterTabMenuState {
    pub fn new(tabs: Vec<FilterTab>, windows: Vec<WindowMatchContext>) -> Self {
        Self {
            visible: false,
            tabs,
            windows,
            active_tab_index: 0,
            selected_visible_index: 0,
            query: String::new(),
            quick_select_armed: false,
        }
    }

    pub fn active_tab(&self) -> Option<&FilterTab> {
        self.tabs.get(self.active_tab_index)
    }

    pub fn active_filter(&self) -> &str {
        self.active_tab()
            .map(|tab| tab.filter.as_str())
            .unwrap_or("")
    }

    pub fn visible_indices(&self) -> Vec<usize> {
        filter_visible_window_indices(&self.windows, self.active_filter(), &self.query)
    }

    pub fn selected_window(&self) -> Option<&WindowMatchContext> {
        self.visible_indices()
            .get(self.selected_visible_index)
            .and_then(|window_index| self.windows.get(*window_index))
    }

    pub fn selected_window_id(&self) -> Option<u32> {
        self.selected_window().map(|window| window.id)
    }

    pub fn reset_selection(&mut self) {
        self.selected_visible_index = 0;
    }

    pub fn clear_quick_select(&mut self) {
        self.quick_select_armed = false;
    }

    pub fn quick_select_bindings(&self) -> Vec<(char, u32)> {
        quick_select_bindings_for_indices(
            &self.windows,
            &self.visible_indices(),
            DEFAULT_QUICK_SELECT_ALPHABET,
        )
    }

    pub fn clamp_selection(&mut self) {
        let len = self.visible_indices().len();
        self.selected_visible_index = clamp_selected_index(self.selected_visible_index, len);
    }
}

pub fn handle_filter_tab_input(
    state: &mut FilterTabMenuState,
    input: FilterTabMenuInput,
) -> FilterTabMenuOutcome {
    match input {
        FilterTabMenuInput::Activate => {
            state.visible = true;
            state.clear_quick_select();
            state.active_tab_index = state
                .active_tab_index
                .min(state.tabs.len().saturating_sub(1));
            state.reset_selection();
            FilterTabMenuOutcome::None
        }
        FilterTabMenuInput::SetQuery => {
            state.query.clear();
            state.clear_quick_select();
            state.reset_selection();
            FilterTabMenuOutcome::None
        }
        FilterTabMenuInput::PushQueryChar(ch) => {
            if state.visible && !ch.is_control() {
                state.query.push(ch);
                state.clear_quick_select();
                state.reset_selection();
            }
            FilterTabMenuOutcome::None
        }
        FilterTabMenuInput::Backspace => {
            if state.visible {
                state.query.pop();
                state.clear_quick_select();
                state.reset_selection();
            }
            FilterTabMenuOutcome::None
        }
        FilterTabMenuInput::SelectTab(key) => {
            if state.visible
                && let Some(index) = state.tabs.iter().position(|tab| tab.key == key)
            {
                state.active_tab_index = index;
                state.clear_quick_select();
                state.reset_selection();
            }
            FilterTabMenuOutcome::None
        }
        FilterTabMenuInput::NextTab => {
            if state.visible && !state.tabs.is_empty() {
                state.active_tab_index = (state.active_tab_index + 1) % state.tabs.len();
                state.clear_quick_select();
                state.reset_selection();
            }
            FilterTabMenuOutcome::None
        }
        FilterTabMenuInput::PreviousTab => {
            if state.visible && !state.tabs.is_empty() {
                state.active_tab_index = if state.active_tab_index == 0 {
                    state.tabs.len() - 1
                } else {
                    state.active_tab_index - 1
                };
                state.clear_quick_select();
                state.reset_selection();
            }
            FilterTabMenuOutcome::None
        }
        FilterTabMenuInput::NextWindow => {
            move_selected_window(state, 1);
            FilterTabMenuOutcome::None
        }
        FilterTabMenuInput::PreviousWindow => {
            move_selected_window(state, -1);
            FilterTabMenuOutcome::None
        }
        FilterTabMenuInput::PageDown => {
            move_selected_window(state, 10);
            FilterTabMenuOutcome::None
        }
        FilterTabMenuInput::PageUp => {
            move_selected_window(state, -10);
            FilterTabMenuOutcome::None
        }
        FilterTabMenuInput::FirstWindow => {
            if state.visible {
                state.selected_visible_index = 0;
            }
            FilterTabMenuOutcome::None
        }
        FilterTabMenuInput::LastWindow => {
            if state.visible {
                let len = state.visible_indices().len();
                state.selected_visible_index = len.saturating_sub(1);
            }
            FilterTabMenuOutcome::None
        }
        FilterTabMenuInput::Confirm | FilterTabMenuInput::ModifierRelease => {
            confirm_selected(state)
        }
        FilterTabMenuInput::ToggleQuickSelect => {
            if state.visible {
                state.quick_select_armed = !state.quick_select_armed;
            }
            FilterTabMenuOutcome::None
        }
        FilterTabMenuInput::QuickSelect(label) => quick_select(state, label),
        FilterTabMenuInput::Escape => escape_step(state),
        FilterTabMenuInput::ContextAction(key) => context_action_for_selected(state, key)
            .and_then(|action| command_for_context_action(&action))
            .map(FilterTabMenuOutcome::ExecuteContextCommand)
            .unwrap_or(FilterTabMenuOutcome::None),
    }
}

fn escape_step(state: &mut FilterTabMenuState) -> FilterTabMenuOutcome {
    if !state.visible {
        return FilterTabMenuOutcome::None;
    }

    if state.quick_select_armed {
        state.clear_quick_select();
        return FilterTabMenuOutcome::None;
    }

    if !state.query.is_empty() {
        state.query.clear();
        state.reset_selection();
        return FilterTabMenuOutcome::None;
    }

    state.visible = false;
    state.reset_selection();
    FilterTabMenuOutcome::Cancelled
}

fn confirm_selected(state: &mut FilterTabMenuState) -> FilterTabMenuOutcome {
    if !state.visible {
        return FilterTabMenuOutcome::None;
    }

    let selected = state.selected_window_id();
    state.visible = false;
    state.query.clear();
    state.clear_quick_select();
    state.reset_selection();

    selected
        .map(FilterTabMenuOutcome::ConfirmWindow)
        .unwrap_or(FilterTabMenuOutcome::None)
}

fn quick_select(state: &mut FilterTabMenuState, label: char) -> FilterTabMenuOutcome {
    if !state.visible || !state.quick_select_armed {
        return FilterTabMenuOutcome::None;
    }

    if let Some(window_id) = quick_select_window_id(state, label, DEFAULT_QUICK_SELECT_ALPHABET) {
        state.visible = false;
        state.query.clear();
        state.clear_quick_select();
        state.reset_selection();
        FilterTabMenuOutcome::ConfirmWindow(window_id)
    } else {
        FilterTabMenuOutcome::None
    }
}

pub fn quick_select_window_id(
    state: &FilterTabMenuState,
    label: char,
    alphabet: &str,
) -> Option<u32> {
    let normalized = label.to_ascii_lowercase();
    let row = alphabet
        .chars()
        .position(|candidate| candidate.to_ascii_lowercase() == normalized)?;
    let visible_indices = state.visible_indices();
    let window_index = *visible_indices.get(row)?;
    state.windows.get(window_index).map(|window| window.id)
}

pub fn quick_select_bindings_for_indices(
    windows: &[WindowMatchContext],
    visible_indices: &[usize],
    alphabet: &str,
) -> Vec<(char, u32)> {
    alphabet
        .chars()
        .zip(visible_indices.iter().copied())
        .filter_map(|(label, index)| windows.get(index).map(|window| (label, window.id)))
        .collect()
}

fn move_selected_window(state: &mut FilterTabMenuState, delta: isize) {
    if !state.visible {
        return;
    }

    let len = state.visible_indices().len();
    if len == 0 {
        state.selected_visible_index = 0;
        return;
    }

    if delta == 1 {
        state.selected_visible_index = (state.selected_visible_index + 1) % len;
        return;
    }
    if delta == -1 {
        state.selected_visible_index = if state.selected_visible_index == 0 {
            len - 1
        } else {
            state.selected_visible_index - 1
        };
        return;
    }

    if delta.is_negative() {
        state.selected_visible_index = state
            .selected_visible_index
            .saturating_sub(delta.unsigned_abs());
    } else {
        state.selected_visible_index = (state.selected_visible_index + delta as usize).min(len - 1);
    }
}

fn clamp_selected_index(index: usize, len: usize) -> usize {
    if len == 0 { 0 } else { index.min(len - 1) }
}

pub fn filter_visible_window_indices(
    windows: &[WindowMatchContext],
    tab_filter: &str,
    query: &str,
) -> Vec<usize> {
    windows
        .iter()
        .enumerate()
        .filter_map(|(index, window)| {
            (strict_filter_matches(tab_filter, window) && query_matches(query, window))
                .then_some(index)
        })
        .collect()
}

pub fn query_matches(query: &str, window: &WindowMatchContext) -> bool {
    let tokens = query
        .split_whitespace()
        .map(str::to_ascii_lowercase)
        .collect::<Vec<_>>();

    if tokens.is_empty() {
        return true;
    }

    let haystack = format!(
        "{} {} {} {} {}",
        window.id,
        window
            .native_window_id
            .map(|id| id.to_string())
            .unwrap_or_default(),
        window.title,
        window.class_name.as_deref().unwrap_or_default(),
        window.workspace.as_deref().unwrap_or_default()
    )
    .to_ascii_lowercase();

    tokens.iter().all(|token| haystack.contains(token))
}

pub fn strict_filter_matches(filter: &str, window: &WindowMatchContext) -> bool {
    let filter = filter.trim();
    if filter.is_empty() || filter == "*" {
        return true;
    }

    let title = window.title.trim();
    let class_name = window.class_name.as_deref().unwrap_or_default().trim();

    if let Some(class_filter) = filter.strip_prefix("class:") {
        return class_name.eq_ignore_ascii_case(class_filter.trim());
    }

    if let Some(workspace_filter) = filter.strip_prefix("workspace:") {
        return window
            .workspace
            .as_deref()
            .is_some_and(|workspace| workspace.eq_ignore_ascii_case(workspace_filter.trim()));
    }

    if filter.eq_ignore_ascii_case("urgent") {
        return window.urgent;
    }

    if let Some(contains_filter) = filter.strip_prefix("contains:") {
        let needle = contains_filter.trim().to_ascii_lowercase();
        return title.to_ascii_lowercase().contains(&needle)
            || class_name.to_ascii_lowercase().contains(&needle);
    }

    if title_has_dash_suffix(title, filter) {
        return true;
    }

    class_name.eq_ignore_ascii_case(filter)
}

fn title_has_dash_suffix(title: &str, filter: &str) -> bool {
    let title = title.trim().to_ascii_lowercase();
    let filter = filter.trim().to_ascii_lowercase();
    title.ends_with(&format!(" - {filter}"))
}

pub fn icon_for_window(window: &WindowMatchContext) -> &'static str {
    let haystack = format!(
        "{} {}",
        window.title.to_lowercase(),
        window
            .class_name
            .as_deref()
            .unwrap_or_default()
            .to_lowercase()
    );

    if haystack.contains("firefox")
        || haystack.contains("chrom")
        || haystack.contains("brave")
        || haystack.contains("browser")
    {
        return "🌐";
    }
    if haystack.contains("code")
        || haystack.contains("nvim")
        || haystack.contains("vim")
        || haystack.contains("emacs")
        || haystack.contains("subl")
    {
        return "📝";
    }
    if haystack.contains("tmux")
        || haystack.contains("alacritty")
        || haystack.contains("kitty")
        || haystack.contains("urxvt")
        || haystack.contains("terminal")
    {
        return "🖥";
    }

    "📦"
}

pub fn compute_popup_sizing(
    menu_width: f64,
    menu_height: f64,
    tab_bar_width: f64,
    tab_bar_height: f64,
) -> PopupSizing {
    const TAB_BAR_WIDTH_OVERRIDE_RATIO: f64 = 3.0;

    let menu_width = menu_width.max(1.0);
    let tab_bar_width = tab_bar_width.max(0.0);
    let desired_width = if tab_bar_width > menu_width * TAB_BAR_WIDTH_OVERRIDE_RATIO {
        tab_bar_width
    } else {
        menu_width
    }
    .ceil()
    .max(1.0);

    PopupSizing {
        width: desired_width as u16,
        height: (menu_height + tab_bar_height).ceil().max(1.0) as u16,
    }
}

pub fn default_action_descriptors() -> Vec<ActionDescriptor> {
    vec![
        ActionDescriptor {
            key: 'q',
            title: "Kill Window",
            description: "Close only the selected window",
        },
        ActionDescriptor {
            key: 'a',
            title: "Move To Next Workspace",
            description: "Move selected window to the next free workspace",
        },
        ActionDescriptor {
            key: 's',
            title: "Send To Scratch Workspace",
            description: "Send selected window to scratchpad",
        },
        ActionDescriptor {
            key: ' ',
            title: "Toggle Floating",
            description: "Toggle selected window between floating and tiled",
        },
        ActionDescriptor {
            key: 'r',
            title: "Move Program Instances",
            description: "Move all instances of selected app class to next free workspace",
        },
    ]
}

pub fn context_action_for_selected(
    state: &FilterTabMenuState,
    key: char,
) -> Option<ContextWindowAction> {
    let window = state.selected_window()?;
    match key {
        'q' => Some(ContextWindowAction::KillWindow(window.id)),
        'a' => Some(ContextWindowAction::MoveToNextWorkspace(window.id)),
        's' => Some(ContextWindowAction::SendToScratchWorkspace(window.id)),
        ' ' => Some(ContextWindowAction::ToggleFloating(window.id)),
        'r' => Some(ContextWindowAction::MoveProgramInstancesToNextWorkspace {
            class_name: window
                .class_name
                .clone()
                .unwrap_or_else(|| "Unknown".to_string()),
        }),
        _ => None,
    }
}

pub fn command_for_context_action(action: &ContextWindowAction) -> Option<String> {
    match action {
        ContextWindowAction::KillWindow(window_id) => Some(format!("[con_id={window_id}] kill")),
        ContextWindowAction::MoveToNextWorkspace(window_id) => Some(format!(
            "[con_id={window_id}] move container to workspace next, workspace next"
        )),
        ContextWindowAction::SendToScratchWorkspace(window_id) => {
            Some(format!("[con_id={window_id}] move scratchpad"))
        }
        ContextWindowAction::ToggleFloating(window_id) => {
            Some(format!("[con_id={window_id}] floating toggle"))
        }
        ContextWindowAction::MoveProgramInstancesToNextWorkspace { class_name } => Some(format!(
            "[class=\"(?i)^{}$\"] move container to workspace next",
            escape_i3_regex_literal(class_name)
        )),
    }
}

fn escape_i3_regex_literal(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for ch in value.chars() {
        if matches!(
            ch,
            '.' | '+' | '*' | '?' | '^' | '$' | '(' | ')' | '[' | ']' | '{' | '}' | '|' | '\\'
        ) {
            escaped.push('\\');
        }
        escaped.push(ch);
    }
    escaped
}

pub fn next_free_workspace_number(used_numbers: &[i64]) -> i64 {
    let mut n = 1_i64;
    while used_numbers.contains(&n) {
        n += 1;
    }
    n
}

pub fn default_filter_tabs() -> Vec<FilterTab> {
    vec![
        FilterTab::new('a', "All", "*"),
        FilterTab::new('b', "Browser", "contains:browser"),
        FilterTab::new('t', "Terminal", "contains:terminal"),
        FilterTab::new('e', "Editor", "contains:emacs"),
        FilterTab::new('u', "Urgent", "urgent"),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::key_engine::{EngineBinding, KeyEngine, KeyTrigger};
    use std::time::{Duration, Instant};

    fn window(id: u32, title: &str, class_name: &str) -> WindowMatchContext {
        WindowMatchContext::new(id, title, Some(class_name.to_string()))
    }

    fn sample_state() -> FilterTabMenuState {
        FilterTabMenuState::new(
            vec![
                FilterTab::new('a', "All", "*"),
                FilterTab::new('t', "Tmux", "tmux"),
                FilterTab::new('f', "Firefox", "class:firefox"),
                FilterTab::new('u', "Urgent", "urgent"),
            ],
            vec![
                window(10, "work - tmux", "Alacritty").with_workspace("1"),
                window(11, "tmuxinator docs", "firefox").with_workspace("2"),
                window(12, "mail - firefox", "firefox")
                    .with_workspace("2")
                    .urgent(true),
            ],
        )
    }

    #[test]
    fn strict_filter_matches_dash_suffix_without_false_positive() {
        assert!(strict_filter_matches(
            "tmux",
            &window(1, "work - tmux", "Alacritty")
        ));
        assert!(!strict_filter_matches(
            "tmux",
            &window(2, "tmuxinator docs", "firefox")
        ));
    }

    #[test]
    fn strict_filter_supports_class_workspace_urgent_and_contains_filters() {
        let urgent = window(1, "mail - firefox", "firefox")
            .with_workspace("2")
            .urgent(true);
        assert!(strict_filter_matches("class:firefox", &urgent));
        assert!(strict_filter_matches("workspace:2", &urgent));
        assert!(strict_filter_matches("urgent", &urgent));
        assert!(strict_filter_matches("contains:mail", &urgent));
        assert!(!strict_filter_matches("class:Alacritty", &urgent));
    }

    #[test]
    fn query_filters_all_tokens_across_title_class_and_workspace() {
        let windows = vec![
            window(10, "work - tmux", "Alacritty").with_workspace("1"),
            window(11, "docs", "firefox").with_workspace("2"),
        ];
        assert_eq!(
            filter_visible_window_indices(&windows, "*", "work alacritty"),
            vec![0]
        );
        assert_eq!(
            filter_visible_window_indices(&windows, "*", "firefox 2"),
            vec![1]
        );
    }

    #[test]
    fn tab_selection_recomputes_visible_rows_and_resets_selection() {
        let mut state = sample_state();
        handle_filter_tab_input(&mut state, FilterTabMenuInput::Activate);
        handle_filter_tab_input(&mut state, FilterTabMenuInput::NextWindow);
        assert_eq!(state.selected_window_id(), Some(11));

        handle_filter_tab_input(&mut state, FilterTabMenuInput::SelectTab('t'));
        assert_eq!(state.active_tab().map(|tab| tab.key), Some('t'));
        assert_eq!(state.selected_visible_index, 0);
        assert_eq!(state.selected_window_id(), Some(10));
    }

    #[test]
    fn modifier_release_confirms_selected_window_like_return_confirm() {
        let mut release_state = sample_state();
        handle_filter_tab_input(&mut release_state, FilterTabMenuInput::Activate);
        handle_filter_tab_input(&mut release_state, FilterTabMenuInput::SelectTab('f'));
        handle_filter_tab_input(&mut release_state, FilterTabMenuInput::NextWindow);
        let release_out =
            handle_filter_tab_input(&mut release_state, FilterTabMenuInput::ModifierRelease);

        let mut confirm_state = sample_state();
        handle_filter_tab_input(&mut confirm_state, FilterTabMenuInput::Activate);
        handle_filter_tab_input(&mut confirm_state, FilterTabMenuInput::SelectTab('f'));
        handle_filter_tab_input(&mut confirm_state, FilterTabMenuInput::NextWindow);
        let confirm_out = handle_filter_tab_input(&mut confirm_state, FilterTabMenuInput::Confirm);

        assert_eq!(release_out, confirm_out);
        assert_eq!(release_out, FilterTabMenuOutcome::ConfirmWindow(12));
        assert!(!release_state.visible);
        assert!(!confirm_state.visible);
    }

    #[test]
    fn modrelease_key_engine_can_drive_filter_tab_confirm() {
        let mut engine = KeyEngine::new(vec![EngineBinding::new(
            "filter-tab".to_string(),
            vec![vec!["KEY_LEFTMETA".to_string()], vec!["KEY_U".to_string()]],
            KeyTrigger::Modrelease,
            100,
            true,
            50,
            None,
            vec!["KEY_U".to_string()],
        )]);
        let mut state = sample_state();
        handle_filter_tab_input(&mut state, FilterTabMenuInput::Activate);
        handle_filter_tab_input(&mut state, FilterTabMenuInput::SelectTab('f'));
        handle_filter_tab_input(&mut state, FilterTabMenuInput::NextWindow);

        let now = Instant::now();
        assert!(
            engine
                .process_event("KEY_LEFTMETA", 1, now)
                .triggered
                .is_empty()
        );
        assert!(
            engine
                .process_event("KEY_U", 1, now + Duration::from_millis(1))
                .triggered
                .is_empty()
        );
        let output = engine.process_event("KEY_LEFTMETA", 0, now + Duration::from_millis(80));
        assert_eq!(output.triggered, vec![0]);

        let outcome = handle_filter_tab_input(&mut state, FilterTabMenuInput::ModifierRelease);
        assert_eq!(outcome, FilterTabMenuOutcome::ConfirmWindow(12));
    }

    #[test]
    fn non_modifier_release_does_not_confirm_through_key_engine() {
        let mut engine = KeyEngine::new(vec![EngineBinding::new(
            "filter-tab".to_string(),
            vec![vec!["KEY_LEFTMETA".to_string()], vec!["KEY_U".to_string()]],
            KeyTrigger::Modrelease,
            100,
            true,
            50,
            None,
            vec!["KEY_U".to_string()],
        )]);
        let now = Instant::now();
        engine.process_event("KEY_LEFTMETA", 1, now);
        engine.process_event("KEY_U", 1, now + Duration::from_millis(1));
        let output = engine.process_event("KEY_U", 0, now + Duration::from_millis(80));
        assert!(output.triggered.is_empty());
    }

    #[test]
    fn escape_clears_query_before_cancelling() {
        let mut state = sample_state();
        handle_filter_tab_input(&mut state, FilterTabMenuInput::Activate);
        handle_filter_tab_input(&mut state, FilterTabMenuInput::PushQueryChar('x'));

        let first = handle_filter_tab_input(&mut state, FilterTabMenuInput::Escape);
        assert_eq!(first, FilterTabMenuOutcome::None);
        assert!(state.visible);
        assert!(state.query.is_empty());

        let second = handle_filter_tab_input(&mut state, FilterTabMenuInput::Escape);
        assert_eq!(second, FilterTabMenuOutcome::Cancelled);
        assert!(!state.visible);
    }

    #[test]
    fn escape_disarms_quick_select_before_query_and_close() {
        let mut state = sample_state();
        handle_filter_tab_input(&mut state, FilterTabMenuInput::Activate);
        handle_filter_tab_input(&mut state, FilterTabMenuInput::PushQueryChar('x'));
        handle_filter_tab_input(&mut state, FilterTabMenuInput::ToggleQuickSelect);

        let first = handle_filter_tab_input(&mut state, FilterTabMenuInput::Escape);
        assert_eq!(first, FilterTabMenuOutcome::None);
        assert!(state.visible);
        assert!(!state.quick_select_armed);
        assert_eq!(state.query, "x");

        let second = handle_filter_tab_input(&mut state, FilterTabMenuInput::Escape);
        assert_eq!(second, FilterTabMenuOutcome::None);
        assert!(state.visible);
        assert!(state.query.is_empty());

        let third = handle_filter_tab_input(&mut state, FilterTabMenuInput::Escape);
        assert_eq!(third, FilterTabMenuOutcome::Cancelled);
        assert!(!state.visible);
    }

    #[test]
    fn navigation_clamps_to_visible_bounds() {
        let mut state = sample_state();
        handle_filter_tab_input(&mut state, FilterTabMenuInput::Activate);
        handle_filter_tab_input(&mut state, FilterTabMenuInput::PageDown);
        assert_eq!(state.selected_visible_index, 2);
        handle_filter_tab_input(&mut state, FilterTabMenuInput::PageUp);
        assert_eq!(state.selected_visible_index, 0);
        handle_filter_tab_input(&mut state, FilterTabMenuInput::LastWindow);
        assert_eq!(state.selected_visible_index, 2);
        handle_filter_tab_input(&mut state, FilterTabMenuInput::FirstWindow);
        assert_eq!(state.selected_visible_index, 0);
    }

    #[test]
    fn context_action_uses_selected_window_and_builds_i3_command() {
        let mut state = sample_state();
        handle_filter_tab_input(&mut state, FilterTabMenuInput::Activate);
        handle_filter_tab_input(&mut state, FilterTabMenuInput::SelectTab('f'));
        let out = handle_filter_tab_input(&mut state, FilterTabMenuInput::ContextAction('q'));
        assert_eq!(
            out,
            FilterTabMenuOutcome::ExecuteContextCommand("[con_id=11] kill".into())
        );
    }

    #[test]
    fn context_action_escapes_class_regex_literals() {
        let action = ContextWindowAction::MoveProgramInstancesToNextWorkspace {
            class_name: "foo.bar+term".to_string(),
        };
        assert_eq!(
            command_for_context_action(&action),
            Some("[class=\"(?i)^foo\\.bar\\+term$\"] move container to workspace next".into())
        );
    }

    #[test]
    fn icon_for_window_prefers_common_roles() {
        assert_eq!(
            icon_for_window(&window(1, "Mozilla Firefox", "firefox")),
            "🌐"
        );
        assert_eq!(icon_for_window(&window(2, "work - tmux", "Alacritty")), "🖥");
        assert_eq!(icon_for_window(&window(3, "notes", "Emacs")), "📝");
    }

    #[test]
    fn popup_sizing_ignores_pathologically_wide_tabbar() {
        assert_eq!(
            compute_popup_sizing(250.0, 200.0, 600.0, 30.0),
            PopupSizing {
                width: 250,
                height: 230
            }
        );
        assert_eq!(
            compute_popup_sizing(460.0, 300.0, 380.0, 30.0),
            PopupSizing {
                width: 460,
                height: 330
            }
        );
    }

    #[test]
    fn next_free_workspace_number_finds_first_gap() {
        assert_eq!(next_free_workspace_number(&[1, 2, 4]), 3);
        assert_eq!(next_free_workspace_number(&[2, 3, 4]), 1);
        assert_eq!(next_free_workspace_number(&[]), 1);
    }

    #[test]
    fn quick_select_bindings_use_home_row_for_visible_windows() {
        let mut state = sample_state();
        handle_filter_tab_input(&mut state, FilterTabMenuInput::Activate);
        let bindings = state.quick_select_bindings();
        assert_eq!(bindings, vec![('a', 10), ('s', 11), ('d', 12)]);
    }

    #[test]
    fn ctrl_q_quick_select_confirms_home_row_target_and_clears_mode() {
        let mut state = sample_state();
        handle_filter_tab_input(&mut state, FilterTabMenuInput::Activate);
        handle_filter_tab_input(&mut state, FilterTabMenuInput::ToggleQuickSelect);
        assert!(state.quick_select_armed);

        let out = handle_filter_tab_input(&mut state, FilterTabMenuInput::QuickSelect('s'));
        assert_eq!(out, FilterTabMenuOutcome::ConfirmWindow(11));
        assert!(!state.visible);
        assert!(!state.quick_select_armed);
    }

    #[test]
    fn quick_select_ignores_unmapped_key_and_stays_armed() {
        let mut state = sample_state();
        handle_filter_tab_input(&mut state, FilterTabMenuInput::Activate);
        handle_filter_tab_input(&mut state, FilterTabMenuInput::SelectTab('t'));
        handle_filter_tab_input(&mut state, FilterTabMenuInput::ToggleQuickSelect);

        let out = handle_filter_tab_input(&mut state, FilterTabMenuInput::QuickSelect('s'));
        assert_eq!(out, FilterTabMenuOutcome::None);
        assert!(state.visible);
        assert!(state.quick_select_armed);
        assert_eq!(state.selected_window_id(), Some(10));
    }

    #[test]
    fn native_window_id_participates_in_query_matching() {
        let window = window(10, "work - tmux", "Alacritty").with_native_window_id(424242);
        assert!(query_matches("424242", &window));
    }

    #[test]
    fn single_step_navigation_wraps_while_page_navigation_clamps() {
        let mut state = sample_state();
        handle_filter_tab_input(&mut state, FilterTabMenuInput::Activate);
        handle_filter_tab_input(&mut state, FilterTabMenuInput::PreviousWindow);
        assert_eq!(state.selected_visible_index, 2);
        handle_filter_tab_input(&mut state, FilterTabMenuInput::NextWindow);
        assert_eq!(state.selected_visible_index, 0);
        handle_filter_tab_input(&mut state, FilterTabMenuInput::PageDown);
        assert_eq!(state.selected_visible_index, 2);
        handle_filter_tab_input(&mut state, FilterTabMenuInput::PageUp);
        assert_eq!(state.selected_visible_index, 0);
    }
}
