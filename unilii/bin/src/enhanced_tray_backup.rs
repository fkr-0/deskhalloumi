//! Enhanced tray system with hierarchical menus and DBus integration
//!
//! This module provides a completely refactored tray system following idiomatic
//! Iced 0.14 patterns for better maintainability and testing.

pub mod core;
pub mod dbus; 
pub mod rendering;
pub mod state;

// Re-export key types for convenience
pub use core::*;
pub use dbus::*;
pub use rendering::*;
pub use state::*;

// Re-export from legacy tray for compatibility
pub use crate::tray::{
    read_network_snapshot, set_wifi_enabled, 
    spawn_command, is_network_icon,
    TrayIcon as LegacyTrayIcon
};
    pub service: String,
    pub path: String,
    pub id: String,
    pub title: String,
    pub icon_name: Option<String>,
    pub status: String,
    pub has_menu: bool,
    pub menu_object_path: Option<String>, // DBus menu path
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TrayMenuAction {
    Activate,
    ContextMenu,
    SecondaryActivate,
    SpawnCommand(String),
    DbusMenuAction { item_id: i32, event_id: String },
    NavigateToApp(String), // Navigate to specific app menu
    ShowAggregated, // Show aggregated view
    ShowFavorites, // Show favorites menu
    ToggleFavorite(String), // Toggle favorite status of menu item
}

// Enhanced menu item with hierarchy and metadata
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrayMenuItem {
    pub id: String,
    pub label: String,
    pub action: TrayMenuAction,
    pub icon: Option<String>,
    pub submenu: Vec<TrayMenuItem>,
    pub enabled: bool,
    pub visible: bool,
    pub checkable: bool,
    pub checked: bool,
    pub shortcut: Option<String>,
    pub is_separator: bool,
    pub app_id: String, // Application/service this item belongs to
    pub full_path: String, // Full menu path for aggregated view
}

