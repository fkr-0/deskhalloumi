#!/usr/bin/env sh
# Canonical safe local/CI test entrypoint.
# Runs the live-session command audit before the Rust workspace tests so normal
# test runs do not mutate a developer's WM, network, audio, power, login, or
# notification session by accident.
set -eu

repo_root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$repo_root"

printf '%s\n' 'running live-session command audit...'
python3 scripts/audit_live_session_commands.py --quiet
printf '%s\n' 'running cargo test --workspace...'
cargo test --workspace
printf '%s\n' 'safe test suite passed'
