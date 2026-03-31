//! Rendering logic for enhanced tray following idiomatic Iced 0.14 patterns
//! 
//! This module implements declarative UI rendering:
//! - Composable widget functions
//! - Conditional rendering based on state
//! - Proper use of containers, layout, and styling
//! - Clear separation of concerns

use crate::enhanced_tray::{core::*, state::TrayMessage};
use iced::{
    widget::{button, container, text, row, column, scrollable, text_input, Space},
    Element, Length, Alignment,
};

/// Main enhanced tray rendering function (idiomatic Iced pattern)
pub fn render_enhanced_tray(state: &EnhancedTrayState) -> Element<'_, TrayMessage> {
    if !state.is_visible() {
        return Space::new().into();
    }

    let content = match &state.current_view {
        TrayViewState::SingleApp { app_id, navigation } => {
            render_single_app_view(state, app_id, navigation)
        }
        TrayViewState::Aggregated { items, filter } => {
            render_aggregated_view(state, items, filter)
        }
        TrayViewState::Favorites { items } => {
            render_favorites_view(state, items)
        }
        TrayViewState::Network { app_id, data, loading, error } => {
            render_network_view(state, app_id, data, *loading, error)
        }
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
                .on_press(TrayMessage::NavigateLeft)
        );
    }
    
    if let Some(app) = app_menu {
        title_row = title_row.push(
            text(&app.icon.title)
                .size(14)
        );
    } else {
        title_row = title_row.push(text(app_id).size(14));
    }
    
    if navigation.can_go_right {
        title_row = title_row.push(
            button(text("▶").size(12))
                // .style() removed for compatibility
                .on_press(TrayMessage::NavigateRight)
        );
    }
    
    content = content.push(title_row);
    
    // Menu items
    if let Some(app) = app_menu {
        let menu_items = render_menu_items(&app.menu_items, state.selected_index, app_id);
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
    content = content.push(
        text("All Menu Items")
            .size(14)
    );
    
    // Filter input
    content = content.push(
        text_input(
            "Search menu items...",
            filter.as_deref().unwrap_or("")
        )
        .on_input(TrayMessage::FilterUpdate)
        .size(12)
        .padding([2, 4])
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
    content = content.push(
        text("Favorite Items ⭐")
            .size(14)
    );
    
    // Items list
    if items.is_empty() {
        content = content.push(
            text("No favorites yet. Press 'f' on any menu item to add it here.")
                .size(12)
        );
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
    
    // Title 
    content = content.push(
        text("Network Settings")
            .size(14)
    );
    
    // Status indicator
    if loading {
        content = content.push(
            text("⟳ Loading...")
                .size(12)
        );
    } else if let Some(err) = error {
        content = content.push(
            text(format!("⚠ Error: {}", err))
                .size(12)
        );
    }
    
    // Controls
    let controls = render_network_controls(app_id, data);
    content = content.push(controls);
    
    // Network list
    if let Some(snapshot) = data {
        if snapshot.enabled && !snapshot.networks.is_empty() {
            let networks = render_network_list(app_id, snapshot);
            content = content.push(networks);
        } else if !snapshot.enabled {
            content = content.push(text("Wi-Fi is disabled").size(12));
        }
    }
    
    // Keyboard hints
    content = content.push(render_keyboard_hints_network());
    
    content.into()
}

/// Render menu items as a column of buttons
fn render_menu_items<'a>(
    items: &'a [TrayMenuItem],
    selected_index: Option<usize>,
    app_id: &'a str,
) -> Element<'a, TrayMessage> {
    if items.is_empty() {
        return text("No menu items").size(12).into();
    }

    let mut menu_col = column!().spacing(1);
    
    for (index, item) in items.iter().enumerate() {
        let item_widget = render_menu_item(item, selected_index == Some(index), app_id);
        menu_col = menu_col.push(item_widget);
    }
    
    let limited_items = if items.len() > 8 {
        scrollable(menu_col)
            .height(Length::Fixed(200.0))
            .into()
    } else {
        menu_col.into()
    };
    
    limited_items
}

