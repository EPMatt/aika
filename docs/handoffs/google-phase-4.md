# Google phase 4 — Ordinary-event two-way sync

Status: **implementation delivered with automated verification; disposable live Google and installed-platform exit evidence remains pending. Google phases 5–6 remain planned.**

## Source and scope

Base revision: `3cce4516bbf48d6936650138dd3e020593df37cd`. Resolve the implementation revision after commit with `git log -1 -- docs/handoffs/google-phase-4.md`. The working tree was initially clean. A concurrent edit to credential-store recovery guidance appeared in `src-tauri/core/src/credentials.rs`, a Linux troubleshooting document, and its README link during this work; preserve them outside this phase's commit.

Read the [integration specification](../integrations/google-calendar.md), [architecture decisions G01–G07](../architecture.md#connected-calendar-decisions-google-integration), [phase 3 handoff](google-phase-3.md), and the subsequent [build](google-build-configuration.md)/[dotenv](google-dotenv-configuration.md) handoffs. Google's [conditional revisions](https://developers.google.com/workspace/calendar/api/guides/version-resources), [creation IDs](https://developers.google.com/workspace/calendar/api/v3/reference/events/insert), and [error guidance](https://developers.google.com/workspace/calendar/api/guides/errors) were consulted during implementation. No live account authorization, real Google event write, cloud configuration, or release/publication was performed.

## Delivered behavior

- Writer/owner calendars support ordinary timed/all-day create/edit/delete, persisted immediately with transactional outbox intent. Checked discovery/selection and regular reconciliation combine permissions with adapter capabilities. Reader calendars, recurring events, meetings/attendees, locked/conference resources, externally organized events, and special Google event types remain read-only. Backend/repository checks and a remote preflight enforce these restrictions independently of the renderer.
- `aika-core::outgoing` shares the existing account lease/epoch and repository. The desktop reconciles incoming data, then drains outbound operations, releasing service/SQLite locks for HTTP. CLI commands persist the same intent and start no worker. Desktop mutations debounce a request to the existing account queue by 400 ms; startup/focus/manual/polling remain available. Processing is bounded to 1,000 outbound operations per cycle.
- Each event's operations depend on unacknowledged predecessors. A successor uses its predecessor's acknowledged revision; a later independent edit uses the latest incoming shadow revision. Late acknowledgements update the shadow while preserving newer effective edits or deletion intent.
- Creation IDs are persisted UUID-derived lowercase hexadecimal strings before HTTP. The same ID survives failed responses, lease takeover, and restart. Google private `aikaOperation` markers identify acknowledged creates/patches after uncertain responses. Remote preflight plus `If-Match` fences updates/deletes, including a change racing the preflight. Patches contain only changed title/location/notes/time plus the operation marker. Untouched private properties and other provider fields survive. Time-kind conversion explicitly clears the previous date/dateTime representation.
- Transient/network/rate-limit failures persist attempts and exponential retry deadlines with jitter. Automatic retries run on subsequent desktop cycles after the deadline (60-second polling may delay them). Authentication pauses that account. Permission/validation/unsupported failures retain blocked work and an actionable error; **Retry pending changes** explicitly reschedules eligible pending/blocked rows without resetting their operation identities or attempt history. Other events continue.
- Conflicts retain both versions or deletion states and pause only the event. Incoming full/incremental replay updates the remote side without dropping local intent. The sidebar compares versions and offers **Keep local** / **Keep Google**. Resolution compares the reviewed DTO, including the latest effective local version, inside an immediate transaction. Keep local rebases queued intent and checks the remote revision again before sending; Keep Google adopts the reviewed remote state. New changes after review require another review.
- Session undo deletes only never-attempted matching pending operations. In-flight, previously attempted, and acknowledged changes queue compensation with dependencies. A timed-out request may have succeeded; undo recovers it before compensating. Restoring a remotely deleted event uses creation permissions and a fresh persisted identity because Google retains tombstones. The mapping retains its last live shadow across cancellation; raw tombstones remain in `incoming_items`. Recreated resources restore supported provider-only properties such as reminders, visibility, attachments, and private/shared properties, with a new operation marker. Undo remains session-scoped and conservative about external changes.
- Permission loss repairs an ineligible creation default, preferring the oldest active writable local calendar, then an imported calendar, then none. Revising a checklist retains an eligible imported default. Disconnect refuses pending work/conflicts, fences workers and disables writes transactionally before credential deletion, preserves cached reading, and repairs the default. If OS credential deletion fails, the account remains disconnected; retry disconnect to finish store cleanup. Exclusion/removal also preserves pending-work protection.

## Interfaces and compatibility

Schema stays **v5**; the existing outbox/conflict/shadow/lease tables are sufficient. No migration, dependency, scope, or pure-domain/WASM contract change is introduced. Existing v1–v5 migration tests remain applicable. `attempts`, `retry_at`, `acknowledgement`, `depends_on`, and outbox states now have delivery semantics. Acknowledged rows remain stored as dependency/recovery history; compaction is deferred.

Backend adapters gain `Provider::outgoing` and `event_capabilities`. `OutgoingOperation`/`OutgoingResult` remain native-only. `SyncConflict` is defined in Rust and exported through the existing TypeScript generator. Native IPC adds `sync_conflicts`, checked `resolve_sync_conflict`, and `retry_outgoing`; no raw payload, HTTP, secret access, SQL, or provider registration is exposed. Existing snapshots already carry pending/conflict counts and errors. Browser preview remains in memory and performs no Google work.

## Verification

Host: October 19, 2026, Arch Linux x64, Linux `7.2.5-3-omarchy`, Node `26.8.2`, pnpm `12.10.1`, Rust/Cargo `1.99.0`. Fixtures use fictional subjects/resources, disposable SQLite databases, and test-only transports. Browser tests mock IPC; the native regression suite uses the real webview/IPC/SQLite with disposable application data.

Commands executed (socket/browser/native checks use access outside the network sandbox):

```sh
pnpm rust check --manifest-path src-tauri/Cargo.toml --workspace
CARGO_BUILD_JOBS=1 pnpm rust test --manifest-path src-tauri/Cargo.toml --workspace
CARGO_BUILD_JOBS=1 pnpm rust test --manifest-path src-tauri/Cargo.toml -p aika-core outgoing
CARGO_BUILD_JOBS=1 pnpm rust clippy --manifest-path src-tauri/Cargo.toml --workspace --all-targets -- -D warnings
pnpm rust fmt --manifest-path src-tauri/Cargo.toml --all --check
pnpm test
pnpm build
CAL_CHROMIUM_PATH=/usr/bin/chromium pnpm test:ui tests/google-sync.spec.ts tests/google-connection.spec.ts --workers=1
CAL_CHROMIUM_PATH=/usr/bin/chromium pnpm test:ui tests/google-sync.spec.ts --workers=1
CAL_CHROMIUM_PATH=/usr/bin/chromium pnpm test:ui --workers=1
CAL_CHROMIUM_PATH=/usr/bin/chromium pnpm test:ui tests/accessibility.spec.ts --workers=1
sha256sum src/generated/*.ts > /tmp/aika-google-phase-4-types.sha256
pnpm types:generate
sha256sum -c /tmp/aika-google-phase-4-types.sha256
pnpm desktop:test:build
pnpm test:native
pnpm format:check
python3 /tmp/aika-google-phase-4-links.py
git diff --check
```

The final outgoing subset passed **22 tests**: recovery of uncertain creates/updates, marker validation, conditional PATCH/DELETE and racing 412, supported-field patches, provider-property recreation, meeting/recurrence gates, offline shared/CLI intent, dependencies, permission/default/disconnect handling, cursor rebuild overlays, checked conflicts, lease takeover/restart, and undo before/during/after send including a replayed cancellation. The real CLI subprocess fixture creates, updates, and deletes a fictional Google event offline and verifies durable ordered pending rows with zero worker leases/attempts.

Frontend/WASM: **26 unit tests passed**, production build passed. All **31 generated TypeScript files reproduced byte-for-byte**. Prettier, Rust formatting, whitespace checks, and **49 local file/anchor links across five documents** passed. The link checker is a disposable `/tmp` helper, not a repository tool.

Browser: the full **49-case invocation passed 48 cases** and timed out in the light views/editor/settings/shortcuts accessibility test while compilation was active. The subsequent isolated accessibility invocation passed **all four cases**, including that case. All 49 scenarios therefore have passing evidence across the complete run and rerun; the complete invocation itself exited with a failure. The Google-focused initial invocation passed five cases but the new offline-edit test looked for the wrong save-button label; correcting the fixture to **Save changes** yielded **two passing Google-sync cases**. Both Google cases also passed in the complete run. The new case verifies editable ordinary events, local pending status, debounced sync, version comparison, both conflict choices, and an Axe audit. Existing reader-cache/coverage and all connection scenarios passed.

Native: the debug/no-bundle build and real webview/IPC regression runner passed on a disposable database. It covers Local timed/all-day CRUD, recurrence/scopes/preview/undo/restart, aggregate CLI conflicts, views, keyboard dismissal, appearance, and multiple calendars. Ignored screenshots live under `test-results/native/`. The native run preceded the final backend-only tombstone/property-recreation refinements; workspace/outgoing tests and static checks were rerun afterward. No renderer/IPC/schema changes followed the native run. This is no claim of native live Google delivery.

Verification issues: the first sandboxed workspace run could not bind the OAuth fixtures' loopback listeners (three failures); running with local socket access passed. A later default-parallel workspace build had two linker processes terminated with **SIGKILL**; the serial `CARGO_BUILD_JOBS=1` rerun passed. Initial compilation required a Debug derive for a fixture assertion, and Clippy required named tuple aliases and matching on an optional mapping; these were corrected. The first real CLI fixture used a nonexistent `--calendar` flag; it now uses the existing `calendars set-default` flow. No automatic approval-review rejection occurred.

Final serial workspace invocation: **113 tests passed** (16 CLI, 86 core, 11 domain), two opt-in core tests ignored, and all doc-test targets passed. This includes the concurrent credential-guidance test outside this phase commit; the phase commit alone contains 112 passing workspace tests. Final workspace Clippy with warnings denied and Rust fmt passed after the last backend changes.

## Remaining gates and phase 5 starting instructions

Live-provider and installed-platform gates from phases 2–3 remain pending, together with phase 4's real write/recovery gate. Automated mocks do not establish real Google propagation, conditional-write/notification behavior, actual permission revocation, expired credentials, or cross-platform keyring/browser behavior. No Windows/macOS, installed upgrade, packaging/release, real-store roundtrip, or manual assistive-technology checks are claimed here.

1. Read the specification, ADRs, this handoff, previous evidence, and current working tree. Preserve concurrent changes. Reproduce the relevant checks; record disposable live ordinary-event CRUD, offline/restart recovery, real permission changes, lost responses, and conflict/undo evidence separately before claiming those exit gates.
2. Extend the common outbox/lease/checked acknowledgement path in `outgoing.rs`; do not add another database, Google-specific view logic, a CLI worker, or a background service. Preserve ordinary-event behavior and partial-patch/private-property preservation.
3. Prove recurrence equivalence before enabling capabilities. Keep unsupported rules, meetings, and special types gated. The existing incoming converter accepts only conservative common patterns; source-zone/DST and richer recurrence still use projections. Test all-day exclusivity, UTC/source zones, DST gaps/folds, counts/end dates, and exception identity against original start.
4. Add compatible occurrence/entire/following-series mutations. Persist trim/create workflows, stable identities, acknowledgements, partial progress, retry/restart recovery, and compensations before sending multi-request splits. One local transaction cannot imply a remote transaction.
5. Extend conflicts, dependencies, and undo to moved/cancelled exceptions, resets, and interrupted splits. Retain the latest local intent through incoming 410/full-resync and late HTTP responses. Current ordinary conflict DTOs carry `EventSeries`, but ordinary delivery writes no exceptions; broaden it deliberately with equivalence/recovery tests.
6. Add meaningful adapter/common/CLI/browser/native fixtures, then record actual commands/results and unexecuted gates in `google-phase-5.md`. Phase 6 remains recovery UX/platform/release hardening. Outbox history compaction, very large calendar performance, richer blocked-work repair/discard UX, and retry countdowns can be evaluated there without weakening durable intent.
