#![allow(dead_code)]
// FIXME(T6): Shared menu traits/helpers are planned MenuModel transition surface and tested in place.

use super::types::MenuLifecycleState;

pub trait SnapshotProvider {
    type Snapshot;

    fn snapshot(&self) -> Self::Snapshot;
}

pub trait FilterableMenu {
    type ItemId: Clone + Eq;

    fn filter_tokens_for(&self, item_id: &Self::ItemId) -> Vec<String>;

    fn matches_filter_query(&self, item_id: &Self::ItemId, query: &str) -> bool {
        let tokens = self
            .filter_tokens_for(item_id)
            .into_iter()
            .map(|token| token.to_ascii_lowercase())
            .collect::<Vec<_>>();
        let terms = query
            .split_whitespace()
            .map(|term| term.to_ascii_lowercase())
            .collect::<Vec<_>>();
        terms
            .iter()
            .all(|term| tokens.iter().any(|token| token.contains(term)))
    }
}

pub trait QuickjumpMenu {
    type ItemId: Clone + Eq;

    fn quickjump_targets(&self) -> Vec<Self::ItemId>;

    fn quickjump_alphabet(&self) -> String {
        "asdfjkl;ghqwertyuiopzxcvbnm".to_string()
    }

    fn quickjump_bindings(&self) -> Vec<(String, Self::ItemId)> {
        let targets = self.quickjump_targets();
        let labels = generate_quickjump_labels(targets.len(), &self.quickjump_alphabet());
        labels.into_iter().zip(targets).collect()
    }
}

pub fn generate_quickjump_labels(target_count: usize, alphabet: &str) -> Vec<String> {
    if target_count == 0 {
        return Vec::new();
    }
    let chars = alphabet.chars().collect::<Vec<_>>();
    if chars.is_empty() {
        return Vec::new();
    }

    let mut labels = Vec::with_capacity(target_count);
    for ch in &chars {
        labels.push(ch.to_string());
        if labels.len() == target_count {
            return labels;
        }
    }

    for first in &chars {
        for second in &chars {
            labels.push(format!("{}{}", first, second));
            if labels.len() == target_count {
                return labels;
            }
        }
    }

    labels.truncate(target_count);
    labels
}

pub trait MenuController {
    type Snapshot;

    fn lifecycle_state(&self) -> &MenuLifecycleState;
    fn lifecycle_state_mut(&mut self) -> &mut MenuLifecycleState;
    fn apply_snapshot(&mut self, snapshot: Self::Snapshot);

    fn set_busy(&mut self, action_id: impl Into<String>) {
        *self.lifecycle_state_mut() = MenuLifecycleState::Busy {
            action_id: action_id.into(),
        };
    }

    fn set_error(
        &mut self,
        scope: impl Into<String>,
        message: impl Into<String>,
        recoverable: bool,
    ) {
        *self.lifecycle_state_mut() = MenuLifecycleState::Error {
            scope: scope.into(),
            message: message.into(),
            recoverable,
        };
    }

    fn set_stale(&mut self) {
        *self.lifecycle_state_mut() = MenuLifecycleState::Stale;
    }

    fn close(&mut self) {
        *self.lifecycle_state_mut() = MenuLifecycleState::Closed;
    }
}

#[cfg(test)]
mod tests {
    use super::generate_quickjump_labels;

    #[test]
    fn quickjump_labels_expand_to_two_chars() {
        let labels = generate_quickjump_labels(6, "as");
        assert_eq!(labels, vec!["a", "s", "aa", "as", "sa", "ss"]);
    }
}
