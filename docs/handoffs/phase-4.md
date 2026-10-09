# Phase 4 — Provider proof and quality

Status: **implemented, with local Linux verification; other-platform/manual checks remain pending.**

## Source and scope

Base revision: `634ce36` in the inherited parent repository. This handoff ships with the phase-4 implementation commit. The user's follow-up requests relocating Git into `project-cal`, rewriting history to put app files at the repository root, renaming the branch to `main`, and force-pushing origin. Historical source revisions in earlier handoffs refer to the pre-rewrite history; the repository relocation record provides their mapping. The pre-existing `.gitignore` edit is preserved separately; sibling folders are untouched.

Read the referenced planning task, `docs/plan.md`, `docs/architecture.md`, and earlier handoffs. Production remains one local calendar with no connected accounts, remote sync, recurrence, reminders, search, export, or backup.

## Delivered

- Backend provider registry and normalized provider contract behind the unchanged calendar IPC. Only SQLite LocalProvider registers in production. Preferences and session undo retain their original storage/behavior.
- Atomic calendar discovery, duplicate/provider identity rejection, bounded ranges, capability enforcement before writes, and snapshot isolation. SQLite reads/deletes/existence checks scope the local calendar; foreign calendar IDs cannot be overwritten by an ID collision.
- A second independent in-memory provider confined to Rust tests, with writable/read-only calendars. Contract checks cover local/test CRUD, normalization, explicit update intent, isolation, read-only operations, unsupported features, duplicate registration, and a deliberately leaking adapter.
- Backend-only credential service backed by macOS Keychain, Windows Credential Manager, and Linux persistent Secret Service. Application/provider/account namespacing, redacted store errors, and idempotent deletion; no plaintext/session fallback or credential IPC. Simulated failure checks preserve local usability. An opt-in disposable real OS-store test is separate.
- Calendar metadata/writability now controls drafts, names, inspection, keyboard creation, mutation controls, and drag/resize affordances in the existing UI, without provider-name branches.
- Corrected ARIA structure for month all-day spans, labelled calendar regions, and grouped view controls. Automated WCAG audits cover all views/editor/Settings, both themes, and reduced motion.
- Cross-platform embedded WebdriverIO runner using actual native webview, IPC, and SQLite. Explicit debug-only `native-test` feature; release builds reject that feature. Test data is isolated and removed. Linux CI runs the native suite under Xvfb; Windows/macOS entries remain commented out.
- Linux startup defaults to WebKitGTK's DMA-BUF rendering compatibility path, respecting an explicit environment override. Native verification discovered that the default accelerated path stalled editor fade/captures on this host; the compatibility default restores rendering. See architecture for the evidence, upstream reference, and tradeoff.

## Verification

Executed October 9, 2026 on Linux/Wayland with Node 26.8.2, pnpm 12.10.1, Rust 1.99.0:

```sh
pnpm test
pnpm rust test --manifest-path src-tauri/Cargo.toml --offline
pnpm rust test --manifest-path src-tauri/Cargo.toml real_os_store_roundtrip -- --ignored
CAL_CHROMIUM_PATH=/usr/bin/chromium pnpm test:ui
pnpm desktop:test:build
pnpm test:native
pnpm desktop:build --debug --no-bundle
pnpm format:check
pnpm rust fmt --manifest-path src-tauri/Cargo.toml --check
pnpm rust clippy --manifest-path src-tauri/Cargo.toml --all-targets --offline -- -D warnings
pnpm rust clippy --manifest-path src-tauri/Cargo.toml --all-targets --features native-test --offline -- -D warnings
pnpm types:generate
git diff --check
```

Results: **13 renderer unit tests, 12 Rust tests, one separately executed real Secret Service roundtrip, and 14 browser scenarios passed.** The OS-store test creates a disposable account and deletes its entry; the default suite ignores it on hosts without an unlocked store. Rust tests use actual SQLite for reopening, migration failure, undo, and foreign-calendar identity protection.

The embedded Linux suite verifies timed/all-day creation and deletion across application restarts, title updates, session-only undo, month/week/day rendering, keyboard opening/dismissal, and durable appearance. It uses actual Rust commands and a fresh SQLite database, without command mocks. WebdriverIO's embedded driver synthesizes some input in the webview; these results do not establish trusted OS pointer capture or native HTML drag-and-drop. Driver click/select behavior required explicit DOM event ordering inside the native webview; the runner documents those compatibility adjustments.

Native captures: ignored `test-results/native/month-light.png`, `week-light.png`, and `day-dark.png`. Browser review captures remain reproducible in `test-results/`. The native month/day images were visually inspected. Production frontend and ordinary native debug builds pass; normal dependency inspection excludes the automation driver. Generated contracts have no drift. No user calendar database or real connected-account credentials were accessed.

## Limits and next phase

Windows/macOS execution, hosted CI, Node 24 execution, trusted native drag/create/move/resize, high-DPI checks, and Narrator/VoiceOver/Orca checks remain unexecuted. The full cross-platform quality gate is therefore **pending**, not claimed complete. No known failing completed local checks. `docs/quality.md` provides the commands and manual platform matrix.

Read the plan, architecture, this handoff, and quality matrix before phase 5. Prepare private installers/checksums/notes and installation/update/restart verification while completing the remaining platform checks. Keep connected accounts and sync deferred. Do not ship the `native-test` feature. The final alpha acceptance remains a user gate.
