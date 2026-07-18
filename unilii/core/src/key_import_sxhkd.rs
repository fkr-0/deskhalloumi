//! Utilities for importing sxhkd configuration into unilii keybindings.

use crate::key_engine::KeyTrigger;
use crate::keys::{CommandType, KeyBinding};

const MAX_EXPANSIONS: usize = 4096;

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
                let (command, synchronous) = normalize_sxhkd_command(trimmed);
                if command.is_empty() {
                    result.warnings.push(ImportWarning {
                        line: line_idx,
                        message: "empty sxhkd command line".to_string(),
                    });
                    continue;
                }

                if synchronous {
                    result.warnings.push(ImportWarning {
                        line: line_idx,
                        message: "sxhkd synchronous command prefix ';' was removed; DeskHalloumi launches imported shell actions asynchronously"
                            .to_string(),
                    });
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
                    Ok(values) => values
                        .into_iter()
                        .map(|value| normalize_expanded_chord(&value))
                        .collect::<Vec<_>>(),
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
                    if keysym.is_empty() {
                        result.warnings.push(ImportWarning {
                            line: chord_line,
                            message: "brace expansion produced an empty chord; alternative skipped"
                                .to_string(),
                        });
                        continue;
                    }
                    if command.trim().is_empty() {
                        result.warnings.push(ImportWarning {
                            line: line_idx,
                            message: format!(
                                "brace expansion produced an empty command for chord '{keysym}'; alternative skipped"
                            ),
                        });
                        continue;
                    }
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

    (chord, release, replay)
}

fn normalize_expanded_chord(input: &str) -> String {
    input
        .split('+')
        .map(|part| part.trim())
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("+")
}

fn normalize_sxhkd_command(input: &str) -> (String, bool) {
    let trimmed = input.trim();
    if let Some(command) = trimmed.strip_prefix(';') {
        (command.trim_start().to_string(), true)
    } else {
        (trimmed.to_string(), false)
    }
}

fn expand_simple_braces(input: &str) -> Result<Vec<String>, String> {
    let Some(open) = find_unescaped_char(input, '{', 0) else {
        if find_unescaped_char(input, '}', 0).is_some() {
            return Err("closing brace without opening brace".to_string());
        }
        return Ok(vec![unescape_braces(input)]);
    };
    let Some(close) = find_unescaped_char(input, '}', open + 1) else {
        return Err("opening brace without closing brace".to_string());
    };
    if find_unescaped_char(input, '{', open + 1).is_some_and(|nested| nested < close) {
        return Err("nested braces are unsupported".to_string());
    }
    let body = &input[open + 1..close];
    if body.is_empty() {
        return Err("empty brace expansion".to_string());
    }

    let mut alternatives = Vec::new();
    for value in body.split(',').map(str::trim) {
        if value.is_empty() {
            return Err(format!(
                "'{{{body}}}' contains an empty sequence element; use '_'"
            ));
        }
        alternatives.extend(expand_brace_element(value)?);
    }

    let suffixes = expand_simple_braces(&input[close + 1..])?;
    let prefix = unescape_braces(&input[..open]);
    let output_len = alternatives
        .len()
        .checked_mul(suffixes.len())
        .ok_or_else(|| "brace expansion size overflow".to_string())?;
    if output_len > MAX_EXPANSIONS {
        return Err(format!(
            "brace expansion would produce {output_len} values (limit {MAX_EXPANSIONS})"
        ));
    }
    let mut output = Vec::with_capacity(output_len);
    for alternative in alternatives {
        for suffix in &suffixes {
            output.push(format!("{prefix}{alternative}{suffix}"));
        }
    }
    Ok(output)
}

fn expand_brace_element(value: &str) -> Result<Vec<String>, String> {
    if value == "_" {
        return Ok(vec![String::new()]);
    }

    let chars = value.chars().collect::<Vec<_>>();
    if chars.len() == 3 && chars[1] == '-' && chars[0].is_ascii_alphanumeric() {
        let start = chars[0];
        let end = chars[2];
        if !end.is_ascii_alphanumeric() || range_class(start) != range_class(end) {
            return Err(format!(
                "range '{value}' must stay within digits, lowercase letters, or uppercase letters"
            ));
        }
        let start = start as u8;
        let end = end as u8;
        let values = if start <= end {
            (start..=end).map(char::from).collect::<Vec<_>>()
        } else {
            (end..=start).rev().map(char::from).collect::<Vec<_>>()
        };
        return Ok(values.into_iter().map(String::from).collect());
    }

    Ok(vec![unescape_braces(value)])
}

fn range_class(value: char) -> u8 {
    if value.is_ascii_digit() {
        1
    } else if value.is_ascii_lowercase() {
        2
    } else if value.is_ascii_uppercase() {
        3
    } else {
        0
    }
}

fn find_unescaped_char(input: &str, target: char, start: usize) -> Option<usize> {
    input[start..]
        .char_indices()
        .map(|(offset, value)| (start + offset, value))
        .find_map(|(index, value)| (value == target && !is_escaped(input, index)).then_some(index))
}

fn is_escaped(input: &str, index: usize) -> bool {
    input.as_bytes()[..index]
        .iter()
        .rev()
        .take_while(|byte| **byte == b'\\')
        .count()
        % 2
        == 1
}

fn unescape_braces(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    while let Some(value) = chars.next() {
        if value == '\\' && chars.peek().is_some_and(|next| matches!(*next, '{' | '}')) {
            output.push(chars.next().unwrap_or_default());
        } else {
            output.push(value);
        }
    }
    output
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expands_ranges_empty_elements_and_escaped_braces() {
        assert_eq!(
            expand_simple_braces("{1-3}{a-b}").expect("ranges"),
            ["1a", "1b", "2a", "2b", "3a", "3b"]
        );
        assert_eq!(
            expand_simple_braces("super + {_,shift + }Return").expect("optional modifier"),
            ["super + Return", "super + shift +Return"]
        );
        assert_eq!(
            expand_simple_braces(r"printf \{literal\}").expect("escaped braces"),
            ["printf {literal}"]
        );
    }

    #[test]
    fn rejects_mixed_ranges_and_unbounded_cartesian_expansion() {
        assert!(expand_simple_braces("{a-9}").is_err());
        let excessive = "{0-9}".repeat(4);
        assert!(
            expand_simple_braces(&excessive)
                .expect_err("expansion limit")
                .contains("limit")
        );
    }
}
