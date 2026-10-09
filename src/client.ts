import { invoke, isTauri } from '@tauri-apps/api/core';
import { DateTime } from 'luxon';
import type { CalendarEvent } from './generated/CalendarEvent';
import type { CalendarSnapshot } from './generated/CalendarSnapshot';
import type { EventRange } from './generated/EventRange';
import type { Preferences } from './generated/Preferences';
import type { ServiceError } from './generated/ServiceError';

export const desktop = isTauri();
let pendingMutation: Promise<unknown> = Promise.resolve();
function mutate<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  const operation = pendingMutation.then(() => invoke<T>(command, args));
  pendingMutation = operation.catch(() => undefined);
  return operation;
}
export const calendarClient = {
  snapshot: (range: EventRange) => invoke<CalendarSnapshot>('calendar_snapshot', { range }),
  save: (event: CalendarEvent, existing: boolean) =>
    mutate<CalendarEvent>('save_event', { event, existing }),
  delete: (id: string) => mutate<void>('delete_event', { id }),
  undo: () => mutate<void>('undo_event'),
  preferences: () => invoke<Preferences>('get_preferences'),
  setPreferences: (preferences: Preferences) => mutate<void>('set_preferences', { preferences }),
};
export function errorMessage(error: unknown): string {
  return typeof error === 'object' && error !== null && 'message' in error
    ? String(error.message)
    : String(error);
}
export async function resolveWallTime(
  local: string,
  offset?: number,
  timeZone = DateTime.local().zoneName!,
): Promise<string> {
  if (desktop)
    return invoke<string>('resolve_local_time', { local, timeZone, offset: offset ?? null });
  const value = DateTime.fromISO(local, { zone: timeZone });
  if (!value.isValid || value.toFormat("yyyy-MM-dd'T'HH:mm") !== local)
    throw {
      code: 'nonexistentTime',
      message: 'This time does not exist because the clocks move forward. Choose another time.',
      offsets: [],
    } satisfies ServiceError;
  const candidates = value.getPossibleOffsets();
  if (candidates.length > 1 && offset === undefined)
    throw {
      code: 'ambiguousTime',
      message: 'This time occurs twice. Choose its UTC offset.',
      offsets: candidates.map((v) => v.offset),
    } satisfies ServiceError;
  const chosen = candidates.find((v) => offset === undefined || v.offset === offset);
  if (!chosen) throw new Error('The selected offset does not match this time.');
  return chosen.toUTC().toISO()!;
}
