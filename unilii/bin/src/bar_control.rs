//! Pure state transitions for runtime bar controls.

use deskhalloumi_core::config::{Config, load_config_strict};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

#[derive(Debug, Default)]
pub struct BarControlState {
    hidden_modules: HashSet<String>,
    focused_module: Option<String>,
    status: Option<String>,
    reload_generation: u64,
}

impl BarControlState {
    pub fn is_hidden(&self, module: &str) -> bool {
        self.hidden_modules.contains(module)
    }

    pub fn is_focused(&self, module: &str) -> bool {
        self.focused_module.as_deref() == Some(module)
    }

    pub fn status(&self) -> Option<&str> {
        self.status.as_deref()
    }

    pub fn dismiss_status(&mut self) {
        self.status = None;
    }

    pub fn set_status(&mut self, message: impl Into<String>) {
        self.status = Some(message.into());
    }

    pub fn begin_reload(&mut self) -> u64 {
        self.reload_generation = self.reload_generation.wrapping_add(1).max(1);
        self.reload_generation
    }

    pub fn is_current_reload(&self, generation: u64) -> bool {
        generation == self.reload_generation
    }

    pub fn toggle_module(
        &mut self,
        available: &HashSet<String>,
        requested: &str,
    ) -> Result<bool, String> {
        let module = resolve_module_name(available, requested)?;
        let visible = if self.hidden_modules.remove(&module) {
            true
        } else {
            self.hidden_modules.insert(module.clone());
            if self.focused_module.as_deref() == Some(module.as_str()) {
                self.focused_module = None;
            }
            false
        };
        self.status = Some(format!(
            "{} {}",
            if visible { "Shown" } else { "Hidden" },
            module
        ));
        Ok(visible)
    }

    pub fn focus_module(
        &mut self,
        available: &HashSet<String>,
        requested: &str,
    ) -> Result<String, String> {
        let module = resolve_module_name(available, requested)?;
        self.hidden_modules.remove(&module);
        self.focused_module = Some(module.clone());
        self.status = Some(format!("Focused {module}"));
        Ok(module)
    }

    pub fn show_all_modules(&mut self) -> usize {
        let shown = self.hidden_modules.len();
        self.hidden_modules.clear();
        self.status = Some(if shown == 0 {
            "All modules are already visible".to_string()
        } else {
            format!("Shown {shown} hidden module(s)")
        });
        shown
    }

    pub fn clear_module_focus(&mut self) -> bool {
        let cleared = self.focused_module.take().is_some();
        self.status = Some(if cleared {
            "Cleared module focus".to_string()
        } else {
            "No module is focused".to_string()
        });
        cleared
    }
}

fn resolve_module_name(available: &HashSet<String>, requested: &str) -> Result<String, String> {
    let requested = requested.trim();
    if requested.is_empty() {
        return Err("module name must not be empty".to_string());
    }
    available
        .iter()
        .find(|name| name.eq_ignore_ascii_case(requested))
        .cloned()
        .ok_or_else(|| format!("unknown module '{requested}'"))
}

#[derive(Debug, Clone)]
pub struct ReloadCandidate {
    pub path: PathBuf,
    pub config: Config,
    pub restart_required: Vec<&'static str>,
}

pub fn load_reload_candidate(path: &Path, current: &Config) -> Result<ReloadCandidate, String> {
    let config = load_config_strict(path)?;
    let mut restart_required = Vec::new();
    if serialized_changed(&current.panels, &config.panels) {
        restart_required.push("panel geometry");
    }
    if serialized_changed(&current.modules, &config.modules) {
        restart_required.push("module topology");
    }
    if serialized_changed(&current.keybindings, &config.keybindings) {
        restart_required.push("embedded hotkeys");
    }
    Ok(ReloadCandidate {
        path: path.to_path_buf(),
        config,
        restart_required,
    })
}

pub fn apply_live_config(current: &mut Config, candidate: Config) {
    current.menus = candidate.menus;
}

fn serialized_changed<T: serde::Serialize>(left: &T, right: &T) -> bool {
    match (toml::to_string(left), toml::to_string(right)) {
        (Ok(left), Ok(right)) => left != right,
        _ => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn modules() -> HashSet<String> {
        ["clock", "battery"]
            .into_iter()
            .map(str::to_string)
            .collect()
    }

    #[test]
    fn toggle_and_focus_are_case_insensitive_and_deterministic() {
        let mut state = BarControlState::default();
        assert!(!state.toggle_module(&modules(), "CLOCK").unwrap());
        assert!(state.is_hidden("clock"));
        assert_eq!(state.focus_module(&modules(), "Clock").unwrap(), "clock");
        assert!(!state.is_hidden("clock"));
        assert!(state.is_focused("clock"));
    }

    #[test]
    fn unknown_module_does_not_mutate_state() {
        let mut state = BarControlState::default();
        assert!(state.toggle_module(&modules(), "weather").is_err());
        assert!(state.status().is_none());
    }

    #[test]
    fn live_reload_only_applies_runtime_safe_menu_configuration() {
        let mut current = Config::default();
        let mut candidate = Config::default();
        candidate.panels[0].width = 1920;
        candidate.menus.ui.max_visible_rows = 24;
        apply_live_config(&mut current, candidate);
        assert_eq!(current.panels[0].width, 800);
        assert_eq!(current.menus.ui.max_visible_rows, 24);
    }

    #[test]
    fn newer_reload_generation_supersedes_older_result() {
        let mut state = BarControlState::default();
        let first = state.begin_reload();
        let second = state.begin_reload();
        assert!(!state.is_current_reload(first));
        assert!(state.is_current_reload(second));
    }

    #[test]
    fn recovery_actions_restore_visibility_and_focus() {
        let mut state = BarControlState::default();
        state.toggle_module(&modules(), "clock").unwrap();
        state.focus_module(&modules(), "battery").unwrap();
        assert_eq!(state.show_all_modules(), 1);
        assert!(!state.is_hidden("clock"));
        assert!(state.clear_module_focus());
        assert!(!state.is_focused("battery"));
    }
}
