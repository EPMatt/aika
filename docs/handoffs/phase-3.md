# Phase 3 — Calendar experience

Status: **implemented and verified locally on Linux.** Phase 4 may proceed with provider proof and native/platform quality checks.

## Source and scope

Base revision: `1ce59e4` in the inherited `/home/srvadmin/Documents` Git repository. This handoff ships in the phase-3 commit. The pre-existing `.gitignore` modification and sibling projects are excluded.

Read the referenced planning task, `docs/plan.md`, `docs/architecture.md`, the earlier handoffs, and `.ai/plans/aika.md`. Phase 2 had already bound native startup to IPC; phase 3 extends that implementation without changing the persistence schema or contracts.

## Delivered

- Drag across month dates in either direction to create an all-day range. The editor shows an inclusive last date; storage retains an exclusive end date. Selection is highlighted before release.
- Drag vertically in a week/day time column to create a timed range. Click creates the existing one-hour default. Pointer selections and drops snap to 15 minutes; selections scroll near the viewport edge. Escape cancels selections.
- Timed drops choose the target date and clock time while retaining elapsed duration. Month and all-day-row drops preserve event type, local start time, and duration/date span.
- Bottom handles resize timed event ends, including midnight. Invalid ordering, DST gaps/folds, or storage failures retain the saved event and undo history. Retry the gesture, or use the editor for an explicit repeated-hour offset. Gestures resolve raw wall times through backend validation before Luxon can normalize them.
- Continuous all-day bars across visible dates, clipped at week boundaries, with continuation markers and accessible range labels. Collision lanes reserve month capacity; hidden events remain available in overflow. Week/day all-day rows grow to fit their lanes.
- Focusable time columns and all-day rows support Enter creation. Event buttons open the date/time editor with Enter for keyboard moving/resizing and all-day range editing. Existing month arrow navigation, shortcuts, focus restoration, and undo remain available; Settings documents the controls.
- Current time refreshes every 30 seconds; the native Today date updates at midnight. Successful event actions clear prior mutation errors.

## Verification

Executed October 9, 2026 on Linux with Node 26.8.2, pnpm 12.10.1, Rust 1.99.0:

```sh
pnpm test
pnpm rust test --manifest-path src-tauri/Cargo.toml --offline
CAL_CHROMIUM_PATH=/usr/bin/chromium pnpm test:ui
pnpm desktop:build --debug --no-bundle
pnpm format:check
pnpm rust fmt --manifest-path src-tauri/Cargo.toml --check
pnpm rust clippy --manifest-path src-tauri/Cargo.toml --all-targets --offline -- -D warnings
pnpm types:generate
git diff --check
```

Results: **13 renderer unit tests, 6 Rust integration tests, and 11 browser scenarios passed.** Added coverage verifies clipped/colliding all-day lanes, crowded all-day overflow, exclusive bounds, range creation, precise drops, timed resizing, invalid-resize recovery, keyboard date/time editing, undo, DST gesture rejection, and Escape cancellation. Mocked IPC verifies failed drops preserve persisted data/history, retry succeeds, and the move survives renderer reload. Actual SQLite/reopening evidence remains the Rust service suite; mocked IPC does not establish native pointer behavior.

The frontend production build and native Linux debug build passed. Generated contracts have no drift. Browser tests required sandbox escalation for the local server/Chromium. Review captures are reproducible under ignored `test-results/prototype-captures-the-review-scenes/`; month/week light captures were visually inspected. Existing `docs/review/` images preserve the earlier design review.

Native smoke check used isolated data:

```sh
XDG_DATA_HOME=/tmp/aika-phase3-native timeout 8s src-tauri/target/debug/cal-desktop
```

The executable reported `runtime: desktop` and `rootChildren: 1`; exit 124 was the intentional timeout. No user database was modified.

## Limits and next phase

No known failing local checks. Embedded native gesture/CRUD automation, comprehensive screen-reader checks, Windows/macOS, hosted CI, Node 24 execution, high-DPI/platform input checks, and installers remain unverified. Timed drag creation stays within one column; use the editor for overnight/multi-day creation or changing event type. End-handle resizing and date/time fields cover resizing; no start-handle gesture is provided.

Read the plan, architecture, and this handoff, then implement phase 4: provider registry, backend secure credential service, a second test-only adapter, read-only/calendar-identity isolation, and test-build-only native automation. Keep production free of test plugins and fixtures. Do not enable remote accounts, recurrence, notifications, search, export, or backup. Windows/macOS CI entries remain commented out according to the user's decision.
