# Data store tour

The **Data tour** card exercises extension migrations, note reads and writes, and the database file under `YARD_HOME`. An unset or unrecognized `DATA_TOUR` leaves both tour extensions inactive.

The commands use isolated homes. Leave `TOGGLE_TOUR` unset so the data tour can place its root. For the first four cases, read `/tmp/yard-data/logs/yard.log` for diagnostics and `/tmp/yard-data/logs/lifecycle.jsonl` for the current run's trace. Inspect or save the trace before restarting. The database is at `/tmp/yard-data/data/data.sqlite`.

```sh
export YARD_HOME=/tmp/yard-data
export YARD_LIFECYCLE=info,data_tour=trace,yard_data=trace
```

## Keep notes across restarts

1. Start with a fresh home using `DATA_TOUR=on cargo run dev`. The card shows `ok` and an empty list. Click **Add note** twice, waiting for each request to finish, then click **Mark done**. The card shows `Note 1 (done)` followed by `Note 2 (not done)`. Click **Mark done** twice more to finish the second note and then check the empty update, which leaves both notes done and the status at `ok`.
2. Check the trace for `data-open` in WAL mode, one `data-migrate` group for each extension, and the `data-tour-load`, `data-tour-add`, and `data-tour-done` groups with their DEBUG steps. The extra **Mark done** logs `no unfinished notes` at INFO. At TRACE, look for the card rendering and the database's `data-request` spans. The requests produce no diagnostic warning or error.
3. Quit and run `DATA_TOUR=on cargo run dev` again. The same notes and done states return in ID order with status `ok`. The trace shows the load again without rerunning the migrations. Quit before the next case.

## Open the same file with fewer migrations

After the persistence case, run these commands and quit the app before taking the second hash.

```sh
shasum "$YARD_HOME/data/data.sqlite"
DATA_TOUR=old cargo run dev
shasum "$YARD_HOME/data/data.sqlite"
```

The card shows `data is from a newer version`. The trace shows `database migrations are newer` and a refused request in the database trace, with the load group finishing with `ok=false`. The diagnostic log has one WARN that `data_tour` declares one migration step while two are recorded. The two hashes match.

## Read another extension's table

Run `DATA_TOUR=peek cargo run dev` and click **Peek**. The card keeps its notes and the status becomes `refused`. The trace shows `data-tour-peek` and the database refusal. The diagnostic WARN names `data_tour` and the owner `data_peer`. Quit when finished.

## Turn the tour off and back on

Add `"extensions": {"data_tour": false}` to `/tmp/yard-data/config/settings.json`, retaining any other settings. Run `DATA_TOUR=on cargo run dev`. The card is absent and the trace shows `turned off` for `data_tour`, with no `data-tour-load` group. Disabling the extension produces no diagnostic warning or error.

Quit, remove the `data_tour` entry, and run `DATA_TOUR=on cargo run dev` again. The card and its saved notes return with status `ok`. The load group is back in the trace and there is no new diagnostic warning or error from the tour.

## Open a corrupt file

Use a fresh isolated home for this case. Run the commands below with every app using that home closed. Take the second hash after quitting the app.

```sh
export YARD_HOME=/tmp/yard-data-corrupt
mkdir -p "$YARD_HOME/data"
rm -f "$YARD_HOME/data/data.sqlite-wal" "$YARD_HOME/data/data.sqlite-shm"
head -c 4096 /dev/urandom > "$YARD_HOME/data/data.sqlite"
shasum "$YARD_HOME/data/data.sqlite"
DATA_TOUR=on cargo run dev
shasum "$YARD_HOME/data/data.sqlite"
```

The card shows `unavailable`. Click **Add note** and check that the status stays `unavailable`. The lifecycle trace shows one failed `data-open` and the database refusing requests as unavailable. The diagnostic log has one database open ERROR. The two hashes match. Read the two logs under `/tmp/yard-data-corrupt/logs/`.

## Share the file between two macOS processes

Build once. These commands use the default target directory. If your Cargo configuration uses another directory, adjust the binary path.

```sh
cargo build
```

From the repository root, start the first command in one terminal and the second in another. Both use the same fresh home.

```sh
YARD_HOME=/tmp/yard-data-shared YARD_LIFECYCLE=info,data_tour=trace,yard_data=trace DATA_TOUR=on target/debug/yard dev 2>/tmp/yard-data-first.trace
```

```sh
YARD_HOME=/tmp/yard-data-shared YARD_LIFECYCLE=info,data_tour=trace,yard_data=trace DATA_TOUR=on target/debug/yard dev 2>/tmp/yard-data-second.trace
```

Click **Add note** in the first window, then in the second. The second window shows both notes. Click **Add note** again in the first window to reload its list. It shows the note from the second window as well as its own two notes. Restart either process with the same command to reload without adding a note. Both windows should show `ok` after their requests.

Look for `data-open` and successful `data-tour-add` groups in the per-process stderr captures. Each process writes to the same `lifecycle.jsonl`, so use the captures to read each trace independently. The shared diagnostic log at `/tmp/yard-data-shared/logs/yard.log` should show no database warning or error for these writes.
