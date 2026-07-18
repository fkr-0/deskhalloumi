//! Renderer-neutral i3 workspace tree visualization model.
//!
//! `i3-vis` is a mod-release menu: it starts with the i3-focused window selected,
//! lets the user move through selectable window leaves, and confirms the current
//! selection on explicit confirm or first modifier release.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum I3VisNodeKind {
    Workspace,
    SplitVertical,
    SplitHorizontal,
    Tabbed,
    Stacked,
    Container,
    Floating,
    Window,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct I3VisNode {
    pub id: u64,
    pub label: String,
    pub kind: I3VisNodeKind,
    pub startup_focused: bool,
    pub selectable: bool,
    pub children: Vec<I3VisNode>,
}

impl I3VisNode {
    pub fn new(id: u64, label: impl Into<String>, kind: I3VisNodeKind) -> Self {
        Self {
            id,
            label: label.into(),
            kind,
            startup_focused: false,
            selectable: false,
            children: Vec::new(),
        }
    }

    pub fn window(id: u64, label: impl Into<String>) -> Self {
        Self {
            id,
            label: label.into(),
            kind: I3VisNodeKind::Window,
            startup_focused: false,
            selectable: true,
            children: Vec::new(),
        }
    }

    pub fn with_startup_focused(mut self, startup_focused: bool) -> Self {
        self.startup_focused = startup_focused;
        self
    }

    pub fn with_selectable(mut self, selectable: bool) -> Self {
        self.selectable = selectable;
        self
    }

