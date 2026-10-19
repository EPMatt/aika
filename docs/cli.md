# aika CLI

`aika-cli` operates on the desktop application's shared SQLite repository through the Rust service. No running desktop app or GUI is required. It reads connected-calendar caches and queues supported ordinary-event edits; it does not connect accounts, fetch uncached data, or run a sync worker. Delivery waits for the desktop. JSON output, search, import/export, and CLI undo are outside this version.

## Build and run

Use the [development setup](../CONTRIBUTING.md#development-setup) for the pinned Node/pnpm/Rust versions:

```sh
pnpm cli --help
pnpm cli events create --help
pnpm cli calendars create --help
pnpm cli:build
pnpm rust install --path src-tauri/cli --locked
```

`pnpm cli` runs the development binary. `pnpm cli:build` produces `src-tauri/target/release/aika-cli` (`aika-cli.exe` on Windows). Cargo installation places it in Cargo's bin directory, which must be on your PATH. Tagged release installers include this binary; see [installed CLI paths](releases.md#use-the-bundled-cli). Private-alpha candidates do not include it.

The commands below use the installed binary. Replace IDs with the exact ID printed on creation or listing.

## Calendars

```sh
aika-cli calendars list
aika-cli calendars get local-default
aika-cli calendars create --name 'Work' --description 'Office' --color '#5B7FA3'
aika-cli calendars update <calendar-id> --name 'Office calendar' --description ''
aika-cli calendars set-default <calendar-id>
aika-cli calendars hide <calendar-id>
aika-cli calendars show <calendar-id>
aika-cli calendars delete <calendar-id>
aika-cli calendars delete <calendar-id> --force
```

Use the exact ID printed by creation or listing. The initial Personal calendar has the ID `local-default`; newly created calendars use UUIDs. IDs are not resolved by name or prefix. Calendar commands do not require dates or a time zone. Names need not be unique.

Creation requires `--name`, defaults to an empty description and sage (`#718675`), and creates a visible calendar. Names are trimmed and must contain 1–100 characters; descriptions allow up to 2000 characters. Updates require at least one field, preserve omitted fields, and accept `--description ''` to clear the description. New or changed colors must exactly match the palette; an unchanged imported provider color is retained:

| Color  | Hex       |
| ------ | --------- |
| Sage   | `#718675` |
| Blue   | `#5B7FA3` |
| Violet | `#8A6FA8` |
| Rose   | `#B56F86` |
| Amber  | `#B38B43` |
| Teal   | `#4F918A` |
| Orange | `#B77B50` |
| Slate  | `#768390` |

List includes hidden calendars in creation order and prints ID, name, color, default status, and visibility. An empty result prints nothing. Get, create, and update print ID, name, description, color, provider, writability, default status, and visibility. Settings commands print a confirmation. User text is sanitized for terminal output.

`set-default` controls subsequent event creation. Show and hide persist the desktop's visibility setting; CLI event listings still include hidden calendars. Saving an event reveals its calendar, as in the desktop app.

Local calendar deletion permanently removes the calendar and all its events. Imported calendar removal instead excludes it from aika without deleting it in the provider; pending work blocks removal even with `--force`. Empty calendars can be deleted directly; calendars containing events require `--force`. The event-count check and deletion run in one transaction, so a concurrently added event cannot bypass the safeguard. No interactive prompt is used. Deleting the default selects the oldest eligible writable local calendar, then an imported calendar, then none. Every calendar can be deleted; listing then stays empty across restarts, and the next created calendar becomes default. Fresh databases still initialize with Personal.

## Events

```sh
aika-cli events list --from 2026-10-01 --to 2026-11-01 --time-zone Europe/Zurich
aika-cli events create --title 'Lunch' --start 2026-10-10T12:00 --end 2026-10-10T13:00 --location 'Cafe' --notes 'Bring agenda'
aika-cli events get <event-uuid>
aika-cli events update <event-uuid> --title 'Lunch with Sam' --notes ''
aika-cli events delete <event-uuid>
```

Listing selects events that overlap `[from, to)`, including events that start before the range. Dates use `YYYY-MM-DD`. `--to` is exclusive, and the range must be positive and at most 366 days. Results are sorted by stored start boundary and then full UUID. Timed boundaries are converted from midnight in the selected IANA zone. An empty result prints nothing. If a boundary midnight falls in a DST gap or repeated time, choose another boundary date or zone.

Creation requires title, start, and end, and uses the default calendar selected through the CLI or desktop UI. If no calendars exist, use `calendars create --name NAME` first. Listings include selected active calendars and connected cached events, including calendars hidden in the desktop UI. Missing remote recurrence coverage is not fetched by this command; inspect coverage in the desktop before treating cached results as complete. Get, update, and delete locate single events by their full UUID; updates preserve their calendar. Calendar creation/settings/deletion are available through `calendars`; event assignment and moves between calendars remain managed in the desktop UI. Updates preserve omitted fields; `--location ''` and `--notes ''` clear those fields. Updating a zone alone preserves the event's existing instants. Delete uses an exact UUID and executes without an interactive prompt. Get, create, and update print ID, calendar, title, time, location, notes, and applicable repeat settings; list prints ID, time, and title. Control characters in user text are rendered as spaces to keep output safe for terminals.

## Recurring events

The full scope/pattern examples below apply to local calendars. Google supports compatible all-day/UTC recurring creation, occurrence edits/cancellations/restoration, series metadata edits and whole-series deletion. Google following splits/deletion use the phase 03 grouped protocol with explicit future-exception reset consent. Structural series edits, regional/DST and ordinal-weekday rules, invitee organizer-field edits and special event types remain unsupported. Phase 05 enables organizer-owned meeting detail edits/cancellation through the existing commands; CLI writes preserve guests and use everyone notifications. Guest-list editing and alternative notification choices are available through the desktop editor/native meeting contract. Actions that would copy an existing conference to a new resource are blocked. Inspect `events actions ID`; Google series edits preserve exceptions automatically and reject `--reset-exceptions`. A cancelled occurrence can be restored using an occurrence update and its original selector.

```sh
aika-cli events create --title 'Stand-up' --start 2026-10-05T09:00 --end 2026-10-05T09:15 --time-zone Europe/Zurich --repeat weekly --weekdays mon,wed,fri
aika-cli events create --title 'Month end' --all-day --start 2026-01-31 --end 2026-02-01 --repeat monthly --count 6
aika-cli events create --title 'Planning' --start 2026-10-05T09:00 --end 2026-10-05T10:00 --repeat monthly --monthly-on weekday --until 2027-06-30
aika-cli events update '<series-uuid>@2026-10-09T07:00:00.000Z' --scope occurrence --title 'Longer stand-up'
aika-cli events delete '<series-uuid>@2026-10-09T07:00:00.000Z' --scope occurrence
aika-cli events update '<series-uuid>@2026-10-12T07:00:00.000Z' --scope following --title 'Future stand-up'
aika-cli events update <series-uuid> --scope series --interval 2 --reset-exceptions
aika-cli events update <series-uuid> --scope series --no-repeat --reset-exceptions
aika-cli events delete <series-uuid> --scope series
```

`--repeat daily|weekly|monthly|yearly` starts a pattern; `--interval` defaults to one. Weekly `--weekdays` accepts comma-separated `mon,tue,wed,thu,fri,sat,sun` and must include the seed weekday. Without it, weekly repeats use the seed weekday. Monthly `--monthly-on date|weekday` defaults to the date number; weekday uses the seed's ordinal weekday, including fifth weekdays. Yearly uses the seed month/day. Invalid dates are skipped, never clamped to the end of a month.

Repeats default to no ending. Choose `--until YYYY-MM-DD` (inclusive in the event's source zone) or `--count N`; they are mutually exclusive. `--forever` clears an existing ending. Omitted update fields remain unchanged. `--no-repeat` removes recurrence and leaves one event at the affected segment's first occurrence. Intervals allow 1–10000 and counts 1–100000.

Timed starts follow the saved IANA zone, keeping local clock time and elapsed duration across DST. Nonexistent generated starts are skipped; ambiguous generated starts use the earlier offset. A manually selected offset on the seed is preserved. Invalid generated dates/times do not consume count; cancelled individual occurrences do. All-day repeats use dates and calendar-day durations.

Listing expands occurrences within the requested range and prints exact selectors `<master-uuid>@<original-start>`. The original start is a canonical UTC timestamp with milliseconds for originally timed schedules or `YYYY-MM-DD` for originally all-day schedules. Quote selectors and copy them exactly. Moving an occurrence retains its selector. A bare UUID retrieves the master and its repeat settings; an occurrence selector retrieves its effective values.

Recurring updates/deletes require `--scope occurrence|following|series`. Occurrence/following scope requires a selector. Series updates edit the master, including when supplied a selector. Occurrence changes leave the rest of the schedule intact; recurrence options require series/following scope. Following changes split into a new series and print its new master UUID. Unchanged timing and pattern retain the original interval phase and the remaining count. Changed timing or a replacement pattern begins at the edited selected occurrence. Supplying `--count N` explicitly sets the new segment’s count, including when N equals the original count; omitting it preserves the remaining count. Moving an unchanged weekly weekday selection rotates its weekdays with the date change.

Series/following edits reset affected individual edits and cancellations. If any exist, the command fails without changes unless `--reset-exceptions` is supplied. Deletions already identify their destructive extent through `--scope`. Create an ordinary event for an additional date; explicit extra-date lists are not supported.

Expansion errors never print partial results. Listings allow at most 10000 events, and expansion allows 100000 schedule steps per series. Use a shorter range or a closer/bounded schedule if a limit is reached.

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

The CLI and desktop can run together. Event mutations and calendar metadata updates compare the original saved values inside a transaction; stale edits fail without replacing current data. The desktop refreshes its visible range when its window receives focus, and keeps unsaved editor input. A conflicted desktop draft must be closed and reopened against the latest event before saving again. Desktop undo also fails if it would overwrite an external change. CLI changes do not appear in desktop undo history.

SQLite waits up to five seconds for a competing transaction. Storage and migration errors leave saved data intact; a database from a newer version is rejected rather than reset.

| Exit code | Meaning                                                     |
| --------- | ----------------------------------------------------------- |
| `0`       | Success, including an empty list, help, or version          |
| `1`       | Storage, migration, or other service failure                |
| `2`       | Invalid arguments, dates, time zone, or unresolved DST time |
| `3`       | Event or calendar not found                                 |
| `4`       | Stale-event, stale-calendar, or identity conflict           |

Command results go to stdout; errors go to stderr.
