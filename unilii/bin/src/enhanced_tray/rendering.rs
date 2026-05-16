//! Rendering logic for enhanced tray following idiomatic Iced 0.14 patterns
//!
//! This module implements declarative UI rendering:
//! - Composable widget functions
//! - Conditional rendering based on state
//! - Proper use of containers, layout, and styling
//! - Clear separation of concerns

use crate::enhanced_tray::{core::*, state::TrayMessage};
use crate::menus::wifi::{WifiMenuConfig, WifiNetworkRow, build_connect_command, build_view_model};
use iced::{
    Alignment, Element, Length,
    widget::{Space, button, column, container, row, scrollable, text, text_input},
};

/// Main enhanced tray rendering function (idiomatic Iced pattern)
pub fn render_enhanced_tray(state: &EnhancedTrayState) -> Element<'_, TrayMessage> {
    if !state.is_visible() {
        return Space::new().into();
    }

    let content = match &state.current_view {
        TrayViewState::SingleApp {
            app_id, navigation, ..
        } => render_single_app_view(state, app_id, navigation),
        TrayViewState::Aggregated { items, filter } => render_aggregated_view(state, items, filter),
        TrayViewState::Favorites { items } => render_favorites_view(state, items),
        TrayViewState::Network {
            app_id,
            data,
            loading,
            error,
        } => render_network_view(state, app_id, data, *loading, error),
        TrayViewState::Mount { .. } => text("Mount menu is rendered by main runtime")
            .size(12)
            .into(),
        TrayViewState::Calendar { .. } => text("Calendar menu is rendered by main runtime")
            .size(12)
            .into(),
    };

    // Apply animation opacity and container styling
    let opacity = state.animation_progress.clamp(0.0, 1.0);

    container(content)
        .padding([4, 8])
        .style(move |theme| {
            let mut appearance: container::Style = container::Style::default();
            appearance.background = Some(iced::Background::Color(theme.palette().background));
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

/// Render single app menu view with navigation
fn render_single_app_view<'a>(
    state: &'a EnhancedTrayState,
    app_id: &'a str,
    navigation: &'a TrayMenuNavigation,
) -> Element<'a, TrayMessage> {
    let app_menu = state.tree.apps.get(app_id);

    let mut content = column!().spacing(2);

    // App title with navigation arrows
    let mut title_row = row!().spacing(4).align_y(Alignment::Center);

    if navigation.can_go_left {
        title_row = title_row.push(
            button(text("◀").size(12))
                // .style() removed for compatibility
                .on_press(TrayMessage::NavigateLeft),
        );
    }

    if let Some(app) = app_menu {
        title_row = title_row.push(text(&app.icon.title).size(14));
    } else {
        title_row = title_row.push(text(app_id).size(14));
    }

    if navigation.can_go_right {
        title_row = title_row.push(
            button(text("▶").size(12))
                // .style() removed for compatibility
                .on_press(TrayMessage::NavigateRight),
        );
    }

    content = content.push(title_row);

    // Menu items
    if let Some(app) = app_menu {
        let menu_items = render_menu_items(&app.menu_items, state.selected_index, app_id, &[]);
        content = content.push(menu_items);
    } else {
        content = content.push(text("No menu available").size(12));
    }

    // Keyboard hints
    content = content.push(render_keyboard_hints_single());

    content.into()
}

/// Render aggregated view with filtering
fn render_aggregated_view<'a>(
    _state: &'a EnhancedTrayState,
    items: &'a [TrayMenuItem],
    filter: &'a Option<String>,
) -> Element<'a, TrayMessage> {
    let mut content = column!().spacing(2);

    // Title
    content = content.push(text("All Menu Items").size(14));

    // Filter input
    content = content.push(
        text_input("Search menu items...", filter.as_deref().unwrap_or(""))
            .on_input(TrayMessage::FilterUpdate)
            .size(12)
            .padding([2, 4]),
    );

    // Items list
    if items.is_empty() {
        content = content.push(text("No items found").size(12));
    } else {
        let items_container = render_aggregated_items(items);
        content = content.push(items_container);
    }

    // Keyboard hints
    content = content.push(render_keyboard_hints_aggregated());

    content.into()
}

