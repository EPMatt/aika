# aika implementation plan

## Current work and next increment

The desktop currently supports multiple local calendars, common recurrence, and a standalone CLI. The provider registry and OS credential service exist; production still registers only the local provider. The original v0 phases below are historical milestones, not a description of today's complete feature set.

The next increment is [extensible connected calendars and Google integration](integrations/google-calendar.md). Its phases use a separate Google phase numbering scheme. **Google phases 0–1 are delivered: specification plus shared repository/account/capability foundations. Phases 2–6 remain planned.** Start with the [Google phase 1 handoff](handoffs/google-phase-1.md) and the [connected-calendar architecture decisions](architecture.md#connected-calendar-decisions-google-integration).

This specification supersedes the earlier instruction to defer all Google work until alpha acceptance. It does not complete or waive outstanding platform, installer, accessibility, or user-acceptance checks; their evidence remains tracked in [alpha verification](alpha/verification.md).

## Original v0 product (historical)

Personal desktop alpha for Windows 11 x64, macOS 14+ Intel/Apple Silicon, and Linux x64 (Ubuntu 24.04+, Arch, and Fedora/Fedora-based distributions with compatible runtimes). Tauri 2 + React/TypeScript, Rust backend. Custom calendar views and a distinct warm-neutral/sage visual identity. Spacious, minimal, premium UI. Light, dark, and system appearance from v0, controlled only from Settings. The sidebar footer contains only Settings. Use functional copy throughout.

One local calendar; month/week/day; title, start/end, timed/all-day, optional location and plain-text notes. Anchored editor, collapsible sidebar, mini calendar, drag manipulation with keyboard alternatives, and undo. English UI with OS/webview regional formatting and week start. Follow the device zone: retain timed instants and all-day dates.

Exclude recurrence, reminders, search, backup/restore, ICS exchange, multiple user-created calendars, connected accounts, application servers, analytics, auto-updates, and paid signing from v0. Private alpha installers use ad-hoc macOS signing. App name aika; identifier `dev.projectcal.desktop`.

## Architecture

Rust owns validation, persistence, provider dispatch, and credentials. SQLite with transactional versioned migrations; no renderer SQL access. Rust contracts generate TypeScript types. Domain/calendar UI uses normalized events and capabilities, not provider names. Test adapters prove the boundary. Later Google integration establishes shared offline caching, outbox, retries, and conflict handling; later providers supply adapter code and registration/config. Recurrence and invitations remain separate shared capabilities.

Credential service uses macOS Keychain, Windows Credential Store, and Linux Secret Service. A locked/unavailable secure store prevents connected-account access; the local calendar still works. Never persist credentials in SQLite, renderer storage, or logs. Calendar data itself uses OS account/file protections. Hardware-backed storage is platform-dependent.

## Phases and exit gates

0. **Foundation:** scaffold, toolchain pinning, typed contracts, least-privilege shell, reproducible local build, Linux CI artifact workflow on Ubuntu 24.04 (Windows/macOS entries remain commented out). Exit: launchable shell and target build evidence; report any unexecuted external checks explicitly.
1. **Design:** interactive sample-data prototype, three views, editor, two themes, empty/dense/overlap/multi-day layouts, responsive desktop sizing, focus and reduced motion. Exit: user accepts visual direction and interaction flows. **User gate.**
2. **Local domain:** default calendar, Rust validation, SQLite persistence/migrations, preferences, session undo, IPC. Handle DST gaps/ambiguity explicitly and keep failed editor input. Exit: CRUD/restart/undo/range/time-zone integration tests pass.
3. **Calendar experience:** bind UI to IPC; complete navigation, editing, drag creation/moving/resizing, keyboard alternatives, overflow, all-day spans, now indicator. Exit: essential actions work with mouse and keyboard, persist, and recover from failures.
4. **Provider proof/quality:** registry, backend credential service, second test adapter, read-only/calendar identity checks, cross-platform desktop automation and manual checks. Exit: no critical data loss, accessibility, or interaction defects; no production test plugins.
5. **Private alpha:** Windows installer, macOS DMGs, Linux AppImage/deb/RPM; install/update/restart verification, artifacts/checksums/notes. Exit: user accepts alpha. **User gate.**

## Handoff procedure

Record each original v0 phase in `docs/handoffs/phase-N.md`: source revision or uncommitted state, delivered behavior, exact commands/results, screenshots/artifact paths, unresolved defects, and next-phase starting instructions. Connected-calendar phases use `docs/handoffs/google-phase-N.md` and the [integration handoff procedure](integrations/google-calendar.md#handoff-procedure). The receiving engineer reads the plan and architecture, checks the working tree, and reproduces relevant checks before continuing. Engineering phases proceed when their gates pass. The original design and final-alpha gates still require user review for those milestones.

## Required validation

CRUD/undo/restart; timed/all-day/overnight/multi-day ranges; leap days and year boundaries; DST gaps/repeated hours/device-zone changes; overlaps/long titles/dense cells; keyboard/focus/screen-reader/reduced-motion checks; persistence and migration failures; provider isolation/read-only behavior; credential-store failures; both themes, regional formatting, high DPI, minimum 900×600 window; target OS installers and upgrades.

## Defaults

System appearance; month on first launch; remember last view/sidebar/appearance in phase 2. New timed events last one hour; timed gestures snap to 15 minutes. Month gestures move by local date; time-grid gestures operate on time. Event type changes in the editor. All-day ranges use an exclusive end date internally. Connected calendars follow the separate [Google integration specification](integrations/google-calendar.md); its shared backend foundation is delivered; production Google connectivity remains planned.

## Implementation status after phase 3

Phase 3 was delivered in `433172a`: native IPC remains the production data path, and gestures reuse normalized event CRUD and session undo. All-day spans and collision lanes are renderer-derived layout. Gesture wall times are resolved before constructing instants; failed writes retain committed data. See `docs/architecture.md` for the decision rationale and `docs/handoffs/phase-3.md` for checks and remaining platform limits.

Phase 4 is next. Establish provider dispatch and credential boundaries behind the existing service, and verify native input/accessibility on the supported platforms. Preserve the distinction between renderer/mocked-IPC tests, actual SQLite tests, and embedded native automation when assessing the exit gate.

## Implementation status after phase 4

Provider dispatch now sits behind the unchanged IPC service. Production registers only the local SQLite adapter; a second adapter is confined to Rust tests. Read-only capabilities and calendar identity are enforced in the backend and reflected in the same renderer. Credentials use persistent OS stores without fallback or renderer exposure. Embedded native automation is confined to an explicit debug test feature; Linux CI includes it without re-enabling Windows/macOS jobs. See `docs/handoffs/phase-4.md` and `docs/quality.md` for the results and the remaining manual/platform gate. Phase 5 packaging can be prepared, but cross-platform/manual quality evidence is still required for alpha acceptance.

## Implementation status after phase 5 preparation

Private alpha build/collection tooling now prepares native release installers, SHA-256 checksums, provenance, and release notes; Linux CI uses the same path. Windows/macOS jobs remain disabled. Install/update/manual platform evidence and user alpha acceptance are tracked in `docs/alpha/verification.md` and `docs/handoffs/phase-5.md`; preparation does not mark those gates passed.
