//! Utilities for importing sxhkd configuration into unilii keybindings.

use crate::key_engine::KeyTrigger;
use crate::keys::{CommandType, KeyBinding};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportWarning {
    pub line: usize,
    pub message: String,
}

#[derive(Debug, Clone, Default)]
pub struct ImportResult {
    pub bindings: Vec<KeyBinding>,
    pub warnings: Vec<ImportWarning>,
}

pub fn import_sxhkd_config(content: &str) -> ImportResult {
    let mut result = ImportResult::default();
    let mut pending_chord: Option<(usize, String)> = None;
    let mut index = 0usize;

    for (line_no, raw_line) in content.lines().enumerate() {
        let line_idx = line_no + 1;
        let line = raw_line.trim_end();
        let trimmed = line.trim();

        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        let is_command_line = raw_line.chars().next().is_some_and(|ch| ch.is_whitespace());

        if is_command_line {
            if let Some((chord_line, chord_raw)) = pending_chord.take() {
                let command = trimmed.to_string();
                if command.is_empty() {
                    result.warnings.push(ImportWarning {
                        line: line_idx,
                        message: "empty sxhkd command line".to_string(),
                    });
                    continue;
                }

                index += 1;
                let (keysym, release) = normalize_sxhkd_chord(&chord_raw);
                if keysym.is_empty() {
                    result.warnings.push(ImportWarning {
                        line: chord_line,
                        message: "failed to normalize sxhkd chord".to_string(),
                    });
                    continue;
                }

                if chord_raw.contains('{') || chord_raw.contains('}') {
                    result.warnings.push(ImportWarning {
                        line: chord_line,
                        message:
                            "brace expansions are not fully supported; imported as literal chord"
                                .to_string(),
                    });
                }

                result.bindings.push(KeyBinding {
                    name: format!("sxhkd_import_{}", index),
                    keysym,
                    command,
                    command_type: CommandType::Shell,
                    release,
                    trigger: if release {
                        KeyTrigger::Release
                    } else {
                        KeyTrigger::Press
                    },
                    hold_ms: None,
                    cooldown_ms: None,
                    priority: 0,
                    consume: false,
                });
            } else {
                result.warnings.push(ImportWarning {
                    line: line_idx,
                    message: "command without preceding chord".to_string(),
                });
            }
            continue;
        }

        if let Some((pending_line, pending)) =
            pending_chord.replace((line_idx, trimmed.to_string()))
        {
            result.warnings.push(ImportWarning {
                line: pending_line,
                message: format!(
                    "chord '{}' was replaced before a command line was found",
                    pending
                ),
            });
        }
    }

    if let Some((line, chord)) = pending_chord {
        result.warnings.push(ImportWarning {
            line,
            message: format!("chord '{}' has no command", chord),
        });
    }

    result
}

fn normalize_sxhkd_chord(input: &str) -> (String, bool) {
    let mut chord = input.trim().to_string();
    let mut release = false;

    if let Some(rest) = chord.strip_prefix('@') {
        release = true;
        chord = rest.trim().to_string();
    }

    let keysym = chord
        .split('+')
        .map(|part| part.trim())
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("+");

    (keysym, release)
}
