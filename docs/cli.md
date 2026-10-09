# aika CLI

`aika-cli` operates on the desktop application's local Personal calendar using the shared Rust service. No running desktop app or GUI is required. Connected calendars, JSON output, search, recurrence, import/export, and CLI undo are outside this version.

## Build and run

Use the repository's Node/pnpm/Rust versions from the README:

```sh
pnpm cli --help
pnpm cli events create --help
pnpm cli:build
pnpm rust install --path src-tauri/cli --locked
```

`pnpm cli` runs the development binary. `pnpm cli:build` produces `src-tauri/target/release/aika-cli` (`aika-cli.exe` on Windows). Cargo installation places it in Cargo's bin directory, which must be on your PATH. Existing desktop installers do not include this binary.

The commands below use the installed binary. Replace IDs with the full UUID printed on creation or listing.

## Events

```sh
aika-cli events list --from 2026-10-01 --to 2026-11-01 --time-zone Europe/Zurich
aika-cli events create --title 'Lunch' --start 2026-10-10T12:00 --end 2026-10-10T13:00 --location 'Cafe' --notes 'Bring agenda'
aika-cli events get <event-uuid>
aika-cli events update <event-uuid> --title 'Lunch with Sam' --notes ''
aika-cli events delete <event-uuid>
```

Listing selects events that overlap `[from, to)`, including events that start before the range. Dates use `YYYY-MM-DD`. `--to` is exclusive, and the range must be positive and at most 366 days. Results are sorted by stored start boundary and then full UUID. Timed boundaries are converted from midnight in the selected IANA zone. An empty result prints nothing. If a boundary midnight falls in a DST gap or repeated time, choose another boundary date or zone.

Creation requires title, start, and end. Updates preserve omitted fields; `--location ''` and `--notes ''` clear those fields. Updating a zone alone preserves the event's existing instants. Delete uses an exact UUID and executes without an interactive prompt. Get, create, and update print ID, calendar, title, time, location, and notes; list prints ID, time, and title. Control characters in user text are rendered as spaces to keep output safe for terminals.

## Time and all-day events

Timed events accept local `YYYY-MM-DDTHH:MM` values or RFC3339 timestamps with explicit offsets. `--time-zone` selects an IANA event zone, such as `Europe/Zurich`. New timed events and listings default to the system IANA zone; `TZ`, when set, must name an IANA zone. Timed updates retain the saved zone unless supplied. If the system zone cannot be resolved, supply `--time-zone` explicitly.

Local times in a DST gap fail. A repeated local time also fails; supply an offset-bearing timestamp to select the occurrence:

```sh
aika-cli events create --title 'Early occurrence' --time-zone Europe/Zurich --start 2026-10-25T02:30:00+02:00 --end 2026-10-25T03:30:00+01:00
```

Offset-bearing timestamps specify instants directly; the IANA zone controls event display. End must be strictly after start.

All-day events use date boundaries with an exclusive end:

```sh
aika-cli events create --title 'Holiday' --all-day --start 2026-10-10 --end 2026-10-12
aika-cli events update <event-uuid> --timed --start 2026-10-10T09:00 --end 2026-10-10T10:00 --time-zone Europe/Zurich
aika-cli events update <event-uuid> --all-day --start 2026-10-10 --end 2026-10-11
```

Changing between timed and all-day requires both boundaries and an explicit `--timed` or `--all-day`. All-day events have no time zone; do not supply `--time-zone` for them.

## Storage and concurrent access

Both clients default to the existing `calendar.sqlite3` beneath the platform's application data directory and `dev.projectcal.desktop` identifier. `--database PATH` overrides the CLI database and can appear before or after subcommands. A new database is initialized with the same schema and default calendar as the desktop. For isolated experimentation:

```sh
aika-cli --database /tmp/aika-sandbox/calendar.sqlite3 events list --from 2026-10-01 --to 2026-11-01
```

The CLI and desktop can run together. Database mutations compare the original event inside a transaction; stale edits fail without replacing current data. The desktop refreshes its visible range when its window receives focus, and keeps unsaved editor input. A conflicted desktop draft must be closed and reopened against the latest event before saving again. Desktop undo also fails if it would overwrite an external change. CLI changes do not appear in desktop undo history.

SQLite waits up to five seconds for a competing transaction. Storage and migration errors leave saved data intact; a database from a newer version is rejected rather than reset.

| Exit code | Meaning                                                     |
| --------- | ----------------------------------------------------------- |
| `0`       | Success, including an empty list, help, or version          |
| `1`       | Storage, migration, or other service failure                |
| `2`       | Invalid arguments, dates, time zone, or unresolved DST time |
| `3`       | Event not found                                             |
| `4`       | Stale-event or identity conflict                            |

Command results go to stdout; errors go to stderr.
