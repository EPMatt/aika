import init, * as wasm from './wasm/aika_domain';
import type { CalendarEvent } from './generated/CalendarEvent';
import type { EventRange } from './generated/EventRange';
import type { EventSeries } from './generated/EventSeries';
import type { EventMutation } from './generated/EventMutation';
import type { SeriesChange } from './generated/SeriesChange';
import { calendarClient, desktop } from './client';

export const initializeRecurrence = init;
function call<T>(operation: () => string): T {
  try {
    return JSON.parse(operation()) as T;
  } catch (error) {
    if (typeof error === 'string') {
      try {
        throw JSON.parse(error);
      } catch (parsed) {
        if (!(parsed instanceof SyntaxError)) throw parsed;
      }
    }
    throw error;
  }
}
export const recurrenceDomain = {
  expand: (series: EventSeries, range: EventRange) =>
    call<CalendarEvent[]>(() => wasm.expand(JSON.stringify(series), JSON.stringify(range))),
  preview: (event: CalendarEvent) =>
    call<CalendarEvent[]>(() => wasm.preview(JSON.stringify(event))),
  getOccurrence: (series: EventSeries, key: string) =>
    call<CalendarEvent>(() => wasm.get_occurrence(JSON.stringify(series), key)),
  expectation: (series: EventSeries) => wasm.expectation(JSON.stringify(series)),
  plan: (series: EventSeries, request: EventMutation) =>
    call<SeriesChange[]>(() =>
      wasm.plan_mutation(JSON.stringify(series), JSON.stringify(request), crypto.randomUUID()),
    ),
  validate: (event: CalendarEvent) =>
    call<CalendarEvent>(() => wasm.validate(JSON.stringify(event))),
};
export async function previewRecurrence(event: CalendarEvent): Promise<CalendarEvent[]> {
  return desktop ? calendarClient.preview(event) : recurrenceDomain.preview(event);
}
