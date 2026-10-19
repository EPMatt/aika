# Google Calendar integration

Google account connection, cached incoming synchronization, and ordinary-event two-way editing are implemented and **experimental**. Automated fixtures cover these boundaries; live-provider and installed-platform acceptance is incomplete. See [accepted decisions G01–G07](../architecture.md#connected-calendar-decisions), [release verification](../releases.md#unresolved-verification), and the [roadmap](../roadmap.md).

## Google setup and authentication boundary

1. Use an application-owned Google Cloud project and enable Calendar API.
2. Configure the OAuth consent screen and test audience. Use disposable accounts/calendars for development; record the actual consent/publication/verification status before distribution.
3. Create an OAuth client of type **Desktop app**. Supply its client ID and the client secret if present in the downloaded configuration using the native settings below.
4. Ensure the desktop's persistent OS credential store is available and unlocked. On Linux, see [credential-store troubleshooting](linux-credential-store.md).

The backend requests `openid`, `email`, `https://www.googleapis.com/auth/calendar.calendarlist.readonly`, and `https://www.googleapis.com/auth/calendar.events`. Import selection controls aika synchronization, not Google's account-wide permission grant. Calendar/ACL management scopes are unnecessary.

Rust opens the system browser, validates fresh state with S256 PKCE and a one-use IPv4 random-port loopback listener, and owns exchange/refresh. Connection sessions are opaque, cancellable, and expire after three minutes. A verified identity response supplies subject/email; refresh retains the previous refresh token if the response omits one. Tokens stay in the OS store, never SQLite, renderer storage, logs, or captures. No plaintext fallback exists. Disconnect deletes local credentials and retains cache; it does not revoke Google's remote grant, which can be revoked through Google account controls.

Protocol references: [desktop OAuth](https://developers.google.com/identity/protocols/oauth2/native-app), [Calendar scopes](https://developers.google.com/workspace/calendar/api/auth), and [consent setup](https://developers.google.com/workspace/guides/configure-oauth-consent).

## Configuration

Copy [`.env.example`](../../.env.example) to repository-root `.env` and fill in the Desktop client settings:

```dotenv
AIKA_GOOGLE_CLIENT_ID=your-desktop-client-id
AIKA_GOOGLE_CLIENT_SECRET=your-desktop-client-secret
```

Leave the secret empty if the client has none. Restart development commands or rebuild packages after changes. `pnpm desktop`, `pnpm desktop:build`, CLI/tool wrappers, and alpha/release packaging share this configuration loader.

Configuration has two distinct steps:

| Step                                  | Precedence                                                                                                                                                                                                      |
| ------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Development/build command environment | Root `.env` entries override inherited variables, including explicit blanks; omitted keys remain inherited. Only root `.env` is loaded, regardless of working directory.                                        |
| Native executable launch              | A nonblank runtime `AIKA_GOOGLE_CLIENT_ID` selects the runtime ID and its optional runtime secret as a complete pair. Otherwise use the embedded build-time pair. A secret alone is ignored; sources never mix. |

Values are trimmed; blanks count as absent in the native resolver. The root file uses Node dotenv parsing (comments, quotes, multiline values, optional `export`), without shell execution or interpolation. `.env.local` and mode files are not native inputs. The ignored root file is not bundled, and packaged apps never search for dotenv files. Cargo tracks changed/removed embedded inputs without requiring a clean build.

Alternatively, set the variables in the build shell:

```sh
AIKA_GOOGLE_CLIENT_ID='your-desktop-client-id' \
AIKA_GOOGLE_CLIENT_SECRET='your-desktop-client-secret' \
pnpm desktop:build
```

In PowerShell use `$env:AIKA_GOOGLE_CLIENT_ID` and optional `$env:AIKA_GOOGLE_CLIENT_SECRET` before the build command. Root `.env` entries still win over those assignments.

Both workflows currently use repository **secrets** named `AIKA_GOOGLE_CLIENT_ID` and `AIKA_GOOGLE_CLIENT_SECRET` for their package-build steps. Ordinary quality/native steps do not receive these inputs. Missing values leave packaging valid with Google disabled. No client or repository setting is created by the build scripts.

Desktop client configuration, including any Desktop client secret, is public/extractable from binaries. It must contain no user access/refresh token or confidential server credential. Native settings are independent of renderer `VITE_` variables. Rebuild existing installers to embed settings; changing OAuth clients may require reconnecting accounts.

## Current technical behavior

### Accounts and calendars

**+ → Google Calendar** starts connection and then a fully paginated checklist, including hidden entries. Eligible calendars start checked; free/busy-only and inaccessible entries explain their exclusion. Discovery remains ephemeral until one-use checked confirmation. Failed/cancelled discovery leaves existing calendars intact; stale selection snapshots are rejected.

Reconnection retains stable account/calendar identities, exclusions, and local name/description/color/visibility overrides. **Manage account** revises selection, reconnects, or disconnects. Visibility does not control sync. After selection confirmation, regular complete discovery adds new eligible calendars and marks missing/inaccessible calendars inactive without deleting cache or pending intent. Provider metadata updates untouched local presentation fields; migrated imports preserve their existing presentation on first refresh.

**Remove from aika** excludes an imported calendar without deleting it in Google. Unresolved pending work/conflicts block ordinary exclusion/disconnect. **Manage account → Disconnect account** offers a review of pending operations, conflicts, and attempted operations, followed by an explicit **Discard these local changes** checkbox and **Discard and disconnect**. The checked transaction rejects newer intent, restores the last known incoming versions (which may be stale), removes local-only creates, retires work without replay on reconnection, and clears affected session undo while retaining unrelated undo. Already sent Google requests may still take effect; discard does not send compensating requests.

**Remove from aika** separately reviews only that calendar’s pending/conflict/attempted counts. With unresolved work, an explicit **Discard this calendar’s local changes** checkbox enables **Discard and remove**. Confirmation compares an opaque revision inside the removal transaction; stale reviews require renewed confirmation. Removal retires that calendar’s operations, clears conflicts and effective cached events/cursor/coverage, removes affected undo, repairs the default, and fences in-flight publication. Other calendars and account credentials remain intact, including when the account needs reconnection or the calendar lost write permission. Stable calendar/remote identities and incoming shadows are retained, and ordinary discovery respects the exclusion. Explicit re-inclusion through **Manage account** starts a full refresh without replaying discarded changes. Full account deletion is not implemented.

Disconnect fences workers and disables writes before deleting credentials; failed store deletion leaves the account disconnected and can be retried with **Disconnect account** without repeating discard. Default fallback prefers the oldest eligible local calendar, then an imported calendar, then none.

### Incoming sync and offline reads

The desktop coalesces account requests on startup, focus/resume, navigation, manual refresh, 60-second polling, and debounced local writes. The CLI reads the cache and queues supported edits without starting a sync worker.

A paginated master/exception feed uses `singleEvents=false`, deleted entries, and no date filters, with stable incremental parameters. Separate bounded instance requests provide unsupported recurrence projections. Complete staged generations publish effective events, shadows, cursor, and coverage together. HTTP 410 restarts a full generation without clearing useful published data or pending overlays. Owner/epoch checks and renewable 90-second leases reject stale publication across processes or selection changes.

Validated simple all-day and UTC timed rules can use the common recurrence domain. Source-zone/DST rules, timed UNTIL, EXDATE/RDATE, ordinal BYDAY, and richer constructs use provider projections. Raw provider fields remain in shadows, including missing titles/redacted data. Projection coverage spans 12 months past and 24 ahead, extended to opened online ranges. Feed changes conservatively invalidate older projection coverage; uncached offline ranges are marked incomplete. Limits bound streams to 1,000 pages / 100,000 items, HTTP responses to 4 MiB, and staged payloads to 64 MiB.

Previously published data survives failed requests. Permission failures isolate a calendar; authentication/store failures pause an account. Status exposes progress, last success, redacted failures, availability, pending/conflict counts, and range completeness. See [incremental sync](https://developers.google.com/workspace/calendar/api/guides/sync) and [events listing](https://developers.google.com/workspace/calendar/api/v3/reference/events/list).

### Outgoing edits, recovery, and undo

Writer/owner calendars support ordinary-event create/edit/delete, including offline changes committed atomically with durable outbox work. Meetings with attendees, recurring events, special event types, and remote calendar moves remain read-only/unsupported. Calendar metadata customization is local presentation.

The desktop reconciles incoming state before draining outgoing work. Operations serialize per event, retaining dependency identities, attempts, and retry deadlines across restarts. Google creates use persisted UUID-derived IDs; private operation markers and reads recover ambiguous responses. Updates patch only changed supported fields and use conditional revisions; deletes are conditional too. Recreated resources retain supported provider-only properties. See [conditional writes](https://developers.google.com/workspace/calendar/api/guides/version-resources).

Transient network/server/rate-limit failures use persisted exponential backoff with jitter, retried on later desktop cycles. Authentication pauses the account; permission/validation failures retain blocked intent. **Retry pending changes** reschedules eligible work without replacing operation identities/history. Conflicts retain local/remote versions or deletion states for checked **Keep local** / **Keep Google** resolution; other events continue.

Session undo cancels only never-attempted matching operations. Attempted/in-flight/acknowledged work queues checked compensation after recovery of its predecessor. Restoring a remotely deleted event uses a fresh persisted identity because Google retains tombstones. Undo history ends on restart; pending operations and conflicts do not.

## Provider extension guide

Implement a backend `Provider` adapter, register/configure it once per type, and expose its descriptor through the existing picker. Supply account authentication/refresh, paginated discovery, remote ID/revision/cursor mapping, normalized incoming changes, event capabilities, and checked outgoing operations. Providers without incremental cursors can use full reconciliation; authentication need not follow Google's flow.

Adapters classify retryable, authentication, permission, validation, conflict, and missing-resource failures and preserve fields outside the editable domain. The common layer owns repository state, settings/selection, scheduling, durable intent, retries, conflicts, and undo. Do not duplicate these systems or branch calendar views by provider name. Runtime provider-plugin loading is outside the current design.

Define public DTOs in Rust and regenerate TypeScript. Backend checks must enforce capabilities even when the UI disables controls. Extend contract tests for multiple accounts, colliding remote IDs, wrong-calendar responses, permission changes, time/recurrence, pagination, failed publication, lease recovery, uncertain writes, checked conflicts, and compensation. Test-only adapters/transports/stores remain under `cfg(test)`.

## Verification boundary

Use the [contribution checks](../../CONTRIBUTING.md#checks) and [native testing guide](../../CONTRIBUTING.md#native-testing). Automated fixtures establish protocol/repository behavior, not real Google propagation or installed browser/store behavior. Live checks must cover actual consent, identity/discovery, refresh/reconnection, external CRUD and recurrence, offline restart, permission loss, conditional-write conflicts, uncertain responses, retries, and undo using disposable data. Record platform, client/consent status, source revision, actual results, and unresolved cases without credentials. Acceptance and remaining evidence live in [releases](../releases.md#unresolved-verification).
