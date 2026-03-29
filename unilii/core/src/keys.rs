//! Global keybinding daemon and parsing utilities.

use crate::Result;
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use tracing::{error, info, warn};

/// Global keybinding configuration.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct KeyBinding {
    pub name: String,
    pub keysym: String,
    pub command: String,
}

#[derive(Debug, Clone)]
struct ParsedBinding {
    binding: KeyBinding,
    /// Every group represents "one required key", where each entry in the
    /// group is accepted as an alternative (e.g. left or right modifier).
    required_groups: Vec<Vec<String>>,
}

/// Keybinding manager using unilii-lib evdev keyboard streams.
pub struct KeybindingDaemon {
    bindings: Vec<ParsedBinding>,
}

impl KeybindingDaemon {
    pub fn new(bindings: Vec<KeyBinding>) -> Self {
        let parsed = bindings
            .into_iter()
            .filter_map(|binding| match parse_binding(binding.clone()) {
                Ok(parsed) => Some(parsed),
                Err(message) => {
                    warn!(
                        "skipping invalid keybinding '{}' (keysym='{}'): {}",
                        binding.name, binding.keysym, message
                    );
                    None
                }
            })
            .collect();

        Self { bindings: parsed }
    }

    pub async fn run(&self) -> Result<()> {
        let listener = match unilii_lib::input::listen_keyboard_events_experimental() {
            Ok(stream) => {
                info!("hotkeys: listener initialized using experimental tokio-udev path");
                Ok(stream)
            }
            Err(error) => {
                warn!(
                    "hotkeys: experimental listener unavailable, falling back to base evdev: {}",
                    error
                );
                unilii_lib::input::listen_keyboard_events()
            }
        }?;

        let mut stream = listener;
        let mut pressed_keys: HashSet<String> = HashSet::new();
        let mut already_triggered: HashSet<usize> = HashSet::new();

        while let Some(event) = stream.next().await {
            let key_name = format!("{:?}", event.code);

            match event.value {
                1 => {
                    pressed_keys.insert(key_name);
                    self.check_bindings(&pressed_keys, &mut already_triggered)?;
                }
                0 => {
                    pressed_keys.remove(&key_name);
                    already_triggered.retain(|index| self.matches_binding(*index, &pressed_keys));
                }
                _ => {}
            }
        }

        Ok(())
    }

    fn check_bindings(
        &self,
        pressed: &HashSet<String>,
        already_triggered: &mut HashSet<usize>,
    ) -> Result<()> {
        for index in 0..self.bindings.len() {
            let matches = self.matches_binding(index, pressed);
            if matches && !already_triggered.contains(&index) {
                self.execute_binding(index)?;
                already_triggered.insert(index);
            }
        }
        Ok(())
    }

    fn matches_binding(&self, index: usize, pressed: &HashSet<String>) -> bool {
        self.bindings[index]
            .required_groups
            .iter()
            .all(|alternatives| alternatives.iter().any(|key| pressed.contains(key)))
    }

    fn execute_binding(&self, index: usize) -> Result<()> {
        let binding = &self.bindings[index].binding;
        let command = binding.command.trim();
        if command.is_empty() {
            return Ok(());
        }

        std::process::Command::new("sh")
            .arg("-lc")
            .arg(command)
            .spawn()
            .map_err(|error| {
                error!(
                    "hotkeys: failed to execute binding '{}' command='{}': {}",
                    binding.name, command, error
                );
                Box::new(error) as Box<dyn std::error::Error + Send + Sync>
            })?;

        info!(
            "hotkeys: executed binding '{}' keysym='{}'",
            binding.name, binding.keysym
        );
        Ok(())
    }
}

fn parse_binding(binding: KeyBinding) -> std::result::Result<ParsedBinding, String> {
    let groups = parse_keysym(&binding.keysym)?;
    if groups.is_empty() {
        return Err("no keys parsed".to_string());
    }

    Ok(ParsedBinding {
        binding,
        required_groups: groups,
    })
}

