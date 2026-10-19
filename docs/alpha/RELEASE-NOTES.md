# aika 0.1.0-alpha.1 — private candidate

Private evaluation candidate; acceptance remains pending. No automatic updater.

## Included

- Month/week/day views, multiple local calendars, visibility/default selection, and light/dark/system appearance.
- Timed and all-day events with location/plain-text notes; drag creation, moves, resizing, deletion, and session undo. Local events, calendar settings, and appearance/view/sidebar preferences persist after restart.
- Local daily/weekly/monthly/yearly recurrence, preview, and occurrence/series/following edits. DST gap input is rejected; repeated wall times require an offset. All-day ends are exclusive.
- A standalone CLI using the shared repository. Private-alpha installers require separate CLI installation; numeric tagged-release packages bundle it.
- **Experimental Google Calendar:** backend account connection/selection, incoming cache and offline reading, ordinary-event two-way edits, durable pending changes/retries, explicit conflicts, and compensating session undo. Google requires a configured Desktop OAuth client and working OS credential store.

## Current boundaries

Google meetings, recurring events, special event types, and remote moves remain outside supported editing. Unsupported recurrence uses bounded cached projections; missing offline ranges are marked incomplete. Live Google and installed-platform acceptance is unfinished. Pending work blocks ordinary account disconnect/calendar exclusion; explicit checked discard-and-disconnect and per-calendar discard-and-remove offer recovery. Already-sent Google requests may still take effect. Undo history ends when the app exits; remote pending changes/conflicts persist.

Search, reminders, import/export, backup/restore, invitation editing, analytics, and an application server are not included. Calendar data uses local SQLite under OS filesystem protections without application encryption; user Google tokens use the persistent OS store.

## Installation and checksums

Use Windows 11 x64 NSIS, the matching macOS 14+ Intel/Apple Silicon DMG, or Linux x64 deb/RPM/AppImage. Linux/Windows packages are unsigned; macOS is ad-hoc signed without notarization. Linux needs a desktop session and compatible GTK 3 / WebKitGTK 4.1. Quit before replacing the app and keep the same OS account/application identifier; downgrades are unsupported.

The candidate includes `manifest.json` with source/target/signing details and `SHA256SUMS` covering installers, manifest, and these notes. On Linux run `sha256sum -c SHA256SUMS`; on macOS run `shasum -a 256 -c SHA256SUMS`; on Windows compare `Get-FileHash <installer> -Algorithm SHA256`. Obtain artifacts and expected hashes through a trusted channel: hashes check bytes, not publisher identity.

Detailed installation commands, bundled CLI paths, packaging, and the acceptance checklist are in the source checkout's `docs/releases.md` (online: [release guide](https://github.com/EPMatt/aika/blob/main/docs/releases.md)). These notes are copied into candidate directories, so repository-relative links are intentionally avoided.

## Verification still required

Current-candidate installed launch/restart/CRUD, true upgrades, trusted native dragging, high DPI, manual keyboard/screen-reader checks, hosted workflow outcomes, and final user acceptance need recorded evidence. Windows/macOS, Ubuntu runtime, and Fedora installation behavior cannot be inferred from Linux debug automation or package inspection. Live Google consent, refresh, sync, failure recovery, and installed browser/store behavior remain incomplete; reaching a sign-in page is only partial evidence.
