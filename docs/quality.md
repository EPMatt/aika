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

## Google phase 2 connection and selection

The native core suite exercises real random-port loopback sockets with test-only Google transport/browser/store fixtures: S256 PKCE/state, authenticated identity, denial/cancellation/timeout/listener cleanup, locked/write-failing credential-store recovery, session expiry, grant validation, refresh-token preservation and refresh failures, full pagination/hidden and ineligible entries, and partial discovery failure. Socket tests require a host environment that permits loopback listeners. Disposable SQLite tests prove confirmation atomicity, stale discovery rejection across connections, duplicate-account/scoped UUID retention, exclusions, permission restoration, presentation/visibility persistence, pending-work protection, restart, and Local isolation. No Google token/code/verifier is stored in SQLite or returned by native IPC.

`tests/google-connection.spec.ts` covers the descriptor picker and keyboard focus, unavailable Google preview explanations, both-theme mocked native connection/checklists/accessibility, selection revisions, discovery retry, disconnect, and a late-start cancellation race. A delayed descriptor fixture verifies keyboard focus after asynchronous picker loading. React Strict Mode startup must not perform two browser/discovery operations. These mocks and native protocol fixtures do not prove Google's live endpoints or installed system-browser behavior. The existing native webview suite separately verifies Local still works after the provider popup change and checks the actual Google descriptor/configuration explanation through native IPC.

Google client configuration is absent unless supplied at build time or launch; real OAuth requires `AIKA_GOOGLE_CLIENT_ID` and optional `AIKA_GOOGLE_CLIENT_SECRET`, an application-owned Calendar API project/consent test audience/Desktop client, an unlocked OS credential store, and disposable Google accounts. Run the opt-in real OS-store test separately from transport mocks. Record live/installed-platform outcomes before marking the phase 2 exit gate complete. At the phase 2 milestone, event import/sync and Google editing remained gated to later phases; phase 3 incoming-sync evidence is recorded below. See the [phase 2 handoff](handoffs/google-phase-2.md) for exact commands, actual results, failures, and pending gates.

Google phase 3 adds shared incoming coordinator/repository fixtures for staging, cursor publication rollback, HTTP 410 after partial pagination, failed full resync, tombstones, pending overlays, moved/cancelled instances, conservative conversion, projection coverage, lease recovery, selection/disconnect races, metadata overrides, independent database connections, and v4-to-v5 migration rollback. `tests/google-sync.spec.ts` checks mocked native cached reads, read-only editing, missing coverage, redacted offline failures, refresh, and recovery. These establish automated boundaries and do not replace disposable live Google or installed-platform evidence; see the [phase 3 handoff](handoffs/google-phase-3.md).

Google build configuration uses pure resolver tests for complete-pair precedence and blank/orphan handling. `pnpm test:google-config` builds the same core target three times with fictional configuration (client A with a secret, client B without a secret, and no configuration), then runs nine test-executable launches with independent runtime environments. This verifies embedding with runtime variables removed, complete runtime overrides, isolation from an orphan secret, changed/removed build input invalidation, and continued runtime-only support. The smoke test is ignored during ordinary Rust suites; its dedicated runner supplies the expected fictional values. No OAuth/network/store operation is performed. CI runs the smoke check before unconfigured native builds.

The native runner accepts optional `AIKA_TEST_EXPECT_GOOGLE_AVAILABLE=true` or `false` to check a binary built with embedded settings independently of its launch environment. Without that override, it retains the runtime-client-ID expectation used by ordinary unconfigured CI. For an embedded-config native check, build the debug test app with fictional client settings, remove both Google variables from its launch environment, and run `pnpm test:native` with the expectation set to `true`. Run the same suite with an unconfigured build and expectation `false` for the disabled case. See the [build configuration handoff](handoffs/google-build-configuration.md) for executed commands and results; descriptor availability does not establish live Google authorization or installed-platform behavior.

`pnpm test:env` verifies optional project-root dotenv loading, file-over-shell precedence (including empty values), inheritance of omitted keys, quoting/comments/export/multiline/literal-dollar syntax, edits/removal, redacted file-read errors, and actual tool subprocess propagation from another working directory. It is included in quality CI. `pnpm test:google-config` passes the explicit tool-wrapper `--no-env-file` flag so its fictional compilation/launch fixtures never consume a developer's real `.env`. The native regression runner loads the same project environment for its subprocesses and availability expectation. See the [dotenv configuration handoff](handoffs/google-dotenv-configuration.md) for commands and limits.
