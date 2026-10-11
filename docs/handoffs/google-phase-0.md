# Google phase 0 — Specification and architecture

Status: **documentation delivered; runtime phases 1–6 remain planned.**

## Source and scope

Base revision: `9b464b615dac826e5c141a277406b4e02088e608` (`feat: support common recurring events in UI and CLI`). This handoff describes uncommitted documentation changes on that base; the implementation revision can be found through `git log -1 -- docs/handoffs/google-phase-0.md` after committing them.

The user requested implementation of **phase 0 only** after accepting the Google integration plan. The pre-existing `.gitignore` edit is preserved. No runtime files, dependencies, schemas, generated contracts, credentials, or cloud settings changed.

## Delivered

- [Integration specification](../integrations/google-calendar.md): agreed UX/defaults, current implementation, data flow and planned interfaces, future-provider guide, phases 0–6 and exit gates, OAuth prerequisites, acceptance scenarios, and handoff procedure.
- [Architecture decisions G01–G07](../architecture.md#connected-calendar-decisions-google-integration): provider/account identity, shared repository/capabilities, cache/outbox/coordinator, explicit conflicts/undo, backend authentication, import/scheduling, and recurrence fallback/recovery, each with rationale and consequences.
- [Main plan](../plan.md) and [README](../../README.md) links and status distinguish the historical v0 scope from the shipped multiple-calendar/recurrence behavior and planned connected-calendar increment. Outstanding alpha/platform gates remain pending.

The decisions preserve the user's choices: all eligible calendars checked before import; automatic addition of newly discovered calendars with exclusions retained; application-open sync; explicit conflict resolution; read-only meetings; phased compatible recurrence editing; and unsupported-rule occurrence caching for 12 months past/24 months ahead plus ranges opened online.

## Verification

Executed on Linux with Node 26.8.2, pnpm 12.10.1, and Python 3.14.7:

```sh
pnpm exec prettier --check README.md docs/plan.md docs/architecture.md docs/integrations/google-calendar.md docs/handoffs/google-phase-0.md
git diff --check
git status --short --untracked-files=all
```

Passed: formatting for all five edited/new documents and whitespace checks. The initial formatting check flagged the new specification's Markdown tables; `pnpm exec prettier --write docs/integrations/google-calendar.md` corrected them before the passing check. Scope review shows only those five documentation files plus the pre-existing `.gitignore` change. All seven integration phases have explicit deliverables and exit gates; only phase 0 is marked delivered.

Local file/anchor audit: **34 links across five documents passed**, using this exact command:

```sh
python3 - <<'PY'
from pathlib import Path
import re
files = [Path(p) for p in ('README.md', 'docs/plan.md', 'docs/architecture.md',
    'docs/integrations/google-calendar.md', 'docs/handoffs/google-phase-0.md')]
checked = 0
for source in files:
    for link in re.findall(r'\[[^\]]+\]\(([^)]+)\)', source.read_text()):
        if re.match(r'[a-z]+:', link):
            continue
        name, _, anchor = link.partition('#')
        target = source.parent / name if name else source
        assert target.is_file(), f'{source}: missing {link}'
        if anchor:
            headings = re.findall(r'^#{1,6}\s+(.+)$', target.read_text(), re.M)
            slugs = {re.sub(r'\s', '-', re.sub(r'[^\w\s-]', '', h.lower())) for h in headings}
            assert anchor in slugs, f'{source}: missing anchor {link}'
        checked += 1
print(f'PASS: {checked} local file/anchor links across {len(files)} documents')
PY
```

Reviewed official Google documentation for the desktop OAuth flow, Calendar scopes, incremental synchronization, conditional writes, and recurrence; references are linked beside the relevant specification/decision. This is documentation research, not a live authentication/API test. Runtime suites, generated-type export, native builds, migrations, and live Google checks were not run because no runtime behavior changed. Phase 0's documentation exit gate passes; changes remain uncommitted.

## Limits and phase 1 starting instructions

Google remains unavailable in production. This phase ships the specification; no provider popup, authentication, import, cache, outbox, conflict UI, or synchronization runtime has been implemented. Live authentication and target-platform evidence require later phases. Original alpha acceptance is still pending.

For phase 1:

1. Read the [specification](../integrations/google-calendar.md), [architecture decisions](../architecture.md#connected-calendar-decisions-google-integration), this handoff, and [quality commands](../quality.md). Check the working tree and preserve unrelated changes.
2. Start from `aika-core`: `LocalProvider` currently owns the schema v3 connection/history; `ProviderRegistry` keeps collection/calendar-management, recurring lookup, and undo on the local adapter. Extract shared persistence and separate provider-type/account identities before adding Google HTTP/authentication.
3. Preserve existing IDs and checked mutations; add transactional migrations and generic capability/collection contracts. Keep pure domain/WASM code independent of network, credentials, and SQLite. Update generated TypeScript through the existing Rust exporter.
4. Prove the model using a second test-only adapter with multiple accounts, writable/read-only calendars, and colliding remote IDs. Include local/CLI regression and migration evidence in the next handoff. No production sample provider or live account belongs to phase 1.
5. Update the phase table and add `google-phase-1.md` only with actual delivered behavior and verification. Connection/import UI belongs to phase 2, incoming sync to phase 3, ordinary-event writes to phase 4, and recurring writes to phase 5.
