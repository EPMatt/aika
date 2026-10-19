import { invoke, isTauri } from '@tauri-apps/api/core';
import { DateTime } from 'luxon';
import type { SyncConflict } from './generated/SyncConflict';
import type { EventSeries } from './generated/EventSeries';
import type { EventMutation } from './generated/EventMutation';
import type { CalendarEvent } from './generated/CalendarEvent';
import type { CalendarCollectionSnapshot } from './generated/CalendarCollectionSnapshot';
import type { CalendarDetails } from './generated/CalendarDetails';
import type { Calendar } from './generated/Calendar';
import type { EventRange } from './generated/EventRange';
import type { Preferences } from './generated/Preferences';
import type { ServiceError } from './generated/ServiceError';

import type { ProviderDescriptor } from './generated/ProviderDescriptor';
import type { ConnectionSession } from './generated/ConnectionSession';
import type { CalendarDiscovery } from './generated/CalendarDiscovery';
export const desktop = isTauri();
let pendingMutation: Promise<unknown> = Promise.resolve();
function mutate<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  const operation = pendingMutation.then(() => invoke<T>(command, args));
  pendingMutation = operation.catch(() => undefined);
  return operation;
}
export const calendarClient = {
  conflicts: () => invoke<SyncConflict[]>('sync_conflicts'),
  resolveConflict: (expected: SyncConflict, keepLocal: boolean) =>
    mutate<void>('resolve_sync_conflict', { expected, keepLocal }),
  retryOutgoing: (accountId: string) => mutate<void>('retry_outgoing', { accountId }),
  requestSync: (range: EventRange) => invoke<void>('request_sync', { range }),
  providers: () => invoke<ProviderDescriptor[]>('list_providers'),
  startConnection: (provider: string) =>
    invoke<ConnectionSession>('start_connection', { provider }),
  connectionStatus: (provider: string, id: string) =>
    invoke<ConnectionSession>('connection_status', { provider, id }),
  cancelConnection: (provider: string, id: string) =>
    invoke<void>('cancel_connection', { provider, id }),
  discoverCalendars: (accountId: string) =>
    invoke<CalendarDiscovery>('discover_calendars', { accountId }),
  commitSelection: (discoveryId: string, selected: string[]) =>
    mutate<void>('commit_calendar_selection', { discoveryId, selected }),
  disconnectAccount: (accountId: string) => invoke<void>('disconnect_account', { accountId }),
  getSeries: (target: string) => invoke<EventSeries>('get_event_series', { target }),
  mutateEvent: (request: EventMutation) =>
    mutate<CalendarEvent[]>('mutate_recurring_event', { request }),
  preview: (
    event: CalendarEvent,
    series: EventSeries | null = null,
    request: EventMutation | null = null,
  ) => invoke<CalendarEvent[]>('preview_recurrence', { event, series, request }),
  createCalendar: (details: CalendarDetails) => mutate<Calendar>('create_calendar', { details }),
  updateCalendar: (id: string, details: CalendarDetails, expected: Calendar) =>
    mutate<void>('update_calendar', { id, details, expected }),
  deleteCalendar: (id: string) => mutate<void>('delete_calendar', { id }),
  calendarEventCount: (id: string) => invoke<number>('calendar_event_count', { id }),
  setDefaultCalendar: (id: string) => mutate<void>('set_default_calendar', { id }),
  setCalendarVisibility: (id: string, visible: boolean) =>
    mutate<void>('set_calendar_visibility', { id, visible }),
  snapshot: (range: EventRange) =>
    invoke<CalendarCollectionSnapshot>('calendar_snapshot', { range }),
  save: (event: CalendarEvent, existing: boolean, expected: CalendarEvent | null) =>
    mutate<CalendarEvent>('save_event', { event, existing, expected }),
  delete: (id: string, expected: CalendarEvent) => mutate<void>('delete_event', { id, expected }),
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
