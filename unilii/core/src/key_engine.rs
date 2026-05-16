//! Deterministic keybinding trigger engine for press/release/modrelease semantics.

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum KeyTrigger {
    #[default]
    Press,
    Release,
    Modrelease,
    Repeat,
}

#[derive(Debug, Clone)]
pub struct EngineBinding {
    pub name: String,
    pub required_groups: Vec<Vec<String>>,
    pub trigger: KeyTrigger,
    pub priority: u16,
    pub consume: bool,
    pub hold_ms: u64,
    pub cooldown_ms: Option<u64>,
    pub trigger_keys: HashSet<String>,
    pub required_keys: HashSet<String>,
    pub specificity: usize,
}

impl EngineBinding {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        name: String,
        required_groups: Vec<Vec<String>>,
        trigger: KeyTrigger,
        priority: u16,
        consume: bool,
        hold_ms: u64,
        cooldown_ms: Option<u64>,
        trigger_keys: Vec<String>,
    ) -> Self {
        let required_keys = required_groups
            .iter()
            .flat_map(|group| group.iter().cloned())
            .collect::<HashSet<_>>();

        Self {
            name,
            specificity: required_groups.len(),
            required_groups,
            trigger,
            priority,
            consume,
            hold_ms,
            cooldown_ms,
            trigger_keys: trigger_keys.into_iter().collect(),
            required_keys,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyEngineTraceReason {
    Matched,
    Suppressed,
    Invalidated,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyEngineTrace {
    pub index: usize,
    pub binding_name: String,
    pub reason: KeyEngineTraceReason,
    pub detail: String,
}

#[derive(Debug, Clone, Default)]
pub struct KeyEngineOutput {
    pub triggered: Vec<usize>,
    pub traces: Vec<KeyEngineTrace>,
}

#[derive(Debug, Default)]
pub struct KeyEngine {
    bindings: Vec<EngineBinding>,
    pressed: HashSet<String>,
    pressed_since: HashMap<String, Instant>,
    press_already_triggered: HashSet<usize>,
    release_armed: HashSet<usize>,
    cooldown_until: HashMap<usize, Instant>,
}

impl KeyEngine {
    pub fn new(bindings: Vec<EngineBinding>) -> Self {
        Self {
            bindings,
            ..Self::default()
        }
    }

    pub fn process_event(&mut self, key: &str, value: i32, now: Instant) -> KeyEngineOutput {
        match value {
            1 => self.handle_press(key, now),
            0 => self.handle_release(key, now),
            _ => KeyEngineOutput::default(),
        }
    }

    fn handle_press(&mut self, key: &str, now: Instant) -> KeyEngineOutput {
        self.pressed.insert(key.to_string());
        self.pressed_since.entry(key.to_string()).or_insert(now);

        let mut traces = Vec::new();
        let mut candidates = Vec::new();

        for index in 0..self.bindings.len() {
            let binding = &self.bindings[index];
            if !self.matches_binding(index) {
                continue;
            }

            match binding.trigger {
                KeyTrigger::Press | KeyTrigger::Repeat => {
                    if self.press_already_triggered.contains(&index) {
                        traces.push(self.trace(
                            index,
                            KeyEngineTraceReason::Suppressed,
                            "already_triggered",
                        ));
                        continue;
                    }
                    if self.is_on_cooldown(index, now) {
                        traces.push(self.trace(
                            index,
                            KeyEngineTraceReason::Suppressed,
                            "cooldown_active",
                        ));
                        continue;
                    }
                    candidates.push(index);
                }
                KeyTrigger::Release | KeyTrigger::Modrelease => {
                    self.release_armed.insert(index);
                }
            }
        }

        let triggered = self.resolve_and_mark(candidates, now, &mut traces);
        for index in &triggered {
            self.press_already_triggered.insert(*index);
        }

        KeyEngineOutput { triggered, traces }
    }

    fn handle_release(&mut self, key: &str, now: Instant) -> KeyEngineOutput {
        let held_for = self
            .pressed_since
            .remove(key)
            .map(|t| now.saturating_duration_since(t))
            .unwrap_or(Duration::ZERO);

        self.pressed.remove(key);

        let mut traces = Vec::new();
        let mut candidates = Vec::new();
        let armed = self.release_armed.iter().copied().collect::<Vec<_>>();

        for index in armed {
            let binding = &self.bindings[index];
            if !binding.required_keys.contains(key) {
                continue;
            }

            let should_fire = match binding.trigger {
                KeyTrigger::Release => binding.trigger_keys.contains(key),
                KeyTrigger::Modrelease => {
                    is_modifier_key(key)
                        && binding.required_keys.contains(key)
                        && held_for.as_millis() >= binding.hold_ms as u128
                }
                _ => false,
            };

            if should_fire {
                if self.is_on_cooldown(index, now) {
                    traces.push(self.trace(
                        index,
                        KeyEngineTraceReason::Suppressed,
                        "cooldown_active",
                    ));
                    self.release_armed.remove(&index);
                    continue;
                }
                candidates.push(index);
            } else if !self.matches_binding(index) {
                self.release_armed.remove(&index);
                traces.push(self.trace(
                    index,
                    KeyEngineTraceReason::Invalidated,
                    "chord_invalidated",
                ));
            }
        }

        let triggered = self.resolve_and_mark(candidates, now, &mut traces);
        for index in &triggered {
            self.release_armed.remove(index);
        }

        let stale_press_indices = self
            .press_already_triggered
            .iter()
            .copied()
            .filter(|index| !self.matches_binding(*index))
            .collect::<Vec<_>>();
        for index in stale_press_indices {
            self.press_already_triggered.remove(&index);
        }

        KeyEngineOutput { triggered, traces }
    }

    fn resolve_and_mark(
        &mut self,
        mut candidates: Vec<usize>,
        now: Instant,
        traces: &mut Vec<KeyEngineTrace>,
    ) -> Vec<usize> {
        candidates.sort_by(|left, right| {
            let l = &self.bindings[*left];
            let r = &self.bindings[*right];
            r.priority
                .cmp(&l.priority)
                .then(r.specificity.cmp(&l.specificity))
                .then(left.cmp(right))
        });

        let mut triggered = Vec::new();
        let mut consumed = false;

        for index in candidates {
            if consumed {
                traces.push(self.trace(
                    index,
                    KeyEngineTraceReason::Suppressed,
                    "consumed_by_higher_priority",
                ));
                continue;
            }

            traces.push(self.trace(index, KeyEngineTraceReason::Matched, "triggered"));
            triggered.push(index);

            if let Some(cooldown_ms) = self.bindings[index].cooldown_ms {
                self.cooldown_until
                    .insert(index, now + Duration::from_millis(cooldown_ms));
            }

            if self.bindings[index].consume {
                consumed = true;
            }
        }

        triggered
    }

    fn is_on_cooldown(&self, index: usize, now: Instant) -> bool {
        self.cooldown_until
            .get(&index)
            .is_some_and(|deadline| *deadline > now)
    }

    fn matches_binding(&self, index: usize) -> bool {
        self.bindings[index]
            .required_groups
            .iter()
            .all(|alternatives| alternatives.iter().any(|key| self.pressed.contains(key)))
    }

    fn trace(&self, index: usize, reason: KeyEngineTraceReason, detail: &str) -> KeyEngineTrace {
        KeyEngineTrace {
            index,
            binding_name: self.bindings[index].name.clone(),
            reason,
            detail: detail.to_string(),
        }
    }
}

fn is_modifier_key(key: &str) -> bool {
    matches!(
        key,
        "KEY_LEFTSHIFT"
            | "KEY_RIGHTSHIFT"
            | "KEY_LEFTCTRL"
            | "KEY_RIGHTCTRL"
            | "KEY_LEFTALT"
            | "KEY_RIGHTALT"
            | "KEY_LEFTMETA"
            | "KEY_RIGHTMETA"
            | "KEY_LEFTSUPER"
            | "KEY_RIGHTSUPER"
    )
}
