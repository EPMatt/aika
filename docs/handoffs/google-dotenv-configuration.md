# Google configuration — Optional project .env

Status: **implemented and verified with tooling/packaging/configuration fixtures.** This follows the [build configuration increment](google-build-configuration.md) and does not advance Google phase 4 or complete pending live/platform gates.

## Source and behavior

Base revision: `9417fa163dc584fcc122612e9509a7fb1a0d60eb`. Changes are uncommitted alongside the pre-existing Google integration/build configuration work; the unrelated `.gitignore` entry is preserved. No actual `.env`, Google client/account, cloud project, repository setting, installed package, or release was created or changed.

Native tool commands now load the optional repository-root `.env` before launching Cargo/Tauri and their child processes. Alpha and tagged-release builds use the same resolved environment, including before the existing Cargo-directory/target guards. The native regression runner shares this environment for app/CLI subprocesses and provider availability assertions. Missing `.env` preserves existing behavior. An unreadable file stops the command with a value-free explanation.

Each declared file entry overrides its matching inherited variable, including empty strings; omitted variables remain inherited. On Windows names are matched case-insensitively. The loader uses Node's built-in dotenv parser (supported by pinned Node 24), including comments, quotes, multiline values, optional `export`, and last-assignment precedence. Values are literal: no shell execution or variable interpolation. The loader returns a new environment without mutating the parent process. Existing local Rust-toolchain path selection is retained.

Only the exact repository-root `.env` is loaded, regardless of the caller's current directory. There is no native `.env.local`, mode-specific, ancestor, executable-directory, or installed-app file search. Packaged apps continue using the existing embedded/runtime configuration resolver. Restart development commands or rebuild packages to apply changes. User tokens remain in the OS credential store; this change adds no Rust dependency, native IPC/type, or schema migration.

`.env` and local variants are ignored by Git, while `.env.example` remains versionable. Copy the example to `.env` and fill in the Google **Desktop app** client ID and optional client secret. Keep the secret entry present but empty when no secret is required, so any inherited secret is cleared. Then use `pnpm desktop`, `pnpm desktop:build`, `pnpm cli`, or the existing alpha/release build commands normally. Desktop client configuration remains extractable when embedded; never put user access/refresh tokens into these inputs.

The fictional Google configuration smoke runner explicitly invokes `scripts/tool.mjs --no-env-file` so a developer's actual `.env` cannot replace fixture values or enter fixture binaries. This is a deliberate explicit test bypass, not normal dotenv precedence. No real local configuration is read or printed by that runner.

## Verification

Host: October 19, 2026, Linux x64, Node 26.8.2, pnpm 12.10.1. Fixtures use temporary directories, fictional settings, a fake Tauri CLI, temporary Git repositories, and mocked GitHub commands. No real Google authorization or release publication occurs.

Executed commands:

```sh
pnpm test:env
pnpm test:packaging
pnpm test:google-config
pnpm build
pnpm format:check
python3 /tmp/aika-google-dotenv-links.py
git check-ignore .env .env.local
git diff --check
```

Passed: **seven environment tests**, covering missing files, file-over-shell and empty-value precedence, omitted-variable inheritance, dotenv syntax/literal dollars, edits/removal, unreadable-file errors, actual tool subprocess propagation from another working directory, explicit smoke bypass, and alpha build guard enforcement. **19 packaging/release fixtures** passed, including the added release guard case using `.env` and existing mocked publication/integrity scenarios. Both packaging guards reject target-directory overrides from the file before building/collecting anything.

The existing Google configuration check passed **nine binary smoke launches across three successive builds** with local dotenv loading explicitly bypassed. This preserves configured/unconfigured builds, complete runtime override behavior, changed/removed compile-input invalidation, and fixture isolation. The frontend production build, Prettier, **58 local file/anchor links across eight documents**, and whitespace checks passed. Git ignore checks confirm `.env` and `.env.local` are ignored while `.env.example` remains versionable.

Initial sandbox execution allowed the five pure file/parser tests but rejected the command-launch fixture with `spawnSync ... node EPERM`; Node's default isolated reporter initially showed only a file-level failure. A no-isolation diagnostic identified the subprocess restriction. The full environment and packaging suites passed outside that restriction. No automatic approval-review rejection occurred.

## Limits and continuation

No Rust behavior, generated contracts, or renderer UI changed in this increment, so their earlier full regression evidence remains in the [build configuration handoff](google-build-configuration.md) and [phase 3 handoff](google-phase-3.md). Native app automation, Windows/macOS execution, installed bundles, and live Google authorization were not rerun for this Node tooling change. Quality CI now runs `pnpm test:env`; the existing Google binary smoke check remains isolated from dotenv. A real application-owned client, consent audience, live provider behavior, and installed-platform checks remain required separately.

Continue phase 4 through the [phase 3 starting instructions](google-phase-3.md#next-starting-instructions--google-phase-4), preserving incoming read-only gates and the shared repository/coordinator. Existing package/CI environment inputs continue working when no `.env` is present; a local file deliberately takes precedence when supplied.
