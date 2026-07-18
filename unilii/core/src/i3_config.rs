//! Recursive i3 configuration loading and keybinding conflict analysis.

use crate::i3_keybindings::canonical_i3_chord;
use crate::key_engine::KeyTrigger;
use crate::keys::KeyBinding;
use glob::glob;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct I3BindingLocation {
    pub path: PathBuf,
    pub line: usize,
    pub mode: String,
    pub directive: String,
    pub chord: String,
    pub canonical_chord: String,
    pub release: bool,
    pub command: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct I3ExistingConflict {
    pub first: I3BindingLocation,
    pub second: I3BindingLocation,
    pub kind: I3ConflictKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct I3Conflict {
    pub generated_binding: String,
    pub generated_chord: String,
    pub existing: I3BindingLocation,
    pub kind: I3ConflictKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum I3ConflictKind {
    Exact,
    ModifierEquivalent,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct I3ConfigAudit {
    pub root: PathBuf,
    pub files: Vec<PathBuf>,
    pub bindings: Vec<I3BindingLocation>,
    pub existing_conflicts: Vec<I3ExistingConflict>,
    pub conflicts: Vec<I3Conflict>,
    pub warnings: Vec<String>,
    pub incomplete: bool,
}

impl I3ConfigAudit {
    pub fn has_conflicts(&self) -> bool {
        !self.existing_conflicts.is_empty() || !self.conflicts.is_empty()
    }
}

pub fn audit_i3_config(
    root: impl AsRef<Path>,
    generated: &[KeyBinding],
    allowed_roots: &[PathBuf],
) -> Result<I3ConfigAudit, String> {
    let root = canonical_existing(root.as_ref())?;
    let roots = allowed_roots
        .iter()
        .filter_map(|path| canonical_existing(path).ok())
        .collect::<Vec<_>>();
    if roots.is_empty() {
        return Err("i3 audit requires at least one existing allowed root".to_string());
    }
    ensure_allowed(&root, &roots)?;

    let mut audit = I3ConfigAudit {
        root: root.clone(),
        ..I3ConfigAudit::default()
    };
    let mut state = ParseState::default();
    parse_file(&root, &roots, &mut state, &mut audit)?;
    audit.files.sort();
    audit.files.dedup();
    let mut existing = HashMap::<(String, bool, String), I3BindingLocation>::new();
    for binding in &audit.bindings {
        let identity = (
            binding.mode.clone(),
            binding.release,
            modifier_equivalent_identity(&binding.canonical_chord),
        );
        if let Some(previous) = existing.insert(identity, binding.clone()) {
            let kind = if previous.canonical_chord == binding.canonical_chord {
                I3ConflictKind::Exact
            } else {
                I3ConflictKind::ModifierEquivalent
            };
            audit.existing_conflicts.push(I3ExistingConflict {
                first: previous,
                second: binding.clone(),
                kind,
            });
        }
    }

    for binding in generated {
        let release = effective_release(binding);
        let generated_chord = match canonical_i3_chord(&binding.keysym) {
            Ok(chord) => chord,
            Err(error) => {
                audit.warnings.push(format!(
                    "generated binding '{}': cannot compare keysym '{}': {error}",
                    binding.name, binding.keysym
                ));
                audit.incomplete = true;
                continue;
            }
        };
        let generated_identity = modifier_equivalent_identity(&generated_chord);
        for existing in &audit.bindings {
            if existing.mode != "default" || existing.release != release {
                continue;
            }
            let kind = if existing.canonical_chord == generated_chord {
                Some(I3ConflictKind::Exact)
            } else if modifier_equivalent_identity(&existing.canonical_chord) == generated_identity
            {
                Some(I3ConflictKind::ModifierEquivalent)
            } else {
                None
            };
            if let Some(kind) = kind {
                audit.conflicts.push(I3Conflict {
                    generated_binding: binding.name.clone(),
                    generated_chord: generated_chord.clone(),
                    existing: existing.clone(),
                    kind,
                });
            }
        }
    }

    Ok(audit)
}

#[derive(Default)]
struct ParseState {
    variables: HashMap<String, String>,
    visited: HashSet<PathBuf>,
    mode_stack: Vec<String>,
}

fn parse_file(
    path: &Path,
    allowed_roots: &[PathBuf],
    state: &mut ParseState,
    audit: &mut I3ConfigAudit,
) -> Result<(), String> {
    let path = canonical_existing(path)?;
    ensure_allowed(&path, allowed_roots)?;
    if !state.visited.insert(path.clone()) {
        return Ok(());
    }
    audit.files.push(path.clone());
    let content = fs::read_to_string(&path)
        .map_err(|error| format!("failed to read i3 config '{}': {error}", path.display()))?;

    for (index, raw_line) in content.lines().enumerate() {
        let line_no = index + 1;
        let line = strip_comment(raw_line).trim();
        if line.is_empty() {
            continue;
        }
        if let Some(rest) = line.strip_prefix("set ") {
            if let Some((name, value)) = split_once_ws(rest) {
                state
                    .variables
                    .insert(name.trim().to_string(), value.trim().to_string());
            }
            continue;
        }
        if let Some(rest) = line.strip_prefix("set_from_resource ") {
            if let Some((name, tail)) = split_once_ws(rest) {
                let fallback = tail
                    .split_whitespace()
                    .last()
                    .unwrap_or_default()
                    .to_string();
                state.variables.insert(name.to_string(), fallback);
            }
            continue;
        }
        if let Some(rest) = line.strip_prefix("include ") {
            let expanded = expand_variables(unquote(rest.trim()), &state.variables);
            if expanded.contains('$') {
                audit.warnings.push(format!(
                    "{}:{} unresolved dynamic include: {}",
                    path.display(),
                    line_no,
                    rest.trim()
                ));
                audit.incomplete = true;
                continue;
            }
            let include_pattern = expand_home(&expanded);
            let include_pattern = if include_pattern.is_absolute() {
                include_pattern
            } else {
                path.parent()
                    .unwrap_or_else(|| Path::new("."))
                    .join(include_pattern)
            };
            let pattern = include_pattern.to_string_lossy().to_string();
            let mut matched = false;
            let entries = glob(&pattern).map_err(|error| {
                format!(
                    "{}:{} invalid include glob '{}': {error}",
                    path.display(),
                    line_no,
                    pattern
                )
            })?;
            for entry in entries {
                matched = true;
                match entry {
                    Ok(include) => {
                        if let Err(error) = parse_file(&include, allowed_roots, state, audit) {
                            audit.warnings.push(format!(
                                "{}:{} include '{}': {error}",
                                path.display(),
                                line_no,
                                include.display()
                            ));
                            audit.incomplete = true;
                        }
                    }
                    Err(error) => {
                        audit.warnings.push(format!(
                            "{}:{} include glob error: {error}",
                            path.display(),
                            line_no
                        ));
                        audit.incomplete = true;
                    }
                }
            }
            if !matched {
                audit.warnings.push(format!(
                    "{}:{} include matched no files: {}",
                    path.display(),
                    line_no,
                    pattern
                ));
                audit.incomplete = true;
            }
            continue;
        }
        if line.starts_with("mode ") && line.ends_with('{') {
            let name = line
                .trim_end_matches('{')
                .trim()
                .strip_prefix("mode")
                .unwrap_or_default()
                .trim();
            state
                .mode_stack
                .push(expand_variables(unquote(name), &state.variables));
            continue;
        }
        if line == "}" {
            state.mode_stack.pop();
            continue;
        }
        if line.starts_with("bindsym ") || line.starts_with("bindsym --") {
            if let Some(binding) = parse_binding_line(
                "bindsym",
                line,
                &path,
                line_no,
                state.mode_stack.last(),
                &state.variables,
            ) {
                audit.bindings.push(binding);
            }
            continue;
        }
        if (line.starts_with("bindcode ") || line.starts_with("bindcode --"))
            && let Some(binding) = parse_binding_line(
                "bindcode",
                line,
                &path,
                line_no,
                state.mode_stack.last(),
                &state.variables,
            )
        {
            audit.bindings.push(binding);
        }
    }
    Ok(())
}

fn parse_binding_line(
    directive: &str,
    line: &str,
    path: &Path,
    line_no: usize,
    mode: Option<&String>,
    variables: &HashMap<String, String>,
) -> Option<I3BindingLocation> {
    let mut words = line.split_whitespace();
    words.next()?;
    let mut release = false;
    let mut chord = None;
    for word in words {
        if word.starts_with("--") {
            if word == "--release" {
                release = true;
            }
            continue;
        }
        chord = Some(word.to_string());
        break;
    }
    let chord = chord?;
    let command_start = line.find(&chord)? + chord.len();
    let command = line[command_start..].trim().trim_end_matches(';').trim();
    let expanded = expand_variables(&chord, variables);
    let canonical_chord = if directive == "bindsym" {
        canonical_existing_chord(&expanded)
    } else {
        format!("bindcode:{expanded}")
    };
    Some(I3BindingLocation {
        path: path.to_path_buf(),
        line: line_no,
        mode: mode.cloned().unwrap_or_else(|| "default".to_string()),
        directive: directive.to_string(),
        chord: expanded,
        canonical_chord,
        release,
        command: command.to_string(),
    })
}

fn canonical_existing_chord(chord: &str) -> String {
    chord
        .split('+')
        .filter(|part| !part.trim().is_empty())
        .map(canonical_token)
        .collect::<Vec<_>>()
        .join("+")
}

fn canonical_token(token: &str) -> String {
    match token.trim().to_ascii_lowercase().as_str() {
        "mod4" | "super" | "meta" | "win" | "windows" => "Mod4".to_string(),
        "mod1" | "alt" => "Mod1".to_string(),
        "ctrl" | "control" => "Control".to_string(),
        "shift" => "Shift".to_string(),
        "enter" | "return" => "Return".to_string(),
        "esc" | "escape" => "Escape".to_string(),
        other if other.len() == 1 => other.to_string(),
        _ => token.trim().to_string(),
    }
}

fn modifier_equivalent_identity(chord: &str) -> String {
    let mut modifiers = BTreeMap::<u8, String>::new();
    let mut keys = Vec::new();
    for token in chord.split('+') {
        let token = canonical_token(token);
        let rank = match token.as_str() {
            "Control" => Some(0),
            "Shift" => Some(1),
            "Mod1" => Some(2),
            "Mod4" => Some(3),
            _ => None,
        };
        if let Some(rank) = rank {
            modifiers.insert(rank, token);
        } else {
            keys.push(token);
        }
    }
    modifiers
        .into_values()
        .chain(keys)
        .collect::<Vec<_>>()
        .join("+")
}

fn effective_release(binding: &KeyBinding) -> bool {
    binding.release || matches!(binding.trigger, KeyTrigger::Release)
}

fn expand_variables(input: &str, variables: &HashMap<String, String>) -> String {
    let mut output = input.to_string();
    let mut ordered = variables.iter().collect::<Vec<_>>();
    ordered.sort_by_key(|(name, _)| std::cmp::Reverse(name.len()));
    for _ in 0..16 {
        let previous = output.clone();
        for &(name, value) in &ordered {
            output = output.replace(name, value);
        }
        if output == previous {
            break;
        }
    }
    output
}

fn expand_home(input: &str) -> PathBuf {
    if input == "~" {
        return std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("~"));
    }
    if let Some(rest) = input.strip_prefix("~/")
        && let Some(home) = std::env::var_os("HOME")
    {
        return PathBuf::from(home).join(rest);
    }
    PathBuf::from(input)
}

fn strip_comment(line: &str) -> &str {
    let mut quoted = false;
    let mut escaped = false;
    for (index, character) in line.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if character == '\\' {
            escaped = true;
        } else if character == '"' {
            quoted = !quoted;
        } else if character == '#' && !quoted {
            return &line[..index];
        }
    }
    line
}

fn unquote(value: &str) -> &str {
    value
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .unwrap_or(value)
}

fn split_once_ws(value: &str) -> Option<(&str, &str)> {
    let index = value.find(char::is_whitespace)?;
    Some((&value[..index], value[index..].trim_start()))
}

fn canonical_existing(path: &Path) -> Result<PathBuf, String> {
    path.canonicalize()
        .map_err(|error| format!("failed to resolve '{}': {error}", path.display()))
}

fn ensure_allowed(path: &Path, roots: &[PathBuf]) -> Result<(), String> {
    if roots.iter().any(|root| path.starts_with(root)) {
        Ok(())
    } else {
        Err(format!(
            "path '{}' escapes allowed i3 audit roots",
            path.display()
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keys::CommandType;

    fn binding(keysym: &str) -> KeyBinding {
        KeyBinding {
            name: "generated".to_string(),
            keysym: keysym.to_string(),
            command: "true".to_string(),
            command_type: CommandType::Shell,
            release: false,
            trigger: KeyTrigger::Press,
            hold_ms: None,
            cooldown_ms: None,
            priority: 0,
            consume: false,
        }
    }

    #[test]
    fn resolves_variables_and_includes_and_reports_collision() {
        let temp = tempfile::tempdir().unwrap();
        let child = temp.path().join("child.conf");
        fs::write(&child, "bindsym Control+Mod4+x exec old\n").unwrap();
        let root = temp.path().join("config");
        fs::write(
            &root,
            format!(
                "set $mod Mod4\ninclude {}\nbindsym $mod+Return exec terminal\n",
                child.display()
            ),
        )
        .unwrap();
        let audit =
            audit_i3_config(&root, &[binding("Super+Ctrl+x")], &[temp.path().into()]).unwrap();
        assert_eq!(audit.files.len(), 2);
        assert_eq!(audit.conflicts.len(), 1);
        assert_eq!(audit.conflicts[0].kind, I3ConflictKind::ModifierEquivalent);
    }

    #[test]
    fn unresolved_include_marks_audit_incomplete() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("config");
        fs::write(&root, "include $unknown/path\n").unwrap();
        let audit = audit_i3_config(&root, &[], &[temp.path().into()]).unwrap();
        assert!(audit.incomplete);
    }

    #[test]
    fn longer_variable_names_expand_before_prefixes() {
        let variables = HashMap::from([
            ("$mod".to_string(), "Mod4".to_string()),
            ("$mod2".to_string(), "Control".to_string()),
        ]);
        assert_eq!(expand_variables("$mod2+x", &variables), "Control+x");
    }
}
