# Google account recovery handoff

Implemented the next recovery step from the roadmap: **Manage account → Disconnect account** reviews unresolved work and offers explicit, checked **Discard and disconnect**. Ordinary disconnect and calendar exclusion continue to reject unresolved work.

The review contains account-scoped pending/conflict/attempted counts and an opaque revision. Confirmation compares it inside an immediate SQLite transaction, restores affected events from normalized incoming cache (including cached exceptions if Google converted an ordinary event to a series, or absence for local-only creates/deletions), retires pending operations without deleting dependency identities, clears conflicts, disables calendar writes, repairs the default calendar, and disconnects the account. Existing epoch guards reject late worker publication. Only affected undo entries are removed. The native command deletes credentials after committing; a store failure leaves the account disconnected and can be retried with ordinary disconnect.

Discard is local and cannot retract an HTTP request already sent to Google. The confirmation explains that attempted writes may still take effect and cached versions may be stale. Reconnect and synchronize before excluding individual calendars. Retired operations do not replay after restart/reconnection. No schema migration, provider HTTP change, or credential material was added to IPC.

Backend regression coverage exercises external conversion to recurrence with moved/cancelled occurrences, stale reviews across SQLite connections, late outgoing responses, local creates, pending deletes, remote deletion conflicts, cache restoration, unrelated undo, account isolation, restart, and reconnection. The renderer regression checks cancellation, explicit confirmation, stale-review rejection, credential cleanup retry, cache retention, and accessibility.

Follow-up work:

- Verify the workflow with disposable live Google calendars, especially a successful remote write whose response was lost, permission loss, and reconnect after local discard.
- Verify installed OS credential deletion failure/retry on supported platforms. Mocked IPC does not establish real credential-store behavior.
- Design direct per-calendar discard/exclusion and full account removal separately, preserving checked review and uncertain-write warnings. The current flow disconnects and retains cached calendars/account identity.
- Recurring-event writes, attendee editing, and interrupted multi-request splits remain future phases.

Sign-in/browser-launch/credential-guidance work landed in a separate preceding commit while this phase was being implemented. Recovery was also verified independently of those edits. This change includes one mechanical Clippy borrow correction in the browser launcher.

Verification: the isolated recovery snapshot passed the production TypeScript/Vite build and all seven Google connection, sync, and recovery Playwright tests (including accessibility assertions). The shared-workspace Vitest suite passed all 26 tests. The final core suite passed 93 tests with two opt-in tests ignored. Both the isolated recovery snapshot and the combined branch passed workspace Clippy with warnings treated as errors. Rust/Prettier formatting and diff whitespace checks passed. Live Google and installed-platform checks remain unperformed.
