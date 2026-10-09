# Phase 1 — Design prototype

Status: **ready for user review; design acceptance pending.** Phase 2 must wait for this gate, as agreed in the planning conversation.

## Delivered

- Custom month/week/day layouts with warm-neutral/sage light and dark themes, plus automatic system appearance.
- Collapsible sidebar, mini calendar, one local-calendar visibility toggle, restrained toolbar, and Settings dialog.
- Anchored editor with title, timed/all-day ranges, optional location/notes, save/cancel, and prototype delete/undo.
- Month overflow popovers, multi-day fixtures, time-grid overlap lanes, and keyboard creation/navigation/view switching/focus restoration.
- Adaptive month-cell capacity at the 900×600 minimum; narrower tiling windows collapse the sidebar and wrap the toolbar.
- Reproducible empty, standard, dense, overlapping, and multi-day fixtures and preserved review screenshots.

## Run and review

```sh
pnpm dev
pnpm desktop
```

Browser preview: http://127.0.0.1:1420. Fixed review date: `/?date=2026-10-09`. Use `theme=light`, `theme=dark`, `scenario=empty`, or `scenario=dense` for repeatable scenes. The native shell runs the same UI in the OS webview.

See [the screenshot gallery](../review/README.md). Review the balance of spacing and typography, palette, month/week/day readability, editor placement, crowded cells, overlapping events, and both themes. Approval may be given with specific refinements; record the accepted design before phase 2.

## Verification

```sh
pnpm build
pnpm format:check
pnpm test
CAL_CHROMIUM_PATH=/usr/bin/chromium pnpm test:ui
```

Results: **7 unit tests and 5 browser scenarios passed**, along with the production frontend build, formatting checks, Rust formatting, and Clippy with warnings denied. The unit suite covers locale normalization, six-week/leap-month boundaries, overlap lanes, touching events, overnight/exclusive all-day ranges, DST date spans, and undo snapshots. Browser scenarios cover POSIX-locale startup, three views, two themes and system changes, appearance persistence, sidebar controls, CRUD/notes/delete/undo, keyboard focus, dense overflow, reduced motion, and window sizing. Screenshots are renderer evidence; the separate native startup check confirms Linux mounting/IPC.

## Deliberate prototype limits

Events are sample data in `PrototypeRepository`; modifications reset on reload. No user calendar database exists yet. Only appearance persists. No sample events should seed phase 2's database.

Drag-to-create, precise timed drop positioning, resize gestures, authoritative DST ambiguity handling, complete screen-reader validation, and preference persistence remain phase 2/3/4 work. Existing drag behavior moves an event between dates while keeping its duration and local time. Browser coverage does not prove native pointer behavior on other OSes.

No connected accounts, credential service, providers, search, recurrence, notifications, export, backup/restore, updater, or application server has been implemented.

## Next-phase handoff

After user design acceptance, read the plan and architecture and rerun the checks. Introduce the Rust calendar service, SQLite repository/migrations, preferences, typed errors, and session undo. Replace the prototype repository in the production entrypoint with a typed IPC client while keeping fixtures available only to the design/test harness. Implement persistence/time-zone failure cases before completing gestures. Document interface changes and migration results in the phase-2 handoff.