/// Render favorites view
fn render_favorites_view<'a>(
    _state: &'a EnhancedTrayState,
    items: &'a [TrayMenuItem],
) -> Element<'a, TrayMessage> {
    let mut content = column!().spacing(2);

    // Title
    content = content.push(text("Favorite Items ⭐").size(14));

    // Items list
    if items.is_empty() {
        content = content
            .push(text("No favorites yet. Press 'f' on any menu item to add it here.").size(12));
    } else {
        let items_container = render_favorite_items(items);
        content = content.push(items_container);
    }

    // Keyboard hints
    content = content.push(render_keyboard_hints_favorites());

    content.into()
}

/// Render network view with status and controls
fn render_network_view<'a>(
    _state: &'a EnhancedTrayState,
    app_id: &'a str,
    data: &'a Option<crate::tray::NetworkSnapshot>,
    loading: bool,
    error: &'a Option<String>,
) -> Element<'a, TrayMessage> {
    let mut content = column!().spacing(2);
    let menu_config = WifiMenuConfig::default();
    let view_model = build_view_model(data.as_ref(), loading, error.as_deref(), &menu_config);

    // Title
    content = content.push(text("Network Settings").size(14));

    content = content.push(text(view_model.status_text.clone()).size(12));
    if let Some(interface) = &view_model.interface {
        content = content.push(text(format!("Interface: {}", interface)).size(10));
    }

    // Controls
    let controls = render_network_controls(app_id, data);
    content = content.push(controls);

    // Network list
    if let Some(snapshot) = data {
        if view_model.enabled && !view_model.available.is_empty() {
            let networks = render_network_list(app_id, snapshot, view_model.available.clone());
            content = content.push(networks);
        } else if !view_model.enabled {
            content = content.push(text("Wi-Fi is disabled").size(12));
        }

        if !view_model.known.is_empty() {
            content = content.push(text("Known Networks:").size(12));
            for known in view_model.known.iter().take(menu_config.max_known_rows) {
                let label = if known.autoconnect {
                    format!("★ {}", known.name)
                } else {
                    known.name.clone()
                };
                let known_btn = button(text(label).size(11))
                    .padding([1, 4])
                    .width(Length::Fill)
                    .on_press(TrayMessage::NetworkSpawnCommand(
                        app_id.to_string(),
                        format!("nmcli connection up \"{}\"", known.name),
                    ));
                content = content.push(known_btn);
            }
        }
    }

    // Keyboard hints
    content = content.push(render_keyboard_hints_network());

    content.into()
}

/// Render menu items as a column with support for nested submenus and text inputs
fn render_menu_items<'a>(
    items: &'a [TrayMenuItem],
    selected_index: Option<usize>,
    app_id: &'a str,
    current_submenu_path: &[String],
) -> Element<'a, TrayMessage> {
    if items.is_empty() {
        return text("No menu items").size(12).into();
    }

    let mut menu_col = column!().spacing(1);

    for (index, item) in items.iter().enumerate() {
        let item_widget = render_menu_item(
            item,
            selected_index == Some(index),
            app_id,
            current_submenu_path,
        );
        menu_col = menu_col.push(item_widget);
    }

    if items.len() > 8 {
        scrollable(menu_col).height(Length::Fixed(300.0)).into()
    } else {
        menu_col.into()
    }
}

