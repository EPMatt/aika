# Google per-calendar recovery handoff

Implemented the next unfinished recovery item in the [roadmap](../roadmap.md), following the [account recovery handoff](google-account-recovery.md): **Calendar options → Remove from aika** now supports checked per-calendar discard and removal. Full account deletion and recurring-event writes remain separate future work.

The removal dialog reviews only the chosen calendar’s pending/conflict/attempted counts. Unresolved work requires the **Discard this calendar’s local changes** checkbox and **Discard and remove**. A clean review uses ordinary **Remove from aika** confirmation; if work arrives afterward, the backend rejects the old review and the renderer shows fresh counts with confirmation reset. Cancellation retains intent. The dialog warns that already-sent requests may still take effect in Google.

`CalendarRemovalReview` is defined in Rust with regenerated TypeScript, two native IPC commands, and typed client methods. Its opaque digest includes calendar metadata, account connection/epoch, scoped outbox/effective event data, conflicts, and incoming cache. Confirmation compares the full review inside an immediate SQLite transaction. New edits, attempts/retries, incoming changes, permission/selection changes, and authentication changes invalidate relevant reviews; account epoch changes can conservatively invalidate reviews of neighboring calendars too.

Removal retires only this calendar’s unresolved operations while keeping durable dependency identities, clears its conflicts/effective events/exceptions/cursor/coverage, excludes it, disables writes, repairs the default, and removes affected session undo. The selection trigger increments the account epoch so old incoming/outgoing workers cannot publish. Other calendars retain their intent, cache, undo, account state, and credentials; the next worker resumes their synchronization. Stable calendar/remote identities, presentation settings, and incoming shadows remain. Ordinary discovery respects the exclusion, while explicit re-inclusion through Manage account starts a full refresh without replaying discarded operations. This flow needs no provider HTTP request, credential-store call, or schema migration. It also works when authentication or calendar access is unavailable. It does not revoke Google access or delete a remote calendar.

Ordinary calendar deletion and discovery-selection exclusion now explicitly reject standalone conflicts in addition to unresolved outbox work. Account recovery copy no longer says reconnecting is required before removing individual calendars.

Verification on the Linux development host:

- Rust workspace tests passed: 96 core, 16 CLI, 11 domain; two opt-in core tests remain ignored. OAuth callback tests required loopback access outside the restricted sandbox. After adding authentication/permission-loss assertions, the two focused calendar-discard tests passed again.
- Workspace Clippy passed with warnings treated as errors after the final backend test additions. Rust formatting and whitespace checks passed.
- Production TypeScript/Vite build, generated type export, all 26 Vitest tests, and repository Prettier checks passed.
- All 12 Playwright tests across Google connection/sync/recovery and calendar IPC passed on the final run. New tests cover both themes, cancellation/focus, a clean review becoming dirty, stale intent with renewed confirmation, retained neighboring calendars/account, and axe audits. An earlier run had an initial page-load timeout and a dialog-detachment timeout while a concurrent build regenerated Vite-watched assets; separating build and browser runs produced the clean result.
- Backend regressions cover separate SQLite connections, forged/stale reviews, standalone conflicts, authentication/permission loss, incoming/outgoing publication fences, local creates/pending deletes, default fallback, retained neighboring intent/undo, restart, discovery exclusion, stable re-inclusion, and retired work not replaying.
- Ignored browser captures: `test-results/google-calendar-removal-light.png` and `test-results/google-calendar-removal-dark.png`. The dark capture was visually inspected.

Remaining acceptance and follow-up:

- Exercise removal/re-inclusion with disposable live Google calendars, especially writes accepted remotely whose responses were lost. Local retirement cannot retract an already-sent request, so that change may appear after re-inclusion.
- Verify installed native behavior and real OS credential cleanup separately. This phase leaves credentials untouched; browser IPC fixtures do not establish live-provider or installed-platform acceptance.
- Design full account deletion separately, including cache/identity retention and uncertain-write warnings. Existing account disconnect retains the account and cache.
- Compatible recurring-event writes and durable multi-request split recovery remain future phases under ADR G07. Attendee/notification editing remains unsupported.