// Hierarchical menu tree structure
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrayMenuTree {
    pub apps: HashMap<String, TrayApp>, // key: app_id, value: app with its menus
    pub favorites: HashSet<String>, // Set of favorite menu item IDs
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrayApp {
    pub icon: TrayIcon,
    pub menu_items: Vec<TrayMenuItem>,
    pub last_updated: std::time::SystemTime,
}

// Enhanced menu state for navigation and viewing
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TrayMenuViewMode {
    SingleApp { app_id: String }, // Show single app menu
    Aggregated { filter: Option<String> }, // Show all menu items with optional filter
    Favorites, // Show only favorited items
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrayMenuNavigation {
    pub current_app_index: usize,
    pub app_order: Vec<String>, // Ordered list of app IDs for navigation
    pub can_go_left: bool,
    pub can_go_right: bool,
}

// Context menu from DBus integration
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DbusMenuItem {
    pub id: i32,
    pub label: String,
    pub enabled: bool,
    pub visible: bool,
    pub icon_name: Option<String>,
    pub checkable: bool,
    pub checked: bool,
    pub shortcut: Option<String>,
    pub children: Vec<DbusMenuItem>,
}

// Events for the enhanced system
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TrayEvent {
    Icons(Vec<TrayIcon>),
    MenuUpdated { app_id: String, menu: Vec<TrayMenuItem> },
    DbusMenuReceived { app_id: String, menu: Vec<DbusMenuItem> },
    FavoritesChanged(HashSet<String>),
}

// Use the existing network structures from the tray module
pub use crate::tray::{WifiNetwork, NetworkSnapshot};

// == Enhanced Menu Construction and Management ==

impl TrayMenuTree {
    pub fn new() -> Self {
        Self {
            apps: HashMap::new(),
            favorites: HashSet::new(),
        }
    }

    pub fn update_app(&mut self, icon: TrayIcon) {
        let app_id = icon.id.clone();
        let menu_items = if icon.has_menu {
            build_menu_items(&icon)
        } else {
            build_default_menu_items(&icon)
        };

        self.apps.insert(app_id, TrayApp {
            icon,
            menu_items,
            last_updated: std::time::SystemTime::now(),
        });
    }

    pub fn get_app_navigation(&self, current_app_id: &str) -> TrayMenuNavigation {
        let app_order: Vec<String> = self.apps.keys().cloned().collect();
        let current_index = app_order.iter().position(|id| id == current_app_id).unwrap_or(0);
        
        TrayMenuNavigation {
            current_app_index: current_index,
            app_order: app_order.clone(),
            can_go_left: current_index > 0,
            can_go_right: current_index < app_order.len().saturating_sub(1),
        }
    }

    pub fn get_aggregated_menu(&self, filter: Option<&str>) -> Vec<TrayMenuItem> {
        let mut items = Vec::new();
        
        for app in self.apps.values() {
            for item in &app.menu_items {
                self.flatten_menu_items(item, &app.icon.id, &mut items);
            }
        }

        if let Some(filter_text) = filter {
            items.retain(|item| {
                item.label.to_lowercase().contains(&filter_text.to_lowercase()) ||
                item.full_path.to_lowercase().contains(&filter_text.to_lowercase())
            });
        }

        items.sort_by(|a, b| a.full_path.cmp(&b.full_path));
        items
    }

    pub fn get_favorites_menu(&self) -> Vec<TrayMenuItem> {
        let mut favorites = Vec::new();
        
        for app in self.apps.values() {
            for item in &app.menu_items {
                self.collect_favorites(item, &mut favorites);
            }
        }

        favorites.sort_by(|a, b| a.full_path.cmp(&b.full_path));
        favorites
    }

    pub fn toggle_favorite(&mut self, item_id: &str) -> bool {
        if self.favorites.contains(item_id) {
            self.favorites.remove(item_id)
        } else {
            self.favorites.insert(item_id.to_string());
            true
        }
    }

    fn flatten_menu_items(&self, item: &TrayMenuItem, app_id: &str, result: &mut Vec<TrayMenuItem>) {
        if !item.is_separator && item.visible {
            let mut flattened = item.clone();
            flattened.full_path = format!("{} → {}", app_id, item.label);
            result.push(flattened);
        }

        for subitem in &item.submenu {
            let mut nested = subitem.clone();
            nested.full_path = format!("{} → {} → {}", app_id, item.label, subitem.label);
            self.flatten_menu_items(&nested, app_id, result);
        }
    }

    fn collect_favorites(&self, item: &TrayMenuItem, result: &mut Vec<TrayMenuItem>) {
        if self.favorites.contains(&item.id) {
            result.push(item.clone());
        }
        for subitem in &item.submenu {
            self.collect_favorites(subitem, result);
        }
    }
}

// Enhanced menu building functions
pub fn build_default_menu_items(icon: &TrayIcon) -> Vec<TrayMenuItem> {
    let mut items = vec![TrayMenuItem {
        id: format!("{}_activate", icon.key),
        label: format!("Activate {}", icon.title),
        action: TrayMenuAction::Activate,
        icon: icon.icon_name.clone(),
        submenu: vec![],
        enabled: true,
        visible: true,
        checkable: false,
        checked: false,
        shortcut: None,
        is_separator: false,
        app_id: icon.id.clone(),
        full_path: format!("Activate {}", icon.title),
    }];

    items.push(TrayMenuItem {
        id: format!("{}_secondary", icon.key),
        label: "Secondary action".to_string(),
        action: TrayMenuAction::SecondaryActivate,
        icon: None,
        submenu: vec![],
        enabled: true,
        visible: true,
        checkable: false,
        checked: false,
        shortcut: None,
        is_separator: false,
        app_id: icon.id.clone(),
        full_path: "Secondary action".to_string(),
    });

    if is_network_icon(&convert_to_legacy_icon(icon)) {
        items.push(TrayMenuItem {
            id: format!("{}_network_settings", icon.key),
            label: "Open Network Settings".to_string(),
            action: TrayMenuAction::SpawnCommand("nm-connection-editor".to_string()),
            icon: Some("preferences-system-network".to_string()),
            submenu: vec![],
            enabled: true,
            visible: true,
            checkable: false,
            checked: false,
            shortcut: None,
            is_separator: false,
            app_id: icon.id.clone(),
            full_path: "Open Network Settings".to_string(),
        });
    }

    items
}

pub fn build_menu_items(icon: &TrayIcon) -> Vec<TrayMenuItem> {
    build_default_menu_items(icon)
}

// == DBus Integration for Enhanced Context Menus ==

pub async fn fetch_dbus_menu(icon: &TrayIcon) -> Result<Vec<DbusMenuItem>, String> {
    if let Some(menu_path) = &icon.menu_object_path {
        let connection = Connection::session().await
            .map_err(|e| format!("Failed to connect to DBus: {}", e))?;

        let proxy = Proxy::new(
            &connection,
            icon.service.as_str(), // Convert to &str
            menu_path.as_str(),    // Convert to &str
            MENU_INTERFACE,
        )
        .await
        .map_err(|e| format!("Failed to create menu proxy: {}", e))?;

        // Simplified menu layout request - just get basic structure
        let result: Result<zbus::Message, zbus::Error> = 
            proxy.call_method("GetLayout", &(0i32, -1i32, vec!["label", "enabled", "visible"])).await;

        match result {
            Ok(_message) => {
                // For now, return empty menu until we implement proper DBus message parsing
                // This is a placeholder implementation to get compilation working
                Ok(vec![DbusMenuItem {
                    id: 0,
                    label: "Menu Item".to_string(),
                    enabled: true,
                    visible: true,
                    icon_name: None,
                    checkable: false,
                    checked: false,
                    shortcut: None,
                    children: vec![],
                }])
            }
            Err(e) => Err(format!("Failed to get menu layout: {}", e))
        }
    } else {
        Err("No menu object path available".to_string())
    }
}

fn convert_dbus_menu_layout(
    _id: i32,
    _properties: HashMap<String, OwnedValue>,
    _children: Vec<OwnedValue>,
) -> Result<Vec<DbusMenuItem>, String> {
    // Simplified implementation for now - return a basic menu structure
    // TODO: Implement proper DBus menu parsing
    Ok(vec![
        DbusMenuItem {
            id: 1,
            label: "Menu Item".to_string(),
            enabled: true,
            visible: true,
            icon_name: None,
            checkable: false,
            checked: false,
            shortcut: None,
            children: vec![],
        }
    ])
}

fn parse_menu_item_value(value: &OwnedValue) -> Option<(i32, HashMap<String, OwnedValue>, Vec<OwnedValue>)> {
    // This is a simplified parser - the actual DBus menu format is quite complex
    // In a real implementation, you'd need more robust parsing
    None // Placeholder implementation
}

pub fn convert_dbus_to_tray_menu(dbus_menu: Vec<DbusMenuItem>, app_id: &str) -> Vec<TrayMenuItem> {
    dbus_menu
        .into_iter()
        .map(|item| convert_dbus_menu_item(item, app_id, ""))
        .collect()
}

fn convert_dbus_menu_item(dbus_item: DbusMenuItem, app_id: &str, path_prefix: &str) -> TrayMenuItem {
    let full_path = if path_prefix.is_empty() {
        dbus_item.label.clone()
    } else {
        format!("{} → {}", path_prefix, dbus_item.label)
    };

    let submenu = dbus_item
        .children
        .into_iter()
        .map(|child| convert_dbus_menu_item(child, app_id, &full_path))
        .collect();

    TrayMenuItem {
        id: format!("{}_{}", app_id, dbus_item.id),
        label: dbus_item.label.clone(),
        action: TrayMenuAction::DbusMenuAction {
            item_id: dbus_item.id,
            event_id: "clicked".to_string(),
        },
        icon: dbus_item.icon_name,
        submenu,
        enabled: dbus_item.enabled,
        visible: dbus_item.visible,
        checkable: dbus_item.checkable,
        checked: dbus_item.checked,
        shortcut: dbus_item.shortcut,
        is_separator: dbus_item.label == "-" || dbus_item.label.is_empty(),
        app_id: app_id.to_string(),
        full_path,
    }
}

// == Enhanced Menu Actions ==

pub async fn invoke_menu_action(icon: &TrayIcon, action: TrayMenuAction) {
    match action {
        TrayMenuAction::SpawnCommand(command) => {
            if let Err(error) = spawn_command(command).await {
                warn!("tray: command spawn failed: {error}");
            }
        }
        TrayMenuAction::DbusMenuAction { item_id, event_id } => {
            if let Err(error) = invoke_dbus_menu_action(icon, item_id, &event_id).await {
                warn!("tray: DBus menu action failed: {error}");
            }
        }
        TrayMenuAction::Activate => {
            if let Err(error) = invoke_standard_action(icon, "Activate").await {
                warn!("tray: activation failed: {error}");
            }
        }
        TrayMenuAction::ContextMenu => {
            if let Err(error) = invoke_standard_action(icon, "ContextMenu").await {
                warn!("tray: context menu failed: {error}");
            }
        }
        TrayMenuAction::SecondaryActivate => {
            if let Err(error) = invoke_standard_action(icon, "SecondaryActivate").await {
                warn!("tray: secondary activation failed: {error}");
            }
        }
        _ => {
            // Navigation actions are handled by the main application
        }
    }
}

async fn invoke_dbus_menu_action(icon: &TrayIcon, item_id: i32, event_id: &str) -> Result<(), String> {
    if let Some(menu_path) = &icon.menu_object_path {
        let connection = Connection::session().await
            .map_err(|e| format!("Failed to connect to DBus: {}", e))?;

        let proxy = Proxy::new(
            &connection,
            icon.service.as_str(), // Convert to &str
            menu_path.as_str(),    // Convert to &str
            MENU_INTERFACE,
        )
        .await
        .map_err(|e| format!("Failed to create menu proxy: {}", e))?;

        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u32;

        proxy
            .call_method("Event", &(item_id, event_id, "", timestamp))
            .await
            .map_err(|e| format!("Failed to invoke menu action: {}", e))?;

        Ok(())
    } else {
        Err("No menu object path available".to_string())
    }
}

async fn invoke_standard_action(icon: &TrayIcon, method: &str) -> Result<(), String> {
    let connection = Connection::session().await
        .map_err(|e| format!("Failed to connect to DBus: {}", e))?;

    let proxy = Proxy::new(
        &connection,
        icon.service.as_str(), // Convert to &str
        icon.path.as_str(),    // Convert to &str
        ITEM_INTERFACE,
    )
    .await
    .map_err(|e| format!("Failed to create item proxy: {}", e))?;

    let x = 0i32;
    let y = 0i32;

    proxy
        .call_method(method, &(x, y))
        .await
        .map_err(|e| format!("Failed to invoke {}: {}", method, e))?;

    Ok(())
}

// == Enhanced Tray Watcher ==

pub async fn run_enhanced_tray_watcher(output: UnboundedSender<TrayEvent>, poll_ms: u64) {
    let mut menu_tree = TrayMenuTree::new();
    
    loop {
        match Connection::session().await {
            Ok(connection) => {
                register_as_host(&connection).await;
                let mut previous_icons: Vec<TrayIcon> = Vec::new();

                loop {
                    let icons = read_enhanced_tray_icons(&connection).await;
                    if icons != previous_icons {
                        // Update menu tree and send icon updates
                        for icon in &icons {
                            menu_tree.update_app(icon.clone());
                        }

                        if output.send(TrayEvent::Icons(icons.clone())).is_err() {
                            return;
                        }

                        // Fetch enhanced menus for apps that have them
                        for icon in &icons {
                            if icon.has_menu && icon.menu_object_path.is_some() {
                                if let Ok(dbus_menu) = fetch_dbus_menu(icon).await {
                                    let tray_menu = convert_dbus_to_tray_menu(dbus_menu, &icon.id);
                                    let _ = output.send(TrayEvent::MenuUpdated {
                                        app_id: icon.id.clone(),
                                        menu: tray_menu,
                                    });
                                }
                            }
                        }

                        previous_icons = icons;
                    }
                    sleep(Duration::from_millis(poll_ms.max(500))).await;
                }
            }
            Err(error) => {
                warn!("tray: failed to connect to DBus session bus: {error}");
                sleep(Duration::from_secs(5)).await;
            }
        }
    }
}

// Existing functions adapted for enhanced system
pub async fn spawn_command(command: String) -> Result<(), String> {
    use tokio::process::Command;
    
    if command.trim().is_empty() {
        return Err("Command cannot be empty".to_string());
    }

    let mut cmd = Command::new("sh");
    cmd.arg("-c").arg(command);

    match cmd.spawn() {
        Ok(_) => Ok(()),
        Err(e) => Err(format!("Failed to spawn command: {}", e)),
    }
}

// Re-export network functions from tray module
pub use crate::tray::{read_network_snapshot, set_wifi_enabled};

async fn register_as_host(connection: &Connection) {
    let proxy = match Proxy::new(
        connection,
        WATCHER_SERVICE,
        WATCHER_PATH,
        WATCHER_INTERFACE,
    ).await {
        Ok(proxy) => proxy,
        Err(error) => {
            warn!("tray: failed to create watcher proxy: {error}");
            return;
        }
    };

    if let Err(error) = proxy.call_method("RegisterStatusNotifierHost", &(TRAY_HOST_NAME,)).await {
        warn!("tray: failed to register as host: {error}");
    }
}

async fn read_enhanced_tray_icons(connection: &Connection) -> Vec<TrayIcon> {
    let proxy = match Proxy::new(
        connection,
        WATCHER_SERVICE,
        WATCHER_PATH,
        WATCHER_INTERFACE,
    ).await {
        Ok(proxy) => proxy,
        Err(error) => {
            warn!("tray: failed to create watcher proxy: {error}");
            return vec![];
        }
    };

    let registered_items: Result<Vec<String>, _> = proxy.get_property("RegisteredStatusNotifierItems").await;
    match registered_items {
        Ok(items) => {
            let mut icons = vec![];
            for identifier in items {
                if let Some(icon) = read_enhanced_tray_icon(connection, &identifier).await {
                    icons.push(icon);
                }
            }
            icons
        }
        Err(error) => {
            warn!("tray: failed to read registered items: {error}");
            vec![]
        }
    }
}

async fn read_enhanced_tray_icon(connection: &Connection, identifier: &str) -> Option<TrayIcon> {
    let (service, object_path) = if let Some(slash_pos) = identifier.rfind('/') {
        let service = &identifier[..slash_pos];
        let path = &identifier[slash_pos..];
        (service, path)
    } else {
        (identifier, "/StatusNotifierItem")
    };

    let proxy = match Proxy::new(connection, service, object_path, ITEM_INTERFACE).await {
        Ok(proxy) => proxy,
        Err(error) => {
            warn!("tray: failed to create item proxy service={service} path={object_path}: {error}");
            return None;
        }
    };

    let id: Result<String, _> = proxy.get_property("Id").await;
    let title: Result<String, _> = proxy.get_property("Title").await;
    let icon_name: Result<String, _> = proxy.get_property("IconName").await;
    let status: Result<String, _> = proxy.get_property("Status").await;
    let menu_path: Result<OwnedObjectPath, _> = proxy.get_property("Menu").await;

    let id = id.unwrap_or_else(|_| service.to_string());
    let title = title.unwrap_or_else(|_| id.clone());
    let icon_name = icon_name.ok();
    let status = status.unwrap_or_else(|_| "Active".to_string());
    let menu_object_path = menu_path.ok().map(|path| path.to_string());
    let has_menu = menu_object_path.is_some();

    Some(TrayIcon {
        key: format!("{service}{object_path}"),
        service: service.to_string(),
        path: object_path.to_string(),
        id,
        title,
        icon_name,
        status,
        has_menu,
        menu_object_path,
    })
}

async fn run_nmcli(nmcli_path: &str, args: &[&str]) -> Result<String, String> {
    use tokio::process::Command;

    let output = Command::new(nmcli_path)
        .args(args)
        .output()
        .await
        .map_err(|e| format!("Failed to run nmcli: {}", e))?;

    if !output.status.success() {
        return Err(format!(
            "nmcli command failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

// Use network utility functions from tray module
pub use crate::tray::is_network_icon;

/// Convert enhanced TrayIcon to legacy TrayIcon for compatibility
fn convert_to_legacy_icon(enhanced_icon: &TrayIcon) -> crate::tray::TrayIcon {
    crate::tray::TrayIcon {
        key: enhanced_icon.key.clone(),
        service: enhanced_icon.service.clone(),
        path: enhanced_icon.path.clone(),
        id: enhanced_icon.id.clone(),
        title: enhanced_icon.title.clone(),
        icon_name: enhanced_icon.icon_name.clone(),
        status: enhanced_icon.status.clone(),
        has_menu: enhanced_icon.has_menu,
    }
}

// == Tests ==

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_tray_menu_tree_creation() {
        let tree = TrayMenuTree::new();
        assert!(tree.apps.is_empty());
        assert!(tree.favorites.is_empty());
    }

    #[test]
    fn test_app_navigation() {
        let mut tree = TrayMenuTree::new();
        
        let icon1 = create_test_icon("app1", "App 1");
        let icon2 = create_test_icon("app2", "App 2");
        let icon3 = create_test_icon("app3", "App 3");
        
        tree.update_app(icon1);
        tree.update_app(icon2);
        tree.update_app(icon3);
        
        let nav = tree.get_app_navigation("app1");
        assert_eq!(nav.app_order.len(), 3);
        assert!(nav.can_go_right);
        
        let nav = tree.get_app_navigation("app3");
        assert!(nav.can_go_left);
    }

    #[test] 
    fn test_favorites_system() {
        let mut tree = TrayMenuTree::new();
        
        // Test toggling favorites
        assert!(tree.toggle_favorite("item1"));
        assert!(tree.favorites.contains("item1"));
        
        assert!(!tree.toggle_favorite("item1"));
        assert!(!tree.favorites.contains("item1"));
    }

    #[test]
    fn test_aggregated_menu() {
        let mut tree = TrayMenuTree::new();
        
        let icon = create_test_icon("test_app", "Test App");
        tree.update_app(icon);
        
        let aggregated = tree.get_aggregated_menu(None);
        assert!(!aggregated.is_empty());
        
        // Test filtering
        let filtered = tree.get_aggregated_menu(Some("Settings"));
        assert!(filtered.iter().any(|item| item.label.contains("Settings")));
    }

    #[test]
    fn test_menu_item_creation() {
        let icon = create_test_icon("test", "Test App");
        let items = build_default_menu_items(&icon);
        
        assert!(!items.is_empty());
        assert!(items[0].label.contains("Activate"));
        assert_eq!(items[0].app_id, "test");
    }

    #[test]
    fn test_network_icon_detection() {
        let network_icon = TrayIcon {
            key: "nm".to_string(),
            service: "org.kde.network".to_string(),
            path: "/StatusNotifierItem".to_string(),
            id: "NetworkManager".to_string(),
            title: "Network".to_string(),
            icon_name: Some("network-wireless".to_string()),
            status: "Active".to_string(),
            has_menu: true,
            menu_object_path: None,
        };
        
        assert!(is_network_icon(&network_icon));
        
        let regular_icon = create_test_icon("app", "Regular App");
        assert!(!is_network_icon(&regular_icon));
    }

    #[test]
    fn test_dbus_menu_conversion() {
        let dbus_item = DbusMenuItem {
            id: 1,
            label: "Test Item".to_string(),
            enabled: true,
            visible: true,
            icon_name: Some("test-icon".to_string()),
            checkable: false,
            checked: false,
            shortcut: None,
            children: vec![],
        };
        
        let tray_item = convert_dbus_menu_item(dbus_item, "test_app", "");
        assert_eq!(tray_item.label, "Test Item");
        assert_eq!(tray_item.app_id, "test_app");
        assert!(tray_item.enabled);
    }

    #[tokio::test]
    async fn test_spawn_command_empty() {
        let result = spawn_command("".to_string()).await;
        assert!(result.is_err());
    }

    fn create_test_icon(id: &str, title: &str) -> TrayIcon {
        TrayIcon {
            key: format!("test_{}", id),
            service: format!("org.test.{}", id),
            path: "/StatusNotifierItem".to_string(),
            id: id.to_string(),
            title: title.to_string(),
            icon_name: Some("test-icon".to_string()),
            status: "Active".to_string(),
            has_menu: false,
            menu_object_path: None,
        }
    }
}