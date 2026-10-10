import fixtures from '../tests/fixtures/recurrence.json';
import { beforeAll, describe, expect, it } from 'vitest';
import { readFile } from 'node:fs/promises';
import { DateTime } from 'luxon';
import { initializeRecurrence, recurrenceDomain } from './recurrence';
import { makeEvent } from './calendar';
import type { CalendarEvent } from './generated/CalendarEvent';
import { PrototypeRepository } from './prototype';

beforeAll(async () => {
  await initializeRecurrence({
    module_or_path: await readFile(new URL('./wasm/aika_domain_bg.wasm', import.meta.url)),
  });
});
function event(): CalendarEvent {
  return {
    ...makeEvent(DateTime.fromISO('2026-01-31'), true),
    title: 'Monthly',
    recurrence: {
      frequency: 'monthly',
      interval: 1,
      weekdays: [],
      monthlyOn: 'date',
      until: null,
      count: 5,
    },
  };
}
const range = {
  start: '2026-01-01T00:00:00Z',
  end: '2026-10-01T00:00:00Z',
  startDate: '2026-01-01',
  endDate: '2026-10-01',
};
describe('shared Rust WebAssembly recurrence', () => {
  for (const fixture of fixtures) {
    it(fixture.name, () => {
      expect(
        recurrenceDomain
          .preview(fixture.event as CalendarEvent)
          .map((e) => (e.time.kind === 'timed' ? e.time.start : e.time.startDate)),
      ).toEqual(fixture.starts);
    });
  }
  it('uses the same month-end skipping and count semantics as native Rust', () => {
    const result = recurrenceDomain.preview(event());
    expect(result.map((e) => (e.time.kind === 'allDay' ? e.time.startDate : ''))).toEqual([
      '2026-01-31',
      '2026-03-31',
      '2026-05-31',
      '2026-07-31',
      '2026-08-31',
    ]);
  });
  it('preserves counts, exceptions, and atomic undo in the browser repository', () => {
    const master = event();
    const repo = new PrototypeRepository([master]);
    const selected = repo.snapshot(range).events[2];
    repo.mutate({
      target: selected.id,
      scope: 'occurrence',
      event: null,
      expected: selected.occurrence!.expected,
      resetExceptions: false,
    });
    expect(repo.snapshot(range).events).toHaveLength(4);
    expect(() =>
      repo.mutate({
        target: master.id,
        scope: 'series',
        event: master,
        expected: recurrenceDomain.expectation(repo.getSeries(master.id)),
        resetExceptions: false,
      }),
    ).toThrow();
    repo.undo();
    expect(repo.snapshot(range).events).toHaveLength(5);
    const fresh = repo.snapshot(range).events[2];
    repo.mutate({
      target: fresh.id,
      scope: 'following',
      event: { ...fresh, title: 'Tail' },
      expected: fresh.occurrence!.expected,
      resetExceptions: false,
    });
    expect(repo.snapshot(range).events.filter((e) => e.title === 'Tail')).toHaveLength(3);
    repo.undo();
    expect(repo.snapshot(range).events).toHaveLength(5);
    expect(repo.list()).toHaveLength(1);
  });
  it('returns structured validation errors', () => {
    expect(() =>
      recurrenceDomain.preview({ ...event(), recurrence: { ...event().recurrence!, interval: 0 } }),
    ).toThrow();
  });
  it('reanchors monthly date edits to include newly valid months', () => {
    const repo = new PrototypeRepository([event()]);
    const selected = repo.snapshot(range).events[1];
    repo.mutate({
      target: selected.id,
      scope: 'following',
      event: {
        ...selected,
        time: { kind: 'allDay', startDate: '2026-03-30', endDate: '2026-03-31' },
      },
      expected: selected.occurrence!.expected,
      resetExceptions: false,
    });
    expect(
      repo.snapshot(range).events.map((e) => e.time.kind === 'allDay' && e.time.startDate),
    ).toEqual(['2026-01-31', '2026-03-30', '2026-04-30', '2026-05-30', '2026-06-30']);
    repo.undo();
    expect(repo.list()).toHaveLength(1);
  });
});

it('deleting an override destination prevents resurrection and prunes prior series undo', () => {
  const master = event();
  const repo = new PrototypeRepository([master]);
  // Record creation in session history, as the app does.
  repo.save(master);
  const destination = repo.createCalendar({ name: 'Other', description: '', color: '#718675' });
  const selected = repo.snapshot(range).events[1];
  repo.mutate({
    target: selected.id,
    scope: 'occurrence',
    event: { ...selected, calendarId: destination.id },
    expected: selected.occurrence!.expected,
    resetExceptions: false,
  });
  expect(repo.calendarEventCount(destination.id)).toBe(1);
  expect(repo.calendarEventCount(master.calendarId)).toBe(1);
  repo.deleteCalendar(destination.id);
  expect(repo.snapshot(range).events).toHaveLength(4);
  expect(repo.canUndo()).toBe(false);
});
