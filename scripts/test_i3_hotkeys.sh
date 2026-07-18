#!/usr/bin/env bash
# unilii-audit: allow-live-session-command-execution
# All window-manager commands below target the private Xvfb display created by
# this script; DISPLAY, HOME, XDG_CONFIG_HOME, and XDG_RUNTIME_DIR are replaced
# before i3 or xdotool is invoked.
set -euo pipefail

required=(Xvfb i3 i3-msg xdotool xev sha256sum timeout)
for command in "${required[@]}"; do
  command -v "$command" >/dev/null || {
    echo "missing integration-test dependency: $command" >&2
    exit 77
  }
done

ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
BIN="$ROOT/target/debug/deskhalloumi-hotkeyd"
cargo build -q -p deskhalloumi-bin --bin deskhalloumi-hotkeyd

TMP=$(mktemp -d)
DISPLAY_FILE="$TMP/display"
cleanup() {
  set +e
  [[ -n "${HOTKEY_PID:-}" ]] && kill "$HOTKEY_PID" 2>/dev/null
  [[ -n "${XEV_PID:-}" ]] && kill "$XEV_PID" 2>/dev/null
  [[ -n "${I3_PID:-}" ]] && kill "$I3_PID" 2>/dev/null
  [[ -n "${XVFB_PID:-}" ]] && kill "$XVFB_PID" 2>/dev/null
  wait 2>/dev/null
  rm -rf "$TMP"
}
trap cleanup EXIT

exec 3>"$DISPLAY_FILE"
Xvfb -displayfd 3 -screen 0 1024x768x24 -nolisten tcp >"$TMP/xvfb.log" 2>&1 &
XVFB_PID=$!
for _ in $(seq 1 100); do
  [[ -s "$DISPLAY_FILE" ]] && break
  sleep 0.02
done
[[ -s "$DISPLAY_FILE" ]] || { echo "Xvfb did not publish a display" >&2; exit 1; }
export DISPLAY=":$(cat "$DISPLAY_FILE")"
export HOME="$TMP/home"
export XDG_CONFIG_HOME="$HOME/.config"
export XDG_RUNTIME_DIR="$TMP/runtime"
export DESKHALLOUMI_RUNTIME_DIR="$XDG_RUNTIME_DIR/deskhalloumi"
mkdir -p "$XDG_CONFIG_HOME/deskhalloumi" "$DESKHALLOUMI_RUNTIME_DIR"
chmod 700 "$XDG_RUNTIME_DIR" "$DESKHALLOUMI_RUNTIME_DIR"

RESULT="$TMP/i3-actions.log"
SOURCE="$TMP/i3-hotkeys.toml"
INCLUDE="$XDG_CONFIG_HOME/deskhalloumi/i3-bindings.conf"
cat >"$SOURCE" <<EOF
[[keybindings]]
name = "i3-press"
keysym = "Super+x"
command = "echo press >> '$RESULT'"

[[keybindings]]
name = "i3-release"
keysym = "Super+y"
command = "echo release >> '$RESULT'"
trigger = "release"
EOF
"$BIN" --config "$SOURCE" --write-i3-bindings "$INCLUDE" --strict

I3_CONFIG="$XDG_CONFIG_HOME/i3/config"
mkdir -p "$(dirname "$I3_CONFIG")"
cat >"$I3_CONFIG" <<EOF
set \$mod Mod4
font pango:monospace 8
include $INCLUDE
EOF
i3 -c "$I3_CONFIG" >"$TMP/i3.log" 2>&1 &
I3_PID=$!
for _ in $(seq 1 100); do
  if i3-msg -t get_version >/dev/null 2>&1; then break; fi
  sleep 0.03
done
i3-msg -t get_version >/dev/null

xdotool key --clearmodifiers Super_L+x
xdotool keydown Super_L keydown y keyup y keyup Super_L
for _ in $(seq 1 100); do
  [[ $(wc -l <"$RESULT" 2>/dev/null || echo 0) -ge 2 ]] && break
  sleep 0.02
done
[[ $(grep -c '^press$' "$RESULT") -eq 1 ]]
[[ $(grep -c '^release$' "$RESULT") -eq 1 ]]

GOOD_HASH=$(sha256sum "$INCLUDE" | cut -d' ' -f1)
cat >"$SOURCE" <<'EOF'
[[keybindings]]
name = "invalid-for-i3"
keysym = "Super+Shift"
command = "true"
trigger = "modrelease"
hold_ms = 50
EOF
if "$BIN" --config "$SOURCE" --write-i3-bindings "$INCLUDE" --strict >"$TMP/strict.out" 2>"$TMP/strict.err"; then
  echo "strict invalid i3 export unexpectedly succeeded" >&2
  exit 1
fi
[[ $(sha256sum "$INCLUDE" | cut -d' ' -f1) == "$GOOD_HASH" ]]

# Exercise the selective X11 backend against a real X server. The focused xev
# client must not receive the grabbed trigger key.
X11_RESULT="$TMP/x11-actions.log"
X11_SOURCE="$TMP/x11-hotkeys.toml"
cat >"$X11_SOURCE" <<EOF
[[keybindings]]
name = "x11-press"
keysym = "Super+z"
command = "echo x11-press >> '$X11_RESULT'"

