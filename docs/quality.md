# Desktop quality verification

## Automated checks

`pnpm test` exercises renderer layout and time handling. `pnpm test:ui` covers browser interactions, mocked IPC failures, normalized read-only calendars, and axe WCAG A/AA audits in both themes. These checks do not establish native platform input behavior or full screen-reader usability.

`pnpm rust test --manifest-path src-tauri/Cargo.toml` covers real SQLite/reopening/undo/migrations, the shared local/test-adapter contract, capability and identity isolation, and unavailable credential-store behavior. Test adapters and injected credential stores are compiled only under `cfg(test)`.

The OS-store check is opt-in and uses a disposable account, deleting the entry after reading it:

```sh
pnpm rust test --manifest-path src-tauri/Cargo.toml real_os_store_roundtrip -- --ignored
```

Build and run the embedded native suite on Linux, Windows, or macOS:

```sh
pnpm desktop:test:build
pnpm test:native
```

The suite starts the debug executable with `AIKA_TEST_DATA_DIR` set to an isolated temporary directory, connects WebdriverIO directly to the embedded loopback server, and uses real IPC and SQLite without mocks. It restarts the application to distinguish durable data/preferences from session undo. `AIKA_TEST_BINARY` can override the executable path; `TAURI_WEBDRIVER_PORT` can override port 4445. Captures go to ignored `test-results/native/`. Linux CI runs under Xvfb; Windows/macOS jobs remain disabled. The scripts support those platforms but require execution there before claiming verification.

`native-test` is an explicit, optional Cargo feature, absent from ordinary builds. A compile-time guard rejects enabling it in release mode. There is no production automation permission or frontend mocking bridge. The embedded driver implements some actions by JavaScript dispatch inside the actual webview; this establishes webview/Rust integration but does not prove trusted OS pointer input, native drag-and-drop, or assistive-technology behavior.

## Manual platform checks before alpha acceptance

Record OS/webview version, input method, display scale, exact outcome, and screenshot paths. Execute on Windows 11 x64, macOS 14+ Intel and Apple Silicon, Ubuntu 24.04+, and Arch Linux:

- Create/edit/delete/undo timed and all-day events, restart after saves/deletes, and verify last view/sidebar/appearance. Use an isolated OS account or disposable app-data directory.
- Drag across month cells and a time column; move events between dates and to precise times; resize ends; cancel with Escape. Confirm failed writes/DST gaps retain the committed event and retry works.
- Complete essential actions using only the keyboard. Verify opening/closing focus, roving month focus, date/time fields, overflow, and Settings focus containment. Check announcements, event ranges, disabled/read-only actions, and labels with Narrator, VoiceOver, or Orca.
- Check both themes, system appearance changes, reduced motion, regional dates/week start, device-zone changes, 900×600 sizing and narrower tiling, 100%/200% display scale, dense/overlapping/multi-day events, and long titles.
- Verify secure store read/write/delete and locked/unavailable-store errors without plaintext fallback. Local calendar operations must remain usable.
- Build an ordinary release and verify it has no test-driver listener or test fixtures. Install/update/packaging checks belong to phase 5.

Do not mark an unexecuted platform or screen-reader check as passed. See the phase-4 handoff for evidence from this host and remaining checks.
