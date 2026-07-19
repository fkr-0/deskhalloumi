# Async runtime and subprocess policy

DeskHalloumi combines Iced application tasks, Tokio services, DBus, Unix
sockets, input backends, provider refreshes, and external desktop commands. This
document defines the runtime rules used to keep those components responsive,
bounded, and safe to reload.

## Goals

- Never block the Iced update path on process I/O, filesystem polling, DBus,
  network, or device work.
- Give every long-lived task an owner and an explicit shutdown path.
- Bound command duration, retained output, queue growth, retry frequency, and
  cleanup time.
- Preserve the previous working generation when a hot reload candidate fails.
- Make timeouts, cancellation, truncation, task failure, and stale provider state
  observable through structured diagnostics.

## Runtime ownership

The preferred ownership hierarchy is:

```text
process
├── panel/application supervisor
│   ├── action-bus listener
│   ├── DBus/tray workers
│   ├── provider workers
│   └── managed popup/menu processes
└── hotkey supervisor
    ├── control socket
    ├── configuration watcher
    └── one active input generation
```

A process-lifetime worker may be detached only when the process itself is its
owner and shutdown cannot leave external resources behind. New subsystems should
instead retain a `JoinHandle`, `JoinSet`, or equivalent supervisor entry and
wait for bounded cleanup during shutdown or reload.

### Implemented shared boundary

`deskhalloumi_core::runtime` is the canonical runtime boundary. It contains:

- `ActionRunner` and `ActionCommand` for bounded text or binary subprocesses;
- `RuntimeSupervisor` and `TaskSpawner` for a bounded spawn queue, owned
  `JoinSet`, cancellation, panic observation, and bounded shutdown;
- `ProviderRefreshRegistry` for global concurrency limits and per-provider
  in-flight coalescing;
- latest-value `ModuleSubscription` channels whose producers are returned to
  the owning supervisor instead of spawning themselves;
- `RuntimeMetrics` and `RuntimeMetricsSnapshot` for task, action, timeout,
  truncation, refresh-pressure, and update-pressure counters.

The main bar owns one supervisor. Its action-bus listener and connections,
embedded hotkey daemon, module producers, and module consumers are children of
that owner. Closing the main window cancels the tree and waits up to two seconds
before forced abortion. The tray subscription uses its own scoped `JoinSet`, so
dropping the subscription also drops its watcher child.

## Structured concurrency

Use these patterns in descending order of preference:

1. Await child work directly when it belongs to the current operation.
2. Use `JoinSet` when a supervisor owns a dynamic set of homogeneous workers.
3. Store a `JoinHandle` when one component owns one worker.
4. Use `spawn_blocking` only for an API that has no practical asynchronous
   interface, and keep both its input and concurrency bounded.
5. Avoid fire-and-forget `tokio::spawn`. When unavoidable, document why process
   lifetime is sufficient ownership and how failures are observed.

A task panic or join failure must not disappear. Supervisors should log it with
the subsystem and generation identifiers and decide whether to restart, degrade,
or fail the candidate generation.

## Cancellation

Dropping a future is cancellation. Code must therefore be correct when an await
is interrupted.

- Do not hold a synchronous mutex guard across `.await`.
- Write replacement files to a temporary path and rename only after validation.
- Bind sockets, X11 grabs, and device streams inside an owned generation so
  dropping the generation releases them.
- When a child process is involved, use kill-on-drop as a safety net and perform
  explicit termination and reaping on timeout.
- Keep cleanup bounded; shutdown must not wait forever for a broken provider or
  descendant process.

The X11 hotkey worker already polls for receiver closure and explicitly releases
passive grabs. The action runner now gives each command its own process group on
Unix so timeout termination also reaches shell descendants that could otherwise
keep pipes open.

## External commands

All new command execution should use one asynchronous policy rather than direct
`std::process::Command` calls from UI-sensitive code.

The shared core action runner provides:

- `tokio::process::Command` instead of a blocking polling thread;
- null stdin and piped stdout/stderr;
- configurable timeout;
- kill-on-drop;
- a distinct Unix process group and descendant termination on timeout;
- bounded retained stdout and stderr while continuing to drain both pipes;
- total byte counts and truncation flags;
- optional working directory and environment overrides;
- structured outcome metadata for success, spawn failure, wait failure,
  non-zero exit, timeout, and generic action failure.

The default retained-output limit is 64 KiB per stream. Reading continues after
the limit so a verbose child cannot deadlock on a full pipe. Callers may lower
the limit for commands whose output is only status text.

The active Iced paths for audio, power, video, Wi-Fi, CopyQ, Tmux, filter-tab,
i3 visualization, tray networking, mount discovery, and system-menu actions now
use this policy. CalDAV uses Tokio process handling with an independent network
timeout and response-size cap because the library crate cannot depend back on
the core crate. Root disk usage uses `statvfs` rather than spawning `df`.

Synchronous process calls remain only in deliberately non-Iced boundaries:
exec-style compatibility launchers, headless i3-visualizer helpers, the retained
hotkey worker launcher, and the separately synchronous `deskhalloumi-bar`
scaffold runtime. Their long-term contract is tracked in `roadmap.yml`.

## Timeouts

