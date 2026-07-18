# Contributing to DeskHalloumi

Thanks for working on DeskHalloumi. The canonical repository is
<https://github.com/fkr-0/deskhalloumi>. This is a desktop/session-facing Rust
workspace, so local tests must be careful not to mutate a developer's running
window manager, network, audio, power, login, or notification session.

Clone it with:

```sh
git clone git@github.com:fkr-0/deskhalloumi.git
cd deskhalloumi
```

## Default safe test command

Use this command before handing off a change:

```sh
scripts/test_safe.sh
```

The wrapper runs the live-session command audit in quiet mode and then runs the full Rust workspace test suite. It is intentionally the canonical local and CI test entrypoint.

Equivalent expanded form:

```sh
python3 scripts/audit_live_session_commands.py --quiet
cargo test --workspace
```

Focused tests are fine while iterating, for example:

```sh
cargo test -p deskhalloumi-core bar
cargo test -p deskhalloumi-bin --bin deskhalloumi-bar
```

Run `scripts/test_safe.sh` before considering the slice complete.

## Live-session command safety

Tests must not execute real desktop/session commands by default. In particular, avoid dispatching commands that can change a live window manager workspace, network state, audio state, power/session state, or notification state.

Preferred patterns:

- Test parsers with fixture text.
- Test command rendering with non-executing helpers.
- Test dispatch paths with harmless mock commands such as `printf`.
- Use environment/file fixtures for module inputs.

Do not use real backend command dispatch in normal tests. If a test needs to mention a live-session command only as inert data, add an explicit audit annotation:

```rust
// unilii-audit: allow-live-session-command-reference -- this test only parses or asserts data; it does not execute commands.
```

Executable scripts that intentionally run live-session commands must use the stronger opt-in marker and should not be part of normal test paths:

```sh
# unilii-audit: allow-live-session-command-execution -- explicit live-session smoke test; opt-in only.
```

## Live integration tests

Live-session integration tests are future work and must be opt-in. They should require an explicit environment variable, restore the original session state, and stay out of normal `scripts/test_safe.sh` runs.

## Async and subprocess changes

Read [the async runtime policy](docs/async-runtime.md) before adding Tokio tasks,
channels, retries, provider refreshes, or external commands.

Every spawned task needs an owner, observable failure handling, and bounded
shutdown. Prefer direct awaits, `JoinSet`, or retained `JoinHandle`s over
fire-and-forget tasks. Do not hold synchronous mutex guards across `.await`.

External commands on UI-sensitive paths must use asynchronous process handling,
timeouts, bounded retained output, and cancellation cleanup. Tests should use
temporary harmless scripts rather than live desktop tools.

## Updating task state

`roadmap.yml` defines release horizons and architecture direction. `todo.yml`
tracks focused known gaps, and `tasks.yml` records detailed implementation
evidence. When completing a slice:

1. Add or update focused tests first.
2. Run the focused gate.
3. Run `scripts/test_safe.sh` when production code, scripts, docs, or test policy changes.
4. Update `tasks.yml` with implementation evidence, limitations, and any emerging follow-up tasks.

Documentation-only corrections made after a release tag belong under
`[Unreleased]`; never move or replace an already published annotated tag.
