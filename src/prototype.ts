import { DateTime } from 'luxon';
import { dateKey, makeEvent } from './calendar';
import { personalCalendar } from './calendars';
import type { Calendar } from './generated/Calendar';
import type { CalendarDetails } from './generated/CalendarDetails';
import type { CalendarCollectionSnapshot } from './generated/CalendarCollectionSnapshot';
import type { CalendarEvent } from './generated/CalendarEvent';

export type Scenario = 'standard' | 'empty' | 'dense';

// Design fixtures only. Phase 2 replaces this repository with Rust-backed IPC.
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
    return { ...event, id: `sample-${day}-${hour}`, title, location: location ?? '' };
  });
  events.push({
    ...makeEvent(month.plus({ days: 14 }), true),
    id: 'sample-away',
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
    events.push({ ...event, id: `sample-relative-${offset}-${hour}`, title });
  }
  if (scenario === 'dense') {
    for (let i = 0; i < 14; i++) {
      events.push({
        ...makeEvent(today.set({ hour: 8 + Math.floor(i / 2), minute: (i % 2) * 15 })),
        id: `dense-${i}`,
        title: `A busy day · ${i + 1}`,
      });
    }
  }
  return events;
}

export class PrototypeRepository {
  private events: CalendarEvent[];
  private calendars: Calendar[] = [structuredClone(personalCalendar)];
  private defaultCalendarId: string | null = personalCalendar.id;
  private hiddenCalendarIds: string[] = [];
  private history: { before: CalendarEvent | null; after: CalendarEvent | null }[] = [];
  constructor(events: CalendarEvent[]) {
    this.events = structuredClone(events);
  }
  list() {
    return structuredClone(this.events);
  }
  snapshot(): CalendarCollectionSnapshot {
    return structuredClone({
      calendars: this.calendars,
      events: this.events,
      defaultCalendarId: this.defaultCalendarId,
      hiddenCalendarIds: this.hiddenCalendarIds,
      canUndo: this.canUndo(),
    });
  }
  canUndo() {
    return this.history.length > 0;
  }
  save(event: CalendarEvent) {
    const before = this.events.find((e) => e.id === event.id) ?? null;
    this.history.push({ before: structuredClone(before), after: structuredClone(event) });
    this.events = [...this.events.filter((item) => item.id !== event.id), structuredClone(event)];
    if (!before || before.calendarId !== event.calendarId)
      this.setCalendarVisibility(event.calendarId, true);
  }
  delete(id: string) {
    const before = this.events.find((e) => e.id === id) ?? null;
    this.history.push({ before: structuredClone(before), after: null });
    this.events = this.events.filter((event) => event.id !== id);
  }
  undo() {
    const entry = this.history.pop();
    if (!entry) return;
    const id = entry.before?.id ?? entry.after!.id;
    this.events = this.events.filter((e) => e.id !== id);
    if (entry.before) this.events.push(entry.before);
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
    this.events = this.events.filter((e) => e.calendarId !== id);
    this.hiddenCalendarIds = this.hiddenCalendarIds.filter((c) => c !== id);
    this.history = this.history.filter(
      (e) => e.before?.calendarId !== id && e.after?.calendarId !== id,
    );
    if (this.defaultCalendarId === id) this.defaultCalendarId = this.calendars[0]?.id ?? null;
  }
  calendarEventCount(id: string) {
    return this.events.filter((e) => e.calendarId === id).length;
  }
  setDefaultCalendar(id: string) {
    this.defaultCalendarId = id;
  }
  setCalendarVisibility(id: string, visible: boolean) {
    this.hiddenCalendarIds = this.hiddenCalendarIds.filter((c) => c !== id);
    if (!visible) this.hiddenCalendarIds.push(id);
  }
}