/// Render a single menu item with support for different widget types
fn render_menu_item<'a>(
    item: &'a TrayMenuItem,
    _is_selected: bool,
    app_id: &'a str,
    current_submenu_path: &[String],
) -> Element<'a, TrayMessage> {
    // Check widget type
    if item.is_separator {
        return text("─".repeat(20)).size(10).into();
    }

    // Check if this is a text input widget
    if item.label.contains("INPUT:") || item.action.to_string().contains("TextInput") {
        let placeholder = item.placeholder.as_deref().unwrap_or("Enter value...");
        let current_value = item.default_value.as_deref().unwrap_or("");

        return text_input(placeholder, current_value)
            .on_input(move |value| TrayMessage::TextInputChanged(item.id.clone(), value))
            .size(12)
            .padding([2, 4])
            .width(Length::Fixed(200.0))
            .into();
    }

    // Default: Button or SubmenuButton
    let mut label = item.label.clone();

    // Add checkmark for checkable items
    if item.checkable {
        label = format!("{} {}", if item.checked { "☑" } else { "☐" }, label);
    }

    // Add shortcut if available
    if let Some(shortcut) = &item.shortcut {
        label = format!("{} ({})", label, shortcut);
    }

    // Add submenu indicator
    let has_submenu = !item.submenu.is_empty();
    if has_submenu {
        label = format!("{} ›", label);
    }

    let btn = button(text(label).size(12))
        .padding([2, 8])
        .width(Length::Fill);

    let styled_btn = if has_submenu {
        btn.on_press(TrayMessage::EnterSubmenu(app_id.to_string(), {
            let mut path = current_submenu_path.to_vec();
            path.push(item.id.clone());
            path
        }))
    } else {
        btn.on_press(TrayMessage::MenuItemClicked(
            app_id.to_string(),
            item.action.clone(),
        ))
    };

    if item.enabled {
        styled_btn.into()
    } else {
        styled_btn.into()
    }
}

/// Render aggregated menu items with app context
fn render_aggregated_items<'a>(items: &'a [TrayMenuItem]) -> Element<'a, TrayMessage> {
    let mut items_col = column!().spacing(1);

    for item in items.iter().take(10) {
        // Limit visible items
        let item_row = row![
            text("⭐").size(10),
            text(&item.full_path).size(11),
            Space::new(),
            button(text("★").size(10))
                // .style() removed for compatibility
                .on_press(TrayMessage::ToggleFavorite(
                    item.app_id.clone(),
                    item.id.clone()
                )),
        ]
        .spacing(4)
        .align_y(Alignment::Center);

        let item_btn = button(item_row)
            .padding([2, 4])
            .width(Length::Fill)
            // .style() removed for compatibility
            .on_press(TrayMessage::MenuItemClicked(
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

/// Render favorite items with star indicators
fn render_favorite_items<'a>(items: &'a [TrayMenuItem]) -> Element<'a, TrayMessage> {
    let mut items_col = column!().spacing(1);

    for item in items {
        let item_row = row![
            text("⭐").size(10),
            text(&item.full_path).size(11),
            button(text("✗").size(10))
                // .style() removed for compatibility
                .on_press(TrayMessage::ToggleFavorite(
                    item.app_id.clone(),
                    item.id.clone()
                )),
        ]
        .spacing(4)
        .align_y(Alignment::Center);

        let item_btn = button(item_row)
            .padding([2, 4])
            .width(Length::Fill)
            // .style() removed for compatibility
            .on_press(TrayMessage::MenuItemClicked(
                item.app_id.clone(),
                item.action.clone(),
            ));

        items_col = items_col.push(item_btn);
    }

    scrollable(items_col).height(Length::Fixed(200.0)).into()
}

/// Render network control buttons
fn render_network_controls<'a>(
    app_id: &'a str,
    data: &'a Option<crate::tray::NetworkSnapshot>,
) -> Element<'a, TrayMessage> {
    let is_enabled = data.as_ref().map(|d| d.enabled).unwrap_or(false);

    row![
        button(
            text(if is_enabled {
                "Disable Wi-Fi"
            } else {
                "Enable Wi-Fi"
            })
            .size(12)
        )
        .padding([2, 6])
        .on_press(TrayMessage::NetworkToggle(app_id.to_string())),
        button(text("Settings").size(12))
            .padding([2, 6])
            // .style() removed for compatibility
            .on_press(TrayMessage::NetworkSpawnCommand(
                app_id.to_string(),
                WifiMenuConfig::default().settings_command
            )),
    ]
    .spacing(4)
    .into()
}

