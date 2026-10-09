# Architecture decisions

## Desktop shell and toolchain

Tauri 2 uses native window chrome and each platform's webview. React + custom CSS controls the calendar visuals; Radix supplies editor/dialog focus behavior. System fonts require no network downloads. Node 24 LTS is the CI baseline; pnpm 12.10.1 and Rust 1.99.0 are pinned. Dependency resolutions are recorded in the pnpm/Cargo lockfiles. The current host has Node 26.8.2; record checks accordingly rather than claiming local Node 24 execution.

The shell exposes only `get_app_info` in phase 0. No filesystem, arbitrary shell, network, or credential commands are exposed to the renderer. CSP allows local assets and IPC; inline styles are necessary for calendar positioning. Native controls keep platform close/minimize/maximize behavior. Production packaging has an application icon derived from the repository SVG.

Canonicalize OS/webview locales before formatting: Linux can expose POSIX tags such as `en_US.UTF-8` or `C`, which Intl/Luxon reject directly. Convert valid regional tags to BCP 47 and fall back to Intl's system locale. Native startup checks caught this incompatibility; regression tests cover it. Below the requested minimum window width, collapse the sidebar and wrap the toolbar to accommodate tiling compositors that override application minimum sizes.

## Boundaries

Data flow: React calendar → typed IPC client → application calendar service → registered provider → persistence or remote adapter. Frontend views own rendering/selection/interaction state, never provider-specific event mapping, SQL, tokens, or authentication.

The browser design harness uses `PrototypeRepository` explicitly. It is in-memory and independently undoable; this is a design harness, not the permanent provider implementation. Native startup binds the typed IPC client and never loads the browser fixture repository. No sample events should seed a user's database.

Contracts originate in `src-tauri/src/contracts.rs`. `pnpm types:generate` uses ts-rs to export declarations to `src/generated`; CI rejects drift. Stable internal UUIDs differ from future remote IDs. Timed values cross IPC as UTC ISO strings with an IANA source zone; all-day values cross as ISO calendar dates, with exclusive end dates. Keep optional fields empty strings in the v0 contract.

## Persistence and undo, phase 2

Use bundled SQLite through rusqlite, at the OS application-data directory, behind a backend repository. Run blocking database work off the UI thread. Version migrations transactionally; never recreate/reset a database after migration or open failures. Add query indexes and select events overlapping the requested interval. Store preferences alongside local application data. Serialize mutations through the calendar service; undo is session-scoped and compensates successful mutations.

The backend validates titles, date ordering, valid time zones, and DST ambiguity. Nonexistent local times must be rejected; repeated local times require selecting an offset. Luxon supports renderer layout/formatting; Rust chrono/chrono-tz owns authoritative validation. Test shared time fixtures across boundaries. The phase-1 editor intentionally does not implement those backend guarantees yet.

## Provider extension, phase 4 and later

The provider registry exposes descriptor, capabilities, normalized calendar discovery, bounded event listing, and normalized create/update/delete operations. Authentication and sync are backend extension points. Unsupported capabilities produce typed errors. Contract tests exercise local and test-only adapters, independent calendar identities, and read-only behavior without changing views.

The first remote provider introduces a common cached adapter/sync coordinator, rather than copying caching into Google-specific code. Durable offline mutations use an outbox with retries and explicit conflicts. Subsequent Google/Microsoft/CalDAV implementations map their auth, IDs, cursors, revisions, event formats, and errors to that common layer. New recurrence/invitation features need a separate domain/UI increment.

The credential service stays behind the backend boundary. Use persistent OS stores (Keychain, Windows Credential Store, Linux Secret Service), namespace entries by application/account, and never return credentials to the renderer. Locked/unavailable stores block connected accounts. Data at rest is not application-encrypted in v0. No session-only or plaintext fallback.

## Verification and distribution

Vitest covers layout and prototype repository behavior; Playwright covers renderer flows and captures review images. Browser checks are not evidence of native integration. Phase 4 adds WebdriverIO embedded Tauri automation on Windows/macOS/Linux, with plugins enabled only in test builds, plus real-platform checks.

The active CI desktop target is Linux on ubuntu-latest; Windows/macOS entries remain commented out per the latest user decision. CI is ready for a repository rooted at this project. This workspace currently inherits a parent Git repository; no external repository is created or connected automatically. Cross-platform CI runs cannot be claimed until such a repository runs the workflow. Windows/macOS launch checks remain external verification tasks.

Private distribution uses Windows installers, per-architecture macOS DMGs with ad-hoc signing, and Linux AppImage/deb. There is no public signing, notarization, updater, analytics, or server in v0.

## Phase 2 implementation

`CalendarService` owns the SQLite connection and session compensation stack. Tauri lazily opens `calendar.sqlite3` in `app_data_dir` from a blocking worker, including directory creation and transactional migration. A shared mutex serializes access; renderer mutations also queue in invocation order. Initialization failures are returned as typed errors and may be retried; no reset fallback exists. Schema version 1 stores the default calendar, normalized event payloads with indexed range columns, and validated preferences. Future schema versions are rejected. Browser fixture URL parameters are ignored in native startup.

IPC commands are `calendar_snapshot`, `save_event` (explicit create/update intent), `delete_event`, `undo_event`, `get_preferences`, `set_preferences`, and `resolve_local_time`. Snapshot queries use exclusive instant/date bounds and include overlapping events. Event IDs are UUIDs; the default calendar retains the contract identity `local-default`. UTC instants are canonicalized before persistence. Errors carry code/message/offset choices without exposing database paths or SQL.

The editor submits raw local wall-clock values to Rust before building its UTC contract. Gaps are rejected and folds require explicit UTC-offset selection. A UTC instant already determines its offset unambiguously; `save_event` validates that normalized contract and the source IANA zone. Shared fixtures in `tests/fixtures/time.json` verify Rust and Luxon resolution. Failed save/delete input remains open. Undo compensates only committed mutations, remains available after failed writes, and clears on application restart. Preferences are durable and do not enter event undo history.

## Phase 3 interaction boundary

Calendar gestures remain renderer interaction state. Month selection creates an all-day date range with an exclusive end; timed selections resolve raw snapped wall-clock fields through `resolve_local_time` before opening the editor. Native gaps/folds therefore cannot be silently normalized by Luxon. Timed drops retain elapsed duration; resizing changes only the end instant and rejects end-before-start. Event types change in the editor. Saves use the existing explicit-update IPC and session undo path; failed gestures never optimistically replace displayed events. Users retry gestures or select offsets in the editor.

All-day bars are clipped to each visible week and assigned nonoverlapping lanes using exclusive date bounds. Month capacity reserves those lanes and exposes hidden events through the existing overflow popover. Week/day rows grow to fit their lanes. Pointer capture supports selection/resizing, Escape cancels selection, and timed selections scroll near the viewport edge. Focusable date cells, all-day rows, time columns, event buttons, and date/time editor inputs provide keyboard alternatives. The live clock refreshes every 30 seconds, including the native current date at midnight.
