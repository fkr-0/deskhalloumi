#[cfg(test)]
mod tests {
    use super::super::dbus::*;
    use zbus::zvariant::{Value, Structure, Dict, Array, OwnedValue};
    use std::collections::HashMap;

    /// Helper to create a test DBus menu item structure
    fn create_test_menu_item(
        id: i32,
        properties: HashMap<&str, Value<'_>>,
        children: Vec<OwnedValue>,
    ) -> OwnedValue {
        let props_dict = Dict::try_from(properties).unwrap();
        let children_array = Array::try_from(children).unwrap();
        
        let structure = Structure::try_from(vec![
            Value::I32(id).into(),
            Value::Dict(props_dict).into(),
            Value::Array(children_array).into(),
        ]).unwrap();
        
        Value::Structure(structure).into()
    }

    /// Helper to create test properties
    fn create_properties(entries: Vec<(&str, Value<'_>)>) -> HashMap<&str, Value<'_>> {
        entries.into_iter().collect()
    }

    #[test]
    fn test_parse_simple_menu_item() {
        let properties = create_properties(vec![
            ("label", Value::Str("Test Item".into())),
            ("enabled", Value::Bool(true)),
            ("visible", Value::Bool(true)),
            ("icon-name", Value::Str("test-icon".into())),
        ]);
        
        let menu_item = create_test_menu_item(1, properties, vec![]);
        
        let result = parse_menu_item_recursive(&menu_item).unwrap();
        
        assert_eq!(result.id, 1);
        assert_eq!(result.label, "Test Item");
        assert!(result.enabled);
        assert!(result.visible);
        assert_eq!(result.icon_name, Some("test-icon".to_string()));
        assert!(!result.checkable);
        assert!(!result.checked);
        assert_eq!(result.shortcut, None);
        assert!(result.children.is_empty());
    }

    #[test]
    fn test_parse_checkable_menu_item() {
        let properties = create_properties(vec![
            ("label", Value::Str("Checkable Item".into())),
            ("enabled", Value::Bool(true)),
            ("visible", Value::Bool(true)),
            ("toggle-type", Value::Str("checkmark".into())),
            ("toggle-state", Value::I32(1)),
        ]);
        
        let menu_item = create_test_menu_item(2, properties, vec![]);
        
        let result = parse_menu_item_recursive(&menu_item).unwrap();
        
        assert_eq!(result.id, 2);
        assert_eq!(result.label, "Checkable Item");
        assert!(result.enabled);
        assert!(result.visible);
        assert!(result.checkable);
        assert!(result.checked);
    }

    #[test]
    fn test_parse_radio_menu_item() {
        let properties = create_properties(vec![
            ("label", Value::Str("Radio Item".into())),
            ("toggle-type", Value::Str("radio".into())),
            ("toggle-state", Value::I32(0)),
        ]);
        
        let menu_item = create_test_menu_item(3, properties, vec![]);
        
        let result = parse_menu_item_recursive(&menu_item).unwrap();
        
        assert_eq!(result.id, 3);
        assert_eq!(result.label, "Radio Item");
        assert!(result.checkable);
        assert!(!result.checked);
    }

    #[test]
    fn test_parse_menu_item_with_shortcut() {
        let properties = create_properties(vec![
            ("label", Value::Str("Shortcut Item".into())),
            ("shortcut", Value::Str("Ctrl+S".into())),
        ]);
        
        let menu_item = create_test_menu_item(4, properties, vec![]);
        
        let result = parse_menu_item_recursive(&menu_item).unwrap();
        
        assert_eq!(result.id, 4);
        assert_eq!(result.label, "Shortcut Item");
        assert_eq!(result.shortcut, Some("Ctrl+S".to_string()));
    }

    #[test]
    fn test_parse_disabled_invisible_item() {
        let properties = create_properties(vec![
            ("label", Value::Str("Hidden Item".into())),
            ("enabled", Value::Bool(false)),
            ("visible", Value::Bool(false)),
        ]);
        
        let menu_item = create_test_menu_item(5, properties, vec![]);
        
        let result = parse_menu_item_recursive(&menu_item).unwrap();
        
        assert_eq!(result.id, 5);
        assert_eq!(result.label, "Hidden Item");
        assert!(!result.enabled);
        assert!(!result.visible);
    }

    #[test]
    fn test_parse_menu_with_children() {
        // Create child menu items
        let child1_props = create_properties(vec![
            ("label", Value::Str("Child 1".into())),
        ]);
        let child1 = create_test_menu_item(11, child1_props, vec![]);

        let child2_props = create_properties(vec![
            ("label", Value::Str("Child 2".into())),
            ("enabled", Value::Bool(false)),
        ]);
        let child2 = create_test_menu_item(12, child2_props, vec![]);

        // Create parent with children
        let parent_props = create_properties(vec![
            ("label", Value::Str("Parent Item".into())),
        ]);
        let parent = create_test_menu_item(10, parent_props, vec![child1, child2]);
        
        let result = parse_menu_item_recursive(&parent).unwrap();
        
        assert_eq!(result.id, 10);
        assert_eq!(result.label, "Parent Item");
        assert_eq!(result.children.len(), 2);
        
        assert_eq!(result.children[0].id, 11);
        assert_eq!(result.children[0].label, "Child 1");
        assert!(result.children[0].enabled);
        
        assert_eq!(result.children[1].id, 12);
        assert_eq!(result.children[1].label, "Child 2");
        assert!(!result.children[1].enabled);
    }

    #[test]
    fn test_parse_nested_menu_hierarchy() {
        // Create grandchild
        let grandchild_props = create_properties(vec![
            ("label", Value::Str("Grandchild".into())),
        ]);
        let grandchild = create_test_menu_item(21, grandchild_props, vec![]);

        // Create child with grandchildren
        let child_props = create_properties(vec![
            ("label", Value::Str("Child with Submenu".into())),
        ]);
        let child = create_test_menu_item(20, child_props, vec![grandchild]);

        // Create parent with child
        let parent_props = create_properties(vec![
            ("label", Value::Str("Root Menu".into())),
        ]);
        let parent = create_test_menu_item(19, parent_props, vec![child]);
        
        let result = parse_menu_item_recursive(&parent).unwrap();
        
        assert_eq!(result.id, 19);
        assert_eq!(result.label, "Root Menu");
        assert_eq!(result.children.len(), 1);
        assert_eq!(result.children[0].children.len(), 1);
        assert_eq!(result.children[0].children[0].id, 21);
        assert_eq!(result.children[0].children[0].label, "Grandchild");
    }

    #[test]
    fn test_parse_menu_item_with_empty_properties() {
        let properties = create_properties(vec![]);
        let menu_item = create_test_menu_item(6, properties, vec![]);
        
        let result = parse_menu_item_recursive(&menu_item).unwrap();
        
        assert_eq!(result.id, 6);
        assert_eq!(result.label, ""); // Should default to empty string
        assert!(result.enabled); // Should default to true
        assert!(result.visible); // Should default to true
        assert_eq!(result.icon_name, None);
        assert!(!result.checkable);
        assert!(!result.checked);
        assert_eq!(result.shortcut, None);
    }

    #[test]
    fn test_parse_invalid_menu_structure() {
        // Create an invalid structure with wrong field count
        let invalid_structure = Structure::try_from(vec![
            Value::I32(1).into(),
            Value::Str("not a dict".into()).into(), // Should be a dict
        ]).unwrap();
        
        let invalid_value = Value::Structure(invalid_structure).into();
        
        let result = parse_menu_item_recursive(&invalid_value);
        assert!(result.is_err());
    }

    #[test]
    fn test_convert_dbus_to_tray_menu() {
        let dbus_menu = vec![
            DbusMenuItem {
                id: 1,
                label: "File".to_string(),
                enabled: true,
                visible: true,
                icon_name: None,
                checkable: false,
                checked: false,
                shortcut: None,
                children: vec![
                    DbusMenuItem {
                        id: 11,
                        label: "New".to_string(),
                        enabled: true,
                        visible: true,
                        icon_name: Some("document-new".to_string()),
                        checkable: false,
                        checked: false,
                        shortcut: Some("Ctrl+N".to_string()),
                        children: vec![],
                    }
                ],
            }
        ];

        let tray_menu = convert_dbus_to_tray_menu(dbus_menu, "test-app");

        assert_eq!(tray_menu.len(), 1);
        assert_eq!(tray_menu[0].label, "File");
        assert_eq!(tray_menu[0].app_id, "test-app");
        assert_eq!(tray_menu[0].submenu.len(), 1);
        assert_eq!(tray_menu[0].submenu[0].label, "New");
        assert_eq!(tray_menu[0].submenu[0].shortcut, Some("Ctrl+N".to_string()));
        assert_eq!(tray_menu[0].submenu[0].icon, Some("document-new".to_string()));
    }

    #[test]
    fn test_convert_separator_menu_item() {
        let dbus_menu = vec![
            DbusMenuItem {
                id: 1,
                label: "".to_string(), // Empty label should become separator
                enabled: true,
                visible: true,
                icon_name: None,
                checkable: false,
                checked: false,
                shortcut: None,
                children: vec![],
            },
            DbusMenuItem {
                id: 2,
                label: "-".to_string(), // Dash should become separator
                enabled: true,
                visible: true,
                icon_name: None,
                checkable: false,
                checked: false,
                shortcut: None,
                children: vec![],
            }
        ];

        let tray_menu = convert_dbus_to_tray_menu(dbus_menu, "test-app");

        assert_eq!(tray_menu.len(), 2);
        assert!(tray_menu[0].is_separator);
        assert_eq!(tray_menu[0].label, "─");
        assert!(tray_menu[1].is_separator);
        assert_eq!(tray_menu[1].label, "─");
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
    fn test_property_parsing_edge_cases() {
        // Test with string values in wrong field types
        let properties = create_properties(vec![
            ("label", Value::Str("Test".into())),
            ("enabled", Value::Str("true".into())), // Wrong type - should default to true
            ("visible", Value::I32(1)), // Wrong type - should default to true
            ("toggle-state", Value::Str("1".into())), // Wrong type - should default to None
        ]);
        
        let menu_item = create_test_menu_item(7, properties, vec![]);
        
        let result = parse_menu_item_recursive(&menu_item).unwrap();
        
        assert_eq!(result.label, "Test");
        assert!(result.enabled); // Should fall back to default
        assert!(result.visible); // Should fall back to default
        assert!(!result.checked); // Should fall back to false since toggle-state is invalid
    }

    #[test]
    fn test_parse_dbus_menu_layout() {
        let child1_props = create_properties(vec![
            ("label", Value::Str("Item 1".into())),
        ]);
        let child1 = create_test_menu_item(1, child1_props, vec![]);

        let child2_props = create_properties(vec![
            ("label", Value::Str("Item 2".into())),
        ]);
        let child2 = create_test_menu_item(2, child2_props, vec![]);

        // Create layout tuple (id, properties, children)
        let layout = (
            0i32, // root id
            HashMap::new(), // root properties (empty)
            vec![child1, child2], // children
        );

        let result = parse_dbus_menu_layout(layout).unwrap();
        
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].label, "Item 1");
        assert_eq!(result[1].label, "Item 2");
    }

    #[test]
    fn test_malformed_children_are_skipped() {
        // Create one valid and one invalid child
        let valid_child_props = create_properties(vec![
            ("label", Value::Str("Valid Item".into())),
        ]);
        let valid_child = create_test_menu_item(1, valid_child_props, vec![]);

        // Create invalid child (not a structure)
        let invalid_child = Value::Str("not a structure").into();

        let layout = (0i32, HashMap::new(), vec![valid_child, invalid_child]);

        let result = parse_dbus_menu_layout(layout).unwrap();
        
        // Should only contain the valid child
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].label, "Valid Item");
    }
}