[[keybindings]]
name = "x11-modrelease"
keysym = "Super+Shift"
command = "echo x11-modrelease >> '$X11_RESULT'"
trigger = "modrelease"
hold_ms = 60
cooldown_ms = 200
priority = 50
consume = true

[[keybindings]]
name = "x11-repeat"
keysym = "Super+r"
command = "echo x11-repeat >> '$X11_RESULT'"
trigger = "repeat"

[[keybindings]]
name = "x11-cooldown"
keysym = "Super+k"
command = "echo x11-cooldown >> '$X11_RESULT'"
cooldown_ms = 500

[[keybindings]]
name = "x11-priority-high"
keysym = "Super+p"
command = "echo x11-priority-high >> '$X11_RESULT'"
priority = 100
consume = true

[[keybindings]]
name = "x11-priority-low"
keysym = "Super+p"
command = "echo x11-priority-low >> '$X11_RESULT'"
priority = 1
EOF
xev -name DeskHalloumiSink -event keyboard >"$TMP/xev.log" 2>&1 &
XEV_PID=$!
for _ in $(seq 1 100); do
  XEV_WINDOW=$(xdotool search --name DeskHalloumiSink 2>/dev/null | head -n1 || true)
  [[ -n "$XEV_WINDOW" ]] && break
  sleep 0.03
done
[[ -n "${XEV_WINDOW:-}" ]]
xdotool windowfocus "$XEV_WINDOW"

"$BIN" --backend x11 --config "$X11_SOURCE" --control-socket "$TMP/hotkey.sock" >"$TMP/hotkey.log" 2>&1 &
HOTKEY_PID=$!
for _ in $(seq 1 100); do
  [[ -S "$TMP/hotkey.sock" ]] && break
  if ! kill -0 "$HOTKEY_PID" 2>/dev/null; then cat "$TMP/hotkey.log" >&2; exit 1; fi
  sleep 0.03
done
[[ -S "$TMP/hotkey.sock" ]]

# A key outside every configured chord must remain untouched and reach the
# focused client.
xdotool key --clearmodifiers b
for _ in $(seq 1 50); do
  grep -Eq 'keysym 0x[0-9a-f]+, b\b' "$TMP/xev.log" && break
  sleep 0.02
done
grep -Eq 'keysym 0x[0-9a-f]+, b\b' "$TMP/xev.log"

xdotool keydown Super_L keydown z keyup z keyup Super_L
xdotool keydown Super_L keydown Shift_L
sleep 0.09
xdotool keyup Shift_L keyup Super_L
xdotool keydown Super_L keydown r
sleep 0.8
xdotool keyup r keyup Super_L
xdotool key --clearmodifiers Super_L+k
xdotool key --clearmodifiers Super_L+k
xdotool key --clearmodifiers Super_L+p
for _ in $(seq 1 100); do
  [[ $(wc -l <"$X11_RESULT" 2>/dev/null || echo 0) -ge 5 ]] && break
  sleep 0.02
done
[[ $(grep -c '^x11-press$' "$X11_RESULT") -eq 1 ]]
[[ $(grep -c '^x11-modrelease$' "$X11_RESULT") -eq 1 ]]
[[ $(grep -c '^x11-repeat$' "$X11_RESULT") -ge 1 ]]
[[ $(grep -c '^x11-cooldown$' "$X11_RESULT") -eq 1 ]]
[[ $(grep -c '^x11-priority-high$' "$X11_RESULT") -eq 1 ]]
[[ $(grep -c '^x11-priority-low$' "$X11_RESULT" || true) -eq 0 ]]
# The passive grab may let the modifier itself reach the client before the
# trigger activates, but the matching trigger key must remain suppressed.
if grep -Eq 'keysym 0x[0-9a-f]+, z\b|keysym 0x[0-9a-f]+, Z\b' "$TMP/xev.log"; then
  echo "selective X11 trigger leaked to focused client" >&2
  cat "$TMP/xev.log" >&2
  exit 1
fi

# i3 still owns Super+x from the generated include. Reloading native hotkeyd
# with the same chord must fail readiness, report rollback, and restore the
# previous Super+z generation.
cp "$X11_SOURCE" "$TMP/x11-hotkeys.good.toml"
cat >"$X11_SOURCE" <<EOF
[[keybindings]]
name = "conflicts-with-i3"
keysym = "Super+x"
command = "true"
EOF
set +e
RELOAD_OUTPUT=$("$BIN" --control-socket "$TMP/hotkey.sock" --reload 2>&1)
RELOAD_STATUS=$?
set -e
cp "$TMP/x11-hotkeys.good.toml" "$X11_SOURCE"
[[ $RELOAD_STATUS -ne 0 ]]
grep -Eqi 'rolled back|grab conflict|reload rejected' <<<"$RELOAD_OUTPUT"

xdotool keydown Super_L keydown z keyup z keyup Super_L
for _ in $(seq 1 100); do
  [[ $(grep -c '^x11-press$' "$X11_RESULT" 2>/dev/null || true) -ge 2 ]] && break
  sleep 0.02
done
[[ $(grep -c '^x11-press$' "$X11_RESULT") -eq 2 ]]

"$BIN" --control-socket "$TMP/hotkey.sock" --shutdown >/dev/null
wait "$HOTKEY_PID"
unset HOTKEY_PID

echo "isolated i3 and X11 hotkey integration passed"
