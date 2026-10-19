# Tagged releases

Push a tag named `vMAJOR.MINOR.PATCH` to publish a GitHub release automatically. The tagged commit must be contained in `main`; an older commit in main's history is allowed. The tagged commit must include `.github/workflows/release.yml`, because GitHub loads tag-push workflows from that revision. Lightweight and annotated tags both work. Leading zeros, prerelease suffixes, and build metadata are rejected.

## Prepare a release

1. Update `package.json`, `src-tauri/tauri.conf.json`, and the package versions in `src-tauri/Cargo.toml`, `src-tauri/core/Cargo.toml`, and `src-tauri/cli/Cargo.toml` to the same numeric version, for example `0.1.0`.
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

Release and private-alpha packaging load the optional repository-root `.env` over the inherited build environment, then use `AIKA_GOOGLE_CLIENT_ID` and optional `AIKA_GOOGLE_CLIENT_SECRET`. The shared Rust core embeds these Desktop OAuth client settings, so packaged apps can connect Google accounts when launched normally. Builds without a client ID remain valid with Google disabled. A nonblank runtime client ID overrides the embedded pair using only its runtime secret; blank values are absent and a secret alone cannot override the embedded client.

For GitHub Actions, set repository variable **AIKA_GOOGLE_CLIENT_ID** and optional repository secret **AIKA_GOOGLE_CLIENT_SECRET**. The release-package and private-alpha-package steps receive these inputs; ordinary quality/native checks remain unconfigured. Missing repository inputs become empty values and do not block packaging. This implementation does not create repository variables, a Cloud project, or an OAuth client.

For local packages, copy `.env.example` to `.env`, fill in the Desktop client settings, and run the build command normally. Every key declared in `.env` overrides the matching inherited variable, including a blank value; omitted keys remain inherited. Keep the secret entry present but empty when the client has no secret. Only the root `.env` is loaded; `.env.local` and mode-specific dotenv files are not native configuration inputs. Values support Node dotenv comments/quotes and optional `export`, with no shell execution or variable interpolation. The file remains local and is not bundled. Packaged applications use embedded settings and do not search their working directory for `.env`.

For local packages, export the variables before `pnpm desktop:build`, `pnpm alpha:build <target>`, or `pnpm release:build <tag> <target>`. For example:

```sh
export AIKA_GOOGLE_CLIENT_ID='your-desktop-client-id'
export AIKA_GOOGLE_CLIENT_SECRET='your-desktop-client-secret'
pnpm desktop:build
```

In PowerShell, set `$env:AIKA_GOOGLE_CLIENT_ID` and, when applicable, `$env:AIKA_GOOGLE_CLIENT_SECRET` before the same build command. Omit the secret assignment if the downloaded Desktop client configuration has no secret. Existing installers must be rebuilt. Embedded settings are public Desktop client configuration and are extractable from executables, even when supplied through GitHub Secrets. User access/refresh tokens remain in the OS credential store. Never use renderer `VITE_` settings or user tokens as build inputs. See [Google setup](integrations/google-calendar.md#google-setup-and-authentication-boundary) and the [build configuration handoff](handoffs/google-build-configuration.md).

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
