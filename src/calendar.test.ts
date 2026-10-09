import { describe, expect, it } from 'vitest';
import { DateTime } from 'luxon';
import {
  dateKey,
  eventsOnDate,
  layoutTimedEvents,
  makeEvent,
  monthDays,
  moveEvent,
  resolveLocale,
} from './calendar';
import { PrototypeRepository } from './prototype';

const day = DateTime.fromISO('2026-10-09', { zone: 'Europe/Zurich' });
const timed = (hour: number, minutes = 60) => {
  const event = makeEvent(day.set({ hour }));
  event.title = 'Example';
  if (event.time.kind === 'timed')
    event.time.end = DateTime.fromISO(event.time.start).plus({ minutes }).toUTC().toISO()!;
  return event;
};

describe('calendar layout and prototype interactions', () => {
  it('normalizes POSIX locale tags and recovers from the C locale', () => {
    expect(resolveLocale('en_US.UTF-8')).toBe('en-US');
    expect(resolveLocale('de_CH.UTF-8')).toBe('de-CH');
    expect(resolveLocale('C')).toBe(new Intl.DateTimeFormat().resolvedOptions().locale);
  });
  it('covers leap February with six Monday-start weeks', () => {
    const days = monthDays(DateTime.fromISO('2028-02-01'), 1);
    expect(days).toHaveLength(42);
    expect(days[0].weekday).toBe(1);
    expect(days.some((item) => dateKey(item) === '2028-02-29')).toBe(true);
  });
  it('places touching events in one lane and overlapping events in separate lanes', () => {
    const events = [timed(9, 120), timed(10), timed(11)];
    const result = layoutTimedEvents(events, day);
    expect(result.map((item) => item.lanes)).toEqual([2, 2, 1]);
    expect(result[0].lane).not.toBe(result[1].lane);
  });
  it('uses the maximum lane count throughout a connected overlap group', () => {
    const events = [timed(9, 180), timed(10, 120), timed(11, 60)];
    expect(layoutTimedEvents(events, day).map((item) => item.lanes)).toEqual([3, 3, 3]);
  });
  it('excludes all-day end dates and includes events crossing midnight', () => {
    const allDay = makeEvent(day, true);
    const overnight = timed(23, 120);
    expect(eventsOnDate([allDay, overnight], day.plus({ days: 1 }))).toEqual([overnight]);
  });
  it('preserves all-day date spans across a DST boundary', () => {
    const event = makeEvent(day, true);
    const moved = moveEvent(event, DateTime.fromISO('2026-10-25', { zone: 'Europe/Zurich' }));
    expect(moved.time).toEqual({ kind: 'allDay', startDate: '2026-10-25', endDate: '2026-10-26' });
  });
  it('undoes creation, editing, and deletion without leaking mutable references', () => {
    const event = timed(9);
    const repo = new PrototypeRepository([]);
    repo.save(event);
    const snapshot = repo.list();
    snapshot[0].title = 'Changed outside';
    expect(repo.list()[0].title).toBe('Example');
    repo.save({ ...event, title: 'Edited' });
    repo.delete(event.id);
    repo.undo();
    expect(repo.list()[0].title).toBe('Edited');
    repo.undo();
    expect(repo.list()[0].title).toBe('Example');
    repo.undo();
    expect(repo.list()).toEqual([]);
  });
});