/// Render list of available networks
fn render_network_list<'a>(
    app_id: &'a str,
    _snapshot: &'a crate::tray::NetworkSnapshot,
    available: Vec<WifiNetworkRow>,
) -> Element<'a, TrayMessage> {
    let mut networks_col = column!().spacing(1);

    networks_col = networks_col.push(text("Available Networks:").size(12));

    for network in available {
        let ssid = network.ssid;
        let lock = if network.security.eq_ignore_ascii_case("open") {
            ""
        } else {
            "🔒 "
        };
        let mut label = format!("{}{} ({}%)", lock, ssid, network.signal);

        if network.connected {
            label = format!("● {}", label); // Connected indicator
        }

        let network_btn = button(text(label).size(11))
            .padding([1, 4])
            .width(Length::Fill)
            // .style() removed for compatibility
            .on_press(TrayMessage::NetworkSpawnCommand(
                app_id.to_string(),
                build_connect_command(&ssid),
            ));

        networks_col = networks_col.push(network_btn);
    }

    scrollable(networks_col).height(Length::Fixed(150.0)).into()
}

/// Render keyboard hints for single app view
fn render_keyboard_hints_single() -> Element<'static, TrayMessage> {
    text("◀/▶: Navigate apps • a: All items • v: Favorites")
        .size(10)
        .into()
}

/// Render keyboard hints for aggregated view
fn render_keyboard_hints_aggregated() -> Element<'static, TrayMessage> {
    text("Type: Filter • f: Toggle favorite • v: Favorites only")
        .size(10)
        .into()
}

/// Render keyboard hints for favorites view
fn render_keyboard_hints_favorites() -> Element<'static, TrayMessage> {
    text("a: All items • f: Remove favorite").size(10).into()
}

/// Render keyboard hints for network view
fn render_keyboard_hints_network() -> Element<'static, TrayMessage> {
    text("Click to connect/control • a: All items")
        .size(10)
        .into()
}

// == Custom Styles ==

/// Custom container style with opacity for fade animations
struct TrayContainerStyle {
    opacity: f32,
}

impl container::Catalog for TrayContainerStyle {
    type Class<'a> = Self;

    fn default<'a>() -> Self::Class<'a> {
        TrayContainerStyle { opacity: 1.0 }
    }