    pub fn with_children(mut self, children: Vec<I3VisNode>) -> Self {
        self.children = children;
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct I3VisState {
    pub visible: bool,
    pub root: Option<I3VisNode>,
    pub selected_window_id: Option<u64>,
    pub startup_focused_window_id: Option<u64>,
}

impl I3VisState {
    pub fn new(root: Option<I3VisNode>) -> Self {
        let startup_focused_window_id = root.as_ref().and_then(startup_focused_window_id);
        let selected_window_id =
            startup_focused_window_id.or_else(|| root.as_ref().and_then(first_window_id));
        Self {
            visible: false,
            root,
            selected_window_id,
            startup_focused_window_id,
        }
    }

    pub fn activate(&mut self) {
        self.visible = true;
        if self.selected_window_id.is_none() {
            self.selected_window_id = self
                .startup_focused_window_id
                .or_else(|| self.root.as_ref().and_then(first_window_id));
        }
    }

    pub fn set_root(&mut self, root: Option<I3VisNode>) {
        self.root = root;
        self.startup_focused_window_id = self.root.as_ref().and_then(startup_focused_window_id);
        self.selected_window_id = self
            .startup_focused_window_id
            .or_else(|| self.root.as_ref().and_then(first_window_id));
    }

    pub fn selectable_window_ids(&self) -> Vec<u64> {
        self.root
            .as_ref()
            .map(selectable_window_ids)
            .unwrap_or_default()
    }

    pub fn rows(&self) -> Vec<I3VisRow> {
        self.root
            .as_ref()
            .map(|root| flatten_rows(root, self.selected_window_id, 0, true, &mut Vec::new()))
            .unwrap_or_default()
    }

    pub fn selected_label(&self) -> Option<String> {
        self.root
            .as_ref()
            .and_then(|root| find_node(root, self.selected_window_id?))
            .map(|node| node.label.clone())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct I3VisRow {
    pub id: u64,
    pub depth: usize,
    pub label: String,
    pub kind: I3VisNodeKind,
    pub selected: bool,
    pub startup_focused: bool,
    pub selectable: bool,
    pub is_last: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum I3VisInput {
    Activate,
    Next,
    Previous,
    First,
    Last,
    Confirm,
    ModifierRelease,
    Escape,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum I3VisOutcome {
    None,
    Cancelled,
    ConfirmWindow(u64),
}

pub fn handle_i3_vis_input(state: &mut I3VisState, input: I3VisInput) -> I3VisOutcome {
    match input {
        I3VisInput::Activate => {
            state.activate();
            I3VisOutcome::None
        }
        I3VisInput::Next => {
            move_selection(state, 1);
            I3VisOutcome::None
        }
        I3VisInput::Previous => {
            move_selection(state, -1);
            I3VisOutcome::None
        }
        I3VisInput::First => {
            state.selected_window_id = state.selectable_window_ids().first().copied();
            I3VisOutcome::None
        }
        I3VisInput::Last => {
            state.selected_window_id = state.selectable_window_ids().last().copied();
            I3VisOutcome::None
        }
        I3VisInput::Confirm | I3VisInput::ModifierRelease => {
            if !state.visible {
                return I3VisOutcome::None;
            }
            state.visible = false;
            state
                .selected_window_id
                .map(I3VisOutcome::ConfirmWindow)
                .unwrap_or(I3VisOutcome::None)
        }
        I3VisInput::Escape => {
            state.visible = false;
            I3VisOutcome::Cancelled
        }
    }
}

fn move_selection(state: &mut I3VisState, delta: isize) {
    if !state.visible {
        return;
    }
    let ids = state.selectable_window_ids();
    if ids.is_empty() {
        state.selected_window_id = None;
        return;
    }
    let current_index = state
        .selected_window_id
        .and_then(|id| ids.iter().position(|candidate| *candidate == id))
        .unwrap_or(0);
    let next_index = if delta.is_negative() {
        current_index.saturating_sub(delta.unsigned_abs())
    } else {
        (current_index + delta as usize).min(ids.len() - 1)
    };
    state.selected_window_id = Some(ids[next_index]);
}

pub fn selectable_window_ids(root: &I3VisNode) -> Vec<u64> {
    let mut ids = Vec::new();
    collect_selectable_window_ids(root, &mut ids);
    ids
}

fn collect_selectable_window_ids(node: &I3VisNode, ids: &mut Vec<u64>) {
    if node.selectable {
        ids.push(node.id);
    }
    for child in &node.children {
        collect_selectable_window_ids(child, ids);
    }
}

pub fn first_window_id(root: &I3VisNode) -> Option<u64> {
    selectable_window_ids(root).first().copied()
}

pub fn startup_focused_window_id(root: &I3VisNode) -> Option<u64> {
    if root.startup_focused && root.selectable {
        return Some(root.id);
    }
    root.children.iter().find_map(startup_focused_window_id)
}

fn find_node(root: &I3VisNode, id: u64) -> Option<&I3VisNode> {
    if root.id == id {
        return Some(root);
    }
    root.children.iter().find_map(|child| find_node(child, id))
}

fn flatten_rows(
    node: &I3VisNode,
    selected_window_id: Option<u64>,
    depth: usize,
    is_last: bool,
    rows: &mut Vec<I3VisRow>,
) -> Vec<I3VisRow> {
    rows.push(I3VisRow {
        id: node.id,
        depth,
        label: node.label.clone(),
        kind: node.kind,
        selected: node.selectable && selected_window_id == Some(node.id),
        startup_focused: node.startup_focused,
        selectable: node.selectable,
        is_last,
    });
    for (index, child) in node.children.iter().enumerate() {
        let child_is_last = index + 1 == node.children.len();
        flatten_rows(child, selected_window_id, depth + 1, child_is_last, rows);
    }
    rows.clone()
}

pub fn kind_icon(kind: I3VisNodeKind) -> &'static str {
    match kind {
        I3VisNodeKind::Workspace => "◇",
        I3VisNodeKind::SplitVertical => "↕",
        I3VisNodeKind::SplitHorizontal => "↔",
        I3VisNodeKind::Tabbed => "▣",
        I3VisNodeKind::Stacked => "▤",
        I3VisNodeKind::Container => "□",
        I3VisNodeKind::Floating => "◌",
        I3VisNodeKind::Window => "▢",
    }
}

pub fn kind_label(kind: I3VisNodeKind) -> &'static str {
    match kind {
        I3VisNodeKind::Workspace => "workspace",
        I3VisNodeKind::SplitVertical => "vertical",
        I3VisNodeKind::SplitHorizontal => "horizontal",
        I3VisNodeKind::Tabbed => "tabbed",
        I3VisNodeKind::Stacked => "stacked",
        I3VisNodeKind::Container => "container",
        I3VisNodeKind::Floating => "floating",
        I3VisNodeKind::Window => "window",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_tree() -> I3VisNode {
        I3VisNode::new(2, "workspace 2", I3VisNodeKind::Workspace).with_children(vec![
            I3VisNode::new(20, "2 vertical", I3VisNodeKind::SplitVertical).with_children(vec![
                I3VisNode::window(21, "emacs"),
                I3VisNode::new(22, "2 horizontal", I3VisNodeKind::SplitHorizontal).with_children(
                    vec![
                        I3VisNode::window(23, "firefox").with_startup_focused(true),
                        I3VisNode::window(24, "xterm"),
                    ],
                ),
            ]),
        ])
    }

    #[test]
    fn starts_selected_on_startup_focused_window() {
        let mut state = I3VisState::new(Some(sample_tree()));
        handle_i3_vis_input(&mut state, I3VisInput::Activate);
        assert_eq!(state.startup_focused_window_id, Some(23));
        assert_eq!(state.selected_window_id, Some(23));
        assert!(state.visible);
    }

    #[test]
    fn rows_represent_structural_and_selectable_nodes() {
        let mut state = I3VisState::new(Some(sample_tree()));
        handle_i3_vis_input(&mut state, I3VisInput::Activate);
        let rows = state.rows();
        assert_eq!(
            rows.iter()
                .map(|row| row.label.as_str())
                .collect::<Vec<_>>(),
            vec![
                "workspace 2",
                "2 vertical",
                "emacs",
                "2 horizontal",
                "firefox",
                "xterm",
            ]
        );
        assert!(!rows[0].selectable);
        assert!(rows[4].selected);
        assert!(rows[4].startup_focused);
    }

    #[test]
    fn navigation_moves_between_windows_only() {
        let mut state = I3VisState::new(Some(sample_tree()));
        handle_i3_vis_input(&mut state, I3VisInput::Activate);
        handle_i3_vis_input(&mut state, I3VisInput::Next);
        assert_eq!(state.selected_window_id, Some(24));
        handle_i3_vis_input(&mut state, I3VisInput::Previous);
        handle_i3_vis_input(&mut state, I3VisInput::Previous);
        assert_eq!(state.selected_window_id, Some(21));
    }

    #[test]
    fn modifier_release_confirms_current_window() {
        let mut state = I3VisState::new(Some(sample_tree()));
        handle_i3_vis_input(&mut state, I3VisInput::Activate);
        handle_i3_vis_input(&mut state, I3VisInput::Next);
        let out = handle_i3_vis_input(&mut state, I3VisInput::ModifierRelease);
        assert_eq!(out, I3VisOutcome::ConfirmWindow(24));
        assert!(!state.visible);
    }

    #[test]
    fn escape_cancels_without_confirming() {
        let mut state = I3VisState::new(Some(sample_tree()));
        handle_i3_vis_input(&mut state, I3VisInput::Activate);
        let out = handle_i3_vis_input(&mut state, I3VisInput::Escape);
        assert_eq!(out, I3VisOutcome::Cancelled);
        assert!(!state.visible);
    }

    #[test]
    fn set_root_reselects_new_startup_focus() {
        let mut state = I3VisState::new(Some(sample_tree()));
        handle_i3_vis_input(&mut state, I3VisInput::Activate);
        let root = I3VisNode::new(3, "workspace 3", I3VisNodeKind::Workspace).with_children(vec![
            I3VisNode::window(31, "terminal").with_startup_focused(true),
        ]);
        state.set_root(Some(root));
        assert_eq!(state.selected_window_id, Some(31));
        assert_eq!(state.startup_focused_window_id, Some(31));
    }
}
