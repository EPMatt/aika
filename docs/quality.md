# Desktop quality verification

## Automated checks

`pnpm test` exercises renderer layout, time handling, and the Rust recurrence domain compiled to WebAssembly. Native Rust and WASM tests consume the same recurrence fixtures. `pnpm test:ui` covers browser interactions, mocked IPC failures, focus refresh with stale editor drafts, normalized read-only calendars, recurrence controls/scopes/preview/undo, and axe WCAG A/AA audits in both themes. These checks do not establish native platform input behavior or full screen-reader usability.

`pnpm rust test --manifest-path src-tauri/Cargo.toml --workspace` covers real SQLite/reopening/undo/migrations, the shared local/test-adapter contract, capability and identity isolation, and unavailable credential-store behavior. CLI process tests cover CRUD persistence, range boundaries, time zones, DST input, and migration rejection; shared service tests cover concurrent mutations and undo conflicts. Test adapters and injected credential stores are compiled only under `cfg(test)`.

The OS-store check is opt-in and uses a disposable account, deleting the entry after reading it:

```sh
pnpm rust test --manifest-path src-tauri/Cargo.toml -p aika-core real_os_store_roundtrip -- --ignored
```

Build and run the embedded native suite on Linux, Windows, or macOS:

```sh
pnpm desktop:test:build
pnpm test:native
```

The native runner builds the CLI and checks it against the same disposable database while the desktop is running, including preservation of editor drafts and rejection of stale saves/undo. Recurrence checks create and preview a finite series, interleave CLI occurrence/series mutations with desktop drafts, split the future, undo scoped changes, and verify restart persistence. Focus refresh is triggered by a synthetic webview focus event; physical OS focus behavior remains a manual check.

The suite starts the debug executable with `AIKA_TEST_DATA_DIR` set to an isolated temporary directory, connects WebdriverIO directly to the embedded loopback server, and uses real IPC and SQLite without mocks. It restarts the application to distinguish durable data/preferences from session undo. `AIKA_TEST_BINARY` can override the executable path; `TAURI_WEBDRIVER_PORT` can override port 4445. Captures go to ignored `test-results/native/`. Linux CI runs under Xvfb; Windows/macOS jobs remain disabled. The scripts support those platforms but require execution there before claiming verification.

`native-test` is an explicit, optional Cargo feature, absent from ordinary builds. A compile-time guard rejects enabling it in release mode. There is no production automation permission or frontend mocking bridge. The embedded driver implements some actions by JavaScript dispatch inside the actual webview; this establishes webview/Rust integration but does not prove trusted OS pointer input, native drag-and-drop, or assistive-technology behavior.

## Multiple local calendars

The SQLite v2 migration preserves the existing Personal calendar, events, and appearance/view/sidebar preferences. Calendar metadata, visibility, and default selection persist independently. The browser repository remains an in-memory design harness; real SQLite restart verification uses the Rust and native suites.

Calendar regression checks cover creation/settings, hover and keyboard menus, default selection, per-calendar colors and filtering in all views, checked event moves and undo, permanent calendar/event deletion, and the persistent zero-calendar state. CLI compatibility covers the current default and operation after Personal is deleted. Calendar deletion does not enter event undo history.

Session verification passed: 24 Playwright tests (including both-theme accessibility audits), 28 Rust/CLI tests, 13 frontend unit tests, packaging tests, the frontend and native test builds, formatting checks, and workspace Clippy with warnings denied. The Linux native suite exercised calendar creation/settings/default/visibility, event moves, deletion, and restart with zero calendars through real IPC and a disposable SQLite database. The existing opt-in OS credential-store test remained skipped; Windows/macOS and manual platform checks remain pending.

## Common recurrence

The schema v3 tests migrate legacy v1/v2 databases without losing events, calendar metadata, or preferences. Storage checks cover atomic split rollback, aggregate concurrency, batch undo, moved exceptions across calendars, destination deletion without resurrection, and reopening. Domain and CLI checks cover daily/weekly/monthly/yearly patterns, intervals, inclusive endings, valid-start counts, unavailable dates, DST gaps/folds, repeated splits, changed monthly seeds, explicit count edits, and bounded expansion. Browser checks exercise the same pure mutation plans through WASM.

Recurrence verification on this Linux host passed 50 Rust/CLI tests (one opt-in credential-store test skipped), 26 frontend/WASM tests, 18 packaging tests, the full 37-test browser suite, and a final rerun of the three recurrence browser tests after date/count corrections. The final Linux native suite passed recurrence preview/scopes/undo, concurrent CLI changes and stale drafts, restart persistence, and existing calendar regressions through real IPC and disposable SQLite. Frontend and native debug builds, generated contracts, formatting, and workspace Clippy with warnings denied passed. Windows/macOS and manual platform verification remain pending as described above.

## Manual platform checks before alpha acceptance

Record OS/webview version, input method, display scale, exact outcome, and screenshot paths. Execute on Windows 11 x64, macOS 14+ Intel and Apple Silicon, Ubuntu 24.04+, and Arch Linux:

- Create/edit/delete/undo timed and all-day events, restart after saves/deletes, and verify last view/sidebar/appearance. Use an isolated OS account or disposable app-data directory.
- Drag across month cells and a time column; move events between dates and to precise times; resize ends; cancel with Escape. Confirm failed writes/DST gaps retain the committed event and retry works.
- Complete essential actions using only the keyboard. Verify opening/closing focus, roving month focus, date/time fields, overflow, and Settings focus containment. Check announcements, event ranges, disabled/read-only actions, and labels with Narrator, VoiceOver, or Orca.
- Check both themes, system appearance changes, reduced motion, regional dates/week start, device-zone changes, 900×600 sizing and narrower tiling, 100%/200% display scale, dense/overlapping/multi-day events, and long titles.
- Verify secure store read/write/delete and locked/unavailable-store errors without plaintext fallback. Local calendar operations must remain usable.
- Build an ordinary release and verify it has no test-driver listener or test fixtures. Install/update/packaging checks belong to phase 5.

Do not mark an unexecuted platform or screen-reader check as passed. See the phase-4 handoff for evidence from this host and remaining checks.

## Google phase 1 foundation

The schema v4 repository suites preserve v1–v3 events, series exceptions, calendar metadata, visibility, defaults, and preferences; reject future schemas; and roll back an injected v4 migration failure. The second provider is compiled only under `cfg(test)` and supplies capabilities while sharing the same repository and service as Local. Tests cover two accounts with colliding calendar/event remote IDs, reconnect/exclusion identity retention, cached reads during authentication/adapter unavailability, event restrictions, checked permission loss, atomic outbox failure, durable restart, and unsent session undo. CLI process tests read a disposable connected-calendar cache without a worker and reject writes when its test adapter is unavailable.

The renderer consumes generated capability maps for creation, editing, deletion, moves, recurrence, and gestures. A mocked IPC browser scenario proves that event restrictions override a writable calendar. Browser evidence does not establish Google authentication or HTTP. See the [Google phase 1 handoff](handoffs/google-phase-1.md) for exact commands, results, and the remaining phase/platform gates.
