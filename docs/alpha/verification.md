# Private alpha build and acceptance

Build from a reviewed checkout with Node 24, pnpm 12.10.1, Rust 1.99.0, locked dependencies, and native Tauri prerequisites. Run the checks in `docs/quality.md`, plus `pnpm test:packaging`, before building. Local tool wrappers support `.tools`; they do not change global configuration.

```sh
pnpm alpha:build x86_64-unknown-linux-gnu
# On Windows:
pnpm alpha:build x86_64-pc-windows-msvc
# On Apple Silicon / Intel macOS respectively:
pnpm alpha:build aarch64-apple-darwin
pnpm alpha:build x86_64-apple-darwin
```

The command only accepts a native OS target from this list. It builds release mode without additional features, clears generated bundle output to reject stale installers, requires matching package/Tauri/Cargo versions, and writes a new `artifacts/aika-<version>-<target>/` candidate with normalized filenames, SHA-256 checksums, provenance, and release notes. It refuses to overwrite an existing candidate. Move a previous candidate aside before rebuilding. A dirty revision is explicitly marked; use a clean revision for distribution. Cargo target environment overrides are rejected to keep artifact discovery unambiguous. The Rust release guard rejects the automation feature even if configuration tries to enable it. No script publishes or sends artifacts.

CI uses the same command and uploads the candidate directory. Only Linux is active; Windows/macOS matrix entries stay commented out. Native-test evidence is a separate artifact and never bundled in the candidate. Local Arch builds are host evidence, not proof of Ubuntu ABI compatibility; build Linux distribution candidates on the supported Ubuntu baseline and execute on both Ubuntu and Arch.

## Isolated Linux production smoke

With Python 3 and a desktop session, run the packaged AppImage (FUSE is not required for this path):

```sh
python3 scripts/linux-release-smoke.py artifacts/aika-0.1.0-alpha.1-x86_64-unknown-linux-gnu/aika-0.1.0-alpha.1-x86_64-unknown-linux-gnu.AppImage --appimage-extract-and-run
```

The script copies the candidate into a temporary portable installation, launches it, replaces it with the same candidate, and relaunches. It uses disposable XDG data/config/cache directories, reads SQLite without modifying it, verifies empty-calendar/default-preference restart, checks that the test-only directory is ignored and port 4445 is closed, then removes the installation/data. A listener already using port 4445 causes a failure. This is **same-version replacement**, not a true upgrade or saved-event/changed-preference CRUD verification. It does not install a deb or certify trusted input/accessibility. Do not run Python with optimization (`-O`), which disables these assertions.

## Installation and upgrade record

Use a disposable OS account for production installers; never point release checks at a real calendar. Production ignores `AIKA_TEST_DATA_DIR`. The embedded debug suite does not test an installed release. Record version/hash, OS/webview, architecture, display scale, package manager/installer output, steps, results, and screenshots for each row.

| Target                      | Clean install/launch                    | Replacement/update                        | Restart/data/preferences                     | Trusted input/accessibility | Status  |
| --------------------------- | --------------------------------------- | ----------------------------------------- | -------------------------------------------- | --------------------------- | ------- |
| Windows 11 x64 NSIS         | Pending                                 | Pending                                   | Pending                                      | Pending                     | Pending |
| macOS 14+ Apple Silicon DMG | Pending                                 | Pending                                   | Pending                                      | Pending                     | Pending |
| macOS 14+ Intel DMG         | Pending                                 | Pending                                   | Pending                                      | Pending                     | Pending |
| Ubuntu 24.04+ x64 deb       | Pending                                 | Pending                                   | Pending                                      | Pending                     | Pending |
| Ubuntu 24.04+ x64 AppImage  | Pending                                 | Pending                                   | Pending                                      | Pending                     | Pending |
| Arch x64 AppImage           | Portable extract-and-run startup passed | Same-version passed; true upgrade pending | Empty database/defaults passed; CRUD pending | Pending                     | Partial |

1. Check every candidate checksum. Inspect package metadata, executable architecture, app identifier/version, icons and shortcuts. On macOS verify ad-hoc signing with `codesign --verify --deep --strict /Applications/aika.app`; record that notarization is absent. On Windows verify current-user install and the displayed version. On Linux inspect deb dependencies/contents and executable permissions.
2. Install and launch in the disposable account. Confirm an empty Personal calendar, no fixtures, no test-driver listener on port 4445, and no connected accounts. Create timed/overnight and all-day/multi-day events; edit location/notes; change appearance/view/sidebar. Record a deletion and successful undo. Quit and restart; verify saved events/preferences and that undo is session-only.
3. Quit, replace the installed candidate, and repeat launch/read/edit/restart checks. For this first alpha, reinstall the same candidate to verify replacement mechanics; label it **same-version replacement**, not a version upgrade. A true upgrade requires separately built previous/new versions and must remain pending until both exist. Retain the identifier and disposable account data. Verify installer removal preserves data or record actual behavior; do not promise platform uninstall behavior without execution.
4. Run the full manual matrix in `docs/quality.md`, including keyboard, trusted native drag/resize, assistive technology, DST/device-zone cases, both themes, and high DPI. Keep failed checks and unexecuted checks visible.
5. Provide candidate artifacts, hashes, release notes, and this completed evidence to the user. Final alpha acceptance is a user gate; record their decision before calling the phase accepted. Platform/tooling limitations do not waive that gate.
