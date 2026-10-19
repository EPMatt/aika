# Google Calendar integration

Google account connection, cached incoming synchronization, and ordinary-event two-way editing and compatible recurring writes are implemented and **experimental**. Automated fixtures cover these boundaries. Existing live Google validation was confirmed by the user on 2026-10-19 without platform/client/revision evidence; installed-platform acceptance and newly introduced behavior require separate evidence. See [accepted decisions G01–G13](../architecture.md#connected-calendar-decisions), [release verification](../releases.md#unresolved-verification), and the [roadmap](../roadmap.md).

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

### Durable recurrence foundation (Google follow-on 01)

Schema v6 transactionally upgrades existing databases, retaining pending operations, dependencies, attempts, and acknowledgements as singleton groups. New intent records one group per action, ordered outbox steps, complete before/after series snapshots, and stable create identities before HTTP. Incoming reads remain cache-first. Groups serialize against every affected series; acknowledged members retain their local overlay while any sibling is unfinished. Incoming shadows continue updating independently. Pending/attempted counts count actions once, including successful steps of an unfinished group.

Permission and validation failures block the group while unrelated resources continue. Authentication retains existing account-wide pause behavior. Restart recovers uncertain requests through the existing operation-marker/conditional-revision protocol before resending, using persisted step/resource identities. Completion publishes all observed members together, preserving later pending overlays. Checked calendar/account discard retires complete same-calendar groups, fences workers, and preserves already successful remote steps. Conflict reviews include group progress and all affected local aggregates; **Keep Google** retires the connected pending component and restores retained incoming observations. Phase 03 supplies checked Keep local replanning inside an ordered group. Wholly unattempted groups cancel atomically; attempted multi-step undo requires completed forward reconciliation before checked compensation.

Native snapshots expose `eventActions` with typed edit/delete scopes and disabled reasons. The renderer consumes these restrictions; `aika-cli events actions ID` prints the same native contract as JSON. Rust validates scopes before mutations, independently of renderer controls. Phase 01 kept every Google recurring scope disabled; phase 02 enables occurrence and series for compatible events. Phase 03 enables following for compatible events with explicit future-exception reset consent. Ordinary-event delivery and local recurrence semantics remain available.

The backend can look up actual instance identities through Google's [instances endpoint](https://developers.google.com/workspace/calendar/api/v3/reference/events/instances), matching series identity and canonical `originalStartTime`, including moved/cancelled instances, pagination, and ambiguous/missing-result rejection. It never constructs an instance ID. Phase 02 binds the selected provider identity and instance revision in SQLite before delivery, outside the lookup HTTP transaction. A fresh lease/epoch check rejects late binding. Retry/restart uses the same binding; mismatched original editable fields produce a conflict.

### Outgoing edits, recovery, and undo

Writer/owner calendars support ordinary-event create/edit/delete, including offline changes committed atomically with durable outbox work. Compatible recurring creation, occurrence edit/cancel/restore, series metadata edits and whole-series deletion are supported. Meetings with attendees, special event types, conference-bearing events, remote moves and structural whole-series edits remain unsupported. Following edits/deletion are supported through the ordered split protocol below. Calendar metadata customization is local presentation.

The desktop reconciles incoming state before draining outgoing work. Operations serialize per event, retaining dependency identities, attempts, and retry deadlines across restarts. Google creates use persisted UUID-derived IDs; private operation markers and reads recover ambiguous responses. Updates patch only changed supported fields and use conditional revisions; deletes are conditional too. Recreated resources retain supported provider-only properties. See [conditional writes](https://developers.google.com/workspace/calendar/api/guides/version-resources).

Transient network/server/rate-limit failures use persisted exponential backoff with jitter, retried on later desktop cycles. Authentication pauses the account; permission/validation failures retain blocked intent. **Retry pending changes** reschedules eligible work without replacing operation identities/history. Conflicts retain local/remote versions or deletion states for checked **Keep local** / **Keep Google** resolution; other events continue.

Session undo cancels only never-attempted matching operations. Attempted/in-flight/acknowledged work queues checked compensation after recovery of its predecessor. Restoring a remotely deleted event uses a fresh persisted identity because Google retains tombstones. Undo history ends on restart; pending operations and conflicts do not.

### Compatible recurring writes (Google follow-on 02)

Creation supports unsplit all-day and whole-second UTC/Etc-UTC daily, weekly, monthly-by-date and yearly rules. Intervals, weekday selection for weekly patterns, counts and inclusive end dates compile exactly. Google end-date values for UTC schedules use that date's fixed start clock as UNTIL. Regional-zone/DST, ordinal-weekday, split-segment and fractional-second UTC rules fail before committing local intent. Unsupported imported schedules retain read-only projections and existing range-coverage behavior.

Occurrence edits/cancellations use actual instance IDs resolved by series and original start, including moved and cancelled instances. Restoring a cancelled occurrence uses conditional `status=confirmed`; it does not create an independent event. The backend requires an instance revision. A missing/ambiguous identity or missing revision blocks delivery safely. A cancelled occurrence can be restored by checked undo or a CLI occurrence update using its original selector. Provider-side meetings and other unsupported field combinations block delivery even if permissions changed after queuing.

Google master metadata edits explicitly preserve individual edits and cancellations. Schedule fields are locked for series edits in the renderer; native/CLI schedule changes fail because complete remote exception correspondence cannot yet be established. Google series reset requests are rejected; following reset requests require confirmation. Local calendars retain their confirmation/reset behavior. Renderer gestures support single-occurrence moves/resizes; following gestures require reset consent; series schedule gestures remain disabled for Google. `events actions ID` reports native scope availability.

Conflict snapshots and checked reviews include aggregate exceptions. Keep Google restores all retained observations. Keep local can retry single-occurrence or metadata intent against checked current resources; phase 03 extends this to combined master/exception and multiple-occurrence intent through ordered checked replanning. Unattempted undo cancels the complete action; delivered single-resource undo queues compensation after predecessor recovery. Restoring a delivered series deletion creates a fresh master and ordered exception-restoration steps with individually resolved provider identities. Partial restoration retains the complete overlay, counts one pending action and resumes forward; phase 03 permits compensation after completed forward reconciliation. Discard never reverses successful remote requests.

### Follow-on 02 disposable acceptance scenarios

These new scenarios require separate live evidence; they have not been executed against Google in this phase. Record client/platform/source revision and actual results, separately from the user-confirmed validation on 2026-10-19.

1. Create disposable all-day and UTC series for each supported frequency, weekly weekdays/interval 2, count, and inclusive end date. Compare complete bounded schedules/count endings in Google and aika; verify invalid month dates and leap-year skips. Attempt a regional/DST and ordinal-weekday rule and verify no local intent or outbox row commits.
2. Edit and cancel an occurrence, including an externally moved one. Restart offline, reconnect, and verify exactly the selected original occurrence changes. Restore a cancelled instance via CLI/undo. Confirm actual instance IDs and independent revisions in non-secret fixture/debug evidence.
3. Add individual edits/cancellations in Google, sync, then edit master title/location/notes. Verify all exceptions and provider-only reminders/color remain intact. Verify structural master edits fail with explanations (following was gated in phase 02 and has separate phase 03 acceptance below), and attendee-bearing events remain read-only.
4. Introduce a remote concurrent occurrence/master edit. Review exceptions in the conflict, test Keep local and Keep Google independently, and repeat after an external exception change invalidates the review. Revoke permission, retry after restoration, and confirm unrelated resources continue.
5. Lose a create/update/cancel response, restart, and verify operation-marker/read recovery without duplicate resources. Undo unattempted and delivered occurrence changes. Delete and undo an exception-bearing series; interrupt after the new master succeeds, then resume exception restoration. Verify complete overlay, one pending action and no duplicate master. Explicit discard retains successful remote steps.

### Following edits and interruption recovery (Google follow-on 03)

“This and following” supports the same exact all-day/UTC patterns as recurring creation. The domain computes the split, and the backend canonicalizes its segments to exact provider rules: trim the original using the number of preceding scheduled starts, then create a tail with its pre-reserved ID and remaining count. A split at the first start deletes the empty original; following deletion trims/deletes without creating a tail. Monday week phase, intervals, exclusive boundaries and preceding individual edits/cancellations are preserved. Regional/DST, ordinal and arbitrary segment schedules remain read-only; structural whole-series edits remain blocked.

Future exceptions reset, including ones absent from the cache, so every Google following edit/delete requires confirmation. The editor and gesture dialog expose the reset checkbox. CLI updates and deletions require `--scope following --reset-exceptions`; missing consent fails before committing any changes. Local-calendar reset behavior remains unchanged.

Account recovery shows completed/total step counts for unfinished groups, including blocked partial writes. Conflict review lists each step. Retry recovers ambiguous requests and resumes forward with the same identities; it never automatically undoes successful steps. Checked Keep local replans the conflicting resource against observed state while preserving successful siblings and other resources' uncertain identities. A concurrently changed schedule or occupied reserved identity can still require Keep Google and a fresh action rather than an unsafe overwrite. Keep Google retires the connected remainder and publishes observed state, including a successful trim without a tail. Calendar/account discard carries the same successful-remote-step warning.

Undo cancels wholly unattempted groups. For attempted multi-step groups, finish reconciliation with Retry/conflict review, then Undo queues conditional compensation. That compensation restores the master and exceptions in order and deletes the tail; completed remote steps do not bypass revision checks. Undo history ends at restart. No schema migration is added beyond v6; generated native contracts add optional account-group and conflict-step progress.

### Follow-on 03 disposable acceptance scenarios

These scenarios remain unexecuted against live Google. Record platform/client/revision separately from the existing user-confirmed validation on 2026-10-19 and the phase 02 acceptance scenarios.

1. Split disposable all-day and UTC daily/weekly/monthly/yearly series at first/middle/final starts; include interval 2, multiple weekly weekdays, count and end-date endings, month-end/leap-date skips. Compare all preceding starts and the remaining tail in Google/aika. Delete following and verify no tail exists.
2. Add preceding and future moved/cancelled exceptions, including future exceptions outside displayed coverage. Verify missing consent commits no intent. Confirm reset, verify preceding exceptions survive and future exceptions reset. Confirm structural series, regional/DST and attendee-bearing writes remain blocked.
3. Interrupt before/after head request, head acknowledgement, tail request and tail acknowledgement. Restart offline, then reconnect. Verify the complete local overlay persists, progress reports partial success, retry uses the same tail identity and no duplicate tail appears.
4. Introduce head and tail conflicts, stale reviews, permission loss and authentication expiry. Verify unrelated resources continue where appropriate. Exercise Keep local, Keep Google and checked discard separately; a successful trim remains effective after discarding a missing tail.
5. Undo an unattempted split; reconcile an attempted split and undo it. Check conditional compensation of master, tail and individual exceptions. Interrupt compensation and resume. Repeat from independent database connections and verify stale workers/reviews cannot publish after discard.

## Provider extension guide

Implement a backend `Provider` adapter, register/configure it once per type, and expose its descriptor through the existing picker. Supply account authentication/refresh, paginated discovery, remote ID/revision/cursor mapping, normalized incoming changes, event capabilities, and checked outgoing operations. Providers without incremental cursors can use full reconciliation; authentication need not follow Google's flow.

Adapters classify retryable, authentication, permission, validation, conflict, and missing-resource failures and preserve fields outside the editable domain. The common layer owns repository state, settings/selection, scheduling, durable intent, retries, conflicts, and undo. Do not duplicate these systems or branch calendar views by provider name. Runtime provider-plugin loading is outside the current design.

Define public DTOs in Rust and regenerate TypeScript. Backend checks must enforce capabilities even when the UI disables controls. Extend contract tests for multiple accounts, colliding remote IDs, wrong-calendar responses, permission changes, time/recurrence, pagination, failed publication, lease recovery, uncertain writes, checked conflicts, and compensation. Test-only adapters/transports/stores remain under `cfg(test)`.

## Verification boundary

Use the [contribution checks](../../CONTRIBUTING.md#checks) and [native testing guide](../../CONTRIBUTING.md#native-testing). Automated fixtures establish protocol/repository behavior, not real Google propagation or installed browser/store behavior. Live checks must cover actual consent, identity/discovery, refresh/reconnection, external CRUD and recurrence, offline restart, permission loss, conditional-write conflicts, uncertain responses, retries, and undo using disposable data. Record platform, client/consent status, source revision, actual results, and unresolved cases without credentials. Acceptance and remaining evidence live in [releases](../releases.md#unresolved-verification).

### Follow-on 01 disposable acceptance scenarios

Use a disposable account/calendar and back up the test database. These scenarios are added for phase 01; they have not received live acceptance in this implementation session.

1. Queue ordinary create/edit/delete intent offline on schema v5, upgrade to v6, restart offline, and verify unchanged cache and action counts. Reconnect and confirm delivery without duplicate creates.
2. Interrupt delivery after a remote write but before its acknowledgement. Restart and confirm that the reserved identity and operation marker recover the result before resend. Repeat after delete/undo/recreate with a subsequent queued edit.
3. Open an imported recurring event and inspect `aika-cli events actions ID`: all edit/delete scopes must have disabled reasons; ordinary edits and local recurrence must remain available.
4. Change permission/authentication while work is pending. Check explicit retry, unrelated-event progress, checked discard warnings, and rejection of late worker publication. Do not interpret local discard as undoing a successful Google write.

Record source revision, platform, client configuration status, and actual provider observations separately from fixture results. Live recurring writes, splits, and meetings belong to later phases.