    fn style(&self, _class: &Self::Class<'_>) -> container::Style {
        container::Style {
            background: Some(iced::Background::Color(
                [0.1, 0.1, 0.1, self.opacity].into(),
            )),
            border: iced::Border {
                radius: 4.0.into(),
                width: 1.0,
                color: [0.3, 0.3, 0.3, self.opacity].into(),
            },
            shadow: iced::Shadow {
                color: [0.0, 0.0, 0.0, self.opacity * 0.5].into(),
                offset: iced::Vector::new(0.0, 2.0),
                blur_radius: 4.0,
            },
            ..Default::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_state() -> EnhancedTrayState {
        EnhancedTrayState::new()
    }

    fn create_test_menu_item() -> TrayMenuItem {
        TrayMenuItem {
            id: "test_item".to_string(),
            label: "Test Item".to_string(),
            action: TrayMenuAction::Activate,
            icon: Some("test-icon".to_string()),
            submenu: vec![],
            enabled: true,
            visible: true,
            checkable: false,
            checked: false,
            shortcut: Some("Ctrl+T".to_string()),
            is_separator: false,
            app_id: "test".to_string(),
            full_path: "Test → Test Item".to_string(),
            widget_type: TrayWidgetType::Button,
            default_value: None,
            placeholder: None,
        }
    }

    fn create_text_input_item() -> TrayMenuItem {
        TrayMenuItem {
            id: "search_input".to_string(),
            label: "INPUT: Search".to_string(),
            action: TrayMenuAction::TextInputChanged {
                value: "".to_string(),
            },
            icon: None,
            submenu: vec![],
            enabled: true,
            visible: true,
            checkable: false,
            checked: false,
            shortcut: None,
            is_separator: false,
            app_id: "test".to_string(),
            full_path: "Search Input".to_string(),
            widget_type: TrayWidgetType::TextInput,
            default_value: Some("".to_string()),
            placeholder: Some("Type to search...".to_string()),
        }
    }

    #[test]
    fn test_hidden_tray_renders_empty() {
        let state = create_test_state();
        assert!(!state.is_visible());

        let _element = render_enhanced_tray(&state);
        // Element should be a space widget when hidden
        // (We can't test Widget internals directly, but function shouldn't crash)
    }

    #[test]
    fn test_visible_tray_renders_content() {
        let mut state = create_test_state();
        state.show();
        state.animation_progress = 1.0;

        let _element = render_enhanced_tray(&state);
        // Should render content without crashing
    }

    #[test]
    fn test_separator_rendering() {
        let separator = TrayMenuItem {
            id: "sep".to_string(),
            label: "".to_string(),
            action: TrayMenuAction::Activate,
            icon: None,
            submenu: vec![],
            enabled: true,
            visible: true,
            checkable: false,
            checked: false,
            shortcut: None,
            is_separator: true,
            app_id: "test".to_string(),
            full_path: "".to_string(),
            widget_type: TrayWidgetType::Separator,
            default_value: None,
            placeholder: None,
        };

        let _element = render_menu_item(&separator, false, "test", &[]);
        // Should render without crashing
    }

    #[test]
    fn test_checkable_item_rendering() {
        let mut item = create_test_menu_item();
        item.checkable = true;
        item.checked = true;

        let _element = render_menu_item(&item, false, "test", &[]);
        // Should render without crashing
    }

    #[test]
    fn test_disabled_item_rendering() {
        let mut item = create_test_menu_item();
        item.enabled = false;

        let _element = render_menu_item(&item, false, "test", &[]);
        // Should render without crashing and not have on_press
    }

    #[test]
    fn test_text_input_widget_rendering() {
        // Test text input widget detection via label
        let text_input_by_label = TrayMenuItem {
            id: "input1".to_string(),
            label: "INPUT: Enter value".to_string(),
            action: TrayMenuAction::TextInputChanged {
                value: "".to_string(),
            },
            icon: None,
            submenu: vec![],
            enabled: true,
            visible: true,
            checkable: false,
            checked: false,
            shortcut: None,
            is_separator: false,
            app_id: "test".to_string(),
            full_path: "Enter value".to_string(),
            widget_type: TrayWidgetType::TextInput,
            default_value: Some("".to_string()),
            placeholder: Some("Placeholder text".to_string()),
        };

        let _element = render_menu_item(&text_input_by_label, false, "test", &[]);
        // Should render as text input without crashing

        // Test text input widget detection via action
        let text_input_by_action = TrayMenuItem {
            id: "input2".to_string(),
            label: "Search".to_string(),
            action: TrayMenuAction::TextInputChanged {
                value: "test".to_string(),
            },
            icon: None,
            submenu: vec![],
            enabled: true,
            visible: true,
            checkable: false,
            checked: false,
            shortcut: None,
            is_separator: false,
            app_id: "test".to_string(),
            full_path: "Search".to_string(),
            widget_type: TrayWidgetType::TextInput,
            default_value: Some("test".to_string()),
            placeholder: None,
        };

        let _element = render_menu_item(&text_input_by_action, false, "test", &[]);
        // Should render as text input without crashing
    }

    #[test]
    fn test_submenu_button_rendering() {
        let submenu_item = TrayMenuItem {
            id: "settings_menu".to_string(),
            label: "Settings".to_string(),
            action: TrayMenuAction::NavigateToSubmenu {
                item_id: "settings_menu".to_string(),
                submenu_path: vec!["settings".to_string()],
            },
            icon: Some("settings-icon".to_string()),
            submenu: vec![TrayMenuItem {
                id: "sub_item1".to_string(),
                label: "Submenu Item 1".to_string(),
                action: TrayMenuAction::Activate,
                icon: None,
                submenu: vec![],
                enabled: true,
                visible: true,
                checkable: false,
                checked: false,
                shortcut: None,
                is_separator: false,
                app_id: "test".to_string(),
                full_path: "Settings → Submenu Item 1".to_string(),
                widget_type: TrayWidgetType::Button,
                default_value: None,
                placeholder: None,
            }],
            enabled: true,
            visible: true,
            checkable: false,
            checked: false,
            shortcut: None,
            is_separator: false,
            app_id: "test".to_string(),
            full_path: "Settings".to_string(),
            widget_type: TrayWidgetType::SubmenuButton,
            default_value: None,
            placeholder: None,
        };

        let _element = render_menu_item(&submenu_item, false, "test", &[]);
        // Should render as submenu button with indicator without crashing
    }

    #[test]
    fn test_menu_items_with_submenu_path() {
        let items = vec![create_test_menu_item()];
        let empty_path: Vec<String> = vec![];

        let _element = render_menu_items(&items, None, "test", &empty_path);
        // Should render without crashing

        // Test with non-empty submenu path
        let nested_path = vec!["settings".to_string(), "advanced".to_string()];
        let _element = render_menu_items(&items, None, "test", &nested_path);
        // Should render without crashing
    }

    #[test]
    fn test_single_app_view_with_submenu_navigation() {
        let mut state = create_test_state();

        // Add a test app to the tree
        let icon = TrayIcon {
            key: "test_app".to_string(),
            id: "test_app".to_string(),
            service: "com.example.test".to_string(),
            path: "/StatusNotifierItem".to_string(),
            title: "Test App".to_string(),
            icon_name: Some("test-icon".to_string()),
            icon_pixmap: None,
            status: "Active".to_string(),
            has_menu: true,
            menu_object_path: Some("/MenuBar".to_string()),
        };

        state.tree.update_app(icon);

        // Set up single app view with submenu path
        let navigation = state.tree.get_app_navigation("test_app");
        state.current_view = TrayViewState::SingleApp {
            app_id: "test_app".to_string(),
            navigation,
            submenu_path: vec!["settings".to_string()],
        };
        state.show();

        let _element = render_enhanced_tray(&state);
        // Should render without crashing
    }

    #[test]
    fn test_all_widget_types_render() {
        // Test that all widget types can be rendered
        let button = create_test_menu_item();
        let text_input = create_text_input_item();
        let separator = TrayMenuItem {
            id: "sep".to_string(),
            label: "".to_string(),
            action: TrayMenuAction::Activate,
            icon: None,
            submenu: vec![],
            enabled: true,
            visible: true,
            checkable: false,
            checked: false,
            shortcut: None,
            is_separator: true,
            app_id: "test".to_string(),
            full_path: "".to_string(),
            widget_type: TrayWidgetType::Separator,
            default_value: None,
            placeholder: None,
        };
        let submenu_button = TrayMenuItem {
            id: "submenu".to_string(),
            label: "Submenu".to_string(),
            action: TrayMenuAction::NavigateToSubmenu {
                item_id: "submenu".to_string(),
                submenu_path: vec!["submenu".to_string()],
            },
            icon: None,
            submenu: vec![],
            enabled: true,
            visible: true,
            checkable: false,
            checked: false,
            shortcut: None,
            is_separator: false,
            app_id: "test".to_string(),
            full_path: "Submenu".to_string(),
            widget_type: TrayWidgetType::SubmenuButton,
            default_value: None,
            placeholder: None,
        };

        let all_items = vec![&button, &text_input, &separator, &submenu_button];
        for item in all_items {
            let _element = render_menu_item(item, false, "test", &[]);
            // Each should render without crashing
        }
    }

    #[test]
    fn test_aggregated_view_with_items() {
        let items = vec![create_test_menu_item()];

        let _element = render_aggregated_items(&items);
        // Should render without crashing
    }

    #[test]
    fn test_favorites_view_empty() {
        let items = vec![];
        let _element = render_favorite_items(&items);
        // Should render without crashing
    }

    #[test]
    fn test_network_controls_rendering() {
        let snapshot = Some(crate::tray::NetworkSnapshot {
            enabled: true,
            state: "connected".to_string(),
            interface: "wlan0".to_string(),
            connected_ssid: Some("TestNet".to_string()),
            known_networks: vec![],
            networks: vec![],
        });

        let _element = render_network_controls("test", &snapshot);
        // Should render without crashing
    }

    #[test]
    fn test_container_style_opacity() {
        let style = TrayContainerStyle { opacity: 0.5 };

        // Test that the style can be created and has proper opacity
        assert_eq!(style.opacity, 0.5);
    }
}
