# Google build configuration — Packaged app support

Status: **implementation delivered with binary smoke and Linux native evidence. Live Google and installed-platform exit gates remain pending.** This is a configuration follow-up to [Google phase 3](google-phase-3.md), not phase 4 two-way synchronization.

## Source and implementation

Base revision: `9417fa163dc584fcc122612e9509a7fb1a0d60eb`. Changes remain uncommitted on the existing phase 2/3 working tree. Resolve a future implementation revision with `git log -1 -- docs/handoffs/google-build-configuration.md` after committing. The pre-existing unrelated `.gitignore` change is preserved. No Cloud project/client, GitHub repository setting, real Google account authorization, installed release package, publishing, or remote event write was performed.

The shared native Rust core now embeds optional `AIKA_GOOGLE_CLIENT_ID` and `AIKA_GOOGLE_CLIENT_SECRET` through `option_env!`. Cargo tracks these environment inputs and recompiles when values change or are removed; no custom build script or clean-build requirement was added. The same core serves desktop and CLI; CLI construction still starts no worker or network operation.

Configuration resolution uses complete pairs:

- A nonblank runtime client ID selects the runtime ID and optional runtime secret exclusively. An omitted runtime secret never inherits the embedded secret.
- Otherwise use the embedded ID and embedded secret. A runtime secret alone cannot modify the embedded pair.
- Values are trimmed; blank values count as absent. A secret without an effective client ID does not enable Google.
- Missing configuration keeps builds valid with Google disabled and a descriptor explanation mentioning build-time or launch-time configuration.

No UI form, JSON/config-file loader, renderer `VITE_` setting, IPC/type/schema change, dependency, token lifecycle, or credential-store migration was introduced. Embedded Desktop client configuration is public/extractable, including its optional client secret. It must never contain user access/refresh tokens or confidential server credentials. OAuth consent/scopes, browser/PKCE/state, credential storage, incoming sync, and read-only gates are unchanged. Changing the OAuth client may require existing accounts to reconnect.

The quality workflow runs `pnpm test:google-config` before unconfigured native checks. Package-build steps in quality/private-alpha and tagged-release workflows optionally receive repository variable `AIKA_GOOGLE_CLIENT_ID` and repository secret `AIKA_GOOGLE_CLIENT_SECRET`. Missing inputs become empty values and do not fail packaging. Ordinary test/native build steps do not receive those repository settings. Existing scripts already inherit their process environment, so direct desktop, alpha, and release build commands share the same inputs.

## Building and checking

