# Phase 0 — Desktop foundation

Status: **local foundation verified; external platform CI pending.**

## Source and environment

Project files are uncommitted in a workspace that inherits `/home/srvadmin/Documents` as its Git root. No external repository, release, or project commit was created. The workflow assumes this project becomes the repository root.

Local checks use Linux x64/Wayland, Node 26.8.2, pnpm 12.10.1, Rust 1.99.0, WebKitGTK 2.52.6, and GTK 3.24.52. CI selects Node 24 LTS. Rust was installed in the ignored workspace `.tools/` directory without changing global shell configuration. Tool wrappers detect that installation automatically.

## Delivered

- Tauri 2 shell with native window controls, a desktop application icon, CSP, and minimal core capabilities.
- React/TypeScript/Vite application, pinned package manager and Rust toolchain, dependency lockfiles, formatter, and testing configuration.
- Rust calendar/event contracts and working ts-rs generation; one native command, `get_app_info`.
- CI jobs for renderer checks and Windows x64, macOS Intel/Apple Silicon, and Linux x64 builds/artifacts.
- Plan, architecture record, and phase handoff procedure.

## Verification

Executed successfully:

```sh
pnpm build
pnpm types:generate
pnpm rust check --manifest-path src-tauri/Cargo.toml --all-targets --offline
pnpm rust fmt --manifest-path src-tauri/Cargo.toml --check
pnpm rust clippy --manifest-path src-tauri/Cargo.toml --all-targets --offline -- -D warnings
pnpm rust test --manifest-path src-tauri/Cargo.toml --offline
pnpm desktop:build --debug --no-bundle
```

The native executable is `src-tauri/target/debug/cal-desktop`. Actual Linux launch verified a mapped Cal window, a mounted React root, and working frontend-to-Rust IPC (`runtime: desktop`). Native startup verification caught and fixed a POSIX-locale incompatibility that Chromium alone did not reveal. Debug builds log only startup mounting/runtime metadata; release builds exclude that diagnostic.

The Rust test harness currently has no domain tests: its successful run proves target compilation, not calendar persistence or provider correctness. Those tests belong to phases 2 and 4.

## Pending checks

Windows/macOS/Ubuntu CI has not executed: no project-root remote repository is connected. Do not describe those platforms, installers, release builds, or Node 24 execution as verified. The local host's Linux webview dependencies were already present.

## Next

Use `pnpm dev` for browser review or `pnpm desktop` for native development. Run the configured pipeline once this project has a project-root GitHub repository. Phase 1 is ready for the agreed design review; see its handoff before starting persistence.
