# Phase 5 — Private alpha preparation

Status: **packaging implementation delivered; target installer execution and final alpha acceptance pending.**

## Source and scope

Base revision: `b918476`; implementation is recorded in the commit adding this handoff (`git log -1 -- docs/handoffs/phase-5.md`). Read `.ai/plans/aika.md` (the on-disk filename is lowercase), `docs/plan.md`, `docs/architecture.md`, and phase-4/earlier handoffs. The pre-existing `.gitignore` rule for `.ai/plans/` remains intact; this phase adds an artifacts ignore rule.

## Delivered

- `pnpm alpha:build <native-target-triple>` prepares Linux x64 deb/AppImage, Windows x64 current-user NSIS, or separate Apple Silicon/Intel macOS DMG candidates. Release builds have no added features; the existing Rust release guard prohibits native-test. macOS minimum remains 14.0 with ad-hoc signing and no notarization. Packaging description now uses functional copy.
- Version consistency, target allowlist/native OS, Cargo target-override checks, clean generated bundle output, exactly-one-required-installer checks, empty-file rejection, and refusal to overwrite a candidate.
- Version/target filenames, installer sizes and SHA-256 hashes, source revision with dirty marker, signing/acceptance status, release notes, and SHA256SUMS covering the installers/manifest/notes. Outputs stay in ignored `artifacts/`; no publish/send/update step exists.
- Linux CI builds/uploads the same candidate directory and fails for absent outputs. Debug quality evidence remains separate. Windows/macOS entries stay commented out.
- Release notes, installation/replacement/true-upgrade instructions, verification matrix, and explicit pending final user gate in `docs/alpha/`. README, plan, and architecture now describe this boundary.
- A reusable Linux production smoke helper tests the packaged portable AppImage in disposable XDG directories, including same-version file replacement and startup/restart; it introduces no production test hook.
- A meaningful artifact test verifies hashes against actual collected bytes, provenance, completeness, non-overwrite behavior, missing/stale installer rejection, and unsupported target rejection.

## Verification

Executed October 9, 2026 on Arch Linux x64 with Node 26.8.2, pnpm 12.10.1, Rust 1.99.0:

```sh
pnpm test:packaging
pnpm test
pnpm build
pnpm rust test --manifest-path src-tauri/Cargo.toml --offline
pnpm rust tree --manifest-path src-tauri/Cargo.toml --edges normal --offline
pnpm format:check
pnpm rust fmt --manifest-path src-tauri/Cargo.toml --check
CAL_CHROMIUM_PATH=/usr/bin/chromium pnpm test:ui
git diff --check
node scripts/alpha.mjs unknown
node scripts/alpha.mjs x86_64-pc-windows-msvc
```

Passed: artifact-collection checks for all four targets, 13 renderer unit tests, 12 Rust tests (real credential-store check ignored), 14 browser scenarios, frontend build, formatting, Rust formatting, whitespace checks, and ordinary dependency graph inspection excluding the test driver. Invalid and foreign-OS target invocations exit nonzero before building as intended. No real calendar or credentials were accessed.

The browser server initially could not bind inside the sandbox; the same suite passed outside that network restriction. Captures are under ignored `test-results/`. Browser checks do not establish installer or production-release execution.

### Linux candidate and production evidence

```sh
pnpm alpha:build x86_64-unknown-linux-gnu
(cd artifacts/aika-0.1.0-alpha.1-x86_64-unknown-linux-gnu && sha256sum -c SHA256SUMS)
python3 scripts/linux-release-smoke.py artifacts/aika-0.1.0-alpha.1-x86_64-unknown-linux-gnu/aika-0.1.0-alpha.1-x86_64-unknown-linux-gnu.AppImage --appimage-extract-and-run > test-results/release-smoke.json
ar t artifacts/aika-0.1.0-alpha.1-x86_64-unknown-linux-gnu/aika-0.1.0-alpha.1-x86_64-unknown-linux-gnu.deb
```

The first build compiled optimized release code and produced a deb, then AppImage packaging failed on a sandbox read-only cache path. Re-running with permitted access to the Tauri utility cache succeeded, downloading AppRun/linuxdeploy/AppImage tooling and producing both installers. The script withheld collection after the initial failure; no incomplete candidate was presented.

Candidate directory: `artifacts/aika-0.1.0-alpha.1-x86_64-unknown-linux-gnu/` (ignored). Contains an AppImage (106,068,472 bytes), deb (6,382,364 bytes), RELEASE-NOTES.md, manifest.json, and SHA256SUMS. All four checksum entries passed. The manifest records `b918476f9b1cf61c94686a1df76374eea425a80f-dirty`; these are host review artifacts, not a clean-source or Ubuntu-baseline distribution claim.

The **packaged production AppImage** passed isolated portable installation, same-version file replacement, and relaunch using `--appimage-extract-and-run`. Both launches created/reopened schema 1 with zero events, Personal/local-default, and unchanged system/month/sidebar defaults. Port 4445 was closed and `AIKA_TEST_DATA_DIR` was ignored. The helper uses disposable XDG directories, a temporary executable copy, read-only SQLite inspection, and process-group cleanup; it never touches a real calendar. This does not establish UI CRUD, changed-preference persistence, true version upgrades, FUSE mounting, or trusted native input. Evidence: `test-results/release-smoke.json`.

Deb ar/tarfile inspection verified package name aika, version 0.1.0-alpha.1, amd64, WebKitGTK/GTK dependencies, ELF64 executable permissions, desktop launcher name/command, and three icons. Evidence: `test-results/package-inspection.txt`. The deb was inspected, not installed on this Arch host. Browser captures remain separate.

## Outstanding gate and continuation

No Windows/macOS runner or Ubuntu disposable account is available on this host. Their installer execution, same-version replacement, true version upgrades, manual native pointer/accessibility/high-DPI checks, and hosted CI remain pending. Arch-built bundles do not prove Ubuntu runtime compatibility. Do not mark these passed based on browser/debug automation or the presence of package files.

Use `docs/alpha/verification.md` to build from a clean reviewed revision on each native platform, inspect and execute installers in disposable OS accounts, verify checksums and release-driver exclusion, and record install/replacement/restart/manual evidence. A same-version replacement is not a true upgrade. Keep the stable application identifier and never reset data on migration failure. Do not ship debug/native-test binaries or enable connected accounts.

Present completed candidate artifacts and evidence for final user acceptance only when reviewable target evidence exists. **User acceptance is pending; phase 5's exit gate is not claimed complete.**
