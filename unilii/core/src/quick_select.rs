//! Renderer-neutral quick-select lifecycle.
//!
//! A quick-select session is deliberately one-shot: it presents an ordered set
//! of actions mapped to the canonical keyboard alphabet, and the next key either
//! activates exactly one action or aborts the session. There is no partially
//! entered prefix and no key is allowed to leak into the underlying UI while the
//! overlay is armed.

use std::fmt;

pub const QUICK_SELECT_ALPHABET: &str = "asdfhjklqwertyuiopzxcvbnm1234567890";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuickSelectOption<A> {
    pub shortcut: char,
    pub label: String,
    pub action: A,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuickSelectSession<A> {
    options: Vec<QuickSelectOption<A>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QuickSelectResolution<A> {
    Action {
        shortcut: char,
        label: String,
        action: A,
    },
    Aborted {
        key: Option<char>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QuickSelectError {
    EmptyAlphabet,
    DuplicateShortcut(char),
    TooManyActions { actions: usize, shortcuts: usize },
}

impl fmt::Display for QuickSelectError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyAlphabet => write!(f, "quick-select alphabet is empty"),
            Self::DuplicateShortcut(shortcut) => {
                write!(
                    f,
                    "quick-select alphabet contains duplicate key '{shortcut}'"
                )
            }
            Self::TooManyActions { actions, shortcuts } => write!(
                f,
                "quick-select has {actions} actions but only {shortcuts} shortcuts"
            ),
        }
    }
}

impl std::error::Error for QuickSelectError {}

impl<A> QuickSelectSession<A> {
    pub fn new(actions: impl IntoIterator<Item = (String, A)>) -> Result<Self, QuickSelectError> {
        Self::with_alphabet(actions, QUICK_SELECT_ALPHABET)
    }

    pub fn with_alphabet(
        actions: impl IntoIterator<Item = (String, A)>,
        alphabet: &str,
    ) -> Result<Self, QuickSelectError> {
        let shortcuts = normalized_shortcuts(alphabet)?;
        let actions = actions.into_iter().collect::<Vec<_>>();
        if actions.len() > shortcuts.len() {
            return Err(QuickSelectError::TooManyActions {
                actions: actions.len(),
                shortcuts: shortcuts.len(),
            });
        }
        Ok(Self {
            options: actions
                .into_iter()
                .zip(shortcuts)
                .map(|((label, action), shortcut)| QuickSelectOption {
                    shortcut,
                    label,
                    action,
                })
                .collect(),
        })
    }

    pub fn options(&self) -> &[QuickSelectOption<A>] {
        &self.options
    }

    pub fn is_empty(&self) -> bool {
        self.options.is_empty()
    }

    pub fn resolve(self, key: Option<char>) -> QuickSelectResolution<A> {
        let normalized = key.map(|key| key.to_ascii_lowercase());
        if let Some(position) = normalized.and_then(|key| {
            self.options
                .iter()
                .position(|option| option.shortcut == key)
        }) {
            let option = self
                .options
                .into_iter()
                .nth(position)
                .expect("valid position");
            return QuickSelectResolution::Action {
                shortcut: option.shortcut,
                label: option.label,
                action: option.action,
            };
        }
        QuickSelectResolution::Aborted { key: normalized }
    }
}

fn normalized_shortcuts(alphabet: &str) -> Result<Vec<char>, QuickSelectError> {
    let mut shortcuts = Vec::new();
    for shortcut in alphabet
        .chars()
        .filter(|shortcut| !shortcut.is_whitespace())
    {
        let shortcut = shortcut.to_ascii_lowercase();
        if shortcuts.contains(&shortcut) {
            return Err(QuickSelectError::DuplicateShortcut(shortcut));
        }
        shortcuts.push(shortcut);
    }
    if shortcuts.is_empty() {
        return Err(QuickSelectError::EmptyAlphabet);
    }
    Ok(shortcuts)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_actions_to_canonical_home_row_order() {
        let session = QuickSelectSession::new([
            ("Alpha".to_string(), 1),
            ("Beta".to_string(), 2),
            ("Gamma".to_string(), 3),
        ])
        .unwrap();
        assert_eq!(
            session
                .options()
                .iter()
                .map(|option| option.shortcut)
                .collect::<Vec<_>>(),
            vec!['a', 's', 'd']
        );
    }

    #[test]
    fn mapped_key_executes_and_terminates_session() {
        let session = QuickSelectSession::new([("Beta".to_string(), 2)]).unwrap();
        assert_eq!(
            session.resolve(Some('A')),
            QuickSelectResolution::Action {
                shortcut: 'a',
                label: "Beta".to_string(),
                action: 2,
            }
        );
    }

    #[test]
    fn every_unmapped_or_named_key_aborts() {
        let session = QuickSelectSession::new([("Alpha".to_string(), 1)]).unwrap();
        assert_eq!(
            session.resolve(Some('x')),
            QuickSelectResolution::Aborted { key: Some('x') }
        );
        let session = QuickSelectSession::new([("Alpha".to_string(), 1)]).unwrap();
        assert_eq!(
            session.resolve(None),
            QuickSelectResolution::Aborted { key: None }
        );
    }

    #[test]
    fn refuses_to_create_partially_bound_overlay() {
        let result = QuickSelectSession::with_alphabet(
            [("one".to_string(), 1), ("two".to_string(), 2)],
            "a",
        );
        assert_eq!(
            result,
            Err(QuickSelectError::TooManyActions {
                actions: 2,
                shortcuts: 1,
            })
        );
    }
}
