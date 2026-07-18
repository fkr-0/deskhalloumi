//! Selective X11 passive-grab backend for global shortcuts.
//!
//! Only configured chords are grabbed. Unmatched keyboard input remains owned by
//! the focused client. The existing key engine still decides hold, release,
//! repeat, cooldown, priority, and consume semantics.

use crate::keys::KeyBinding;
use std::collections::{HashMap, HashSet};
use std::thread;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;
use x11rb::connection::Connection;
use x11rb::protocol::Event;
use x11rb::protocol::xproto::{ConnectionExt, GrabMode, KeyButMask, Keycode, ModMask, Window};
use x11rb::rust_connection::RustConnection;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct X11KeyEvent {
    pub code: String,
    pub value: i32,
}

fn grab_key_with_retry(
    connection: &RustConnection,
    root: Window,
    binding: &KeyBinding,
    keycode: Keycode,
    modifiers: u16,
) -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_millis(300);
    loop {
        let cookie = connection
            .grab_key(
                false,
                root,
                ModMask::from(modifiers),
                keycode,
                GrabMode::ASYNC,
                GrabMode::ASYNC,
            )
            .map_err(|error| {
                format!(
                    "failed to request X11 grab for '{}' ({}): {error}",
                    binding.name, binding.keysym
                )
            })?;
        match cookie.check() {
            Ok(()) => return Ok(()),
            Err(_error) if Instant::now() < deadline => {
                thread::sleep(Duration::from_millis(10));
            }
            Err(error) => {
                return Err(format!(
                    "X11 grab conflict for '{}' chord='{}' keycode={} modifiers={:#x}: {error}",
                    binding.name, binding.keysym, keycode, modifiers
                ));
            }
        }
    }
}

