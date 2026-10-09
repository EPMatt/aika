import { DateTime, Settings } from 'luxon';
import type { CalendarEvent } from './generated/CalendarEvent';

export type View = 'month' | 'week' | 'day';
export type Appearance = 'system' | 'light' | 'dark';

export function resolveLocale(
  language = typeof navigator === 'undefined' ? undefined : navigator.language,
): string {
  if (language) {
    const normalized = language.split(/[.@]/)[0].replaceAll('_', '-');
    try {
      return Intl.getCanonicalLocales(normalized)[0];
    } catch {
      // Some Linux webviews expose POSIX tags (including C) instead of BCP 47.
    }
  }
  return new Intl.DateTimeFormat().resolvedOptions().locale;
}

Settings.defaultLocale = resolveLocale();

export function dateKey(date: DateTime): string {
  return date.toISODate()!;
}

export function firstWeekday(locale = resolveLocale()): number {
  // Luxon uses the same locale week settings as the calendar formatter.
  return DateTime.local().setLocale(resolveLocale(locale)).startOf('week', { useLocaleWeeks: true })
    .weekday;
}

export function weekStart(date: DateTime, start = firstWeekday()): DateTime {
  return date.startOf('day').minus({ days: (date.weekday - start + 7) % 7 });
}

export function monthDays(date: DateTime, start = firstWeekday()): DateTime[] {
  const first = weekStart(date.startOf('month'), start);
  return Array.from({ length: 42 }, (_, i) => first.plus({ days: i }));
}

export function daysForView(date: DateTime, view: View): DateTime[] {
  if (view === 'month') return monthDays(date);
  if (view === 'day') return [date.startOf('day')];
  return Array.from({ length: 7 }, (_, i) => weekStart(date).plus({ days: i }));
}

export function eventsOnDate(events: CalendarEvent[], day: DateTime): CalendarEvent[] {
  const key = dateKey(day);
  return events
    .filter(({ time }) =>
      time.kind === 'allDay'
        ? time.startDate <= key && time.endDate > key
        : DateTime.fromISO(time.start).toMillis() <
            day.plus({ days: 1 }).startOf('day').toMillis() &&
          DateTime.fromISO(time.end).toMillis() > day.startOf('day').toMillis(),
    )
    .sort((a, b) => {
      if (a.time.kind !== b.time.kind) return a.time.kind === 'allDay' ? -1 : 1;
      if (a.time.kind === 'timed' && b.time.kind === 'timed')
        return a.time.start.localeCompare(b.time.start);
      return a.title.localeCompare(b.title);
    });
}

export function timeLabel(iso: string): string {
  return DateTime.fromISO(iso).toLocaleString(DateTime.TIME_SIMPLE);
}

export function makeEvent(date: DateTime, allDay = false): CalendarEvent {
  const start = date.set({ second: 0, millisecond: 0 });
  return {
    id: crypto.randomUUID(),
    calendarId: 'local-default',
    title: '',
    location: '',
    notes: '',
    time: allDay
      ? { kind: 'allDay', startDate: dateKey(start), endDate: dateKey(start.plus({ days: 1 })) }
      : {
          kind: 'timed',
          start: start.toUTC().toISO()!,
          end: start.plus({ hours: 1 }).toUTC().toISO()!,
          timeZone: start.zoneName!,
        },
  };
}

export function moveEvent(event: CalendarEvent, target: DateTime): CalendarEvent {
  if (event.time.kind === 'allDay') {
    const span = DateTime.fromISO(event.time.endDate).diff(
      DateTime.fromISO(event.time.startDate),
      'days',
    ).days;
    return {
      ...event,
      time: {
        ...event.time,
        startDate: dateKey(target),
        endDate: dateKey(target.plus({ days: span })),
      },
    };
  }
  const oldStart = DateTime.fromISO(event.time.start);
  const duration = DateTime.fromISO(event.time.end).diff(oldStart).milliseconds;
  const start = target.set({
    hour: oldStart.hour,
    minute: oldStart.minute,
    second: 0,
    millisecond: 0,
  });
  return {
    ...event,
    time: {
      ...event.time,
      start: start.toUTC().toISO()!,
      end: start.plus({ milliseconds: duration }).toUTC().toISO()!,
    },
  };
}

export type PositionedEvent = {
  event: CalendarEvent;
  start: number;
  end: number;
  lane: number;
  lanes: number;
};

export function layoutTimedEvents(events: CalendarEvent[], day: DateTime): PositionedEvent[] {
  const startOfDay = day.startOf('day');
  const positioned: PositionedEvent[] = eventsOnDate(events, day)
    .filter((event) => event.time.kind === 'timed')
    .map((event) => {
      if (event.time.kind !== 'timed') throw new Error('Expected timed event');
      const start = DateTime.max(DateTime.fromISO(event.time.start), startOfDay);
      const end = DateTime.min(DateTime.fromISO(event.time.end), startOfDay.plus({ days: 1 }));
      return {
        event,
        start: start.hour * 60 + start.minute,
        end: end.hasSame(startOfDay, 'day') ? end.hour * 60 + end.minute : 1440,
        lane: 0,
        lanes: 1,
      };
    })
    .sort((a, b) => a.start - b.start || b.end - a.end);
  let group: PositionedEvent[] = [];
  let groupEnd = -1;
  let laneEnds: number[] = [];
  const finishGroup = () => {
    for (const item of group) item.lanes = laneEnds.length;
  };
  for (const item of positioned) {
    if (item.start >= groupEnd) {
      finishGroup();
      group = [];
      laneEnds = [];
      groupEnd = -1;
    }
    let lane = laneEnds.findIndex((end) => end <= item.start);
    if (lane < 0) lane = laneEnds.length;
    laneEnds[lane] = item.end;
    item.lane = lane;
    group.push(item);
    groupEnd = Math.max(groupEnd, item.end);
  }
  finishGroup();
  return positioned;
}

export function layoutAllDaySpans(events: CalendarEvent[], days: DateTime[]) {
  const first = dateKey(days[0]);
  const last = dateKey(days[days.length - 1].plus({ days: 1 }));
  const laneEnds: number[] = [];
  return events
    .filter(
      (event) =>
        event.time.kind === 'allDay' && event.time.startDate < last && event.time.endDate > first,
    )
    .sort((a, b) =>
      a.time.kind === 'allDay' && b.time.kind === 'allDay'
        ? a.time.startDate.localeCompare(b.time.startDate) ||
          b.time.endDate.localeCompare(a.time.endDate)
        : 0,
    )
    .map((event) => {
      if (event.time.kind !== 'allDay') throw new Error('Expected all-day event');
      const time = event.time;
      const start = Math.max(
        0,
        days.findIndex((day) => dateKey(day) >= time.startDate),
      );
      const endIndex = days.findIndex((day) => dateKey(day) >= time.endDate);
      const end = endIndex < 0 ? days.length : endIndex;
      let lane = laneEnds.findIndex((value) => value <= start);
      if (lane < 0) lane = laneEnds.length;
      laneEnds[lane] = end;
      return { event, start, end, lane };
    });
}
