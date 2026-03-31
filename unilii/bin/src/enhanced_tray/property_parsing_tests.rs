#[cfg(test)]
mod property_parsing_tests {
    use super::super::dbus::*;
    use zbus::zvariant::{Value, Dict};
    use std::collections::HashMap;

    /// Test helper for property extraction functions
    fn create_test_dict(entries: Vec<(&str, Value<'_>)>) -> Dict<'_, '_> {
        let map: HashMap<&str, Value<'_>> = entries.into_iter().collect();
        Dict::try_from(map).unwrap()
    }

    #[test]
    fn test_extract_string_property_valid() {
        let dict = create_test_dict(vec![
            ("label", Value::Str("Test Label".into())),
            ("icon-name", Value::Str("test-icon".into())),
            ("shortcut", Value::Str("Ctrl+T".into())),
        ]);

        assert_eq!(extract_string_property(&dict, "label"), Some("Test Label".to_string()));
        assert_eq!(extract_string_property(&dict, "icon-name"), Some("test-icon".to_string()));
        assert_eq!(extract_string_property(&dict, "shortcut"), Some("Ctrl+T".to_string()));
    }

    #[test]
    fn test_extract_string_property_missing() {
        let dict = create_test_dict(vec![
            ("label", Value::Str("Test Label".into())),
        ]);

        assert_eq!(extract_string_property(&dict, "missing-key"), None);
        assert_eq!(extract_string_property(&dict, "icon-name"), None);
    }

    #[test] 
    fn test_extract_string_property_wrong_type() {
        let dict = create_test_dict(vec![
            ("label", Value::I32(42)), // Wrong type - should be string
            ("enabled", Value::Bool(true)), // Wrong type for string property
        ]);

        assert_eq!(extract_string_property(&dict, "label"), None);
        assert_eq!(extract_string_property(&dict, "enabled"), None);
    }

    #[test]
    fn test_extract_bool_property_valid() {
        let dict = create_test_dict(vec![
            ("enabled", Value::Bool(true)),
            ("visible", Value::Bool(false)),
        ]);

        assert_eq!(extract_bool_property(&dict, "enabled"), Some(true));
        assert_eq!(extract_bool_property(&dict, "visible"), Some(false));
    }

    #[test]
    fn test_extract_bool_property_missing() {
        let dict = create_test_dict(vec![
            ("enabled", Value::Bool(true)),
        ]);

        assert_eq!(extract_bool_property(&dict, "missing"), None);
        assert_eq!(extract_bool_property(&dict, "visible"), None);
    }

    #[test]
    fn test_extract_bool_property_wrong_type() {
        let dict = create_test_dict(vec![
            ("enabled", Value::Str("true".into())), // Wrong type
            ("visible", Value::I32(1)), // Wrong type
        ]);

        assert_eq!(extract_bool_property(&dict, "enabled"), None);
        assert_eq!(extract_bool_property(&dict, "visible"), None);
    }

    #[test]
    fn test_extract_int_property_valid() {
        let dict = create_test_dict(vec![
            ("toggle-state", Value::I32(1)),
            ("priority", Value::I32(0)),
            ("negative", Value::I32(-5)),
        ]);

        assert_eq!(extract_int_property(&dict, "toggle-state"), Some(1));
        assert_eq!(extract_int_property(&dict, "priority"), Some(0));
        assert_eq!(extract_int_property(&dict, "negative"), Some(-5));
    }

    #[test]
    fn test_extract_int_property_missing() {
        let dict = create_test_dict(vec![
            ("toggle-state", Value::I32(1)),
        ]);

        assert_eq!(extract_int_property(&dict, "missing"), None);
        assert_eq!(extract_int_property(&dict, "priority"), None);
    }

    #[test]
    fn test_extract_int_property_wrong_type() {
        let dict = create_test_dict(vec![
            ("toggle-state", Value::Str("1".into())), // Wrong type
            ("priority", Value::Bool(true)), // Wrong type 
        ]);

        assert_eq!(extract_int_property(&dict, "toggle-state"), None);
        assert_eq!(extract_int_property(&dict, "priority"), None);
    }

    #[test]
    fn test_toggle_type_detection() {
        // Test checkmark toggle
        let checkmark_dict = create_test_dict(vec![
            ("toggle-type", Value::Str("checkmark".into())),
            ("toggle-state", Value::I32(1)),
        ]);

        let toggle_type = extract_string_property(&checkmark_dict, "toggle-type").unwrap_or_default();
        let toggle_state = extract_int_property(&checkmark_dict, "toggle-state").unwrap_or(0);
        
        let checkable = !toggle_type.is_empty() && (toggle_type == "checkmark" || toggle_type == "radio");
        let checked = toggle_state != 0;

        assert!(checkable);
        assert!(checked);
        assert_eq!(toggle_type, "checkmark");

        // Test radio toggle
        let radio_dict = create_test_dict(vec![
            ("toggle-type", Value::Str("radio".into())),
            ("toggle-state", Value::I32(0)),
        ]);

        let toggle_type = extract_string_property(&radio_dict, "toggle-type").unwrap_or_default();
        let toggle_state = extract_int_property(&radio_dict, "toggle-state").unwrap_or(0);
        
        let checkable = !toggle_type.is_empty() && (toggle_type == "checkmark" || toggle_type == "radio");
        let checked = toggle_state != 0;

        assert!(checkable);
        assert!(!checked);
        assert_eq!(toggle_type, "radio");

        // Test invalid toggle type
        let invalid_dict = create_test_dict(vec![
            ("toggle-type", Value::Str("invalid".into())),
            ("toggle-state", Value::I32(1)),
        ]);

        let toggle_type = extract_string_property(&invalid_dict, "toggle-type").unwrap_or_default();
        let checkable = !toggle_type.is_empty() && (toggle_type == "checkmark" || toggle_type == "radio");

        assert!(!checkable);
        assert_eq!(toggle_type, "invalid");
    }

    #[test]
    fn test_empty_properties_defaults() {
        let empty_dict = create_test_dict(vec![]);

        // Test that missing properties return appropriate defaults
        assert_eq!(extract_string_property(&empty_dict, "label"), None);
        assert_eq!(extract_string_property(&empty_dict, "icon-name"), None);
        assert_eq!(extract_string_property(&empty_dict, "shortcut"), None);
        assert_eq!(extract_string_property(&empty_dict, "toggle-type"), None);
        
        assert_eq!(extract_bool_property(&empty_dict, "enabled"), None);
        assert_eq!(extract_bool_property(&empty_dict, "visible"), None);
        
        assert_eq!(extract_int_property(&empty_dict, "toggle-state"), None);
    }

    #[test]
    fn test_property_extraction_comprehensive() {
        let comprehensive_dict = create_test_dict(vec![
            ("label", Value::Str("Comprehensive Test Item".into())),
            ("enabled", Value::Bool(true)),
            ("visible", Value::Bool(false)),
            ("icon-name", Value::Str("dialog-information".into())),
            ("shortcut", Value::Str("Ctrl+Shift+I".into())),
            ("toggle-type", Value::Str("checkmark".into())),
            ("toggle-state", Value::I32(1)),
        ]);

        // Test all property extractions
        assert_eq!(extract_string_property(&comprehensive_dict, "label"), 
                  Some("Comprehensive Test Item".to_string()));
        assert_eq!(extract_bool_property(&comprehensive_dict, "enabled"), Some(true));
        assert_eq!(extract_bool_property(&comprehensive_dict, "visible"), Some(false));
        assert_eq!(extract_string_property(&comprehensive_dict, "icon-name"), 
                  Some("dialog-information".to_string()));
        assert_eq!(extract_string_property(&comprehensive_dict, "shortcut"), 
                  Some("Ctrl+Shift+I".to_string()));
        assert_eq!(extract_string_property(&comprehensive_dict, "toggle-type"), 
                  Some("checkmark".to_string()));
        assert_eq!(extract_int_property(&comprehensive_dict, "toggle-state"), Some(1));
    }

    #[test]
    fn test_mixed_type_properties_robustness() {
        // Test Dict with mixed types including some invalid ones
        let mixed_dict = create_test_dict(vec![
            ("label", Value::Str("Mixed Test".into())),
            ("enabled", Value::Str("not-a-bool".into())), // Wrong type
            ("visible", Value::Bool(true)),
            ("toggle-state", Value::Str("1".into())), // Wrong type
            ("valid-int", Value::I32(42)),
        ]);

        // Valid extractions should work
        assert_eq!(extract_string_property(&mixed_dict, "label"), Some("Mixed Test".to_string()));
        assert_eq!(extract_bool_property(&mixed_dict, "visible"), Some(true));
        assert_eq!(extract_int_property(&mixed_dict, "valid-int"), Some(42));

        // Invalid type extractions should return None
        assert_eq!(extract_bool_property(&mixed_dict, "enabled"), None);
        assert_eq!(extract_int_property(&mixed_dict, "toggle-state"), None);
    }
}