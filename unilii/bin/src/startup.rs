use crate::cli::RunOptions;
use deskhalloumi_core::config::PanelConfig;
use iced::{Point, Size, window};

pub fn default_panel_config() -> PanelConfig {
    PanelConfig {
        name: "default".to_string(),
        width: 1024,
        height: 24,
        position_x: 0,
        position_y: 0,
        background_color: Some("#1e1e1e".to_string()),
        text_color: Some("#ffffff".to_string()),
    }
}

pub fn build_window_settings(panel: &PanelConfig, run_options: &RunOptions) -> window::Settings {
    let window_position = window::Position::Specific(Point {
        x: panel.position_x as f32,
        y: panel.position_y as f32,
    });

    let mut window_settings = window::Settings {
        size: Size::new(panel.width as f32, panel.height as f32),
        position: window_position,
        resizable: false,
        decorations: false,
        level: window::Level::AlwaysOnTop,
        ..window::Settings::default()
    };

    #[cfg(target_os = "linux")]
    {
        window_settings.platform_specific = window::settings::PlatformSpecific {
            application_id: "com.unilii.bar".to_string(),
            override_redirect: !run_options.debug_focus,
        };
        if run_options.debug_focus {
            window_settings.decorations = true;
            window_settings.resizable = true;
            window_settings.level = window::Level::Normal;
        }
    }

    window_settings
}

#[cfg(test)]
mod tests {
    use super::{build_window_settings, default_panel_config};
    use crate::cli::RunOptions;

    #[test]
    fn default_panel_config_matches_legacy_main_defaults() {
        let panel = default_panel_config();

        assert_eq!(panel.name, "default");
        assert_eq!(panel.width, 1024);
        assert_eq!(panel.height, 24);
        assert_eq!(panel.position_x, 0);
        assert_eq!(panel.position_y, 0);
        assert_eq!(panel.background_color.as_deref(), Some("#1e1e1e"));
        assert_eq!(panel.text_color.as_deref(), Some("#ffffff"));
    }

    #[test]
    fn build_window_settings_uses_panel_geometry() {
        let mut panel = default_panel_config();
        panel.width = 777;
        panel.height = 33;
        panel.position_x = 8;
        panel.position_y = 13;

        let settings = build_window_settings(&panel, &RunOptions::default());

        assert_eq!(settings.size.width, 777.0);
        assert_eq!(settings.size.height, 33.0);
        assert!(!settings.resizable);
        assert!(!settings.decorations);
        assert_eq!(settings.level, iced::window::Level::AlwaysOnTop);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn debug_focus_enables_normal_resizable_decorated_window_on_linux() {
        let panel = default_panel_config();
        let run_options = RunOptions {
            debug_focus: true,
            ..RunOptions::default()
        };

        let settings = build_window_settings(&panel, &run_options);

        assert!(settings.decorations);
        assert!(settings.resizable);
        assert_eq!(settings.level, iced::window::Level::Normal);
        assert!(!settings.platform_specific.override_redirect);
        assert_eq!(settings.platform_specific.application_id, "com.unilii.bar");
    }
}
