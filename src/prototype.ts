import { DateTime } from 'luxon';
import { recurrenceDomain } from './recurrence';
import type { EventSeries } from './generated/EventSeries';
import type { EventMutation } from './generated/EventMutation';
import type { SeriesChange } from './generated/SeriesChange';
import type { EventRange } from './generated/EventRange';
import { dateKey, makeEvent } from './calendar';
import { personalCalendar } from './calendars';
import type { Calendar } from './generated/Calendar';
import type { CalendarDetails } from './generated/CalendarDetails';
import type { CalendarCollectionSnapshot } from './generated/CalendarCollectionSnapshot';
import type { CalendarEvent } from './generated/CalendarEvent';

export type Scenario = 'standard' | 'empty' | 'dense';

// Browser design fixtures only; native startup never loads these events.
const fixtureId = (value: number) =>
  `00000000-0000-4000-8000-${value.toString(16).padStart(12, '0')}`;
export function sampleEvents(today: DateTime, scenario: Scenario): CalendarEvent[] {
  if (scenario === 'empty') return [];
  const month = today.startOf('month');
  const examples: Array<[number, number, string, string?]> = [
    [2, 10, 'A slow start'],
    [4, 9, 'Weekly planning'],
    [5, 14, 'Design exploration'],
    [7, 10, 'Coffee with Alex', 'The little café'],
    [9, 9, 'Space to focus'],
    [9, 13, 'Lunch with Jamie', 'By the river'],
    [12, 10, 'Fresh perspectives'],
    [13, 16, 'An afternoon walk'],
    [18, 11, 'Read & reflect'],
    [20, 10, 'A new chapter'],
    [22, 15, 'Make something'],
    [24, 10, 'Coffee with Sam'],
    [26, 9, 'Weekly planning'],
    [27, 14, 'Studio afternoon'],
    [29, 18, 'Dinner with friends', 'At home'],
  ];
  const events = examples.map(([day, hour, title, location]) => {
    const event = makeEvent(month.plus({ days: day - 1 }).set({ hour }));
    return { ...event, id: fixtureId(day * 24 + hour), title, location: location ?? '' };
  });
  events.push({
    ...makeEvent(month.plus({ days: 14 }), true),
    id: fixtureId(999),
    title: 'A little time away',
    time: {
      kind: 'allDay',
      startDate: dateKey(month.plus({ days: 14 })),
      endDate: dateKey(month.plus({ days: 17 })),
    },
  });
  // Give the current week a useful mixture, regardless of the month used in review.
  for (const [offset, hour, title, duration] of [
    [0, 10, 'Room to think', 2],
    [0, 11, 'A quick catch-up', 1],
    [1, 14, 'Creative afternoon', 2],
    [-1, 16, 'A breath of fresh air', 1],
  ] as const) {
    const event = makeEvent(today.plus({ days: offset }).set({ hour, minute: 0 }));
    if (event.time.kind === 'timed')
      event.time.end = DateTime.fromISO(event.time.start)
        .plus({ hours: duration })
        .toUTC()
        .toISO()!;
    events.push({ ...event, id: fixtureId(1000 + (offset + 1) * 24 + hour), title });
  }
  if (scenario === 'dense') {
    for (let i = 0; i < 14; i++) {
      events.push({
        ...makeEvent(today.set({ hour: 8 + Math.floor(i / 2), minute: (i % 2) * 15 })),
        id: fixtureId(2000 + i),
        title: `A busy day · ${i + 1}`,
      });
    }
  }
  return events;
}

