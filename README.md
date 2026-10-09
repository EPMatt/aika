# aika

A desktop calendar with room to breathe. Tauri 2, React, TypeScript, and Rust.

**Current milestone: phase 1 design prototype.** Events are sample data, and edits live only in memory. Reloading resets them. Appearance is saved on this device. This is not yet the persistent local-calendar alpha.

## Run

Use Node 24 LTS and pnpm 12.10.1:

```sh
pnpm install --frozen-lockfile
pnpm dev
```

Open http://127.0.0.1:1420. For the desktop application, install Rust 1.99.0 and the [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/), then run:

```sh
pnpm desktop
```

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

For an existing Chromium installation, run `CAL_CHROMIUM_PATH=/usr/bin/chromium pnpm test:ui` on Linux. Browser tests exercise the prototype, not native IPC or OS credential storage.

## Design review

Month/week/day, light/dark/system, an anchored event editor, a collapsible sidebar, calendar visibility, keyboard creation, and prototype undo are interactive.

Review URLs use a fixed date for reproducible screenshots:

- `/?date=2026-10-09&theme=light`
- `/?date=2026-10-09&theme=dark`
- `/?date=2026-10-09&scenario=empty`
- `/?date=2026-10-09&scenario=dense`

Shortcuts: **N** creates an event; **T** returns to today; **1/2/3** selects month/week/day; **Ctrl/Cmd+Z** undoes a prototype change; **Escape** dismisses the editor. Dragging a prototype event between dates preserves its time and duration. Complete drag-to-create, timed drop positioning, and resizing belong to phase 3.

Read [the implementation plan](docs/plan.md), [architecture decisions](docs/architecture.md), and the [latest handoff](docs/handoffs/phase-1.md) before continuing. The design review gate must pass before phase 2 begins.
