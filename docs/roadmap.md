# Roadmap

## Work in progress

- Validate experimental Google connection, incoming sync, and ordinary-event two-way edits against disposable live accounts and installed platform packages. Automated implementation is present; acceptance remains incomplete.
- Complete alpha installation/replacement/true-upgrade evidence, trusted native input, high DPI, keyboard and screen-reader checks across Linux, Windows, and both macOS architectures. See the [acceptance checklist](releases.md#acceptance-checklist).
- Finish recovery UX and evidence for credentials, permissions, unresolved pending work, and account removal. Current retry/conflict controls exist; pending-work discard/removal workflows need further design and verification.

## Future plans

- Compatible Google recurring-event writes with occurrence/series/following scopes, proven recurrence equivalence, and durable recovery for interrupted multi-request splits.
- Invitation/attendee editing and notification choices; unsupported Google event types remain outside current editing capabilities.
- Search, reminders, import/export, and backup/restore remain unimplemented product possibilities, without a committed schedule.

Additional providers can use the [shared adapter boundary](integrations/google-calendar.md#provider-extension-guide). They are not current integrations.
