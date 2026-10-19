# Google phase 3 — Incoming sync and offline reading

Status: **implementation delivered with automated evidence; live Google and installed-platform exit evidence remains pending. Google phases 4–6 remain planned.**

## Source and scope

Base revision: `e0bea151633bb36def92993213091965360cc415`. This phase builds on the existing uncommitted phase 2 implementation. Resolve a future implementation revision with `git log -1 -- docs/handoffs/google-phase-3.md` after committing. Preserve the unrelated `.gitignore` edit and the concurrent keyboard-shortcut UI changes; neither is part of this integration increment. No Google cloud/client configuration, real account connection, publishing, packaging release, or remote event mutation was performed.

Read the [phase 2 handoff](google-phase-2.md), [integration specification](../integrations/google-calendar.md), and [ADRs G01–G07](../architecture.md#connected-calendar-decisions-google-integration). Google's current [events list](https://developers.google.com/workspace/calendar/api/v3/reference/events/list), [incremental sync](https://developers.google.com/workspace/calendar/api/guides/sync), and [recurring-event identity](https://developers.google.com/workspace/calendar/api/guides/recurringevents) references were checked during implementation. Previous phase 2 evidence remains historical; it does not establish phase 3's live exit gate.

## Delivered behavior

- The desktop requests incoming sync at startup, navigation, focus/resume, manual refresh, and every 60 seconds while open. A native queue runs one worker per account and retains the latest coalesced request. Accounts can run independently. CLI construction starts no worker or network operation.
- The shared `aika-core::sync` coordinator receives provider pages through `SyncAccess`. Google converts protocol resources through `google_events.rs`; the adapter owns protocol conversion, while the shared service/repository owns staging, mappings, publication, leases, selection, and status. Transport, browser, and credential calls remain outside the service/SQLite lock; calendar-discovery checkpoints renew/check leases between pages.
- Full calendar-list reconciliation includes hidden entries, adds newly eligible calendars, respects explicit exclusions, and marks missing/inaccessible entries inactive without deleting cache or pending intent. Automatic reconciliation starts only after checklist confirmation, including an explicitly empty checklist. Migrated v4 accounts with no calendar rows must reconfirm selection before automatic import. Name/color/description changes follow prior provider values when untouched locally; the first v4-import refresh preserves legacy presentation. Visibility stays independent. Checked customization retains an unchanged provider color even when it is outside the local palette.
- The raw feed uses `singleEvents=false`, deleted/hidden invitations included, stable parameters, and no date bounds. It stores untouched provider payloads and opaque revisions with scoped UUID mappings. Date-bounded `singleEvents=true` projections are separate from the incremental cursor feed. Repeated/empty page tokens, duplicate event identities, missing final cursors, and incomplete generations fail without publication.
- Each generation stages both feeds. Only completion publishes effective events, remote shadows, final cursor, and coverage in one immediate transaction. HTTP 410 restarts staging once as a full feed; useful published cache and pending intent remain intact even if rebuilding fails. Sparse cancellations and full-feed omissions become tombstones. Outbox/conflict overlays are retained; imported events stay read-only.
- Renewable 90-second account leases serialize workers across database connections. Owner, expiry, connected state, and account/selection epochs fence page staging/publication. Reconnection, disconnect, selection, newly imported calendars, and availability changes invalidate stale work. An expired owner cannot release a newer lease or publish late data. New owners clean abandoned staging without changing published data.
- Conservative common recurrence supports validated simple all-day rules and UTC timed rules: daily/weekly/monthly-by-date/yearly, intervals/counts, Monday week start, plain weekly BYDAY, and all-day date UNTIL. Moved exceptions use original-start identity and cancellations need no start/end/title. Source-zone/DST patterns, timed UNTIL, ordinal BYDAY, EXDATE/RDATE, and other constructs retain raw shadows and use Google projections. Missing/redacted ordinary event fields and long titles bypass local creation validation. Empty titles display as “Untitled event” without altering stored data.
- Unsupported projection coverage includes 12 calendar months past and 24 ahead, extended to cover the opened range with a two-day instant margin for all-day/source-zone boundaries. Any raw feed change conservatively invalidates older projection coverage; current projections/coverage publish atomically. Bounded refresh removes overlapping obsolete projections, including instances omitted after moves/cancellations. Missing offline coverage is surfaced instead of implying a complete empty range.
- Collection snapshots expose generated account/calendar sync status with syncing, last success, redacted failures, pending/conflict counts, availability, and range completeness. The sidebar offers manual refresh and recovery messages. Cached reads and Local behavior are independent of remote failures.

## Interfaces and compatibility

Schema **v5** migrates v4 transactionally, preserving v1–v4 data/settings/identity. It adds account confirmation/epochs/status, prior remote calendar metadata, normalized incoming item metadata, and projection mappings. Existing v4 staging/cursor/coverage/lease tables now have implemented semantics. Selection/exclusion clears cursor/coverage so re-including a calendar performs a complete import. Existing pending intent remains durable; no outgoing worker consumes it yet.

`Provider::incoming_page`, `incremental_sync`, and `discover_for_sync` are backend-only extension points. `request_sync` is the only new native command; it accepts a validated event range, never credentials, arbitrary URLs, registration, or remote payloads. `calendar-sync-changed` carries no personal data and requests another cached snapshot. Rust generates `AccountSyncStatus`, `CalendarSyncStatus`, and the enriched `CalendarCollectionSnapshot`; pure event/domain/WASM contracts remain unchanged. The browser preview supplies empty sync status and performs no remote work.

Safety/resource bounds: 20-second HTTP timeout, 4 MiB per HTTP response, 1,000 pages per feed, 100,000 normalized items per stream, and 64 MiB per staged generation. Those limits produce a redacted failure and preserve the earlier cache. This first coordinator stores normalized staging as JSON; very large calendars and distant navigation can require substantial pagination. Every successful cycle currently refreshes instance projections; optimizing unchanged projections is future performance work.

## Verification

Host: October 14, 2026, Arch Linux x64 (`Linux 7.2.5-3-omarchy`), Node 26.8.2, pnpm 12.10.1, Rust/Cargo 1.99.0, GTK 3.24.52, WebKitGTK 2.52.6. Fixtures use fictional subjects, event data, disposable SQLite databases, and test-only transport/store implementations. The browser tests mock native IPC; the native regression runner uses real IPC/SQLite with no configured Google account.

Executed commands (socket/browser/native checks run outside the network sandbox):

```sh
pnpm rust check --manifest-path src-tauri/Cargo.toml --workspace
pnpm rust test --manifest-path src-tauri/Cargo.toml -p aika-core sync::
pnpm rust test --manifest-path src-tauri/Cargo.toml -p aika-core metadata_tracks_remote_changes_while_preserving_local_overrides
pnpm rust test --manifest-path src-tauri/Cargo.toml -p aika-core
pnpm rust test --manifest-path src-tauri/Cargo.toml --workspace
pnpm rust clippy --manifest-path src-tauri/Cargo.toml --workspace --all-targets -- -D warnings
pnpm rust fmt --manifest-path src-tauri/Cargo.toml --all --check
pnpm test
pnpm build
CAL_CHROMIUM_PATH=/usr/bin/chromium pnpm test:ui tests/google-sync.spec.ts tests/google-connection.spec.ts --workers=1
CAL_CHROMIUM_PATH=/usr/bin/chromium pnpm test:ui --workers=1
sha256sum src/generated/*.ts > /tmp/aika-google-phase-3-types.sha256
pnpm types:generate
sha256sum -c /tmp/aika-google-phase-3-types.sha256
pnpm desktop:test:build
DISPLAY=:0 WAYLAND_DISPLAY=wayland-1 pnpm test:native
pnpm format:check
python3 /tmp/aika-google-phase-3-links.py
git diff --check
```

Passed: **88 Rust/CLI/domain tests** (15 CLI, 62 core, 11 domain; the opt-in real-store test remains skipped), all workspace doc-test targets, workspace Clippy with warnings denied, Rust fmt, **26 frontend/WASM unit tests**, and production/frontend builds. The focused shared sync subset passed **14 tests**; the final full-workspace run includes **15 sync/migration fixtures**, plus Google conversion/protocol/discovery checkpoint fixtures. **All 30 generated contract files reproduced byte-for-byte** using the SHA-256 commands above. Prettier, whitespace checks, and **55 local file/anchor links across nine documents** passed.

Browser evidence: the complete 48-test run passed **47 tests**, with one light accessibility timeout during competing compilation. The final focused suite passed **all nine tests**: four light/dark accessibility regressions, four Google connection tests, and the final sync test including cached read-only events, untitled imported events, missing coverage, offline failures, refresh/recovery, and sync-status accessibility. All 48 scenarios therefore have passing evidence across the complete run and focused rerun; this is not a claim that the complete invocation exited successfully.

The Linux native debug build and native webview regression suite passed using a disposable schema v5 database and real IPC/SQLite/CLI. It verifies the unconfigured Google descriptor plus Local timed/all-day CRUD, recurrence/scopes/preview/undo/restart, CLI aggregate conflicts, views, keyboard dismissal, appearance, and multiple calendars. Native screenshot artifacts are ignored `test-results/native/provider-popup.png`, `month-light.png`, `week-light.png`, and `day-dark.png`. It does not verify native live Google event ingestion. After that native run, final review corrected metadata customization to preserve an unchanged provider color outside the local palette. The targeted metadata fixture, workspace Clippy, and a final complete core rerun (**62 passed; one real-store test ignored; doc tests passed**) all passed after this correction. No renderer, IPC, or schema change followed the native run.

Failures corrected or diagnosed during verification: the initial TypeScript build required adding empty `syncStatus` to the preview snapshot and removing an unused fixture argument. Initial Clippy required moving production Google helpers before its test module. Final metadata review found that the existing local-palette validator rejected renaming imported calendars with non-palette Google colors. Checked updates now preserve the existing provider color while retaining palette validation for newly chosen colors; the metadata override fixture uses `#123456` to exercise this case. The first focused browser run passed four tests including sync, while its light connection test failed during a dev-server reload coinciding with concurrent workspace edits. The stable rerun passed both connection themes and the complete focused suite. A simultaneous workspace/native build run passed all 86 then-current unit/integration tests but rustdoc could not resolve the core crate. The subsequent sequential workspace run passed its tests and all doc-test targets. The complete 48-test browser run passed 47 tests; its first light accessibility case exceeded the 60-second budget while native/Rust compilation was active. A focused rerun without competing compilation passed all nine accessibility/Google tests, including that case and the final untitled-event/status accessibility assertions. No automatic approval-review rejection occurred.

## Exit gate and remaining limits

The implementation/automated boundary is present; the complete phase 3 exit gate remains pending disposable live Google and installed-platform evidence. Phase 2's live authorization/refresh/discovery/browser/store gate is also still pending. No real account, actual external CRUD/DST recurrence equivalence, real expired cursor, installed browser/store upgrade, Windows/macOS, manual assistive-technology, or packaging/release check was performed here. The earlier Linux real-store evidence belongs to phase 2; this phase does not claim it was rerun.

No Google event writes, outgoing retries/acknowledgements, conflict resolution, recurring-event editing, or remote undo compensation are enabled. Authentication failures pause that account; locked/unavailable stores retain connected state and expose unlock/retry recovery. Network/rate-limit/server errors retry on polling/manual refresh. Persisted exponential backoff/jitter and outbound recovery belong to phase 4. Calendar permission failures allow other calendars to continue; the next complete calendar-list refresh records availability changes. Cached inactive calendars remain stored even though the normal event collection hides inactive calendars.

A subsequent [build configuration follow-up](google-build-configuration.md) adds optional embedded Desktop OAuth client settings for packaged apps. Consult that handoff for the current configuration behavior and verification; it does not advance phase 4 or complete live/platform gates.

## Next starting instructions — Google phase 4

1. Read this handoff/specification/ADRs and inspect the working tree. Preserve phase 2/3 and unrelated shortcut/ignore changes. Reproduce relevant checks. Configure a disposable Desktop OAuth client/account and record outstanding phase 2/3 live/platform evidence separately; never copy credentials or personal event data into docs.
2. Continue through `CalendarService`/`ProviderRegistry`/`Repository` and `sync::SyncAccess`; extend the shared coordinator for outgoing work. Inspect `Repository::queue_change`, `outbox`, `remote_events`, `conflicts`, and the account leases/epochs. Do not create a second persistence layer or start a CLI/background daemon.
3. Keep imported calendars/events read-only until ordinary-event outbound contracts are implemented and verified. Enable supported ordinary create/edit/delete only for current writable permissions; meetings, unsupported event types, and recurrence remain gated. The current Google descriptor intentionally supplies read-only capabilities, and regular reconciliation stores read-only calendars; both gates need a deliberate phase 4 update.
4. Persist effective mutation/outbox intent together; serialize work per event and preserve dependencies. Create valid UUID-derived Google creation IDs once, persist them before sending, and reconcile duplicate/lost responses before retrying. Patch only supported edited fields while retaining untouched provider payloads.
5. Use conditional revisions, preserve both local/remote versions or deletion states on mismatch, and pause only the affected event for explicit Keep local/Keep Google resolution. Incoming full-resync/410 must retain queued intent and conflicts. Implement persisted retry/backoff/jitter and classify permission/validation/authentication/transient failures without leaking response bodies.
6. Implement checked acknowledgement and uncertain-write/restart recovery, plus session undo that cancels unsent intent or queues checked compensation with in-flight dependencies. Add CLI-to-desktop outbox, offline CRUD, late acknowledgements, permission changes, lost successful responses, crash/restart, conflicts, and before/during/after-send undo fixtures. Debounced writes should request the same account queue; incoming-only polling is not proof of outbound correctness.
7. Update the main phase table and write `google-phase-4.md` with exact commands/results and limits. Phase 5 remains compatible recurrence two-way sync; phase 6 remains platform/live/release hardening. Do not mark a live or alpha exit gate passed from mocked/native compilation evidence.
