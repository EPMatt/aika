# Google integration follow-on plan — 2026-10-19

Status: phases 01–06 implemented; see the [phase 01 handoff](../handoffs/google-follow-on-01-durable-recurrence.md), [phase 02 handoff](../handoffs/google-follow-on-02-recurring-writes.md), and [phase 03 handoff](../handoffs/google-follow-on-03-following-recovery.md). See the [phase 04 handoff](../handoffs/google-follow-on-04-account-removal.md). See the [phase 05 handoff](../handoffs/google-follow-on-05-meeting-organization.md). See the [phase 06 handoff](../handoffs/google-follow-on-06-invitation-responses.md). Live acceptance for the new features remains separate from automated verification. Structural whole-series edits remain conservatively blocked because complete exception correspondence cannot be established from the cache. Following splits explicitly confirm resetting future exceptions and preserve preceding exceptions. Attempted multi-step undo requires forward reconciliation to finish before checked compensation can be queued.

This is a **separate follow-on plan**, not a replacement for the initial Google integration plan or its handoffs. Phases below belong exclusively to the `google-follow-on` sequence. Do not reinterpret phase numbers in earlier plans or overwrite existing Google handoffs.

Planning handoff: [google-follow-on-00-planning](../handoffs/google-follow-on-00-planning.md). Existing implementation context: [per-calendar recovery](../handoffs/google-calendar-recovery.md), [account recovery](../handoffs/google-account-recovery.md), [browser launch](../handoffs/google-browser-launch.md), and [architecture](../architecture.md).

## Summary and agreed boundaries

Implement recurrence first, followed by full account removal, meeting organization, and invitation responses. Each phase must be independently reviewable and finish with committed architecture documentation and handoff notes.

Record existing live Google validation as **confirmed by the user on 2026-10-19**. Preserve historical test records, and keep acceptance for newly introduced features separate. This confirmation does not establish validation of every installed platform; platform/client/revision evidence was not supplied in this conversation.

Agreed defaults:

- Initial recurrence writes support proven-compatible all-day and UTC rules. Regional-timezone/DST and richer rules retain their current read-only projection behavior.
- Whole-series Google edits preserve individual exceptions. Local-calendar exception-reset behavior remains unchanged.
- Meetings support organizing and responding to invitations.
- Notification choices are everyone, external guests only, and none; everyone is the default.
- Special Google event types, remote calendar moves, conference creation, and broader product features remain outside this plan.

## Architecture decisions to document

Before implementation, update the architecture documentation with the following decisions, including rationale, alternatives, compatibility, and failure behavior. These are planned decisions, not claims about existing code. Preserve ADR G01–G07 and their history; add G08–G13 in the order below if those identifiers remain free, otherwise allocate the next unused identifiers and record the mapping in the phase handoff. Explicitly document any amendment to G07.

| Decision                   | Approach and rationale                                                                                                                                                                                                           |
| -------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Recurrence compatibility   | Share the existing pure recurrence domain. Add a backend Google recurrence compiler that accepts only schedules with an exact provider representation. A matching bounded preview alone is insufficient evidence of equivalence. |
| Durable grouped mutations  | Extend the current outbox with durable operation groups and ordered steps. One user action can affect several remote resources; SQLite atomicity does not imply remote atomicity.                                                |
| Scope-specific actions     | Add typed available edit/delete scopes and meeting actions, enforced by Rust and consumed by the renderer/CLI. The existing recurrence boolean cannot safely express partial support.                                            |
| Exception semantics        | Google master edits preserve exceptions. Reject schedule changes when preserving their correspondence cannot be established. Following edits explicitly confirm resetting affected future exceptions.                            |
| Account deletion           | Use a durable cleanup workflow across SQLite and the credential store, with lifecycle fencing against late workers, OAuth completion, and credential refresh.                                                                    |
| Meetings and notifications | Keep attendees, organizer identity, responses, and delivery policy in native contracts and durable intent, outside the pure recurrence domain. Persist notification choices with operations.                                     |

Keep provider protocol details behind the existing adapter boundary. Preserve cache-first reads, conditional revisions, backend-owned credentials, and generated Rust-to-TypeScript contracts.

