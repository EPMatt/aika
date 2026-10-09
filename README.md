# aika

A desktop calendar. Tauri 2, React, TypeScript, and Rust.

**Current milestone: phase 2 local domain.** The desktop app saves one local calendar in SQLite, restores events and appearance/view/sidebar preferences after restart, and supports session undo. The browser preview remains an in-memory design harness with sample data.

## Run

Use Node 24 LTS and pnpm 12.10.1:

```sh
pnpm install --frozen-lockfile
pnpm desktop
```

Desktop development requires Rust 1.99.0 and the [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/). For a browser preview, run `pnpm dev` and open http://127.0.0.1:1420.

The tooling wrapper also detects a workspace-local Rust installation in `.tools/`. It does not alter global shell configuration. This workspace already has that installation; on another machine use the normal Rust toolchain setup.

## Verify

```sh
pnpm build
pnpm test
pnpm exec playwright install chromium
pnpm test:ui
pnpm rust fmt --manifest-path src-tauri/Cargo.toml --check
pnpm rust clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
pnpm types:generate
pnpm desktop:build --no-bundle
```

For an existing Chromium installation, run `CAL_CHROMIUM_PATH=/usr/bin/chromium pnpm test:ui` on Linux. Browser tests cover the design harness and mocked IPC error handling; Rust integration tests cover SQLite CRUD, restart, undo, ranges, migrations, and time zones. Native launch is checked separately.

## Design review

Month/week/day, light/dark/system, an anchored event editor, a collapsible sidebar, calendar visibility, keyboard creation, and undo are interactive.

Review URLs use a fixed date for reproducible screenshots:

- `/?date=2026-10-09&theme=light`
- `/?date=2026-10-09&theme=dark`
- `/?date=2026-10-09&scenario=empty`
- `/?date=2026-10-09&scenario=dense`

Shortcuts: **N** creates an event; **T** returns to today; **1/2/3** selects month/week/day; **Ctrl/Cmd+Z** undoes a session change; **Escape** dismisses the editor. Dragging an event between dates preserves its local time and duration; DST gaps and repeated times require editing explicitly. Complete drag-to-create, timed drop positioning, and resizing belong to phase 3.

Read [the implementation plan](docs/plan.md), [architecture decisions](docs/architecture.md), and the [latest handoff](docs/handoffs/phase-2.md) before continuing. Phase 3 completes creation/move/resize gestures and their keyboard alternatives.
