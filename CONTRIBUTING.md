# Contributing to aika

Bug reports, usability feedback, documentation, and code contributions are welcome. Describe the expected behavior, what happened, and steps to reproduce; include OS/webview versions for native issues. Keep credentials and personal calendar data out of reports and captures.

## Development setup

Use Node 24 (`.node-version`), pnpm 12.10.1 (`package.json`), Rust 1.99.0 (`rust-toolchain.toml`), and the [Tauri native prerequisites](https://v2.tauri.app/start/prerequisites/) for your OS. Browser development also requires the Rust WebAssembly target and matching wasm-bindgen CLI:

```sh
pnpm install --frozen-lockfile
node scripts/tool.mjs rustup target add wasm32-unknown-unknown
pnpm rust install wasm-bindgen-cli --version 0.2.129 --locked
pnpm desktop
```

`pnpm desktop` starts the native app with SQLite persistence. `pnpm dev` serves the browser preview at http://127.0.0.1:1420 with sample data held in memory. The preview supports design and interaction work; it does not authenticate accounts or establish native persistence. Native startup ignores preview fixtures. For repeatable browser scenes, use `/?date=2026-10-09&theme=light`, `theme=dark`, `scenario=empty`, or `scenario=dense`.

The tool wrapper detects a workspace-local Rust installation under `.tools/` without changing global shell configuration. Native commands load an optional root `.env`; see [Google configuration](docs/integrations/google-calendar.md#configuration) for precedence and setup.

## Shared domain and generated bindings

`aika-domain` owns event contracts, validation, recurrence, and mutation planning. Native code links it directly; the browser uses its WASM build. `pnpm dev`, `pnpm build`, and `pnpm test` run `pnpm domain:build`, generating ignored bindings under `src/wasm/`.

Public types originate in Rust (`src-tauri/domain/src/contracts.rs` and native contracts in `src-tauri/core/src/contracts.rs`). After changing contracts, run:

```sh
pnpm types:generate
```

Commit the resulting `src/generated/` declarations with the Rust changes. Do not edit generated TypeScript or WASM bindings by hand. CI regenerates TypeScript and rejects drift.

## Checks

Run checks relevant to the change. The normal development checks are:

```sh
pnpm format:check
pnpm build
pnpm test
pnpm test:env
pnpm test:packaging
pnpm exec playwright install chromium
pnpm test:ui
pnpm rust fmt --manifest-path src-tauri/Cargo.toml --all --check
pnpm rust clippy --manifest-path src-tauri/Cargo.toml --workspace --all-targets -- -D warnings
pnpm rust test --manifest-path src-tauri/Cargo.toml --workspace
pnpm types:generate
pnpm desktop:build --no-bundle
```

On Linux, an existing Chromium can be selected with `CAL_CHROMIUM_PATH=/usr/bin/chromium pnpm test:ui`. Browser captures and results go to ignored `test-results/`.

Vitest exercises layout, client behavior, and the shared WASM domain. Playwright covers renderer interactions, mocked IPC failures, and axe accessibility audits. Rust suites cover the domain, real SQLite migrations/restarts, checked concurrency/undo, CLI subprocesses, provider isolation, and injected Google transports/stores. Browser and mocked-provider tests do not prove native input, OS credentials, or live Google behavior.

`pnpm test:google-config` separately builds fictional embedded client configurations and launches resolver smoke tests, including changed/removed build inputs and runtime precedence. It performs no Google authorization. `pnpm test:env` checks dotenv behavior; `pnpm test:packaging` uses temporary artifacts and Git/release fixtures rather than publishing a release. Some tests require loopback sockets and subprocess execution.

## Native testing

```sh
pnpm desktop:test:build
pnpm test:native
```

The optional debug-only `native-test` feature exposes an embedded WebdriverIO loopback driver. The runner builds the CLI, uses disposable app data and real IPC/SQLite, tests stale desktop/CLI edits, recurrence/scoped undo, calendars, and restart persistence, and writes captures under `test-results/native/`. `AIKA_TEST_BINARY` overrides the executable; `TAURI_WEBDRIVER_PORT` overrides port 4445. Production builds omit this feature, and release-mode compilation rejects it.

Linux CI uses `dbus-run-session -- xvfb-run -a pnpm test:native` with `WEBKIT_DISABLE_DMABUF_RENDERER=1`. Windows/macOS build and Rust checks are active; native webview automation currently runs only on Linux in CI. The runner supports other platforms, but execution must be recorded separately. Its synthetic webview actions do not certify trusted OS dragging, physical focus changes, or screen-reader use.

For a binary built with fictional embedded Google settings, set `AIKA_TEST_EXPECT_GOOGLE_AVAILABLE=true` when launching the native suite without Google runtime variables; use `false` for an unconfigured binary. Descriptor availability does not prove a valid OAuth grant.

The real OS credential-store test is opt-in, creates a disposable entry, and removes it afterward:

```sh
pnpm rust test --manifest-path src-tauri/Cargo.toml -p aika-core real_os_store_roundtrip -- --ignored
```

Live Google tests require an application-owned Desktop client and disposable test account/calendars. Record actual consent, refresh, sync, recovery, and platform outcomes separately from mocks. See [Google technical behavior](docs/integrations/google-calendar.md) and the [release acceptance checklist](docs/releases.md#acceptance-checklist).

## Contribution workflow

Keep changes focused and preserve unrelated work. Read the [architecture](docs/architecture.md) before changing shared contracts, persistence, or providers. Add regression coverage for meaningful behavior changes and run the relevant checks. Format edited files with `pnpm exec prettier --write <files>` and check whitespace with `git diff --check`.

Open a pull request explaining the problem, resulting behavior, and checks actually run. Include screenshots for visible changes, migration/recovery details for storage changes, and explicit unverified cases. Keep automated, native, live-provider, and manual-platform evidence distinct. Documentation-only changes need formatting and local link/image/anchor checks; old test results do not validate a new revision.

Packaging and installation guidance lives in [releases](docs/releases.md); unfinished work lives in the [roadmap](docs/roadmap.md).