Timeouts are part of the public runtime contract, not only a test convenience.
Choose them from the operation class:

- local status query: usually hundreds of milliseconds to a few seconds;
- user-requested desktop action: a few seconds, configurable where needed;
- service startup or reload: bounded but long enough for resource handoff;
- network synchronization: explicitly longer, with stale-state feedback.

A timeout should identify the menu/action/provider, elapsed limit, and recovery
behavior. Retrying must use capped exponential backoff with jitter where many
instances could otherwise synchronize.

## Channels and backpressure

Choose channel semantics from the data:

- `watch`: latest state where intermediate values are irrelevant;
- bounded `mpsc`: ordered work that must apply backpressure;
- `broadcast`: events for multiple independent subscribers, with lag handling;
- oneshot: one request/response or readiness signal.

Unbounded channels are acceptable only when the producer is intrinsically
bounded and documented. Provider refresh requests and action execution must not
create unlimited queued work.

The runtime implements two concrete pressure policies:

- `TaskSpawner` uses a bounded queue. `try_spawn` rejects saturation and records
  a dropped-update counter rather than silently accumulating work.
- `ProviderRefreshRegistry` permits only a configured number of concurrent
  refreshes and allows only one in-flight refresh for each provider key.
  Duplicate requests are coalesced; global saturation is reported separately.

Clock, battery, and Tmux producers publish through latest-value watch channels.
If a producer overwrites an unread value, that is recorded as a coalesced update;
sending after receiver closure is recorded as dropped.

## Blocking boundaries

Synchronous operations are allowed when they are demonstrably short and cannot
block on external state. Otherwise:

- use an asynchronous library or Tokio adapter;
- use `spawn_blocking` for bounded filesystem, parsing, or legacy system-library
  work;
- protect blocking pools from unbounded fan-out with a semaphore;
- never call a potentially blocking command directly from Iced update logic.

`std::sync::Mutex` is appropriate for very short in-memory critical sections
that never cross `.await`. Async mutexes are for guards that genuinely need to
span awaits; they should not replace ordinary mutexes mechanically.

## Provider lifecycle

A provider publishes the canonical typed states defined in
[`runtime-contracts.md`](runtime-contracts.md):

```text
startup
loading(previous?)
fresh(value)
stale(value, reason)
error(reason)
disabled(reason)
shutting_down
stopped
```

Refresh should be idempotent, timeout-bounded, and safe to cancel. A provider
failure must not crash the panel. The UI should retain the last known value when
that is safer than replacing it with an empty state.

Each snapshot carries a provider-instance generation and an in-instance refresh
generation. A late result is accepted only when both still match the active
provider. Keyed refresh admission coalesces duplicate in-flight requests and
prevents one provider from creating unbounded overlapping work.

## Reload generations

Configuration reload follows a candidate/commit model:

1. Parse and validate candidate configuration.
2. Acquire candidate resources without disturbing the active generation where
   possible.
3. Confirm readiness.
4. Atomically publish or switch ownership.
5. Cancel and join the old generation.
6. On candidate failure, keep or restore the previous generation.

Generation identifiers should be included in diagnostics and used to suppress
late events from replaced device paths or tasks.

## Testing

Async tests should use paused Tokio time when timing logic can be modeled
without real elapsed time. Process tests should use harmless temporary scripts,
short timeouts, and explicit output bounds. Tests must cover:

- success and non-zero exit;
- spawn and wait failures where practical;
- timeout and descendant termination;
- stdout/stderr draining and truncation;
- cancellation during reload;
- receiver closure and task shutdown;
- saturated channels or coalesced refresh requests;
- hardware-free and timezone-independent behavior.

Live desktop commands remain prohibited in ordinary tests. The isolated
`scripts/test_i3_hotkeys.sh` test is allowed because it creates a private Xvfb,
i3 instance, HOME, configuration directory, and runtime directory.

## Observability

Use `tracing` fields rather than interpolated prose for high-volume runtime
signals. Useful fields include:

```text
subsystem
action
provider
generation
duration_ms
exit_code
error_class
stdout_bytes
stderr_bytes
stdout_truncated
stderr_truncated
queue_depth
retry_attempt
```

Do not log full command output by default. Output may contain secrets and can be
large even after retention limits.

The process-wide metrics snapshot currently records:

```text
active_tasks
tasks_started / completed / cancelled / panicked
actions_started / completed / failed
action_timeouts
action_duration_ms_total / action_duration_ms_max
truncated_outputs / truncated_bytes
provider_refreshes_started / completed / coalesced / saturated
updates_coalesced / updates_dropped
```

Each action also emits a structured completion event containing its menu,
action, duration, result class, output byte counts, and truncation flags. The
bar logs the aggregate snapshot during supervised shutdown. A live diagnostic
query is planned as `ASYNC-08` in `roadmap.yml`.

## Review checklist

Before merging asynchronous code, confirm:

- Who owns every spawned task?
- How is it cancelled and joined?
- Can any input, queue, output, retry, or concurrency grow without a bound?
- Is a synchronous call hiding on an async/UI path?
- Does timeout cleanup reach descendants and release external resources?
- Are errors and task panics observable?
- Do tests avoid real hardware, locale, timing, and desktop-session assumptions?
