# Roadmap

## Work in progress

- Existing Google connection, incoming sync, and ordinary-event two-way editing received user-confirmed live validation on 2026-10-19. Platform/client/revision evidence was not supplied; installed-platform acceptance and new-feature evidence remain separate.
- Complete alpha installation/replacement/true-upgrade evidence, trusted native input, high DPI, keyboard and screen-reader checks across Linux, Windows, and both macOS architectures. See the [acceptance checklist](releases.md#acceptance-checklist).
- Finish recovery UX and evidence for credentials, permissions, unresolved pending work, and account removal. Retry/conflict controls, checked account-wide discard-and-disconnect, and checked per-calendar discard-and-remove are implemented. Full account removal is designed in follow-on phase 04 and remains unimplemented. Installed credential cleanup and live uncertain-write acceptance remain unverified.

- Google follow-on 01 supplies schema v6 operation groups, ordered steps, persisted aggregate snapshots/identities, group-aware recovery and scope restrictions. Phase 02 adds compatible recurring creation, occurrence edit/cancel/restore, series metadata edits/deletion, explicit exception preservation and checked compensation. Following, structural schedule changes, attempted multi-step undo and combined conflict replanning remain restricted. See the [phase 01 handoff](handoffs/google-follow-on-01-durable-recurrence.md) and [phase 02 handoff](handoffs/google-follow-on-02-recurring-writes.md).

## Future plans

The [Google integration follow-on plan](plans/google-integration-follow-on-2026-10-19.md) defines a separate six-phase sequence and handoff names for the remaining Google features. Its [planning handoff](handoffs/google-follow-on-00-planning.md) records user decisions and the confirmation of existing live validation.

- Google following-scope splits with exact recurrence equivalence, durable interruption recovery, partial-result UI, and checked combined conflict replanning. Regional-zone/DST write expansion requires separate later equivalence evidence.
- Invitation/attendee editing and notification choices; unsupported Google event types remain outside current editing capabilities.
- Search, reminders, import/export, and backup/restore remain unimplemented product possibilities, without a committed schedule.

Additional providers can use the [shared adapter boundary](integrations/google-calendar.md#provider-extension-guide). They are not current integrations.
