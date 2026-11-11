# log_tour

A test extension for the log engine, `riso_log`. When `app/riso/src/dev/review.rs` lists it among the extensions it passes to the startup, and `app/riso/Cargo.toml` depends on it, `cargo run dev` loads it into the running app. The log engine is in production, so it is not loaded now; add both lines back to check the log engine again. Without a case it does nothing, so a plain `cargo run dev` shows only what the app really does. With a case, it exercises the engine through work that really runs.

It has no stage and is never in production.

## Cases

The case is chosen with the environment variable `LOG_TOUR`. An unknown case logs a WARN that names the known ones.

| Command | What to look for |
|---|---|
| `LOG_TOUR=levels cargo run dev` | Nested groups; a WARN whose group still succeeds (✓); an expected refusal at INFO; an ERROR that fails its group (✗); async work that stays in its group; a window opened and then closed, after which GPUI's known error is marked ⚠. The app runs until its window is closed, then `app-shutdown` and `run-end`. |
| `LOG_TOUR=panic cargo run dev` | A panic on the main thread: every open group and the phase fail (✗), and `run-end` is written before the process aborts. |
| `LOG_TOUR=panic-thread cargo run dev` | A panic on a background thread: its group fails (✗), and the app keeps running. |
| `LOG_TOUR=interrupt cargo run dev` | The process exits while groups are open: they end as interrupted (⊘), then `run-end`. |

The log filter is a parameter of the app, not of the extension:

| Command | What to look for |
|---|---|
| `RISO_LIFECYCLE=trace LOG_TOUR=levels cargo run dev` | The tour's own DEBUG and TRACE steps. By default, only the crates under review log at DEBUG. |
| `RISO_LIFECYCLE=info,riso_log=debug LOG_TOUR=levels cargo run dev` | The log engine's own DEBUG lines, such as the files it opened. |
| `RISO_LIFECYCLE='info,[' cargo run dev` | A WARN that the filter is invalid, and the fallback to `info`. |

Each run prints the folder of its log files first: `riso.log` and `lifecycle.jsonl`.

The rotation of `riso.log` to `riso.log.old` cannot be seen in one run over a fresh folder. The test `crates/riso_log/tests/rotation.rs` covers it.

## Removing it

Remove it from the list of extensions in `app/riso/src/dev/review.rs` and from the dependencies in `app/riso/Cargo.toml`, and the app runs as it should without it.