Provide an application-owned **Desktop app** OAuth client, enable the Calendar API, and configure the consent/test audience as described in [Google setup](../integrations/google-calendar.md#google-setup-and-authentication-boundary). Example:

```sh
export AIKA_GOOGLE_CLIENT_ID='your-desktop-client-id'
export AIKA_GOOGLE_CLIENT_SECRET='your-desktop-client-secret'
pnpm desktop:build
```

Omit the secret assignment if it is absent in the downloaded Desktop client configuration. The same environment works with `pnpm alpha:build <target>` or `pnpm release:build <tag> <target>`. PowerShell uses `$env:AIKA_GOOGLE_CLIENT_ID` and optional `$env:AIKA_GOOGLE_CLIENT_SECRET`. Existing installers must be rebuilt; setting environment variables during an older build did not embed them. Configured packages then use the embedded pair when launched from the desktop without those variables.

The native test runner accepts `AIKA_TEST_EXPECT_GOOGLE_AVAILABLE=true` or `false`; absent an explicit expectation, it retains the existing runtime-client-ID expectation for unconfigured CI. This affects only test assertions, not application configuration. An invalid expectation fails before creating the disposable test directory.

## Verification

Host: October 14, 2026, Arch Linux x64 (`Linux 7.2.5-3-omarchy`), Node 26.8.2, pnpm 12.10.1, Rust/Cargo 1.99.0. Only fictional client IDs/secrets, disposable SQLite databases, test-only OAuth transports/stores, and mocked GitHub commands were used. Socket/native and temporary Git-fixture checks require execution outside the sandbox restrictions.

Executed commands:

```sh
pnpm test:google-config
pnpm rust test --manifest-path src-tauri/Cargo.toml --workspace
pnpm rust clippy --manifest-path src-tauri/Cargo.toml --workspace --all-targets -- -D warnings
pnpm rust fmt --manifest-path src-tauri/Cargo.toml --all --check
pnpm test:packaging
node --test --test-reporter=tap scripts/alpha.test.mjs scripts/release.test.mjs
AIKA_GOOGLE_CLIENT_ID=fictional-desktop-client AIKA_GOOGLE_CLIENT_SECRET=fictional-desktop-secret pnpm desktop:test:build
env -u AIKA_GOOGLE_CLIENT_ID -u AIKA_GOOGLE_CLIENT_SECRET DISPLAY=:0 WAYLAND_DISPLAY=wayland-1 AIKA_TEST_EXPECT_GOOGLE_AVAILABLE=true pnpm test:native
env -u AIKA_GOOGLE_CLIENT_ID -u AIKA_GOOGLE_CLIENT_SECRET pnpm desktop:test:build
env -u AIKA_GOOGLE_CLIENT_ID -u AIKA_GOOGLE_CLIENT_SECRET DISPLAY=:0 WAYLAND_DISPLAY=wayland-1 AIKA_TEST_EXPECT_GOOGLE_AVAILABLE=false pnpm test:native
pnpm format:check
python3 /tmp/aika-google-build-config-links.py
git diff --check
```

Passed: **nine binary smoke launches across three successive builds using the same Cargo build directory**. Build A embeds a fictional ID/secret; its executable is launched directly with runtime configuration removed, with a complete runtime override, with an ID-only override, with an orphan runtime secret, and with a blank runtime ID. Build B changes the embedded ID and removes its secret; its no-runtime launch verifies recompilation. The last build removes both build inputs; no-runtime/orphan-secret launches verify Google disabled, while a runtime-configured launch verifies backward compatibility. The dedicated smoke test is ignored in ordinary Rust suites and invoked explicitly by its runner; it starts no OAuth, network, or credential-store work.

The Rust workspace passed **89 tests** (15 CLI, 63 core, 11 domain), all doc-test targets, Clippy with warnings denied, and Rust fmt. The ordinary suite skips the opt-in real-store test and dedicated binary smoke test; the smoke runner executes the latter separately. Pure resolver coverage checks both configuration sources, optional secrets, trimming/blanks, and complete-pair precedence without mutating shared process environment. **18 packaging/release fixtures passed** outside sandbox restrictions, including fake release publication/retry/integrity checks; they did not publish a real release.

Both Linux native debug builds and full native webview/SQLite/CLI regression runs passed. The configured binary embedded the fictional Desktop client ID/secret and launched with both Google runtime variables removed; its Google provider item was enabled. The unconfigured binary also launched with both variables removed; its provider item was disabled with the configuration explanation. Each run used its explicit expected availability, disposable schema v5 databases, actual IPC, and Local timed/all-day CRUD, recurrence/scopes/preview/undo/restart, CLI aggregate conflicts, views, keyboard dismissal, appearance, and multiple-calendar checks. Google authorization was never started. The final debug binary is deliberately unconfigured.

Both native builds rebuilt the frontend successfully. Prettier, Rust fmt, whitespace checks, and **52 local file/anchor links across eight documents** passed. No generated event/IPC contracts changed in this follow-up. Final native screenshots are ignored `test-results/native/provider-popup.png`, `month-light.png`, `week-light.png`, and `day-dark.png`; the final provider-popup screenshot shows the unconfigured case. The configured screenshot was overwritten by the second regression run, so do not claim it remains as a separate artifact. The native fixtures check provider availability and Local/SQLite regressions, not actual Google grant validity.

Initial packaging attempts failed inside the sandbox: Node's default isolated reporter showed only file-level `test failed`; a no-isolation diagnostic exposed `spawnSync git EPERM` in disposable release fixtures. The unchanged fixtures passed outside that restriction. No automatic approval-review rejection occurred.

A subsequent [dotenv configuration follow-up](google-dotenv-configuration.md) adds optional root `.env` loading for native development/build commands and packaging. Consult that handoff for file-over-shell precedence and fixture isolation. The build/runtime configuration pairing and pending live/platform gates remain unchanged.

## Remaining gates and next work

Set the optional repository variable/secret yourself before expecting configured CI packages; this change does not create them. Use an application-owned Google Cloud Desktop client, actual consent/test audience, and disposable Google calendars for live verification. Validate browser callback, real OAuth grant, secure-store persistence, refresh/reconnection, incoming events, offline/restart behavior, and installed Linux/Windows/macOS packages separately. An enabled descriptor or fictional client string does not establish a working Google grant.

Google phases 2/3 retain pending live/platform exit evidence. Phase 4 remains ordinary-event two-way synchronization with durable outbox/retry/acknowledgement recovery, explicit conflicts, and undo; follow the [phase 3 starting instructions](google-phase-3.md#next-starting-instructions--google-phase-4) and [integration specification](../integrations/google-calendar.md). Preserve the existing shared repository/coordinator, credential boundary, and read-only imported-event gates until that phase is implemented and verified.
