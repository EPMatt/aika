# Phase 2 — Local domain

Status: **implemented and verified locally on Linux.** The user’s October 9, 2026 instruction to proceed clears the design gate. Phase 3 may continue with the gesture and interaction work in the plan.

## Source and scope

Base revision: `c1b7fdf` in the inherited `/home/srvadmin/Documents` Git repository. Implementation revision: `1ce59e4`. This handoff shipped in that phase-2 commit. The pre-existing `.gitignore` modification and sibling projects are excluded from that commit.

## Delivered

- One default Personal calendar, initially empty. Native startup ignores design fixture URL parameters and never loads the browser sample repository.
- Rust calendar service, typed IPC client, generated snapshot/range/preferences/error contracts, and serialized mutations. SQLite opening, migrations, queries, preferences, and writes run on blocking workers behind a shared service mutex.
- Bundled SQLite at the OS application-data directory: `dev.projectcal.desktop/calendar.sqlite3` on Linux. Schema version 1 stores calendar identity, event payloads/indexed range columns, and preferences. Initialization can be retried; open/migration failures never trigger deletion or database reset.
- Persistent creation/editing/deletion, timed UTC instants with source IANA zones, all-day dates with exclusive end dates, and overlap queries for the visible range. UUID event identities and explicit create/update intent prevent accidental upserts from stale editor state.
- Session undo compensates successful event mutations; failed validation/writes do not consume undo. Undo history clears on restart. Appearance, view, and sidebar preferences persist independently.
- Raw wall-clock validation rejects nonexistent DST times. Repeated times require explicit UTC-offset selection. Failed save/delete input remains open, with visible errors and controls disabled while saving. Date moves also reject gaps/folds; choosing a repeated-hour offset is available through the editor.
- Browser fixtures remain an in-memory review harness. No SQLite, credentials, SQL, or filesystem access is exposed to the renderer.

## Design decision record

The [phase-2 decisions and rationale](../architecture.md#phase-2-decisions-and-rationale) document the storage representation, instant/date query bounds, mutation ordering, per-event compensation, existence-only conflict checks, preference behavior, and fixed local-calendar identity. The later documentation audit records these implementation choices without changing phase-3 behavior.

## Verification

Executed October 9, 2026 with Linux, Node 26.8.2, pnpm 12.10.1, Rust 1.99.0:

```sh
pnpm build
pnpm format:check
pnpm test
pnpm types:generate
pnpm rust fmt --manifest-path src-tauri/Cargo.toml --check
pnpm rust test --manifest-path src-tauri/Cargo.toml --offline
pnpm rust clippy --manifest-path src-tauri/Cargo.toml --all-targets --offline -- -D warnings
pnpm desktop:build --debug --no-bundle
CAL_CHROMIUM_PATH=/usr/bin/chromium pnpm test:ui
```

Results: **12 renderer unit tests, 6 Rust service integration tests, and 7 browser scenarios passed.** Rust tests use actual SQLite and cover CRUD/reopening, durable preferences, session undo, failed-write undo retention, overlap/exclusive boundaries, leap days, multi-day/year-boundary instants, zone changes, unknown schema rejection, transactional migration rollback, corrupt files, and validation failures. Shared time fixtures exercise Rust and Luxon resolution. Browser tests include DST gap/fold editing and a mocked desktop IPC test for failed input retention, retry, empty startup, fixture isolation, and preference hydration; that mock does not prove native persistence.

The native debug executable is `src-tauri/target/debug/cal-desktop`. A separate Linux startup used isolated data:

```sh
XDG_DATA_HOME=/tmp/aika-phase2-native timeout 8s src-tauri/target/debug/cal-desktop
```

It reported `runtime: desktop` and `rootChildren: 1`; inspecting its SQLite database confirmed schema version 1, zero events, and default preferences. Exit 124 was the intentional timeout. No user database was seeded or modified by this check. Existing review screenshots remain under `docs/review/`; test artifacts are under ignored `test-results/`.

## Limits and next phase

No known failing local checks. Linux startup plus service/browser integration is the current evidence; full embedded native CRUD automation, Windows/macOS launch, hosted CI, installers, and Node 24 execution remain unverified. Browser renderer checks cannot establish other-platform pointer or accessibility behavior.

Read `docs/plan.md` and `docs/architecture.md`, then continue phase 3. Complete drag creation, precise timed moves/resizing, and keyboard alternatives, with persistence and failure recovery checks. Existing date moves preserve local time/duration; repeated-hour date drops report an error and require editor selection. Follow the device zone while retaining stored timed instants/all-day dates. Provider proof, credentials, comprehensive accessibility/platform automation, and distribution remain phases 4/5. No remote accounts, recurrence, notifications, search, export, or backup is included.
