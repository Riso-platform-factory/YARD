# Logging

Every crate in YARD logs from startup through `yard_log`. There are two separate logs, each serving a different purpose:

- **Diagnostic log (`log` macros):** persisted to disk in `<Name>.log` for diagnosing failures and warnings after the fact.
- **Lifecycle trace (`tracing` spans and events):** structured execution trace for developers, kept only for the current run.

## Startup and Shutdown Order

An application initializes `yard_log` before any other engine:

1. `yard_log::start(product)` installs both loggers immediately and buffers early records in memory before the logs directory is known. The lifecycle trace starts in the `app-startup` phase.
2. `yard_log::open_files(paths.logs_dir())` opens `<Name>.log` (rotating an existing file to `<Name>.log.old`) and `lifecycle.jsonl`, then flushes buffered startup records.
3. `yard_log::init_app(cx)` hooks GPUI's `on_app_quit` to switch the lifecycle trace to the `app-shutdown` phase.
4. `yard_log::startup_finished()` transitions the lifecycle trace from `app-startup` to `app-running`.
5. `yard_log::exit(code)` (and an `atexit` handler) closes any still-open trace groups as `interrupted`, writes `run-end`, and flushes the writer thread (waiting up to `100ms`).

## Diagnostic Log

- Written through standard `log` macros (`log::error!`, `log::warn!`, `log::info!`, `log::debug!`, `log::trace!`) backed by `zlog`.
- Stored in `<logs>/<Name>.log`, with the previous run rotated to `<logs>/<Name>.log.old`.
- The first line of each run identifies the product: `<name> <version> (<identifier>) starting`.
- Filter verbosity with `<PREFIX>_LOG` (for the sample app, `YARD_LOG`).
- A panic hook writes the panic message and backtrace to the diagnostic log, marks the lifecycle trace as failed, and flushes before aborting or unwinding.

## Lifecycle Trace

The lifecycle trace is enabled in debug builds or whenever `<PREFIX>_LIFECYCLE` (for example, `YARD_LIFECYCLE`) is set and non-empty.

- Default filter is `info` (an invalid filter string logs a `WARN` and falls back to `info`).
- Output streams to `stderr` (colored on terminals) and to `<logs>/lifecycle.jsonl` via a dedicated background writer thread.
- When the trace is disabled, any existing `lifecycle.jsonl` from a prior run is removed.

### `lifecycle.jsonl` Format

Each line in `lifecycle.jsonl` is a JSON object tagged by `"kind"`:

- `run-start`: `t` (microseconds since start), `name`, `version`, `identifier`, `os`.
- `group-start`: `t`, `id`, `parent`, `name`, `fields`, `thread`.
- `line`: `t`, `group`, `level`, `target`, `message`, optional `known`, `fields`.
- `group-end`: `t`, `id`, `outcome` (`success`, `known-failure`, `failure`, or `interrupted`).
- `run-end`: `t`.

Three top-level phase groups span the process lifetime: `app-startup`, `app-running`, and `app-shutdown`.

## Logging Conventions

- **Groups:** Wrap each main operation in `tracing::info_span!("<slug>", fields...)`, entered while the work runs. Use a stable slug for the span name and put instance data in fields. Never emit a retrospective span after work has already finished.
- **Levels:**
  - `tracing::debug!` for steps inside a group.
  - `tracing::trace!` for high-frequency work (per frame, per keystroke, drag motion).
  - `tracing::info!` for expected refusals (such as an action blocked by a layout rule).
  - `log::warn!` for handled problems (missing optional files, unreadable user JSON, network errors).
  - `log::error!` only for bugs and real failures—every `ERROR` marks its enclosing group as `failure`.
- **Async work:** Attach `.instrument(span)` to spawned futures so their lines stay inside their parent group.

## Testing Logs

- Capture `tracing` output with `tracing::subscriber::with_default`, never by installing a process-wide subscriber in a shared test binary.
- Because `tracing` caches callsite interest per log line, tests that capture `tracing` output must not run in parallel with other tests hitting the same lines: put them in their own integration test file (`tests/<name>.rs`) or guard every test in the binary with a shared lock.
- Tests that install a process-wide `log` logger (`log::set_logger`) must live alone in their own integration test file (`tests/<name>.rs`).
