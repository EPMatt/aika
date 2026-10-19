# Roadmap

## Work in progress

- Existing Google connection, incoming sync, and ordinary-event two-way editing received user-confirmed live validation on 2026-10-19. Platform/client/revision evidence was not supplied; installed-platform acceptance and new-feature evidence remain separate.
- Complete alpha installation/replacement/true-upgrade evidence, trusted native input, high DPI, keyboard and screen-reader checks across Linux, Windows, and both macOS architectures. See the [acceptance checklist](releases.md#acceptance-checklist).
- Finish recovery UX and evidence for credentials, permissions, unresolved pending work, and account removal. Retry/conflict controls, checked account-wide discard-and-disconnect, and checked per-calendar discard-and-remove are implemented. Follow-on phase 04 implements checked full account removal, lifecycle fencing and durable credential/cache cleanup; see the [phase 04 handoff](handoffs/google-follow-on-04-account-removal.md). Installed credential cleanup and live uncertain-write acceptance remain unverified.

- Google follow-on 01 supplies schema v6 operation groups, ordered steps, persisted aggregate snapshots/identities, group-aware recovery and scope restrictions. Phase 02 adds compatible recurring creation, occurrence edit/cancel/restore, series metadata edits/deletion, explicit exception preservation and checked compensation. Phase 03 adds exact following splits/deletion, reset consent, partial progress, checked replanning and compensation after forward reconciliation. Structural whole-series schedule changes remain restricted. See the [phase 01 handoff](handoffs/google-follow-on-01-durable-recurrence.md), [phase 02 handoff](handoffs/google-follow-on-02-recurring-writes.md), and [phase 03 handoff](handoffs/google-follow-on-03-following-recovery.md).

- Google follow-on 05 enables organizer-owned meeting creation, guest/optional-status edits, cancellation and compatible recurrence, with durable everyone/external-only/none notification policy. Responses and conference links are displayed; unsafe conference copying is blocked. See the [phase 05 handoff](handoffs/google-follow-on-05-meeting-organization.md). Live invitation delivery and propagation acceptance remain outstanding.

## Future plans

The [Google integration follow-on plan](plans/google-integration-follow-on-2026-10-19.md) defines a separate six-phase sequence and handoff names for the remaining Google features. Its [planning handoff](handoffs/google-follow-on-00-planning.md) records user decisions and the confirmation of existing live validation.

- Regional-zone/DST write expansion requires separate later equivalence evidence.
- Google follow-on 06 adds checked accept/tentative/decline on the authenticated attendee’s own permitted calendar copy, with occurrence/series scopes, durable recovery and participant-only undo. See the [phase 06 handoff](handoffs/google-follow-on-06-invitation-responses.md). Live RSVP propagation remains unverified; delegated copies, unproven aliases, unsupported recurrence and special Google event types remain restricted.
- Search, reminders, import/export, and backup/restore remain unimplemented product possibilities, without a committed schedule.

Additional providers can use the [shared adapter boundary](integrations/google-calendar.md#provider-extension-guide). They are not current integrations.
