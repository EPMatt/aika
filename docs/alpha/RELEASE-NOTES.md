# aika 0.1.0-alpha.1 — private candidate

Acceptance pending. This candidate is for private evaluation, with no public release or automatic updater.

Multiple local calendars with month/week/day views, timed and all-day events, location and plain-text notes. Create, edit, move, resize, delete, and undo within the current session. Events, calendar names/descriptions/colors, calendar visibility/default selection, and appearance/view/sidebar preferences persist after restart. Create calendars with the sidebar +; use each calendar’s … menu for settings, default selection, or permanent deletion with its events. The editor supports moving events between local calendars. Light, dark, and system appearance; dates and times follow device settings. DST gaps are rejected and repeated times require an offset choice.

No connected accounts, sync, recurrence, reminders, search, import/export, backup/restore, analytics, or server. Calendar data is stored in local SQLite under the OS account's filesystem protections; it is not application-encrypted. Undo ends when the application exits.

## Requirements and installation

- Windows 11 x64: unsigned, current-user NSIS installer. Run the `.exe`; Windows may show an unsigned-publisher warning. WebView2 is required (the installer uses Tauri's default bootstrapper behavior).
- macOS 14+ Intel or Apple Silicon: use the matching DMG, open it, and drag aika to Applications. The app is ad-hoc signed and not notarized. Gatekeeper may block a downloaded candidate; follow your organization's policy and verify the source and checksum before using the system's explicit Open Anyway option. No blanket quarantine removal is required by these instructions.
- Linux x64: Ubuntu 24.04+ or Arch. For Debian/Ubuntu, install the `.deb` using `sudo apt install ./<filename>.deb`. For AppImage, `chmod +x <filename>.AppImage` and run it. FUSE availability depends on the host; `--appimage-extract-and-run` is an alternative. Arch does not install `.deb` packages. A desktop session and compatible WebKitGTK runtime are required.

Quit aika before replacing/upgrading it. Install the newer candidate using the same method and OS account, then restart. AppImage users replace the file; macOS users replace the app in Applications. Keep the application identifier unchanged to retain data. There is no automatic update or supported downgrade path; do not downgrade a database after a future schema migration.

## Verify the download

The candidate directory includes `manifest.json` (version, source revision, target, signing status, sizes, and installer hashes) and `SHA256SUMS` (installers, manifest, and these notes). On Linux run `sha256sum -c SHA256SUMS`; on macOS run `shasum -a 256 -c SHA256SUMS`. On Windows compare `Get-FileHash <installer> -Algorithm SHA256` with its entry. Checksums detect altered bytes; they do not authenticate the publisher. Obtain the candidate and expected hashes through a trusted private channel.

## Known verification limits

Windows/macOS installer execution and the complete manual platform/accessibility matrix remain pending. Automated Linux webview CRUD/restart checks establish IPC and persistence, not trusted pointer gestures or screen-reader usability. Packaging, install/update checks, and user acceptance must be recorded separately; the presence of an installer is not acceptance evidence. See `docs/alpha/verification.md` and `docs/handoffs/phase-5.md` in the source checkout.