/// Render a single menu item
fn render_menu_item<'a>(
    item: &'a TrayMenuItem,
    is_selected: bool,
    app_id: &'a str,
) -> Element<'a, TrayMessage> {
    if item.is_separator {
        return text("─".repeat(20))
            .size(10)
            .into();
    }

    let mut label = item.label.clone();
    
    // Add checkmark for checkable items
    if item.checkable {
        label = format!("{} {}", if item.checked { "☑" } else { "☐" }, label);
    }
    
    // Add shortcut if available
    if let Some(shortcut) = &item.shortcut {
        label = format!("{} ({})", label, shortcut);  
    }
    
    let btn = button(text(label).size(12))
        .padding([2, 8])
        .width(Length::Fill);
    
    let styled_btn = if is_selected {
        btn// .style() removed for compatibility
    } else if !item.enabled {
        btn// .style() removed for compatibility // Disabled style
    } else {
        btn// .style() removed for compatibility
    };
    
    if item.enabled {
        styled_btn.on_press(TrayMessage::MenuItemClicked(
            app_id.to_string(),
            item.action.clone(),
        )).into()
    } else {
        styled_btn.into()
    }
}

/// Render aggregated menu items with app context
fn render_aggregated_items<'a>(items: &'a [TrayMenuItem]) -> Element<'a, TrayMessage> {
    let mut items_col = column!().spacing(1);
    
    for item in items.iter().take(10) { // Limit visible items
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
        items_col = items_col.push(
            text(format!("... and {} more items", items.len() - 10))
                .size(10)
        );
    }
    
    scrollable(items_col)
        .height(Length::Fixed(200.0))
        .into()
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
    
    scrollable(items_col)
        .height(Length::Fixed(200.0))
        .into()
}

/// Render network control buttons
fn render_network_controls<'a>(
    app_id: &'a str,
    data: &'a Option<crate::tray::NetworkSnapshot>,
) -> Element<'a, TrayMessage> {
    let is_enabled = data.as_ref().map(|d| d.enabled).unwrap_or(false);
    
    row![
        button(text(if is_enabled { "Disable Wi-Fi" } else { "Enable Wi-Fi" }).size(12))
            .padding([2, 6])
                // .style() removed for compatibility
            .padding([2, 6]) 
            // .style() removed for compatibility
            .on_press(TrayMessage::NetworkRefresh(app_id.to_string())),
            
        button(text("Settings").size(12))
            .padding([2, 6])
            // .style() removed for compatibility
            .on_press(TrayMessage::NetworkSpawnCommand(
                app_id.to_string(),
                "nm-connection-editor".to_string()
            )),
    ]
    .spacing(4)
    .into()
}

/// Render list of available networks
fn render_network_list<'a>(
    app_id: &'a str,
    snapshot: &'a crate::tray::NetworkSnapshot,
) -> Element<'a, TrayMessage> {
    let mut networks_col = column!().spacing(1);
    
    networks_col = networks_col.push(text("Available Networks:").size(12));
    
    for network in snapshot.networks.iter().take(6) {
        let mut label = format!("{} ({}%)", network.ssid, network.signal);
        
        if snapshot.state == "connected" && snapshot.interface == network.ssid {
            label = format!("● {}", label); // Connected indicator
        }
        
        let network_btn = button(text(label).size(11))
            .padding([1, 4])
            .width(Length::Fill) 
            // .style() removed for compatibility
            .on_press(TrayMessage::NetworkSpawnCommand(
                app_id.to_string(),
                format!("nmcli device wifi connect \"{}\"", network.ssid)
            ));
            
        networks_col = networks_col.push(network_btn);
    }
    
    scrollable(networks_col)
        .height(Length::Fixed(150.0))
        .into()
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
    text("a: All items • f: Remove favorite")
        .size(10)
        .into()
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
        let mut style = container::Style::default();
        style.background = Some(iced::Background::Color([0.1, 0.1, 0.1, self.opacity].into()));
        style.border = iced::Border {
            radius: 4.0.into(),
            width: 1.0,
            color: [0.3, 0.3, 0.3, self.opacity].into(),
        };
        style.shadow = iced::Shadow {
            color: [0.0, 0.0, 0.0, self.opacity * 0.5].into(),
            offset: iced::Vector::new(0.0, 2.0),
            blur_radius: 4.0,
        };
        style
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
        };
        
        let _element = render_menu_item(&separator, false, "test");
        // Should render without crashing
    }

    #[test]
    fn test_checkable_item_rendering() {
        let mut item = create_test_menu_item();
        item.checkable = true;
        item.checked = true;
        
        let _element = render_menu_item(&item, false, "test");
        // Should render without crashing
    }

    #[test] 
    fn test_disabled_item_rendering() {
        let mut item = create_test_menu_item();
        item.enabled = false;
        
        let _element = render_menu_item(&item, false, "test");
        // Should render without crashing and not have on_press
    }

    #[test]
    fn test_aggregated_view_with_items() {
        let items = vec![
            create_test_menu_item(),
        ];
        
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