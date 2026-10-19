# Extensible calendar providers and Google integration

Status: **phases 0–1 delivered; phase 2 connection/selection implemented with live/platform exit evidence pending; phase 3 incoming sync implemented with live/platform evidence pending; phase 4 ordinary-event two-way sync implemented with live/platform evidence pending; phases 5–6 planned.** Accepted decisions were agreed with the user before implementation. This document is the implementation specification for the next increment, not a claim that live Google verification or synchronization is complete.

Read the [main plan](../plan.md), [architecture decisions](../architecture.md#connected-calendar-decisions-google-integration), and [phase 4 handoff](../handoffs/google-phase-4.md). Original v0 phase numbers refer to earlier milestones; Google phase numbers below form a separate sequence. Outstanding alpha/platform checks remain tracked in [alpha verification](../alpha/verification.md).

## Current implementation and intended behavior

The desktop and CLI share `aika-core`, SQLite schema v5, and checked mutations. Multiple local calendars and common recurring events already exist. `ProviderRegistry` owns the shared `Repository` and registers Local and Google once by provider type. Google now supplies backend OAuth sessions, persistent OS credentials/refresh, every-page calendar discovery, and checked import selection. The sidebar plus button opens a descriptor-driven provider popup; Local reuses its existing form. Account management revises selections, reconnects, and disconnects. The desktop coordinator now imports paginated event feeds and bounded unsupported-recurrence projections through checked shared repository transactions. Writable imported calendars now support ordinary-event writes through the durable shared outbox, conditional revisions, explicit conflicts, retries, and session undo. Meetings/recurrence/special types remain read-only. Caches reopen offline, and coverage/status accompany snapshots. Automated evidence and pending live/platform gates are distinguished in the phase 4 handoff.

The intended complete integration provides **+ → provider popup → Local or Google**. Local opens the existing calendar form. Google connects an account in the system browser, discovers calendars, and shows an import checklist with every eligible calendar checked. Confirming adds the selected calendars and starts their synchronization. The same generic flows and coordinator serve future providers.

### Agreed defaults

| Concern                        | Accepted behavior                                                                                                                                                          |
| ------------------------------ | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Import                         | All eligible calendars checked by default; confirmation imports only the selection. Inaccessible/free-busy-only calendars are listed with a reason and cannot be selected. |
| Accounts                       | Multiple accounts per provider; reconnecting an existing account retains mappings and exclusions.                                                                          |
| Discovery                      | Import new eligible calendars automatically; keep explicitly excluded calendars excluded.                                                                                  |
| Visibility                     | Showing/hiding calendars is a local preference independent of sync selection. Newly imported calendars start visible.                                                      |
| Sync lifetime                  | Run while the desktop is open; preserve pending work when it closes. No background service.                                                                                |
| Scheduling                     | Startup, focus/resume, manual refresh, 60-second polling, and debounced local writes. Coalesce duplicate requests for an account.                                          |
| Conflicts                      | Preserve both versions, pause that event, and ask Keep local or Keep Google. Other events continue.                                                                        |
| Recurrence                     | Two-way editing for compatible series and exceptions in phase 5; unsupported rules remain read-only.                                                                       |
| Unsupported recurrence offline | Cache expanded occurrences for 12 months past and 24 months ahead, plus other ranges opened online; identify missing coverage explicitly.                                  |
| Meetings                       | Display meetings with attendees and preserve their data; invitation editing and notification choices are deferred.                                                         |
| Defaults                       | Retain the current default calendar. Only active writable calendars are eligible; if none exist, event creation is unavailable.                                            |
| Calendar management            | Imported name/color customization is local presentation. Removal means exclusion from aika, never remote deletion.                                                         |
| Moves                          | Preserve local-calendar moves. Remote-calendar and cross-provider moves are deferred.                                                                                      |

After disconnection or loss of default-calendar eligibility, select the oldest remaining active writable local calendar, then an active writable imported calendar, or no default. Explicitly removing an account or excluding a calendar must first synchronize its pending work or obtain an explicit discard choice. Remove its cache only after that decision; dropping an account never deletes events in Google.

## Architecture and planned interfaces

### Ownership and data flow

The flow is renderer/CLI → `CalendarService` → shared repository for reads and immediate mutations; a desktop-only backend coordinator communicates with provider adapters and applies checked results to that repository. See ADRs G01–G04 in the [architecture document](../architecture.md#connected-calendar-decisions-google-integration).

- Register provider implementations by stable provider type. Model accounts separately, including provider identity, stable remote subject, display metadata, connection state, and selections.
- Preserve existing internal IDs, including `local-default`. Scope remote-calendar mappings by account, and remote-event mappings by account/calendar. Use internal UUIDs for newly imported identities. A shared calendar connected through two accounts remains two account-labelled local identities.
- Extract SQLite storage from `LocalProvider`. Keep application preferences/defaults/visibility in the shared repository rather than delegating global settings to one adapter.
- Persist remote shadow payloads, opaque revisions, effective cached domain events/series, cursors, coverage, outbox operations, deletion intent, and conflicts. Editable fields and provider-only fields must survive synchronization together.
- Commit local changes and their outbox operations in one transaction. Run HTTP outside database/service locks, using short claim/apply transactions and checked revisions. A durable account lease prevents multiple desktop workers from sending the same work concurrently.
- Stage paginated import/resync generations. Publish their completed remote shadow and final cursor atomically; retain pending local intent and conflicts on failure or cursor invalidation. Pending edits overlay the rebuilt remote shadow.
- Save operation identities and acknowledgements. Retries of uncertain creates must discover the prior result rather than create another event. Late responses must not replace newer local edits.
- Keep pure recurrence/time validation in `aika-domain`, including its WASM build; network, persistence, accounts, and secrets remain native backend responsibilities. Remote import must accommodate valid provider data such as missing titles or redacted details without sending it through local creation assumptions.

Google event synchronization uses a paginated master/exception feed with `singleEvents=false`, deleted entries included, and no date filters. Keep its request parameters stable across incremental requests. Cursor invalidation rebuilds the remote shadow; it does not wipe queued edits. Date-bounded instance requests for unsupported recurrence are a separate projection cache, not the incremental feed. See [events list](https://developers.google.com/workspace/calendar/api/v3/reference/events/list) and [incremental synchronization](https://developers.google.com/workspace/calendar/api/guides/sync).

### Public contracts and commands

Phase 1 exports `ProviderDescriptor`, `AccountSummary`, `ConnectionState`, `Capabilities`, and the enriched native `CalendarCollectionSnapshot`. Phase 2 additionally exports `ConnectionSession`, `ConnectionProgress`, `DiscoveredCalendar`, and `CalendarDiscovery`, with native connection/discovery/selection/disconnect IPC. Phase 3 adds generated account/calendar sync status and `request_sync` IPC. Phase 4 adds generated `SyncConflict` plus list/checked resolution/retry IPC. Define them in Rust and generate TypeScript with the existing exporter; do not hand-edit generated files. Native-only DTOs need not introduce accounts or credentials into the pure event domain.

| Interface                   | Required information or operation                                                                                                                                               |
| --------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Provider descriptor         | Stable provider ID, display name, connection availability/reason, and supported capabilities. Drives the Local/Google picker.                                                   |
| Account summary             | Internal account ID, provider ID, display identity, connection/sync state, selected calendars, and actionable failures. No credentials.                                         |
| Calendar/event capabilities | Read/create/edit/delete, recurrence scopes, metadata customization, and move eligibility. Combine adapter support, access permissions, and event restrictions.                  |
| Sync status                 | Initial progress, last success, pending count, conflicts, retry/error state, and coverage. Distinguish syncing, offline/stale, and unavailable ranges.                          |
| Conflict summary            | Stable conflict identity, retained local/remote versions or deletion, and available resolution actions.                                                                         |
| Provider/account commands   | List providers/accounts, start/cancel connection, discover calendars, commit/revise selection, and disconnect. Connection sessions expose an opaque handle and progress.        |
| Sync commands               | Request/coalesce synchronization, query status/conflicts, and resolve a conflict. Keep local rechecks the current remote revision; Keep Google adopts the current remote state. |
| Collection snapshot         | Preserve existing calendars/events/default/visibility/undo semantics; add account, capability, sync, and coverage information. Isolate failures by account/calendar.            |

Continue backend validation of capabilities even when controls are disabled. No token retrieval, arbitrary URL-opening command, SQL, provider registration, or Google HTTP call is exposed to the renderer. The browser preview remains an explicit in-memory harness; native test adapters stay test-only.

### Provider extension guide

A new provider supplies its descriptor/configuration, account authentication and refresh lifecycle, paginated discovery, remote identity/revision/cursor mapping, normalized incoming changes, and checked outgoing operations. Providers with no incremental cursor use the common full-reconciliation path. Authentication need not use Google's OAuth mechanism.

Adapters own protocol/event-format conversion and classify failures as retryable, authentication, permission, validation, conflict, or missing resource. They must report unsupported operations and preserve fields outside the editor. The common layer owns storage, selections/visibility, scheduling, durable intent, retries, conflicts, and undo; adding a provider must not copy those systems or add provider-name branches to calendar views.

Register and configure the adapter in the backend; expose its descriptor through the shared picker. Verify multiple accounts, ID collisions, permissions, normalized time, recurrence limits, revisions, pagination, and uncertain-write recovery through the shared contract suite. Runtime installation/loading of provider plugins is outside this rollout.

## Delivery phases and exit gates

| Google phase | Status                         | Deliverable                                                                              |
| ------------ | ------------------------------ | ---------------------------------------------------------------------------------------- |
| 0            | Documentation delivered        | Specification, accepted architecture decisions, links, and handoff.                      |
| 1            | Delivered                      | Shared repository, account model, generic dispatch/capabilities, and test-adapter proof. |
| 2            | Implemented; live gate pending | Provider popup, Google authentication, discovery, and import selection.                  |
| 3            | Implemented; live gate pending | Incoming sync, persisted cache, and offline reading.                                     |
| 4            | Implemented; live gate pending | Ordinary-event two-way sync, offline mutations, conflicts, and undo.                     |
| 5            | Planned                        | Compatible recurring-event two-way sync and recoverable splits.                          |
| 6            | Planned                        | Recovery UX, platform evidence, upgrade checks, and release documentation.               |

### Phase 0: Specification and decisions

Record the agreed behavior, implementation boundaries, provider extension guide, OAuth setup, acceptance gates, and handoff convention in this document. Link it from the main plan and README and record accepted decisions with rationale/consequences in the architecture document. Preserve earlier phase history and pending alpha evidence. No runtime, migration, dependency, generated type, or cloud configuration changes belong to this phase.

**Exit:** documentation is consistent with inspected code, formatted, linked, and ready for review/commit. The handoff records its source revision or uncommitted state and checks. Phases 1–6 remain planned.

### Phase 1: Generalize storage and provider dispatch

Extract the repository, introduce transactional account/cache/outbox migrations, and remove local-only collection/calendar-management, series, default/visibility, and undo routing assumptions. Add capability DTOs and backend checks; preserve existing local behavior and explicit expected-value concurrency semantics.

Prove the common layer with a test-only second adapter holding multiple accounts, writable/read-only calendars, and colliding remote IDs. Add no live connection or sample production data. CLI cached reads and supported remote writes must use the same repository/outbox, without starting a background sync worker.

**Exit:** local CRUD, moves, recurrence, session undo, and CLI checks pass; migrations preserve schema v1–v3 data; generic adapter/account isolation and capability checks pass.

### Phase 2: Provider popup, connection, and selection

Make plus open a keyboard-accessible provider popup populated from descriptors. Local reuses the current form. Google connection uses backend sessions, cancellation/timeout, the system browser, token refresh, and secure-store failure recovery. Reconnecting an existing account must not duplicate calendars or reset exclusions.

Discover every calendar-list page, including hidden entries. Show names/colors/account identity/access status, all eligible calendars checked, and disabled explanations for others. Confirming persists the selection; account management supports revising it. Selection and visibility are separate. Cancelled/failed connection or discovery leaves existing calendars intact. See [calendar-list discovery](https://developers.google.com/workspace/calendar/api/v3/reference/calendarList/list).

This milestone proves connection and selection only; successful event import is gated by phase 3. Do not enable editing or claim completed synchronization before the corresponding exit gate.

**Exit:** native connection/discovery/selection, cancellation, duplicate-account recovery, pagination, and credential isolation are verified; Local still works independently.

Implementation is present and tested with disposable native socket/SQLite fixtures and test-only provider transport/store implementations. Real Google authorization/refresh/discovery and installed OS-browser/store behavior still require a configured desktop client and disposable Google account, so phase 2's full exit gate remains pending. Confirming selection adds empty read-only calendars; it does not start synchronization at this milestone. Revising selection requires a new complete discovery. Disconnect retains cached calendars and deletes local OS-store credentials; it does not revoke Google's remote grant. See the [phase 2 handoff](../handoffs/google-phase-2.md).

### Phase 3: Incoming sync and offline reading

Import ordinary events, masters, exceptions, and cancellations. Implement staged pagination, incremental cursors, and full-resync recovery. Reconcile discovery regularly, including a full calendar-list refresh to catch metadata/permission changes that incremental discovery can miss. Auto-add new eligible calendars and respect exclusions. Mark unavailable calendars without discarding unresolved local intent.

Expand compatible rules with the common domain. Refresh unsupported occurrence projections for the agreed window and online navigation ranges; invalidate changed series projections and track completed coverage. Preserve missing/redacted event fields and unsupported provider payloads. Show progress, last success, account failures, and missing offline coverage. Imported calendars remain read-only at this phase even when remote permissions allow writing.

**Exit:** external changes appear locally, caches reopen after restart, read-only permissions work, failed resync preserves useful data, and offline ranges never falsely appear complete/empty.

Phase 3 implementation uses schema v5, a desktop request queue with one active worker per account, and renewable persisted 90-second account leases. Account/selection epochs fence stale pages and publications. A complete raw feed and separate instance projection are staged before publishing effective events, shadows, cursor, and coverage together. HTTP 410 restarts a full generation without clearing useful cache. Automatic reconciliation begins only after checklist confirmation, including an explicitly empty selection. Previous metadata values let provider changes update untouched fields while preserving local overrides; the first refresh of a legacy import preserves its existing presentation.

Compatibility is conservative: simple daily/weekly/monthly-by-date/yearly rules with supported intervals/counts, Monday week start, weekly plain BYDAY, and all-day date UNTIL use the common domain when validation succeeds. Timed common rules currently require UTC; source-zone/DST rules, timed UNTIL, EXDATE/RDATE, ordinal BYDAY, and other constructs use Google projections. Raw fields remain intact in shadows; missing titles and redacted ordinary events bypass local creation validation. Projections cover 12 calendar months past and 24 ahead, extended to include the opened range. Any feed change invalidates older projection coverage; successful bounded refresh replaces overlapping projections. This conservative invalidation may require reopening/refetching previously cached distant ranges. Feed/projection limits are 1,000 pages, 100,000 items per stream, 4 MiB per HTTP response, and 64 MiB per staged generation.

The full exit gate still requires disposable live-provider and installed-platform evidence. Failed requests preserve previously published data and surface redacted recovery status. A calendar permission failure does not stop other calendars; authentication/store failures stop that account. Polling/manual refresh retries incoming work; persisted exponential backoff and outgoing mutation/conflict recovery belong to phase 4. See the [phase 3 handoff](../handoffs/google-phase-3.md) for commands, evidence, and next starting instructions.

### Phase 4: Ordinary-event two-way sync

Enable supported create/edit/delete on writable imported calendars. Persist locally and show pending status immediately; serialize outbox work per event, survive closure/restart, and recover uncertain responses with stable remote IDs. Google supports client-supplied creation IDs; use valid UUID-derived IDs and reconcile duplicates rather than generating another ID on retry. See [event creation](https://developers.google.com/workspace/calendar/api/v3/reference/events/insert).

Use checked remote revisions for edit/delete. A mismatch retains both versions or deletion states and blocks only that event for explicit resolution. Retry transient network/server/rate-limit failures with persisted exponential backoff and jitter. Pause an account on authentication failure; expose permission/validation failures and preserve recoverable work. See [conditional writes](https://developers.google.com/workspace/calendar/api/guides/version-resources) and [API error handling](https://developers.google.com/workspace/calendar/api/guides/errors).

Session undo cancels unsent changes or enqueues checked compensation for acknowledged writes; preserve dependencies for work already in flight. Patch edited supported fields only. Meetings with attendees, unsupported event types, and recurrence remain read-only until a later applicable gate.

**Exit:** offline CRUD, restart/crash recovery, retries without duplicates, permission failures, explicit conflicts, late acknowledgements, CLI writes, and undo converge safely in both directions.

Implementation uses schema v5's existing outbox/conflict/lease tables; no migration or OAuth scope change is needed. The desktop reconciles incoming data before draining ordinary-event work, coalesces debounced writes into the same queue, and persists attempts/deadlines. Stable Google creation IDs and private operation markers recover uncertain writes; updates/deletes use `If-Match`. The [phase 4 handoff](../handoffs/google-phase-4.md) records automated checks and the remaining disposable live-provider/platform gate. Phase 5 remains planned.

### Phase 5: Compatible recurring-event two-way sync

Map provably equivalent rules to the common recurrence domain, keeping unsupported rules intact/read-only. Add occurrence, entire-series, and following-series editing, including moved exceptions and cancellations. Map occurrences by original start, independently of edited time.

Implement following-series edits as durable multi-request workflows with stable identities and persisted progress. Resume or surface partial completion rather than assuming a remote transaction. Preserve the user's queued intent, conflict checks, and compensation behavior through failures. Google's [recurrence guide](https://developers.google.com/workspace/calendar/api/guides/recurringevents) describes the separate trim/create requests and original-start identity.

**Exit:** compatible scoped edits converge both ways; DST, duration, interval/count/end dates, cancellations, exception resets, interrupted splits, conflicts, and undo pass equivalence/recovery checks. Restrict editing for any pattern without proven equivalence.

### Phase 6: Harden and release

Finish accessible account status, retry/reconnection, conflict resolution, and calendar-selection management. Protect pending work during removal. Verify installed-app browser/callback/store behavior, offline operation, restarts, and schema upgrades on supported Linux, Windows, and macOS targets.

Document actual Google configuration, live test setup, consent/verification status, user behavior, and release limitations. Update README/product metadata only to describe delivered behavior. Keep automated, mocked, live-provider, native, and manual/platform evidence distinct; no release/publishing action is implied by this documentation milestone.

**Exit:** relevant automated checks pass, native/platform and live Google evidence is recorded, and remaining acceptance requirements are explicitly pending rather than inferred from successful builds.

## Google setup and authentication boundary

Google Desktop client settings can be embedded at build time; no real client is checked into the repository. Unconfigured builds remain valid with Google disabled. These prerequisites are required for native Google connection/live verification:

1. Use an application-owned Google Cloud project and enable the Calendar API.
2. Configure the OAuth consent screen and test audience; use disposable test accounts during development. Record consent/verification status before public distribution.
3. Create an OAuth client of type **Desktop app** and provide its client configuration to the native backend/build configuration. Do not use renderer `VITE_` configuration for tokens. Native development/build commands first load optional repository-root `.env` entries over inherited variables; values in the file win, including blanks, while omitted variables remain inherited. Rust embeds optional `AIKA_GOOGLE_CLIENT_ID` and `AIKA_GOOGLE_CLIENT_SECRET` values supplied during compilation. The same variables at launch provide a complete runtime override when the client ID is nonblank. Otherwise the embedded pair is used; a runtime secret alone is ignored and secrets never mix across sources. Blank values count as absent. Cargo tracks build-input changes, so changing/removing settings rebuilds the core without requiring a clean build. Supply the Desktop app client ID and the client secret if present in the downloaded Google client configuration. No user credential may be placed in either setting.
4. Request `openid`, `email`, `https://www.googleapis.com/auth/calendar.calendarlist.readonly`, and `https://www.googleapis.com/auth/calendar.events`. Import selection controls aika's synchronization; it does not narrow Google's account-wide permission grant. Calendar/ACL management scopes are not needed for the chosen scope.
5. Use the system browser, S256 PKCE, fresh state, and a one-use random-port loopback listener. Exchange/refresh in Rust, request offline access, and preserve an existing refresh token when a refresh response omits it. Resolve account subject through a verified Google identity response.
6. Store user credentials only in the existing persistent OS store. A locked store pauses remote access; cached and local calendars remain accessible. Missing client configuration disables connection with a clear reason.

The [desktop OAuth guide](https://developers.google.com/identity/protocols/oauth2/native-app) documents the supported flow. The [scope reference](https://developers.google.com/workspace/calendar/api/auth) and [OAuth consent setup](https://developers.google.com/workspace/guides/configure-oauth-consent) describe permissions and publication prerequisites. Treat desktop client configuration as public-client configuration, not a confidential server credential; embedded values, including the Desktop client secret, are extractable from the executable. Rebuild existing installers to embed configuration. Changing the OAuth client may require existing accounts to reconnect. See the [build configuration handoff](../handoffs/google-build-configuration.md) for build commands, optional CI inputs, and evidence. No user token, authorization code, PKCE verifier, or raw authentication failure body belongs in SQLite, renderer storage, logs, screenshots, or handoffs.

The optional root `.env` uses Node dotenv parsing (comments, quotes, multiline values, and optional `export`; no shell execution or variable interpolation). Copy `.env.example` to configure it locally. Only this file is read, regardless of the command working directory. CLI/tool wrappers and alpha/release builds share the loader. Normal packaged apps do not load dotenv files; restart development commands or rebuild to apply file changes. The [dotenv follow-up](../handoffs/google-dotenv-configuration.md) records verification.

## Verification and handoff procedure

### Acceptance scenarios

| Area                      | Required evidence                                                                                                                                                                                                        |
| ------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| Provider/account boundary | Multiple accounts/providers, duplicate connection, colliding remote IDs, wrong-calendar results, capability enforcement, and account failure isolation.                                                                  |
| Persistence/concurrency   | v1–v3 migration, future-schema rejection, rollback, restart, interrupted pagination, cursor commit failure, desktop/CLI races, lease recovery, and late network responses.                                               |
| Authentication/import     | Denial/cancellation/timeout, invalid state, listener cleanup, locked store, refresh failure, every discovery page, hidden/ineligible entries, exclusions, visibility, and new calendars.                                 |
| Two-way ordinary events   | External and local CRUD, offline edits/deletions, lost response after successful write, duplicate retries, invalid cursor with pending work, permission loss, explicit resolution, and undo before/during/after sending. |
| Recurrence/preservation   | UTC/source zones, all-day exclusive ends, DST, counts/end dates, moved/cancelled instances, unsupported rules and coverage, interrupted splits, unchanged provider fields, read-only meetings, and compensation.         |
| UX/native/platform        | Keyboard/focus, both themes, status/progress/retry, missing offline coverage, real webview IPC/SQLite, OS-store/browser/callback behavior, and installed-version upgrades on each supported platform.                    |

Use existing checks as relevant to each runtime change: `pnpm test`, `pnpm test:ui`, `pnpm build`, Rust workspace tests/fmt/clippy, `pnpm types:generate`, and native build/automation commands from [quality verification](../quality.md). Generated contracts must have no unexplained drift. Live tests use disposable Google calendars and must distinguish real-provider evidence from mocks; browser tests do not establish native authentication or persistence. Phase 0 needs documentation formatting, link/scope review, and whitespace checks only.

### Handoff procedure

Write `docs/handoffs/google-phase-N.md` with:

- Status (planned, in progress, delivered, or verified), base/implementation revision or uncommitted state, and the delivered scope.
- Behavior and interface/schema changes, decision references, migration/compatibility details, and artifacts/screenshots when applicable.
- Exact commands and actual results, including host/toolchain, fixtures/accounts used, and failures or checks not executed. Never include real credentials or personal event data.
- Remaining defects/limits and the next phase's concrete starting instructions. Identify which exit gates passed and which still require evidence.

The receiver reads this specification, architecture, and latest Google handoff, checks the working tree, reproduces relevant evidence, and continues only within the requested phase. Do not advance the status table based on planned functionality or the presence of an adapter stub. Preserve the original v0 handoffs and their unresolved acceptance evidence.
