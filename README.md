# aika

A desktop calendar. Tauri 2, React, TypeScript, and Rust.

**Current milestone: phase 5 private alpha preparation; acceptance pending.** The desktop app saves multiple local calendars in SQLite, restores events, calendar settings, visibility, default selection, and appearance/view/sidebar preferences after restart, and supports session event undo. The browser preview remains an in-memory design harness with sample data.

## Run

Use Node 24 LTS and pnpm 12.10.1:

```sh
pnpm install --frozen-lockfile
node scripts/tool.mjs rustup target add wasm32-unknown-unknown
pnpm rust install wasm-bindgen-cli --version 0.2.129 --locked
pnpm desktop
```

Desktop development requires Rust 1.99.0 and the [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/). For a browser preview, run `pnpm dev` and open http://127.0.0.1:1420.

The browser preview and frontend builds require the WebAssembly target and pinned wasm-bindgen tool shown above. `pnpm domain:build` generates bindings from the shared Rust domain; generated bindings are not checked in.

The tooling wrapper also detects a workspace-local Rust installation in `.tools/`. It does not alter global shell configuration. This workspace already has that installation; on another machine use the normal Rust toolchain setup.

## Local calendars

Use **+** beside **Calendars** in the sidebar to create a calendar. Click a calendar row to show or hide its events. Hover or focus the row to open **…**, where you can select the default, edit its name/description/color, or permanently delete the calendar and its events. The event editor lets you choose a calendar and move existing events. Saving a new or moved event to a hidden calendar reveals it. You can delete every calendar; event creation becomes available again after creating one.

## Recurring events

The event editor's **Repeat** controls support daily, weekly with selected weekdays, monthly by date or ordinal weekday, and yearly schedules. Set an interval and an optional ending date/count, then use **Preview next dates** to inspect the schedule. Timed repeats keep their saved source zone; unavailable dates and DST gap starts are skipped.

Editing a recurring event offers **This occurrence**, **This and following**, and **Entire series**. Individual deletions skip a date. Series/following edits ask before resetting affected individual changes. Moves and resizes also request a scope, and a split is one undoable action. The CLI supports the same patterns and scopes; see [recurring CLI usage](docs/cli.md#recurring-events).

## CLI

The standalone `aika-cli` uses the same Rust core and local calendars as the desktop app. It works with the desktop open or closed, without launching a GUI. See [CLI usage](docs/cli.md) for time formats, exit codes, and concurrency behavior.

```sh
pnpm cli --help
pnpm cli calendars list
pnpm cli calendars create --name "Work" --color '#5B7FA3'
pnpm cli calendars set-default <calendar-id>
pnpm cli events list --from 2026-10-01 --to 2026-11-01
pnpm cli events create --title "Lunch" --start 2026-10-10T12:00 --end 2026-10-10T13:00 --time-zone Europe/Zurich
pnpm cli events get <event-uuid>
pnpm cli events update <event-uuid> --title "Lunch with Sam"
pnpm cli events delete <event-uuid>
```

Build a standalone release binary with `pnpm cli:build` (`src-tauri/target/release/aika-cli`, `.exe` on Windows), or install it onto Cargo's bin path with:

```sh
pnpm rust install --path src-tauri/cli --locked
```

Tagged release installers include the CLI; see [release packages and installed CLI paths](docs/releases.md). Private-alpha candidates still install the desktop separately.

## Verify

```sh
pnpm build
pnpm test
pnpm exec playwright install chromium
pnpm test:ui
pnpm rust test --manifest-path src-tauri/Cargo.toml --workspace
pnpm rust fmt --manifest-path src-tauri/Cargo.toml --all --check
pnpm rust clippy --manifest-path src-tauri/Cargo.toml --workspace --all-targets -- -D warnings
pnpm types:generate
pnpm desktop:build --no-bundle
```

For an existing Chromium installation, run `CAL_CHROMIUM_PATH=/usr/bin/chromium pnpm test:ui` on Linux. Browser tests cover the design harness and mocked IPC error handling; Rust integration tests cover SQLite CRUD, restart, undo, ranges, migrations, and time zones. Native webview CRUD/restart is checked separately with `pnpm desktop:test:build` and `pnpm test:native`; see [quality verification](docs/quality.md).

## Design review

Month/week/day, light/dark/system, an anchored event editor, a collapsible sidebar, calendar visibility, keyboard creation, and undo are interactive.

Review URLs use a fixed date for reproducible screenshots:

- `/?date=2026-10-09&theme=light`
- `/?date=2026-10-09&theme=dark`
- `/?date=2026-10-09&scenario=empty`
- `/?date=2026-10-09&scenario=dense`

Shortcuts: **N** creates an event; **T** returns to today; **1/2/3** selects month/week/day; **Ctrl/Cmd+Z** undoes a session change; **Escape** dismisses the editor. In month view, dragging an event between dates preserves its local time and duration; DST gaps and repeated times require editing explicitly. Drag across month dates to create an all-day range, or across a time column to create a timed range. Timed drops and the bottom resize handle snap to 15 minutes. Escape cancels a selection. All-day events span visible dates; crowded month rows use overflow. Enter on a time column or all-day row creates an event; Enter on an event opens its dates/times for keyboard moving and resizing. Gesture failures leave saved data unchanged; retry the gesture or use the editor to select a DST offset.

Read [the implementation plan](docs/plan.md), [architecture decisions](docs/architecture.md), and the [private-alpha handoff](docs/handoffs/phase-5.md) before continuing. The backend provider registry and OS credential service are in place; production calendars remain local. The [Google integration specification](docs/integrations/google-calendar.md) and [Google phase 1 handoff](docs/handoffs/google-phase-1.md) describe the shared repository, account model, capabilities, and outbox foundation; Google connection and synchronization remain planned. Windows/macOS and manual assistive-technology checks remain pending before alpha acceptance.

## Private alpha candidates

Run `pnpm alpha:build <native-target-triple>` to build release installers and collect checksums, provenance, and release notes under `artifacts/`. Linux x86_64 builds produce `.deb`, `.rpm`, and AppImage packages; `pnpm desktop:build` also includes these formats by default. Fedora users can install the RPM with `sudo dnf install ./<filename>.rpm`. Supported targets and installation/update evidence are documented in [alpha verification](docs/alpha/verification.md). `pnpm test:packaging` verifies artifact collection. Private-alpha CI builds Linux x64, Windows x64, and both macOS architectures. Linux uses Ubuntu 24.04 and inspects RPM contents and candidate checksums before upload. Fedora installation/launch, Windows/macOS, and the manual acceptance matrix remain pending. Private-alpha candidates are not published automatically. Numeric tags on main publish desktop packages with the bundled CLI through the separate [tagged release workflow](docs/releases.md).
