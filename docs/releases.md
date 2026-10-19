# Releases and installation

Push a tag named `vMAJOR.MINOR.PATCH` to publish a GitHub release automatically. The tagged commit must be contained in `main`; an older commit in main's history is allowed. The tagged commit must include `.github/workflows/release.yml`, because GitHub loads tag-push workflows from that revision. Lightweight and annotated tags both work. Leading zeros, prerelease suffixes, and build metadata are rejected.

## Prepare a release

1. Update `package.json`, `src-tauri/tauri.conf.json`, and the package versions in `src-tauri/Cargo.toml`, `src-tauri/core/Cargo.toml`, `src-tauri/domain/Cargo.toml`, and `src-tauri/cli/Cargo.toml` to the same numeric version, for example `0.1.0`.
2. Refresh the Rust lockfile with `pnpm rust check --manifest-path src-tauri/Cargo.toml --workspace`. Commit the version changes and `src-tauri/Cargo.lock` through the normal review process and merge into `main`.
3. After checks pass, tag the intended commit and push the tag:

```sh
git switch main
git pull --ff-only
git tag -a v0.1.0 -m 'aika 0.1.0'
git push origin v0.1.0
```

Tags alone do not update application versions. A mismatch fails validation before builds start. Push release tags individually: GitHub does not emit tag push events when more than three tags are pushed together. Tags pushed by another Actions workflow using `GITHUB_TOKEN` do not trigger this workflow; push them with a user credential or GitHub App token instead.

The current `0.1.0-alpha.1` version cannot be released through this workflow until a numeric version is committed. Private-alpha candidate builds remain available through `pnpm alpha:build`.

## Packages and checks

| Target                                 | Release files         |
| -------------------------------------- | --------------------- |
| Linux x64, Ubuntu 24.04 build baseline | deb, RPM, AppImage    |
| Windows x64                            | Current-user NSIS exe |
| macOS 14+ Apple Silicon                | DMG                   |
| macOS 14+ Intel                        | DMG                   |

Every desktop package contains `aika-cli`, built for the same target and version. Windows/Linux packages remain unsigned; macOS bundles are ad-hoc signed and are not notarized. CLI installation does not change PATH on Windows/macOS.

Release builds use frozen pnpm and locked Cargo dependencies, with no native-test feature. Frontend formatting, build, unit, packaging, and browser tests and each target's Rust formatting, Clippy, and workspace tests must pass. Package inspection checks the desktop and CLI architectures, executable permissions where applicable, and the bundled CLI version. Linux inspection extracts every format and checks RPM metadata, runtime requirements, launcher, and icons; macOS checks the mounted DMG and code signature; Windows installs and uninstalls in a disposable CI directory. These checks do not replace manual desktop installation, upgrade, or accessibility acceptance.

Assets use `aika-<version>-<target>` filenames. Each target includes a provenance manifest recording source revision, signing status, bundled CLI version, installer sizes, and SHA-256 hashes. The release also includes a combined `SHA256SUMS`. Download assets into one directory and check them on Linux with `sha256sum --check SHA256SUMS`, or on macOS with `shasum -a 256 --check SHA256SUMS`.

To build packages locally from a clean, tagged checkout with `origin/main` fetched:

```sh
pnpm install --frozen-lockfile
git fetch origin main --tags
pnpm release:build v0.1.0 x86_64-unknown-linux-gnu
```

Use the Windows or macOS target from the table in `scripts/alpha.mjs` on the corresponding native OS. Outputs are collected under `artifacts/release-<target>/`; move an existing output aside before rebuilding. Local builds do not publish.

## Google configuration in packages

