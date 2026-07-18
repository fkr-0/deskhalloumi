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

                let (keysym, release, replay) = normalize_sxhkd_chord(&chord_raw);
                if keysym.is_empty() {
                    result.warnings.push(ImportWarning {
                        line: chord_line,
                        message: "failed to normalize sxhkd chord".to_string(),
                    });
                    continue;
                }

                if replay {
                    result.warnings.push(ImportWarning {
                        line: chord_line,
                        message: "sxhkd replay prefix '~' has no native unilii equivalent; the binding is imported without replay semantics"
                            .to_string(),
                    });
                }

                if keysym.contains(';') {
                    result.warnings.push(ImportWarning {
                        line: chord_line,
                        message:
                            "sxhkd chord chains/modes using ';' are unsupported and were skipped"
                                .to_string(),
                    });
                    continue;
                }

                let chords = match expand_simple_braces(&keysym) {
                    Ok(values) => values,
                    Err(error) => {
                        result.warnings.push(ImportWarning {
                            line: chord_line,
                            message: format!(
                                "unsupported chord expansion: {error}; binding skipped"
                            ),
                        });
                        continue;
                    }
                };
                let commands = match expand_simple_braces(&command) {
                    Ok(values) => values,
                    Err(error) => {
                        result.warnings.push(ImportWarning {
                            line: line_idx,
                            message: format!(
                                "unsupported command expansion: {error}; binding skipped"
                            ),
                        });
                        continue;
                    }
                };
                let expanded = match pair_expansions(chords, commands) {
                    Ok(values) => values,
                    Err(error) => {
                        result.warnings.push(ImportWarning {
                            line: chord_line,
                            message: format!("brace expansion mismatch: {error}; binding skipped"),
                        });
                        continue;
                    }
                };

                for (keysym, command) in expanded {
                    index += 1;
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
                }
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

fn normalize_sxhkd_chord(input: &str) -> (String, bool, bool) {
    let mut chord = input.trim().to_string();
    let mut release = false;
    let mut replay = false;

    loop {
        if let Some(rest) = chord.strip_prefix('@') {
            release = true;
            chord = rest.trim().to_string();
        } else if let Some(rest) = chord.strip_prefix('~') {
            replay = true;
            chord = rest.trim().to_string();
        } else {
            break;
        }
    }

    let keysym = chord
        .split('+')
        .map(|part| part.trim())
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("+");

    (keysym, release, replay)
}

fn expand_simple_braces(input: &str) -> Result<Vec<String>, String> {
    let Some(open) = input.find('{') else {
        if input.contains('}') {
            return Err("closing brace without opening brace".to_string());
        }
        return Ok(vec![input.to_string()]);
    };
    let Some(relative_close) = input[open + 1..].find('}') else {
        return Err("opening brace without closing brace".to_string());
    };
    let close = open + 1 + relative_close;
    let body = &input[open + 1..close];
    if body.contains(['{', '}']) || input[close + 1..].starts_with('}') {
        return Err("nested braces are unsupported".to_string());
    }
    let alternatives = body.split(',').map(str::trim).collect::<Vec<_>>();
    if alternatives.len() < 2 || alternatives.iter().any(|value| value.is_empty()) {
        return Err(format!(
            "'{{{body}}}' is not a simple comma-separated expansion"
        ));
    }

    let suffixes = expand_simple_braces(&input[close + 1..])?;
    let prefix = &input[..open];
    let mut output = Vec::with_capacity(alternatives.len() * suffixes.len());
    for alternative in alternatives {
        for suffix in &suffixes {
            output.push(format!("{prefix}{alternative}{suffix}"));
        }
    }
    Ok(output)
}

fn pair_expansions(
    chords: Vec<String>,
    commands: Vec<String>,
) -> Result<Vec<(String, String)>, String> {
    match (chords.len(), commands.len()) {
        (_, 1) => Ok(chords
            .into_iter()
            .map(|chord| (chord, commands[0].clone()))
            .collect()),
        (left, right) if left == right => Ok(chords.into_iter().zip(commands).collect()),
        (left, right) => Err(format!(
            "{left} chord alternatives but {right} command alternatives"
        )),
    }
}