export class PrototypeRepository {
  private series: EventSeries[];
  private calendars: Calendar[] = [structuredClone(personalCalendar)];
  private defaultCalendarId: string | null = personalCalendar.id;
  private hiddenCalendarIds: string[] = [];
  private history: SeriesChange[][] = [];
  constructor(events: CalendarEvent[]) {
    this.series = structuredClone(events.map((event) => ({ event, exceptions: [] })));
  }
  list() {
    return structuredClone(this.series.map((s) => s.event));
  }
  getSeries(target: string): EventSeries {
    const id = target.split('@')[0];
    const series = this.series.find((s) => s.event.id === id);
    if (!series) throw new Error('This event no longer exists.');
    return structuredClone(series);
  }
  snapshot(range?: EventRange): CalendarCollectionSnapshot {
    const events = this.series.flatMap((series) =>
      series.event.recurrence && range ? recurrenceDomain.expand(series, range) : [series.event],
    );
    return structuredClone({
      calendars: this.calendars,
      events,
      defaultCalendarId: this.defaultCalendarId,
      hiddenCalendarIds: this.hiddenCalendarIds,
      canUndo: this.canUndo(),
      accounts: [],
      syncStatus: [],
      calendarCapabilities: Object.fromEntries(
        this.calendars.map((c) => [
          c.id,
          {
            read: true,
            create: c.writable,
            edit: c.writable,
            delete: c.writable,
            recurrence: c.writable,
            customizeMetadata: true,
            moveEvent: c.writable,
          },
        ]),
      ),
      eventCapabilities: Object.fromEntries(
        events.map((e) => [
          e.id,
          {
            read: true,
            create: true,
            edit: true,
            delete: true,
            recurrence: true,
            customizeMetadata: true,
            moveEvent: true,
          },
        ]),
      ),
    });
  }
  canUndo() {
    return this.history.length > 0;
  }
  private apply(changes: SeriesChange[], undo = false) {
    for (const change of changes) {
      const value = undo ? change.before : change.after;
      this.series = this.series.filter((s) => s.event.id !== change.id);
      if (value) {
        this.series.push(structuredClone(value));
        if (!undo) {
          this.setCalendarVisibility(value.event.calendarId, true);
          for (const exception of value.exceptions)
            if (exception.event) this.setCalendarVisibility(exception.event.calendarId, true);
        }
      }
    }
  }
  mutate(request: EventMutation) {
    const changes = recurrenceDomain.plan(this.getSeries(request.target), request);
    this.apply(changes);
    this.history.push(changes);
  }
  save(event: CalendarEvent) {
    if (event.recurrence) event = recurrenceDomain.validate(event);
    const before = this.series.find((s) => s.event.id === event.id) ?? null;
    const changes = [
      {
        id: event.id,
        before: structuredClone(before),
        after: { event: structuredClone(event), exceptions: [] },
      },
    ];
    this.apply(changes);
    this.history.push(changes);
  }
  delete(id: string) {
    const before = this.series.find((s) => s.event.id === id) ?? null;
    const changes = [{ id, before: structuredClone(before), after: null }];
    this.apply(changes);
    this.history.push(changes);
  }
  undo() {
    const changes = this.history.pop();
    if (changes) this.apply(changes, true);
  }
  createCalendar(details: CalendarDetails) {
    const calendar = {
      ...details,
      name: details.name.trim(),
      id: crypto.randomUUID(),
      providerId: 'local',
      writable: true,
    };
    this.calendars.push(calendar);
    this.defaultCalendarId ??= calendar.id;
    return structuredClone(calendar);
  }
  updateCalendar(id: string, details: CalendarDetails) {
    this.calendars = this.calendars.map((c) =>
      c.id === id ? { ...c, ...details, name: details.name.trim() } : c,
    );
  }
  deleteCalendar(id: string) {
    this.calendars = this.calendars.filter((c) => c.id !== id);
    this.series = this.series
      .filter((s) => s.event.calendarId !== id)
      .map((s) => ({
        ...s,
        exceptions: s.exceptions.map((e) =>
          e.event?.calendarId === id ? { ...e, event: null } : e,
        ),
      }));
    this.hiddenCalendarIds = this.hiddenCalendarIds.filter((c) => c !== id);
    const removedIds = new Set(
      this.history.flatMap((changes) =>
        changes
          .filter((c) =>
            [c.before, c.after].some(
              (s) =>
                s &&
                (s.event.calendarId === id || s.exceptions.some((e) => e.event?.calendarId === id)),
            ),
          )
          .map((c) => c.id),
      ),
    );
    this.history = this.history.filter((changes) => changes.every((c) => !removedIds.has(c.id)));
    if (this.defaultCalendarId === id) this.defaultCalendarId = this.calendars[0]?.id ?? null;
  }
  calendarEventCount(id: string) {
    return (
      this.series.filter((s) => s.event.calendarId === id).length +
      this.series.flatMap((s) =>
        s.exceptions.filter((e) => e.event?.calendarId === id && s.event.calendarId !== id),
      ).length
    );
  }
  setDefaultCalendar(id: string) {
    this.defaultCalendarId = id;
  }
  setCalendarVisibility(id: string, visible: boolean) {
    this.hiddenCalendarIds = this.hiddenCalendarIds.filter((c) => c !== id);
    if (!visible) this.hiddenCalendarIds.push(id);
  }
}