Native packages embed optional public Desktop client settings. Both workflows currently read repository secrets `AIKA_GOOGLE_CLIENT_ID` and `AIKA_GOOGLE_CLIENT_SECRET` only for package builds. Unconfigured packages remain valid with Google disabled. See [Google configuration](integrations/google-calendar.md#configuration) for local `.env`, build/runtime precedence, and credential boundaries.

## Use the bundled CLI

- **Linux deb/RPM:** run `aika-cli --help`; the package installs it under `/usr/bin`.
- **Linux AppImage:** extract the downloaded AppImage with `./aika-<version>-x86_64-unknown-linux-gnu.AppImage --appimage-extract`, then run `./squashfs-root/usr/bin/aika-cli --help`. Set executable permissions on the AppImage first if needed.
- **macOS:** install `aika.app` into Applications, then run `/Applications/aika.app/Contents/MacOS/aika-cli --help`.
- **Windows PowerShell:** run `& "$env:LOCALAPPDATA\aika\aika-cli.exe" --help` for the default current-user install location. If a different installation directory was selected, use that path.

The CLI uses the same local database as the desktop app and does not need the desktop running. See [CLI usage](cli.md) for commands and database options. You can add the installed binary's directory to PATH yourself.

## Publication and retries

Only the final publishing job has repository write permission. It requires all builds and checks to succeed, rechecks the remote tag and main ancestry, verifies the exact asset set, then creates a draft with GitHub-generated release notes. It uploads the installers, manifests, and checksums and downloads them again to verify their contents before publishing.

A build failure creates no release. An upload or verification failure leaves a draft. Fix the cause and rerun the failed jobs in the Actions run; the publisher replaces draft assets and removes obsolete files. To rebuild everything, rerun all jobs. Per-tag concurrency prevents simultaneous publishers, and the publisher refuses to overwrite a published release. Do not move release tags; publish corrections under a new version.

No credentials beyond the built-in `GITHUB_TOKEN` are required. Repository or organization policy must allow the publishing job's `contents: write` permission. Do not enable public publication by removing a failed verification gate.

Workflow behavior follows GitHub's [tag push events](https://docs.github.com/en/actions/reference/workflows-and-actions/events-that-trigger-workflows#push), [token-trigger restrictions](https://docs.github.com/en/actions/how-tos/write-workflows/choose-when-workflows-run/trigger-a-workflow#triggering-a-workflow-from-a-workflow), and [generated release notes](https://cli.github.com/manual/gh_release_create). CLI embedding uses Tauri's [external binary support](https://v2.tauri.app/develop/sidecar/).

## Private-alpha candidates

Use the [development prerequisites and checks](../CONTRIBUTING.md). Private candidates use a separate packaging path and do not include the CLI. The source notes at [`docs/alpha/RELEASE-NOTES.md`](alpha/RELEASE-NOTES.md) are required by `scripts/alpha.mjs` and copied into each candidate.

```sh
pnpm alpha:build x86_64-unknown-linux-gnu
# On Windows:
pnpm alpha:build x86_64-pc-windows-msvc
# On Apple Silicon / Intel macOS respectively:
pnpm alpha:build aarch64-apple-darwin
pnpm alpha:build x86_64-apple-darwin
```

The command only accepts a native OS target from this list. It builds release mode without additional features, clears generated bundle output to reject stale installers, requires matching package/Tauri/Cargo versions, and writes a new `artifacts/aika-<version>-<target>/` candidate with normalized filenames, SHA-256 checksums, provenance, and release notes. It refuses to overwrite an existing candidate. Move a previous candidate aside before rebuilding. A dirty revision is explicitly marked; use a clean revision for distribution. Cargo target environment overrides are rejected to keep artifact discovery unambiguous. The Rust release guard rejects the automation feature even if configuration tries to enable it. The alpha script does not publish or send artifacts. The separate [tagged release workflow](#prepare-a-release) publishes numeric release tags on main and bundles the CLI with the desktop.

Linux x86_64 candidates contain exactly one `.deb`, `.rpm`, and `.AppImage`. RPM filenames are normalized to `aika-<version>-x86_64-unknown-linux-gnu.rpm` alongside the existing installers, with sizes and hashes recorded in the manifest. The RPM includes the desktop executable, launcher, and icons; Tauri supplies GTK 3 and WebKitGTK 4.1 shared-library requirements. The standalone CLI is installed separately.

CI uses the same command and uploads the candidate directory. The active matrix includes Linux x64 on Ubuntu 24.04, Windows x64 on Windows 2025, Apple Silicon on macOS 15, and Intel on macOS 15 Intel. Each macOS architecture uses a native runner so Rust tests execute on the architecture being packaged. Native-test evidence is a separate artifact and never bundled in the candidate. Local Arch builds are host evidence, not proof of distribution ABI compatibility; build Linux distribution candidates on the supported Ubuntu baseline and execute on Ubuntu, Arch, and Fedora as applicable.

## RPM inspection and Fedora installation

CI installs `rpm` and `libarchive-tools`, then checks candidate checksums, RPM metadata and runtime requirements, x86_64 executable architecture/permissions, desktop launcher, and icons before upload. To repeat the inspection locally with Python 3, `rpm`, and `bsdtar`:

```sh
python3 scripts/linux-rpm-check.py artifacts/aika-0.1.0-alpha.1-x86_64-unknown-linux-gnu
```

Local validation on October 10, 2026, on Arch/Omarchy x86_64 built all three installers. The RPM inspection script and `sha256sum --check SHA256SUMS` passed for the collected candidate. This is build/inspection evidence; hosted Ubuntu CI and Fedora installation/launch were not executed locally.

On Fedora or a Fedora-based distribution with DNF and the required runtimes, install in a disposable test environment:

```sh
sudo dnf install ./artifacts/aika-0.1.0-alpha.1-x86_64-unknown-linux-gnu/aika-0.1.0-alpha.1-x86_64-unknown-linux-gnu.rpm
```

Launch aika from the application menu or run `cal-desktop`, then perform the installation/restart checks below. The RPM is unsigned, matching other Linux candidates. Building and inspecting an RPM does not establish Fedora dependency resolution or desktop execution; those checks remain pending until run on the recorded distribution/version. Older RPM-based distributions may lack compatible WebKitGTK 4.1 or system libraries.

## Isolated Linux production smoke

With Python 3 and a desktop session, run the packaged AppImage (FUSE is not required for this path):

```sh
python3 scripts/linux-release-smoke.py artifacts/aika-0.1.0-alpha.1-x86_64-unknown-linux-gnu/aika-0.1.0-alpha.1-x86_64-unknown-linux-gnu.AppImage --appimage-extract-and-run
```

The script copies the candidate into a temporary portable installation, launches it, replaces it with the same candidate, and relaunches. It uses disposable XDG data/config/cache directories, reads SQLite without modifying it, verifies empty-calendar/default-preference restart, checks that the test-only directory is ignored and port 4445 is closed, then removes the installation/data. A listener already using port 4445 causes a failure. This is **same-version replacement**, not a true upgrade or saved-event/changed-preference CRUD verification. It does not install a deb or certify trusted input/accessibility. Do not run Python with optimization (`-O`), which disables these assertions.

## Installation

Use the matching native architecture. Windows 11 x64 uses the current-user NSIS `.exe` and requires WebView2 (default Tauri bootstrapper). On macOS 14+, open the DMG and copy aika to Applications. The app is ad-hoc signed without notarization; verify source/checksums and use the system's explicit Open Anyway flow if needed, according to local policy. Do not remove quarantine indiscriminately.

On Ubuntu 24.04+, install deb with `sudo apt install ./<filename>.deb`. On Fedora use `sudo dnf install ./<filename>.rpm`. For AppImage, `chmod +x <filename>.AppImage` then run it; `--appimage-extract-and-run` is an alternative where FUSE is unavailable. Arch uses AppImage rather than deb. Linux requires a desktop session and compatible GTK 3 / WebKitGTK 4.1 libraries. Launch from the application menu or run `cal-desktop` for deb/RPM installs.

Quit before replacement. Replace the AppImage or macOS app, or run the newer installer with the same OS account. Keep identifier `dev.projectcal.desktop` to retain data. There is no automatic updater or supported downgrade path; never downgrade a migrated database. Record actual uninstall behavior rather than assuming data retention.

## Acceptance checklist

Use a disposable OS account for production installers. Production ignores `AIKA_TEST_DATA_DIR`; the debug suite does not test an installed release. Record source revision, version/hash, OS/webview, architecture, display scale, installation output, results, and evidence paths.

1. Verify every checksum, executable architecture, metadata, version, launcher/icons, permissions, and signing status. On macOS use `codesign --verify --deep --strict /Applications/aika.app`. Confirm current-user installation on Windows. Confirm no native-test listener on port 4445 or fixture data in production.
2. Install and launch with a fresh database. Verify Personal, no events/accounts, then local timed/overnight/all-day/multi-day CRUD, calendar creation/settings/visibility/default/deletion, recurrence/scoped edits, and session undo. Restart to check durable events/preferences and cleared undo history.
3. Quit, replace, and repeat read/edit/restart checks. Record same-version replacement separately from a true upgrade, which requires previous/new versions and migration evidence. Confirm failure never resets data. Record actual removal behavior.
4. Use trusted native input to create/move/resize, select ranges, and cancel gestures. Check DST gaps/folds and source/device-zone changes; failed gestures must retain saved data. Check physical focus refresh and stale editor/CLI conflicts.
5. Complete essential actions by keyboard. Verify focus restoration/containment, overflow, date/time fields, read-only states, event-range announcements, and Narrator/VoiceOver/Orca behavior. Test both themes/system changes, reduced motion, regional formats/week start, dense/long events, 900×600/narrow tiling, and 100%/200% scale.
6. Test OS-store read/write/delete and unavailable/locked cases. For Google-configured packages, verify system-browser/callback, actual consent, refresh/reconnection, incoming/outgoing/offline/restart behavior, permission loss, conflicts, retries, and undo. Keep local/cache behavior usable during failures.
7. Present artifacts, hashes, notes, and recorded target evidence for final user acceptance. Keep failures and unexecuted checks visible; final alpha acceptance remains pending until explicitly recorded.

## Unresolved verification

These are retained evidence boundaries from earlier work, not results for the current revision. Linux native debug automation and injected provider fixtures have historical passing evidence. An Arch AppImage previously passed portable startup, empty-database/default-preference restart, and same-version replacement; Linux package inspection also has historical evidence. These do not establish current installed CRUD, true upgrades, Ubuntu ABI compatibility, Fedora dependency resolution, or manual input/accessibility.

| Target                           | Outstanding evidence                                                                                                       |
| -------------------------------- | -------------------------------------------------------------------------------------------------------------------------- |
| Windows 11 x64 NSIS              | Manual clean install/launch, replacement/true upgrade, restart/data/preferences, trusted input/accessibility               |
| macOS 14+ Apple Silicon DMG      | Manual install/launch, replacement/true upgrade, restart/data/preferences, trusted input/accessibility                     |
| macOS 14+ Intel DMG              | Manual install/launch, replacement/true upgrade, restart/data/preferences, trusted input/accessibility                     |
| Ubuntu 24.04+ x64 deb / AppImage | Installed runtime, replacement/true upgrade, saved-data restart, trusted input/accessibility                               |
| Fedora x64 RPM / AppImage        | Dependency resolution/install/launch, replacement/true upgrade, saved-data restart, trusted input/accessibility            |
| Arch x64 AppImage                | Current-candidate CRUD/preferences/restart, true upgrade, trusted input/accessibility; previous portable smoke was partial |

Hosted workflow results must be observed before claiming CI success; active jobs are not evidence of successful execution. Package inspection and installer subprocess checks also do not replace manual acceptance.

Google live consent/token exchange, refresh/discovery, propagation, conditional-write/permission behavior, expired cursors, recovery/undo, and installed browser/store behavior remain pending. The [Linux credential investigation](integrations/linux-credential-store.md#investigation-evidence-2026-10-19) records a real-store roundtrip and live sign-in-page startup on one host with a redirected launcher; no account consent or sync was completed, and default-browser dispatch/Fedora AppImage behavior was not established. That partial evidence does not complete the live-provider gate.