fn parse_keysym(keysym: &str) -> std::result::Result<Vec<Vec<String>>, String> {
    let mut groups = Vec::new();
    let mut seen = HashMap::<String, usize>::new();

    for token in keysym.split('+') {
        let alternatives = token_to_key_candidates(token)?;
        if alternatives.is_empty() {
            continue;
        }

        // De-duplicate logical keys while preserving order.
        let canonical = alternatives.join("|");
        if seen.contains_key(&canonical) {
            continue;
        }
        seen.insert(canonical, groups.len());
        groups.push(alternatives);
    }

    Ok(groups)
}

fn token_to_key_candidates(token: &str) -> std::result::Result<Vec<String>, String> {
    let raw = token.trim().to_ascii_uppercase().replace('-', "_");
    if raw.starts_with("KEY_") {
        return Ok(vec![raw]);
    }

    let normalized = normalize_token(token);
    if normalized.is_empty() {
        return Err("empty token".to_string());
    }

    let candidates = match normalized.as_str() {
        "SHIFT" => vec!["KEY_LEFTSHIFT".to_string(), "KEY_RIGHTSHIFT".to_string()],
        "CTRL" | "CONTROL" => vec!["KEY_LEFTCTRL".to_string(), "KEY_RIGHTCTRL".to_string()],
        "ALT" => vec!["KEY_LEFTALT".to_string(), "KEY_RIGHTALT".to_string()],
        "SUPER" | "META" | "WIN" | "WINDOWS" => {
            vec!["KEY_LEFTMETA".to_string(), "KEY_RIGHTMETA".to_string()]
        }
        "RETURN" | "ENTER" => vec!["KEY_ENTER".to_string()],
        "ESC" | "ESCAPE" => vec!["KEY_ESC".to_string()],
        "SPACE" => vec!["KEY_SPACE".to_string()],
        "TAB" => vec!["KEY_TAB".to_string()],
        "BACKSPACE" => vec!["KEY_BACKSPACE".to_string()],
        "DELETE" | "DEL" => vec!["KEY_DELETE".to_string()],
        "HOME" => vec!["KEY_HOME".to_string()],
        "END" => vec!["KEY_END".to_string()],
        "PAGEUP" => vec!["KEY_PAGEUP".to_string()],
        "PAGEDOWN" => vec!["KEY_PAGEDOWN".to_string()],
        "UP" => vec!["KEY_UP".to_string()],
        "DOWN" => vec!["KEY_DOWN".to_string()],
        "LEFT" => vec!["KEY_LEFT".to_string()],
        "RIGHT" => vec!["KEY_RIGHT".to_string()],
        _ if normalized.len() == 1 && normalized.chars().all(|c| c.is_ascii_alphabetic()) => {
            vec![format!("KEY_{}", normalized)]
        }
        _ if normalized.len() == 1 && normalized.chars().all(|c| c.is_ascii_digit()) => {
            vec![format!("KEY_{}", normalized)]
        }
        _ if normalized.starts_with('F')
            && normalized.len() <= 3
            && normalized[1..].chars().all(|c| c.is_ascii_digit()) =>
        {
            vec![format!("KEY_{}", normalized)]
        }
        _ => return Err(format!("unsupported key token '{}'", token.trim())),
    };

    Ok(candidates)
}

fn normalize_token(token: &str) -> String {
    token
        .chars()
        .filter(|c| !matches!(c, ' ' | '-' | '_'))
        .flat_map(|c| c.to_uppercase())
        .collect::<String>()
}

#[cfg(test)]
mod tests {
    use super::{parse_keysym, token_to_key_candidates};

    #[test]
    fn parses_modifiers_with_left_right_variants() {
        let parsed = parse_keysym("Super+Shift+q").expect("keysym should parse");
        assert_eq!(parsed.len(), 3);
        assert_eq!(parsed[0], vec!["KEY_LEFTMETA", "KEY_RIGHTMETA"]);
        assert_eq!(parsed[1], vec!["KEY_LEFTSHIFT", "KEY_RIGHTSHIFT"]);
        assert_eq!(parsed[2], vec!["KEY_Q"]);
    }

    #[test]
    fn parses_key_prefixed_tokens_without_changes() {
        let parsed = token_to_key_candidates("KEY_ENTER").expect("token should parse");
        assert_eq!(parsed, vec!["KEY_ENTER"]);
    }

    #[test]
    fn rejects_unknown_tokens() {
        let err = token_to_key_candidates("HyperMega").expect_err("token should fail");
        assert!(err.contains("unsupported key token"));
    }
}
