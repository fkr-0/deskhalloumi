#[cfg(test)]
mod tests {
    use super::super::dbus::*;

    /// Helper to create test DBus menu items (simplified to avoid zbus version conflicts)
    fn create_dbus_menu_item(id: i32, label: &str) -> DbusMenuItem {
        DbusMenuItem {
            id,
            label: label.to_string(),
            enabled: true,
            visible: true,
            icon_name: None,
            checkable: false,
            checked: false,
            shortcut: None,
            children: Vec::new(),
        }
    }

    #[test]
    fn test_dbus_menu_item_structure() {
        let item = create_dbus_menu_item(1, "Test Item");

        assert_eq!(item.id, 1);
        assert_eq!(item.label, "Test Item");
        assert!(item.enabled);
        assert!(item.visible);
        assert!(item.icon_name.is_none());
        assert!(!item.checkable);
        assert!(!item.checked);
        assert!(item.shortcut.is_none());
        assert!(item.children.is_empty());
    }

    #[test]
    fn test_dbus_menu_item_with_children() {
        let _child1 = create_dbus_menu_item(2, "Child 1");
        let _child2 = create_dbus_menu_item(3, "Child 2");

        let parent = DbusMenuItem {
            id: 1,
            label: "Parent".to_string(),
            enabled: true,
            visible: true,
            icon_name: None,
            checkable: false,
            checked: false,
            shortcut: None,
            children: vec![
                create_dbus_menu_item(2, "Child 1"),
                create_dbus_menu_item(3, "Child 2"),
            ],
        };

        assert_eq!(parent.children.len(), 2);
        assert_eq!(parent.children[0].label, "Child 1");
        assert_eq!(parent.children[1].label, "Child 2");
    }

    #[test]
    fn test_dbus_menu_item_with_all_fields() {
        let item = DbusMenuItem {
            id: 42,
            label: "Full Item".to_string(),
            enabled: false,
            visible: true,
            icon_name: Some("test-icon".to_string()),
            checkable: true,
            checked: true,
            shortcut: Some("Ctrl+S".to_string()),
            children: vec![],
        };

        assert_eq!(item.id, 42);
        assert_eq!(item.label, "Full Item");
        assert!(!item.enabled);
        assert!(item.visible);
        assert_eq!(item.icon_name, Some("test-icon".to_string()));
        assert!(item.checkable);
        assert!(item.checked);
        assert_eq!(item.shortcut, Some("Ctrl+S".to_string()));
    }

    #[test]
    fn test_dbus_error_display() {
        let errors = vec![
            DbusError::Connection("Failed to connect".to_string()),
            DbusError::Proxy("Invalid proxy".to_string()),
            DbusError::MethodCall("Method failed".to_string()),
            DbusError::ResponseParsing("Parsing failed".to_string()),
            DbusError::NoMenu,
            DbusError::InvalidMenuData("Invalid data".to_string()),
        ];

        for error in errors {
            // Should not panic when displaying error
            let _ = format!("{}", error);
            let _ = format!("{:?}", error);
        }
    }

    #[test]
    fn test_parse_dbus_menu_layout() {
        let _child1 = create_dbus_menu_item(1, "Item 1");
        let _child2 = create_dbus_menu_item(2, "Item 2");

        let layout = (
            0i32, // root id
            std::collections::HashMap::new(), // root properties (empty)
            vec![], // children would be OwnedValue in real usage
        );

        let result = parse_dbus_menu_layout(layout);
        assert!(result.is_ok());
        // With empty children vec, we should get an empty result
        assert!(result.unwrap().is_empty());
    }

    #[test]
    fn test_dbus_menu_item_equality() {
        let item1 = create_dbus_menu_item(1, "Test");
        let item2 = create_dbus_menu_item(1, "Test");
        let item3 = create_dbus_menu_item(2, "Different");

        assert_eq!(item1, item2);
        assert_ne!(item1, item3);
    }
}