fn modifier_events(state: KeyButMask, trigger_code: &str) -> Vec<String> {
    let raw = u16::from(state);
    let candidates = [
        (u16::from(KeyButMask::CONTROL), "KEY_LEFTCTRL"),
        (u16::from(KeyButMask::SHIFT), "KEY_LEFTSHIFT"),
        (u16::from(KeyButMask::MOD1), "KEY_LEFTALT"),
        (u16::from(KeyButMask::MOD4), "KEY_LEFTMETA"),
    ];
    candidates
        .into_iter()
        .filter(|(mask, code)| raw & mask != 0 && *code != trigger_code)
        .map(|(_, code)| code.to_string())
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct X11GrabDiagnostic {
    pub binding: String,
    pub chord: String,
    pub keycode: u8,
    pub modifiers: u16,
}

pub struct X11HotkeyListener {
    connection: RustConnection,
    root: Window,
    keycode_names: HashMap<Keycode, String>,
    diagnostics: Vec<X11GrabDiagnostic>,
}

impl X11HotkeyListener {
    pub fn connect(bindings: &[KeyBinding]) -> Result<Self, String> {
        let (connection, screen_index) = RustConnection::connect(None)
            .map_err(|error| format!("failed to connect to X11 display: {error}"))?;
        let setup = connection.setup();
        let root = setup
            .roots
            .get(screen_index)
            .ok_or_else(|| format!("X11 screen index {screen_index} is unavailable"))?
            .root;
        let mapping = connection
            .get_keyboard_mapping(setup.min_keycode, setup.max_keycode - setup.min_keycode + 1)
            .map_err(|error| format!("failed to request X11 keyboard mapping: {error}"))?
            .reply()
            .map_err(|error| format!("failed to read X11 keyboard mapping: {error}"))?;
        let keycode_symbols = keyboard_symbols(
            setup.min_keycode,
            setup.max_keycode,
            mapping.keysyms_per_keycode,
            &mapping.keysyms,
        );
        let keycode_names = keycode_symbols
            .iter()
            .filter_map(|(keycode, symbols)| {
                symbols
                    .iter()
                    .find_map(|symbol| engine_key_name(symbol))
                    .map(|name| (*keycode, name))
            })
            .collect::<HashMap<_, _>>();

        let num_lock_mask = discover_num_lock_mask(&connection, &keycode_symbols)?;
        let lock_variants = lock_variants(num_lock_mask);
        let mut seen = HashSet::<(Keycode, u16)>::new();
        let mut diagnostics = Vec::new();

        for binding in bindings {
            let plan = parse_grab_plan(binding, &keycode_symbols)?;
            for keycode in plan.keycodes {
                for lock_bits in &lock_variants {
                    let modifiers = plan.modifiers | *lock_bits;
                    if !seen.insert((keycode, modifiers)) {
                        continue;
                    }
                    grab_key_with_retry(&connection, root, binding, keycode, modifiers)?;
                    diagnostics.push(X11GrabDiagnostic {
                        binding: binding.name.clone(),
                        chord: binding.keysym.clone(),
                        keycode,
                        modifiers,
                    });
                }
            }
        }
        connection
            .flush()
            .map_err(|error| format!("failed to flush X11 grabs: {error}"))?;

        Ok(Self {
            connection,
            root,
            keycode_names,
            diagnostics,
        })
    }

    pub fn diagnostics(&self) -> &[X11GrabDiagnostic] {
        &self.diagnostics
    }

    pub fn into_event_stream(self) -> mpsc::UnboundedReceiver<Result<X11KeyEvent, String>> {
        let (sender, receiver) = mpsc::unbounded_channel();
        thread::Builder::new()
            .name("deskhalloumi-x11-hotkeys".to_string())
            .spawn(move || {
                let mut pressed = HashSet::<Keycode>::new();
                let mut synthetic_modifiers = HashMap::<Keycode, Vec<String>>::new();
                loop {
                    if sender.is_closed() {
                        break;
                    }
                    let event = match self.connection.poll_for_event() {
                        Ok(Some(event)) => event,
                        Ok(None) => {
                            thread::sleep(Duration::from_millis(5));
                            continue;
                        }
                        Err(error) => {
                            let _ = sender.send(Err(format!("X11 event loop failed: {error}")));
                            break;
                        }
                    };
                    let (keycode, value, state) = match event {
                        Event::KeyPress(event) => {
                            let value = if pressed.insert(event.detail) { 1 } else { 2 };
                            (event.detail, value, event.state)
                        }
                        Event::KeyRelease(event) => {
                            pressed.remove(&event.detail);
                            (event.detail, 0, event.state)
                        }
                        _ => continue,
                    };
                    let Some(code) = self.keycode_names.get(&keycode).cloned() else {
                        continue;
                    };
                    if value == 1 {
                        let modifiers = modifier_events(state, &code);
                        for modifier in &modifiers {
                            if sender
                                .send(Ok(X11KeyEvent {
                                    code: modifier.clone(),
                                    value: 1,
                                }))
                                .is_err()
                            {
                                return;
                            }
                        }
                        synthetic_modifiers.insert(keycode, modifiers);
                    }
                    if sender.send(Ok(X11KeyEvent { code, value })).is_err() {
                        break;
                    }
                    if value == 0
                        && let Some(modifiers) = synthetic_modifiers.remove(&keycode)
                    {
                        for modifier in modifiers.into_iter().rev() {
                            if sender
                                .send(Ok(X11KeyEvent {
                                    code: modifier,
                                    value: 0,
                                }))
                                .is_err()
                            {
                                return;
                            }
                        }
                    }
                }
                if let Ok(cookie) = self.connection.ungrab_key(0, self.root, ModMask::ANY) {
                    let _ = cookie.check();
                }
                let _ = self.connection.flush();
            })
            .expect("failed to spawn X11 hotkey event thread");
        receiver
    }
}

struct GrabPlan {
    modifiers: u16,
    keycodes: Vec<Keycode>,
}

fn parse_grab_plan(
    binding: &KeyBinding,
    keycode_symbols: &HashMap<Keycode, Vec<String>>,
) -> Result<GrabPlan, String> {
    let tokens = binding
        .keysym
        .split('+')
        .map(str::trim)
        .filter(|token| !token.is_empty())
        .collect::<Vec<_>>();
    if tokens.len() < 2 {
        return Err(format!(
            "X11 binding '{}' must contain a modifier and trigger key",
            binding.name
        ));
    }
    let trigger = tokens.last().copied().unwrap_or_default();
    let trigger_modifier = modifier_mask(trigger);
    let mut modifiers = 0u16;
    for token in &tokens[..tokens.len() - 1] {
        modifiers |= modifier_mask(token).ok_or_else(|| {
            format!(
                "X11 binding '{}' has non-modifier before trigger: '{}'",
                binding.name, token
            )
        })?;
    }
    if let Some(mask) = trigger_modifier {
        modifiers &= !mask;
    }
    let wanted = target_symbol_names(trigger)?;
    let keycodes = keycode_symbols
        .iter()
        .filter(|(_, symbols)| {
            symbols.iter().any(|symbol| {
                wanted
                    .iter()
                    .any(|wanted| symbol.eq_ignore_ascii_case(wanted))
            })
        })
        .map(|(keycode, _)| *keycode)
        .collect::<Vec<_>>();
    if keycodes.is_empty() {
        return Err(format!(
            "X11 binding '{}' trigger '{}' is not present in the active keyboard layout",
            binding.name, trigger
        ));
    }
    Ok(GrabPlan {
        modifiers,
        keycodes,
    })
}

fn keyboard_symbols(
    min_keycode: Keycode,
    max_keycode: Keycode,
    per_keycode: u8,
    raw: &[u32],
) -> HashMap<Keycode, Vec<String>> {
    let mut result = HashMap::new();
    for keycode in min_keycode..=max_keycode {
        let index = usize::from(keycode - min_keycode) * usize::from(per_keycode);
        let symbols = raw
            .get(index..index + usize::from(per_keycode))
            .unwrap_or_default()
            .iter()
            .flat_map(|raw| {
                let keysym = xkeysym::Keysym::new(*raw);
                keysym
                    .name()
                    .map(|name| name.trim_start_matches("XK_").to_string())
                    .into_iter()
                    .chain(keysym.key_char().map(|value| value.to_string()))
            })
            .collect::<Vec<_>>();
        result.insert(keycode, symbols);
    }
    result
}

fn discover_num_lock_mask(
    connection: &RustConnection,
    symbols: &HashMap<Keycode, Vec<String>>,
) -> Result<u16, String> {
    let reply = connection
        .get_modifier_mapping()
        .map_err(|error| format!("failed to request X11 modifier map: {error}"))?
        .reply()
        .map_err(|error| format!("failed to read X11 modifier map: {error}"))?;
    let per = usize::from(reply.keycodes_per_modifier());
    for modifier_index in 0..8usize {
        let start = modifier_index * per;
        let end = start + per;
        if reply
            .keycodes
            .get(start..end)
            .unwrap_or_default()
            .iter()
            .any(|keycode| {
                symbols
                    .get(keycode)
                    .is_some_and(|names| names.iter().any(|name| name == "Num_Lock"))
            })
        {
            return Ok(1u16 << modifier_index);
        }
    }
    Ok(u16::from(ModMask::M2))
}

fn lock_variants(num_lock_mask: u16) -> Vec<u16> {
    let caps = u16::from(ModMask::LOCK);
    let mut variants = vec![0, caps, num_lock_mask, caps | num_lock_mask];
    variants.sort_unstable();
    variants.dedup();
    variants
}

fn modifier_mask(token: &str) -> Option<u16> {
    let token = normalize_token(token);
    match token.as_str() {
        "SHIFT" | "LEFTSHIFT" | "RIGHTSHIFT" => Some(u16::from(ModMask::SHIFT)),
        "CTRL" | "CONTROL" | "LEFTCTRL" | "RIGHTCTRL" => Some(u16::from(ModMask::CONTROL)),
        "ALT" | "LEFTALT" | "RIGHTALT" => Some(u16::from(ModMask::M1)),
        "SUPER" | "META" | "WIN" | "WINDOWS" | "LEFTMETA" | "RIGHTMETA" | "LEFTSUPER"
        | "RIGHTSUPER" => Some(u16::from(ModMask::M4)),
        _ => None,
    }
}

fn target_symbol_names(token: &str) -> Result<Vec<String>, String> {
    let normalized = normalize_token(token);
    let names = match normalized.as_str() {
        "SHIFT" => vec!["Shift_L", "Shift_R"],
        "LEFTSHIFT" => vec!["Shift_L"],
        "RIGHTSHIFT" => vec!["Shift_R"],
        "CTRL" | "CONTROL" => vec!["Control_L", "Control_R"],
        "LEFTCTRL" => vec!["Control_L"],
        "RIGHTCTRL" => vec!["Control_R"],
        "ALT" => vec!["Alt_L", "Alt_R"],
        "LEFTALT" => vec!["Alt_L"],
        "RIGHTALT" => vec!["Alt_R"],
        "SUPER" | "META" | "WIN" | "WINDOWS" => vec!["Super_L", "Super_R"],
        "LEFTMETA" | "LEFTSUPER" => vec!["Super_L"],
        "RIGHTMETA" | "RIGHTSUPER" => vec!["Super_R"],
        "RETURN" | "ENTER" => vec!["Return"],
        "ESC" | "ESCAPE" => vec!["Escape"],
        "SPACE" => vec!["space"],
        "TAB" => vec!["Tab"],
        "BACKSPACE" => vec!["BackSpace"],
        "DELETE" | "DEL" => vec!["Delete"],
        "HOME" => vec!["Home"],
        "END" => vec!["End"],
        "PAGEUP" => vec!["Prior"],
        "PAGEDOWN" => vec!["Next"],
        "UP" => vec!["Up"],
        "DOWN" => vec!["Down"],
        "LEFT" => vec!["Left"],
        "RIGHT" => vec!["Right"],
        "MINUS" => vec!["minus"],
        "EQUAL" => vec!["equal"],
        "COMMA" => vec!["comma"],
        "DOT" | "PERIOD" => vec!["period"],
        "SLASH" => vec!["slash"],
        "SEMICOLON" => vec!["semicolon"],
        value if value.len() == 1 => vec![value],
        value if value.starts_with('F') && value[1..].chars().all(|c| c.is_ascii_digit()) => {
            vec![value]
        }
        _ => return Err(format!("unsupported X11 key token '{token}'")),
    };
    Ok(names.into_iter().map(str::to_string).collect())
}

fn engine_key_name(symbol: &str) -> Option<String> {
    let name = match symbol {
        "Shift_L" => "KEY_LEFTSHIFT".to_string(),
        "Shift_R" => "KEY_RIGHTSHIFT".to_string(),
        "Control_L" => "KEY_LEFTCTRL".to_string(),
        "Control_R" => "KEY_RIGHTCTRL".to_string(),
        "Alt_L" => "KEY_LEFTALT".to_string(),
        "Alt_R" => "KEY_RIGHTALT".to_string(),
        "Super_L" | "Meta_L" => "KEY_LEFTMETA".to_string(),
        "Super_R" | "Meta_R" => "KEY_RIGHTMETA".to_string(),
        "Return" => "KEY_ENTER".to_string(),
        "Escape" => "KEY_ESC".to_string(),
        "space" => "KEY_SPACE".to_string(),
        "Tab" => "KEY_TAB".to_string(),
        "BackSpace" => "KEY_BACKSPACE".to_string(),
        "Delete" => "KEY_DELETE".to_string(),
        "Home" => "KEY_HOME".to_string(),
        "End" => "KEY_END".to_string(),
        "Prior" => "KEY_PAGEUP".to_string(),
        "Next" => "KEY_PAGEDOWN".to_string(),
        "Up" => "KEY_UP".to_string(),
        "Down" => "KEY_DOWN".to_string(),
        "Left" => "KEY_LEFT".to_string(),
        "Right" => "KEY_RIGHT".to_string(),
        value if value.len() == 1 && value.chars().all(|c| c.is_ascii_alphabetic()) => {
            format!("KEY_{}", value.to_ascii_uppercase())
        }
        value if value.len() == 1 && value.chars().all(|c| c.is_ascii_digit()) => {
            format!("KEY_{value}")
        }
        value if value.starts_with('F') && value[1..].chars().all(|c| c.is_ascii_digit()) => {
            format!("KEY_{value}")
        }
        _ => return None,
    };
    Some(name)
}

fn normalize_token(token: &str) -> String {
    token
        .trim()
        .to_ascii_uppercase()
        .replace("KEY_", "")
        .replace(['-', '_', ' '], "")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lock_variants_cover_caps_and_num_lock() {
        let variants = lock_variants(u16::from(ModMask::M2));
        assert_eq!(variants.len(), 4);
        assert!(variants.contains(&(u16::from(ModMask::LOCK) | u16::from(ModMask::M2))));
    }

    #[test]
    fn left_and_right_modifiers_share_semantic_masks() {
        assert_eq!(modifier_mask("LeftCtrl"), modifier_mask("RightCtrl"));
        assert_eq!(modifier_mask("Super"), Some(u16::from(ModMask::M4)));
    }

    #[test]
    fn german_layout_symbol_name_can_map_to_engine_key_when_ascii_trigger_is_present() {
        assert_eq!(engine_key_name("z"), Some("KEY_Z".to_string()));
        assert_eq!(engine_key_name("y"), Some("KEY_Y".to_string()));
    }

    #[test]
    fn x11_state_becomes_engine_modifier_events_without_duplicating_trigger() {
        let state = KeyButMask::from(
            u16::from(KeyButMask::CONTROL)
                | u16::from(KeyButMask::SHIFT)
                | u16::from(KeyButMask::MOD4),
        );
        assert_eq!(
            modifier_events(state, "KEY_LEFTSHIFT"),
            vec!["KEY_LEFTCTRL", "KEY_LEFTMETA"]
        );
    }
}