## Implementation phases

### Google follow-on 01 — Durable recurrence foundation

- Introduce a transactional schema migration adding operation groups, ordered outbox-step metadata, and persisted aggregate before/after state. Existing ordinary-event operations continue unchanged.
- Persist stable operation/resource identities before sending requests. Resolve instance IDs through Google using series identity and `originalStartTime`; never manufacture instance IDs. See [Google recurrence documentation](https://developers.google.com/workspace/calendar/api/guides/recurringevents).
- Serialize groups against every affected series. Retain incoming shadows while preserving the complete local overlay until the group completes or the user resolves it.
- Make retry, conflict review, pending counts, undo, disconnect, and calendar discard group-aware. Count one user action once, and never retire only part of a group.
- Recover uncertain requests before resending. Authentication pauses the account; permission/validation failures block the affected group; unrelated events continue.
- Add scope-specific action DTOs and disabled reasons. Keep recurring writes disabled until phase 02.

**Exit:** migration/restart and crash-boundary tests establish durable recovery without changing existing ordinary-event behavior.

**Handoff:** `docs/handoffs/google-follow-on-01-durable-recurrence.md`.

### Google follow-on 02 — Compatible recurring create, occurrence, and series writes

- Enable recurring creation for the agreed compatible subset, including supported interval/count/end-date semantics. Reject unsupported schedules before committing local intent.
- Support editing and cancelling one occurrence through its actual provider instance identity, including moved and previously cancelled occurrences.
- Support whole-series deletion and master edits. Preserve existing exceptions; metadata edits remain available where safe. Block structural edits when exception preservation or recurrence equivalence cannot be established.
- Extend the mutation request with an explicit exception policy. Native validation selects the supported policy; Google operations must not inherit the local planner’s reset behavior accidentally.
- Preserve provider-only fields with narrow conditional updates. Keep attendee-bearing events read-only until the meeting phases.
- Integrate renderer controls, gestures, CLI mutation scopes, conflict handling, and checked compensation. Unsupported scopes explain their restriction.

**Exit:** supported recurrence operations survive offline edits, restart, conflicts, lost responses, and undo; unsupported rules retain their current projections.

**Handoff:** `docs/handoffs/google-follow-on-02-recurring-writes.md`.

### Google follow-on 03 — “This and following” with interruption recovery

- Compile the existing domain split into an ordered group: conditionally trim the original series, then create the tail using its persisted identity. Following deletion trims the original without creating a tail; a first-occurrence split handles the empty head explicitly.
- Preserve interval phase, remaining count, exclusive boundaries, and preceding exceptions. Reject splits that cannot be represented exactly.
- Require confirmation when affected future exceptions will reset. Google documents both the two-request split and this reset behavior. See [Google following-instance guidance](https://developers.google.com/workspace/calendar/api/guides/recurringevents?authuser=19).
- Resume forward after interruption; do not automatically roll back successful remote steps. Persist step acknowledgements and recover a lost tail-create response without duplication.
- Expose partially applied groups in recovery UI. **Keep local** replans unfinished intent against checked current resources; **Keep Google** retires the remainder and publishes observed remote state, explaining any partial result.
- Undo cancels wholly unattempted groups. Attempted groups must first reconcile, then queue checked compensation. Discard warns that successful remote steps remain effective.

**Exit:** fault injection at every request/acknowledgement boundary proves restart recovery, isolation, and absence of duplicate tails.

**Handoff:** `docs/handoffs/google-follow-on-03-following-recovery.md`.

### Google follow-on 04 — Full account removal

- Add checked account-removal review and confirmation, distinct from disconnect. Review includes selected/excluded calendars, cached data, unresolved groups/conflicts, attempted writes, and an opaque revision.
- Refuse unresolved work by default; provide explicit discard-and-remove confirmation using the established uncertain-write warning.
- Persist a removal-in-progress state and fence workers, reconnection, OAuth completion, and credential writes. Coordinate credential persistence/deletion through a cross-process identity lock and checked lifecycle generation; do not hold it during provider HTTP.
- Delete credentials, then transactionally remove account-owned calendars, cache, mappings, outbox/groups, conflicts, staging, and leases. Repair the default and remove affected undo.
- On store failure or crash, retain a retryable cleanup record and resume cleanup on startup. After completion, retain only an opaque lifecycle tombstone needed to reject stale work; fresh explicit sign-in creates a new lifecycle.
- Removal affects aika data and credentials only. Google calendars and the remote authorization grant remain intact.

**Exit:** credential failures and crashes cannot silently lose cleanup work, restore deleted credentials, or affect another account.

**Handoff:** `docs/handoffs/google-follow-on-04-account-removal.md`.

### Google follow-on 05 — Meeting organization

- Add native meeting-details and mutation DTOs, including organizer, attendees, optional status, response display, expected revision, and notification policy. Extend conflict/recovery snapshots to include meeting intent.
- Enable create/edit/cancel for organizer-owned default events. Preserve existing attendee response data and unknown fields; retrieve a complete attendee representation before changing the guest list.
- Persist `sendUpdates=all|externalOnly|none` for every applicable write, including cancellation and compensation. Default to `all`; choosing `none` requires acknowledging its propagation warning. Google documents possible synchronization problems with silent creation. See [Google insert reference](https://developers.google.com/workspace/calendar/api/v3/reference/events/insert).
- Support compatible recurring meetings through the established group machinery. Block operations requiring unsafe conference copying; preserve existing conference data on narrow updates and display existing links.
- Explain that delivered invitations cannot be recalled by undo or local discard. Recover ambiguous writes before retrying; do not promise exactly-once email delivery.

**Exit:** organizer edits preserve attendee responses and unrelated provider fields, and notification policy survives retry/restart.

**Handoff:** `docs/handoffs/google-follow-on-05-meeting-organization.md`.

### Google follow-on 06 — Invitation responses

- Add explicit accept/tentative/decline actions for the authenticated attendee on a permitted calendar copy. Resolve that attendee in the backend from verified identity/provider metadata.
- Permit response changes without granting organizer-edit capabilities. Do not allow renderer-supplied attendee identity or overwrite other attendees.
- Use participant-only updates where supported, conditional revisions, durable offline intent, conflict handling, and checked undo. Google exposes `attendeesOmitted` for participant-response updates. See [Google event reference](https://developers.google.com/workspace/calendar/api/v3/reference/events).
- Offer response scopes for a single occurrence or whole series where supported. Do not offer “following” RSVP, which would require organizer-style splitting.
- Keep organizer-controlled fields read-only for invitees.

**Exit:** responses propagate correctly without changing other guests or organizer-owned fields.

**Handoff:** `docs/handoffs/google-follow-on-06-invitation-responses.md`.

## Verification and handoff requirements

For every phase:

- Run relevant Rust/domain/SQLite/CLI regressions, generated-contract checks, production build, Clippy, formatting, and browser interaction/accessibility checks. Keep builds separate from running Vite browser tests.
- Test independent database connections, restart, lost responses, stale reviews, permission/authentication changes, and late-worker publication. Exercise migrations against existing databases containing pending work.
- Add disposable live acceptance scenarios for newly enabled behavior. Existing confirmed validation remains accepted; new features receive their own evidence.
- Update the roadmap and integration guide to describe precisely what that phase enables and still rejects.

Each phase ends with its separately named committed handoff containing:

- Phase status and commit; implemented behavior and accepted architectural decisions.
- Schema/API changes and migration compatibility.
- Exact checks run, results, evidence locations, and unresolved acceptance.
- Recovery behavior for interrupted or partially applied operations.
- Remaining restrictions, next phase, and its prerequisites.

Phase 01’s handoff must carry forward the agreed choices above. Subsequent handoffs must describe the actual shipped state rather than copying planned guarantees. Source-zone/DST write expansion remains a separate later phase requiring equivalence evidence.

Keep this plan and the `google-follow-on-*` handoffs separate from all initial-integration artifacts. Link to earlier handoffs as historical prerequisites; do not rename them or change their recorded results. If scope or an architectural decision changes during implementation, update this plan and record the reason and superseded decision in that phase’s handoff.
