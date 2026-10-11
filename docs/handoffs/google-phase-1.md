# Google phase 1 — Shared repository and provider foundation

Status: **delivered. Google phases 2–6 remain planned.**

## Source and steps

Base revision: `9b464b615dac826e5c141a277406b4e02088e608` (`feat: support common recurring events in UI and CLI`). The working tree already contained the phase 0 README/plan/architecture/specification/handoff changes and an unrelated `.gitignore` edit; these are preserved. Implementation revision: resolve with `git log -1 -- docs/handoffs/google-phase-1.md`. The user subsequently requested committing the phase 0 documentation and phase 1 implementation together; the unrelated `.gitignore` edit is excluded. No account connection, cloud configuration, or release was performed.

1. Read the [phase 0 handoff](google-phase-0.md), [integration specification](../integrations/google-calendar.md), [ADRs G01–G07](../architecture.md#connected-calendar-decisions-google-integration), and [quality commands](../quality.md); inspected the working tree and local-only routes before editing.
2. Moved SQLite persistence and the existing regression suites from `LocalProvider` to `Repository`. The registry owns the repository and stateless provider implementations; `CalendarService` remains the common desktop/CLI facade. Collection/calendar management, series lookup, preferences/default/visibility, checked event mutations, and session undo no longer dispatch through a local storage adapter.
3. Added transactional schema v4, native account/capability DTOs, and generated TypeScript through the Rust exporter. Kept account/persistence/network responsibilities outside the pure domain/WASM layer; no new dependency or renderer SQL/provider-registration command was added.
4. Added a second adapter exclusively under `cfg(test)` and disposable repository/CLI fixtures. Proved multiple accounts, colliding remote IDs, operation-specific permissions, cached reads, outbox atomicity/reopening, and shared session undo. Wired capability maps into renderer controls and the explicit browser harness, with a mocked IPC restriction test.
5. Ran automated/native checks, corrected failures described below, and updated the integration phase table, README, main plan, architecture implementation notes, and quality documentation. This handoff records the delivered boundary and phase 2 starting point.

## Delivered behavior and compatibility

- **One repository for all accounts:** production registers only Local. Missing adapters or account authentication do not prevent cached reads or independent local operations. Supported remote ordinary writes through a registered test adapter use the same checked service/repository/outbox; the CLI starts no worker. The process CLI fixture verifies cached reads and rejects writes when its adapter is absent.
- **Stable identities:** accounts are unique by provider type and remote subject; reconnection updates display/state without resetting selections. Imported calendar UUIDs map by account/remote ID, and event UUIDs map by account/calendar/remote ID. Wrong-account ingestion and database mapping insertions fail. Identical remote IDs in two accounts remain separate calendars/events.
- **Persistence:** schema v4 preserves v1–v3 IDs (including `local-default`), normalized payloads, recurrence masters/exceptions/cancellations, calendar presentation, default/visibility, and preferences. Migration runs in the existing immediate transaction, rejects future versions, and rolls back on failure. Remote shadows/revisions and effective events remain separate. Tables also reserve cursor/generation, coverage, conflict, and lease storage for later phases; these have no worker implementation yet.
- **Capabilities:** generated `Capabilities`, `ProviderDescriptor`, `AccountSummary`, and `ConnectionState` are native contracts. The native collection adds accounts plus calendar/event capability maps; recurrence occurrence maps also identify their master. Adapter support intersects persisted calendar/event permissions. Service checks and mutation transactions enforce create/edit/delete/recurrence restrictions; read-only meetings remain inspectable. Edit-only calendars can edit existing events but cannot become creation defaults. Unsupported remote recurrence writes and remote/cross-provider calendar moves are rejected; existing local recurrence/moves continue to work.
- **Durable intent and undo foundation:** ordinary remote effective writes/deletions and outbox before/after intent commit atomically, retaining operation UUID, expected revision, predecessor dependency, and deletion intent. Pending work survives reopening while history remains session-scoped. Undo cancels a matching unsent tail in the same checked transaction. An outbox insertion failure leaves effective data and undo history intact. In-flight/acknowledged compensation behavior still needs the phase 4 worker/recovery implementation and evidence.
- **Generic calendar management:** visibility is separate from sync selection. Imported metadata changes remain local presentation. Imported removal excludes its preserved identity row and clears effective events without a remote deletion; pending work blocks removal even with force. Default fallback chooses the oldest selected active writable creation-capable Local calendar, then an imported calendar, then none.

The domain event/calendar DTOs and WASM recurrence implementation remain unchanged. The enriched collection is a backend DTO; the browser prototype supplies equivalent capability maps and remains an explicit in-memory harness. Generated files were exported, never hand-edited.

## Verification

Executed October 11, 2026 on Arch Linux x64 (`Linux 7.2.5-3-omarchy`), Node 26.8.2, pnpm 12.10.1, Rust 1.99.0. Fixtures use temporary SQLite databases, fictional account subjects, and disposable events; no Google account or credentials were used.

```sh
pnpm rust test --manifest-path src-tauri/Cargo.toml --workspace
pnpm rust test --manifest-path src-tauri/Cargo.toml -p aika-core -p aika-cli
pnpm rust clippy --manifest-path src-tauri/Cargo.toml --workspace --all-targets -- -D warnings
pnpm rust fmt --manifest-path src-tauri/Cargo.toml --all --check
pnpm test
pnpm build
sha256sum src/generated/*.ts > /tmp/aika-google-phase-1-types.sha256
pnpm types:generate
sha256sum -c /tmp/aika-google-phase-1-types.sha256
pnpm desktop:test:build
DISPLAY=:0 WAYLAND_DISPLAY=wayland-1 pnpm test:native
CAL_CHROMIUM_PATH=/usr/bin/chromium pnpm test:ui --workers=1
pnpm format:check
git diff --check
git status --short --untracked-files=all
```

Passed: **58 Rust/CLI tests** (15 CLI, 32 core, 11 domain), with one opt-in real OS-store check skipped; the final core/CLI rerun passed 47 tests after the default-eligibility transaction check. **26 frontend/WASM unit tests** and **38 browser tests** passed, including both-theme accessibility/recurrence and the writable-calendar event-capability restriction scenario. Workspace Clippy with warnings denied, Rust fmt, frontend/native debug builds, Prettier, whitespace checks, and all **40 local file/anchor links across seven documents** passed. Exporting again reproduced all **24 generated contract files** byte-for-byte using the recorded SHA-256 commands.

The final **Linux native webview suite passed** against a disposable SQLite v4 database: timed/all-day CRUD, recurrence/scopes/preview/undo/restart, CLI aggregate conflicts, views, keyboard dismissal, appearance, and multiple calendars. Native IPC and persistence evidence is distinct from mocked browser and test-adapter evidence. Native screenshots are written to ignored `test-results/native/month-light.png`, `week-light.png`, and `day-dark.png`; browser review captures/traces remain under ignored `test-results/`.

The phase 1 automated exit gate passes: local CRUD/moves/recurrence/session undo, CLI compatibility, v1–v3 preservation/future-schema rejection/rollback, and generic adapter/account isolation/capability/outbox tests have passing evidence. Google connectivity and the later synchronization/recovery gates remain unimplemented; native/platform/live evidence is limited as stated below. Packaging/release and real OS-store/live-Google checks were not run for this phase.

Failures corrected during implementation: the first compile exposed a moved Rust string and the CLI's old `LocalProvider::validate_details` import; both were corrected. The v2 downgrade fixture initially retained the new mapping index/trigger while dropping columns; it now constructs a real v2 schema before migration. The new CLI fixture initially used unsupported `events show` and list `--calendar` arguments; it now uses existing `get` and collection-list commands. The first browser invocation could not start its local server inside the network sandbox; outside that restriction it then lacked Playwright's bundled Chromium. The documented system Chromium override resolves that environment issue. Intermediate browser runs were invalidated by development-server reloads from renderer edits and regenerated WASM during concurrent builds. A four-worker run also encountered timeouts and was interrupted; the final browser run uses one worker and stable files. A concurrent native rerun failed when Playwright cleared its screenshot directory under the shared `test-results` root; the final native run is performed after browser completion. No automatic approval review rejection occurred.

## Limits and phase 2 starting instructions

Phase 1 implements the shared backend foundation and test proof. Production remains Local-only. There is no Google HTTP/authentication adapter, provider popup, connection session, discovery/import checklist, polling/sync worker, staged cursor publication, lease acquisition/recovery, retry/acknowledgement handling, conflict resolution, unsupported recurrence coverage, or live Google evidence. Storage placeholders do not prove those later guarantees. The opt-in real OS-store test and Windows/macOS/manual platform/assistive-technology evidence remain pending; earlier alpha gates are unchanged.

For phase 2:

1. Read this handoff, the specification, ADRs, and quality commands; preserve the working tree and reproduce relevant checks. Continue through `CalendarService`/`ProviderRegistry`/`Repository`, not provider-owned storage.
2. Extend the backend adapter interface with account connection/discovery operations and register Google once by provider type. Define phase 2 connection-session/discovery/selection DTOs and IPC in Rust, then export TypeScript. Provider/account listing already exists on the service; no renderer registration, arbitrary URL, token retrieval, or SQL API should be added.
3. Implement the documented backend system-browser OAuth/PKCE/loopback lifecycle and persistent credential-store boundary. Configure only the phase 2 public desktop-client settings; use disposable accounts and keep credential material out of SQLite/logs/handoffs.
4. Build the keyboard-accessible provider popup and reuse Local's existing form. Discover every calendar-list page and expose eligible/disabled reasons; confirm the selected calendars with the existing account/scoped mapping model. Preserve UUID mappings, local metadata/visibility, and explicit exclusions on reconnection/rediscovery. Add the actual selection-revision API rather than resetting selection through `import_calendar`.
5. Keep imported calendars read-only in production until incoming sync (phase 3) and outgoing sync (phase 4) exit gates pass. `cache_event` is a normalized ingestion primitive, not a paginated sync implementation: phase 3 must stage generations and publish shadows/cursors atomically while preserving pending intent. Do not start a worker or claim synchronization during phase 2.
6. Record cancellation/denial/timeout, duplicate account, pagination, secure-store failures, selection/exclusion/visibility, and Local isolation evidence in `google-phase-2.md`. Advance only phase 2 when its actual exit gate passes.
