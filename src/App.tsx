import type { EventActions } from './generated/EventActions';
import type { SyncConflict } from './generated/SyncConflict';
import type { CalendarRemovalReview } from './generated/CalendarRemovalReview';
import { createContext, useContext, useCallback, useEffect, useRef, useState } from 'react';
import type { CSSProperties, FormEvent, KeyboardEvent, MouseEvent } from 'react';
import * as Popover from '@radix-ui/react-popover';
import * as Dialog from '@radix-ui/react-dialog';
import {
  CalendarDays,
  ChevronLeft,
  ChevronRight,
  Clock3,
  Eye,
  EyeOff,
  Keyboard,
  MapPin,
  PanelLeft,
  Plus,
  Repeat2,
  Settings2,
  Trash2,
  Undo2,
  X,
} from 'lucide-react';
import { DateTime } from 'luxon';
import { listen } from '@tauri-apps/api/event';
import { invoke, isTauri } from '@tauri-apps/api/core';
import type { AppInfo } from './generated/AppInfo';
import type { Calendar } from './generated/Calendar';
import type { CalendarCollectionSnapshot } from './generated/CalendarCollectionSnapshot';
import { personalCalendar } from './calendars';
import type { CalendarEvent } from './generated/CalendarEvent';
import {
  dateKey,
  daysForView,
  eventsOnDate,
  firstWeekday,
  layoutTimedEvents,
  layoutAllDaySpans,
  makeEvent,
  monthDays,
  moveEvent,
  timeLabel,
  resolveLocale,
  weekStart,
} from './calendar';
import type { Appearance, View } from './calendar';
import type { PrototypeRepository } from './prototype';
import { calendarClient, desktop, errorMessage, resolveWallTime } from './client';
import { ProviderControls } from './ProviderControls';
import type { AccountSyncStatus } from './generated/AccountSyncStatus';
import type { AccountSummary } from './generated/AccountSummary';
import { CalendarForm, CalendarMenu } from './CalendarControls';
import { RepeatControls } from './RepeatControls';
import { previewRecurrence, recurrenceDomain } from './recurrence';
import type { Recurrence } from './generated/Recurrence';
import type { MutationScope } from './generated/MutationScope';
import type { EventSeries } from './generated/EventSeries';
import type { EventMutation } from './generated/EventMutation';
import { SidebarPopup } from './SidebarPopup';
import { KeyboardShortcuts } from './KeyboardShortcuts';
import type { ServiceError } from './generated/ServiceError';
import type { Capabilities } from './generated/Capabilities';

const defaultCalendar = personalCalendar;
const emptyCalendar: Calendar = {
  ...personalCalendar,
  id: '',
  name: 'No calendar',
  writable: false,
};
const CalendarContext = createContext(defaultCalendar);
const CapabilitiesContext = createContext<{
  calendars: Record<string, Capabilities>;
  events: Record<string, Capabilities>;
  actions?: Record<string, EventActions>;
}>({ calendars: {}, events: {} });
const CalendarsContext = createContext<Calendar[]>([defaultCalendar]);
function CalendarProviders({
  calendars,
  capabilities,
  children,
}: {
  calendars: Calendar[];
  capabilities: {
    calendars: Record<string, Capabilities>;
    events: Record<string, Capabilities>;
    actions?: Record<string, EventActions>;
  };
  children: React.ReactNode;
}) {
  return (
    <CapabilitiesContext.Provider value={capabilities}>
      <CalendarsContext.Provider value={calendars}>{children}</CalendarsContext.Provider>
    </CapabilitiesContext.Provider>
  );
}
function useEventCalendar(event: CalendarEvent) {
  const calendar =
    useContext(CalendarsContext).find((c) => c.id === event.calendarId) ?? emptyCalendar;
  const capabilities = useContext(CapabilitiesContext);
  return editableEventCalendar(calendar, event, capabilities);
}
function editableEventCalendar(
  calendar: Calendar,
  event: CalendarEvent,
  capabilities: { calendars: Record<string, Capabilities>; events: Record<string, Capabilities> },
) {
  const caps =
    capabilities.events[event.id] ??
    capabilities.events[event.id.split('@')[0]] ??
    capabilities.calendars[event.calendarId];
  return {
    ...calendar,
    writable:
      calendar.writable &&
      (caps?.edit ?? true) &&
      (!(event.recurrence || event.occurrence) || (caps?.recurrence ?? true)),
  };
}
function calendarStyle(calendar: Calendar): CSSProperties {
  return { '--calendar-color': calendar.color } as CSSProperties;
}

const HOUR_HEIGHT = 72;
const EDITOR_MARGIN = 18;
type ClickPosition = { x: number; y: number };
type CreateEvent = (
  date: DateTime,
  anchor: HTMLElement,
  allDay?: boolean,
  end?: DateTime,
  position?: ClickPosition,
) => void;

function editorPosition(anchor: HTMLElement, position?: ClickPosition) {
  return {
    getBoundingClientRect: () => {
      const rect = position
        ? new DOMRect(position.x, position.y, 0, 0)
        : anchor.getBoundingClientRect();
      const clampX = (x: number) =>
        Math.max(EDITOR_MARGIN, Math.min(x, window.innerWidth - EDITOR_MARGIN));
      const clampY = (y: number) =>
        Math.max(EDITOR_MARGIN, Math.min(y, window.innerHeight - EDITOR_MARGIN));
      const left = clampX(rect.left);
      const top = clampY(rect.top);
      return new DOMRect(left, top, clampX(rect.right) - left, clampY(rect.bottom) - top);
    },
  };
}

const DRAG_TYPE = 'application/x-cal-event';
const params = new URLSearchParams(window.location.search);
const initialDate =
  !desktop && params.get('date') && DateTime.fromISO(params.get('date')!).isValid
    ? DateTime.fromISO(params.get('date')!)
    : DateTime.local();
const initialAppearance = (): Appearance => {
  const value = desktop
    ? 'system'
    : (params.get('theme') ?? localStorage.getItem('cal.appearance'));
  return value === 'light' || value === 'dark' ? value : 'system';
};

function IconButton({
  label,
  children,
  onClick,
  disabled,
  className = '',
}: {
  label: string;
  children: React.ReactNode;
  onClick?: (event: MouseEvent<HTMLButtonElement>) => void;
  disabled?: boolean;
  className?: string;
}) {
  return (
    <button
      type="button"
      className={`icon-button ${className}`}
      aria-label={label}
      title={label}
      onClick={onClick}
      disabled={disabled}
    >
      {children}
    </button>
  );
}

function MiniCalendar({
  selected,
  today,
  onSelect,
}: {
  selected: DateTime;
  today: DateTime;
  onSelect: (date: DateTime) => void;
}) {
  const [month, setMonth] = useState(selected.startOf('month'));
  useEffect(() => setMonth(selected.startOf('month')), [selected.month, selected.year]);
  const days = monthDays(month);
  const weekdays = days.slice(0, 7);
  return (
    <section className="mini-calendar" aria-label="Date navigator">
      <div className="mini-heading">
        <span>{month.toFormat('LLLL yyyy')}</span>
        <div className="mini-arrows">
          <IconButton
            label="Previous mini-calendar month"
            onClick={() => setMonth(month.minus({ months: 1 }))}
          >
            <ChevronLeft size={13} />
          </IconButton>
          <IconButton
            label="Next mini-calendar month"
            onClick={() => setMonth(month.plus({ months: 1 }))}
          >
            <ChevronRight size={13} />
          </IconButton>
        </div>
      </div>
      <div className="mini-grid">
        <div className="mini-weekdays">
          {weekdays.map((day) => (
            <span key={day.weekday}>{day.toFormat('ccccc')}</span>
          ))}
        </div>
        {days.map((day) => (
          <button
            key={dateKey(day)}
            aria-label={day.toLocaleString(DateTime.DATE_FULL)}
            aria-pressed={day.hasSame(selected, 'day')}
            onClick={() => onSelect(day)}
            className={`mini-day ${day.month !== month.month ? 'outside' : ''} ${day.hasSame(selected, 'day') ? 'selected' : ''} ${day.hasSame(today, 'day') ? 'today' : ''}`}
          >
            {day.day}
          </button>
        ))}
      </div>
    </section>
  );
}

function EventChip({
  event,
  onClick,
  compact = false,
  day,
}: {
  event: CalendarEvent;
  onClick: (event: CalendarEvent, anchor: HTMLElement) => void;
  compact?: boolean;
  day?: DateTime;
}) {
  const calendar = useEventCalendar(event);
  return (
    <button
      type="button"
      style={calendarStyle(calendar)}
      className={`event-chip ${event.time.kind === 'allDay' ? 'all-day-chip' : ''} ${compact ? 'compact' : ''}`}
      draggable={calendar.writable}
      onDragStart={(e) => {
        e.dataTransfer.setData(DRAG_TYPE, event.id);
        e.dataTransfer.effectAllowed = 'move';
      }}
      onClick={(e) => {
        e.stopPropagation();
        onClick(event, e.currentTarget);
      }}
      aria-label={`${event.title || 'Untitled event'}${event.recurrence ? ', recurring' : ''}, ${calendar.name}, ${event.time.kind === 'allDay' ? `all day, ${event.time.startDate} through ${DateTime.fromISO(event.time.endDate).minus({ days: 1 }).toISODate()}` : timeLabel(event.time.start)}`}
    >
      {event.recurrence && <Repeat2 size={11} aria-label="Recurring event" />}
      {event.time.kind === 'timed' && <span className="event-dot" />}
      {day && event.time.kind === 'allDay' && event.time.startDate < dateKey(day) && (
        <span aria-hidden="true">‹</span>
      )}
      <span className="event-title">{event.title || 'Untitled event'}</span>
      {day &&
        event.time.kind === 'allDay' &&
        event.time.endDate > dateKey(day.plus({ days: 1 })) && <span aria-hidden="true">›</span>}
      {event.time.kind === 'timed' && !compact && (
        <span className="event-time">{timeLabel(event.time.start)}</span>
      )}
    </button>
  );
}

function MonthView({
  date,
  today,
  events,
  onSelect,
  onCreate,
  onEdit,
  onMove,
}: {
  date: DateTime;
  today: DateTime;
  events: CalendarEvent[];
  onSelect: (date: DateTime) => void;
  onCreate: CreateEvent;
  onEdit: (event: CalendarEvent, anchor: HTMLElement) => void;
  onMove: (id: string, date: DateTime, precise?: boolean) => void;
}) {
  const calendar = useContext(CalendarContext);
  const days = monthDays(date);
  const [overflow, setOverflow] = useState<string | null>(null);
  const [capacity, setCapacity] = useState({ full: 3, overflow: 2 });
  const gridRef = useRef<HTMLDivElement>(null);
  const keyboardFocus = useRef(false);
  const rangeStart = useRef<DateTime | null>(null);
  const [rangeEnd, setRangeEnd] = useState<string | null>(null);
  const suppressClick = useRef(false);
  useEffect(() => {
    const cancel = (e: globalThis.KeyboardEvent) => {
      if (e.key === 'Escape' && rangeStart.current) {
        rangeStart.current = null;
        setRangeEnd(null);
        suppressClick.current = true;
      }
    };
    window.addEventListener('keydown', cancel);
    return () => window.removeEventListener('keydown', cancel);
  }, []);
  useEffect(() => {
    if (!gridRef.current) return;
    const observer = new ResizeObserver((entries) => {
      const rowHeight = entries[0].contentRect.height / 6;
      const compact = window.innerHeight <= 760;
      const headingSpace = compact ? 36 : 46;
      const eventHeight = compact ? 21 : 26;
      setCapacity({
        full: Math.max(0, Math.min(3, Math.floor((rowHeight - headingSpace) / eventHeight))),
        overflow: Math.max(
          0,
          Math.min(3, Math.floor((rowHeight - headingSpace - 19) / eventHeight)),
        ),
      });
    });
    observer.observe(gridRef.current);
    return () => observer.disconnect();
  }, []);
  useEffect(() => {
    if (keyboardFocus.current) {
      gridRef.current?.querySelector<HTMLElement>(`[data-date="${dateKey(date)}"]`)?.focus();
      keyboardFocus.current = false;
    }
  }, [date]);
  const keyNavigate = (e: KeyboardEvent<HTMLDivElement>, day: DateTime) => {
    if (e.target !== e.currentTarget) return;
    const offsets: Record<string, number> = {
      ArrowLeft: -1,
      ArrowRight: 1,
      ArrowUp: -7,
      ArrowDown: 7,
    };
    if (e.key in offsets) {
      e.preventDefault();
      keyboardFocus.current = true;
      onSelect(day.plus({ days: offsets[e.key] }));
    }
    if (e.key === 'Enter') {
      e.preventDefault();
      onCreate(day.set({ hour: 9 }), e.currentTarget, true);
    }
  };
  return (
    <div className="month-view" role="region" aria-label="Month calendar">
      <div className="weekday-heading">
        {days.slice(0, 7).map((day) => (
          <span key={day.weekday}>{day.toFormat('ccc')}</span>
        ))}
      </div>
      <div className="month-grid" role="grid" ref={gridRef} aria-label={date.toFormat('LLLL yyyy')}>
        {Array.from({ length: 6 }, (_, row) => (
          <div
            className="month-row"
            role="row"
            key={row}
            onDragOver={(e) => {
              if (Array.from(e.dataTransfer.types).includes(DRAG_TYPE)) e.preventDefault();
            }}
            onDrop={(e) => {
              if ((e.target as HTMLElement).closest('.month-cell')) return;
              e.preventDefault();
              const rect = e.currentTarget.getBoundingClientRect();
              const column = Math.min(
                6,
                Math.max(0, Math.floor(((e.clientX - rect.left) / rect.width) * 7)),
              );
              onMove(e.dataTransfer.getData(DRAG_TYPE), days[row * 7 + column]);
            }}
          >
            <div className="month-spans">
              {layoutAllDaySpans(events, days.slice(row * 7, row * 7 + 7))
                .filter((span) => span.lane < capacity.overflow)
                .map(({ event, start, end, lane }) => (
                  <div
                    key={event.id}
                    className="month-span"
                    role="gridcell"
                    aria-colindex={start + 1}
                    aria-colspan={end - start}
                    style={{ gridColumn: `${start + 1} / ${end + 1}`, gridRow: lane + 1 }}
                  >
                    <EventChip event={event} day={days[row * 7 + start]} onClick={onEdit} />
                  </div>
                ))}
            </div>
            {days.slice(row * 7, row * 7 + 7).map((day) => {
              const allItems = eventsOnDate(events, day);
              const column = days
                .slice(row * 7, row * 7 + 7)
                .findIndex((value) => dateKey(value) === dateKey(day));
              const visibleSpans = layoutAllDaySpans(
                events,
                days.slice(row * 7, row * 7 + 7),
              ).filter(
                (span) =>
                  span.lane < capacity.overflow && span.start <= column && span.end > column,
              );
              const reserved = visibleSpans.length
                ? Math.max(...visibleSpans.map((span) => span.lane)) + 1
                : 0;
              const items = allItems.filter(
                (event) => !visibleSpans.some((span) => span.event.id === event.id),
              );
              const full = Math.max(0, capacity.full - reserved);
              const limit =
                items.length <= full && items.every((event) => event.time.kind === 'timed')
                  ? full
                  : Math.max(0, capacity.overflow - reserved);
              const chips = items.filter((event) => event.time.kind === 'timed').slice(0, limit);
              const key = dateKey(day);
              return (
                <div
                  key={key}
                  role="gridcell"
                  aria-label={day.toLocaleString(DateTime.DATE_FULL)}
                  aria-selected={day.hasSame(date, 'day')}
                  tabIndex={day.hasSame(date, 'day') ? 0 : -1}
                  data-date={key}
                  className={`month-cell ${day.month !== date.month ? 'outside' : ''} ${day.hasSame(today, 'day') ? 'is-today' : ''} ${rangeStart.current && rangeEnd && key >= [dateKey(rangeStart.current), rangeEnd].sort()[0] && key <= [dateKey(rangeStart.current), rangeEnd].sort()[1] ? 'range-selected' : ''}`}
                  onKeyDown={(e) => keyNavigate(e, day)}
                  onPointerDown={(e) => {
                    if (
                      !calendar.writable ||
                      e.button !== 0 ||
                      (e.target as HTMLElement).closest('button')
                    )
                      return;
                    rangeStart.current = day;
                    e.currentTarget.setPointerCapture(e.pointerId);
                  }}
                  onPointerMove={(e) => {
                    if (!rangeStart.current) return;
                    const cell = document
                      .elementFromPoint(e.clientX, e.clientY)
                      ?.closest<HTMLElement>('[data-date]');
                    if (cell?.dataset.date) setRangeEnd(cell.dataset.date);
                  }}
                  onPointerCancel={() => {
                    rangeStart.current = null;
                    setRangeEnd(null);
                  }}
                  onPointerUp={(e) => {
                    const first = rangeStart.current;
                    rangeStart.current = null;
                    setRangeEnd(null);
                    const key = document
                      .elementFromPoint(e.clientX, e.clientY)
                      ?.closest<HTMLElement>('[data-date]')?.dataset.date;
                    if (!first || !key || key === dateKey(first)) return;
                    suppressClick.current = true;
                    const last = DateTime.fromISO(key);
                    onCreate(
                      DateTime.min(first, last),
                      e.currentTarget,
                      true,
                      DateTime.max(first, last).plus({ days: 1 }),
                    );
                  }}
                  onClick={(e) => {
                    if (suppressClick.current) {
                      suppressClick.current = false;
                      return;
                    }
                    if ((e.target as HTMLElement).closest('button')) return;
                    onSelect(day);
                    onCreate(day.set({ hour: 9 }), e.currentTarget, true, undefined, {
                      x: e.clientX,
                      y: e.clientY,
                    });
                  }}
                  onDragOver={(e) => {
                    if (Array.from(e.dataTransfer.types).includes(DRAG_TYPE)) {
                      e.preventDefault();
                      e.dataTransfer.dropEffect = 'move';
                    }
                  }}
                  onDrop={(e) => {
                    e.preventDefault();
                    onMove(e.dataTransfer.getData(DRAG_TYPE), day);
                  }}
                >
                  <div className="cell-heading">
                    <button
                      className="day-number"
                      aria-label={`Open ${day.toLocaleString(DateTime.DATE_FULL)}`}
                      onClick={(e) => {
                        e.stopPropagation();
                        onCreate(day.set({ hour: 9 }), e.currentTarget, true);
                      }}
                    >
                      {day.day}
                    </button>
                    <button
                      className="cell-add"
                      aria-label={`Create event on ${day.toLocaleString(DateTime.DATE_FULL)}`}
                      onClick={(e) => {
                        e.stopPropagation();
                        onCreate(day.set({ hour: 9 }), e.currentTarget, true);
                      }}
                    >
                      <Plus size={13} />
                    </button>
                  </div>
                  <div
                    className="cell-events"
                    style={{
                      paddingTop: reserved
                        ? `calc(${reserved} * var(--month-event-height))`
                        : undefined,
                    }}
                  >
                    {chips.map((event) => (
                      <EventChip key={event.id} event={event} day={day} onClick={onEdit} />
                    ))}
                    {items.length > chips.length && (
                      <Popover.Root
                        open={overflow === key}
                        onOpenChange={(open) => setOverflow(open ? key : null)}
                      >
                        <Popover.Trigger asChild>
                          <button className="overflow-button" onClick={(e) => e.stopPropagation()}>
                            +{items.length - chips.length} more
                          </button>
                        </Popover.Trigger>
                        <Popover.Portal>
                          <Popover.Content
                            className="overflow-popover"
                            sideOffset={8}
                            collisionPadding={16}
                          >
                            <div className="overflow-heading">{day.toFormat('cccc, LLLL d')}</div>
                            {allItems.map((event) => (
                              <EventChip
                                key={event.id}
                                event={event}
                                onClick={(item, anchor) => {
                                  setOverflow(null);
                                  onEdit(item, anchor);
                                }}
                              />
                            ))}
                            <Popover.Arrow className="popover-arrow" />
                          </Popover.Content>
                        </Popover.Portal>
                      </Popover.Root>
                    )}
                  </div>
                </div>
              );
            })}
          </div>
        ))}
      </div>
    </div>
  );
}

function TimeView({
  date,
  today,
  view,
  events,
  onCreate,
  onEdit,
  onMove,
  onResize,
}: {
  date: DateTime;
  today: DateTime;
  view: 'week' | 'day';
  events: CalendarEvent[];
  onCreate: CreateEvent;
  onEdit: (event: CalendarEvent, anchor: HTMLElement) => void;
  onMove: (id: string, date: DateTime, precise?: boolean) => void;
  onResize: (id: string, end: DateTime) => void;
}) {
  const days = daysForView(date, view);
  const scrollRef = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (scrollRef.current) scrollRef.current.scrollTop = HOUR_HEIGHT * 7.5;
  }, [view]);
  const [selection, setSelection] = useState<{ day: string; start: number; end: number } | null>(
    null,
  );
  const calendar = useContext(CalendarContext);
  const calendars = useContext(CalendarsContext);
  const capabilities = useContext(CapabilitiesContext);
  const gesture = useRef<{ start: number; y: number; id?: string } | null>(null);
  const suppressClick = useRef(false);
  useEffect(() => {
    const cancel = (e: globalThis.KeyboardEvent) => {
      if (e.key === 'Escape' && gesture.current) {
        gesture.current = null;
        setSelection(null);
        suppressClick.current = true;
      }
    };
    window.addEventListener('keydown', cancel);
    return () => window.removeEventListener('keydown', cancel);
  }, []);
  const minuteAt = (y: number, element: HTMLElement) =>
    Math.min(
      1440,
      Math.max(0, Math.round(((y - element.getBoundingClientRect().top) / HOUR_HEIGHT) * 4) * 15),
    );
  const [gestureFailure, setGestureFailure] = useState('');
  const perform = (action: () => Promise<void>) => {
    action()
      .then(() => setGestureFailure(''))
      .catch((err) => setGestureFailure(errorMessage(err)));
  };
  const at = async (day: DateTime, minute: number) => {
    const date = dateKey(day.plus({ days: Math.floor(minute / 1440) }));
    const clock = `${String(Math.floor((minute % 1440) / 60)).padStart(2, '0')}:${String(minute % 60).padStart(2, '0')}`;
    return DateTime.fromISO(await resolveWallTime(`${date}T${clock}`));
  };
  const countStyle = { '--days': days.length } as CSSProperties;
  const clickSlot = (e: MouseEvent<HTMLDivElement>, day: DateTime) => {
    if (suppressClick.current) {
      suppressClick.current = false;
      return;
    }
    if ((e.target as HTMLElement).closest('button')) return;
    const rect = e.currentTarget.getBoundingClientRect();
    const minute = Math.min(
      1425,
      Math.max(0, Math.floor(((e.clientY - rect.top) / HOUR_HEIGHT) * 4) * 15),
    );
    const anchor = e.currentTarget;
    const position = { x: e.clientX, y: e.clientY };
    perform(async () => onCreate(await at(day, minute), anchor, false, undefined, position));
  };
  return (
    <div
      className="time-view"
      style={countStyle}
      role="region"
      aria-label={`${view === 'week' ? 'Week' : 'Day'} calendar`}
    >
      {gestureFailure && (
        <p role="alert" className="editor-error">
          {gestureFailure} Use the editor to choose a time or offset.
        </p>
      )}
      <div className="time-head">
        <span className="zone-label">{DateTime.local().toFormat('ZZZZ')}</span>
        {days.map((day) => (
          <div
            className={`time-day-heading ${day.hasSame(today, 'day') ? 'is-today' : ''}`}
            key={dateKey(day)}
          >
            <span>{day.toFormat('ccc')}</span>
            <span className="time-day-number">{day.day}</span>
          </div>
        ))}
      </div>
      <div
        className="all-day-row"
        onDragOver={(e) => {
          if (Array.from(e.dataTransfer.types).includes(DRAG_TYPE)) e.preventDefault();
        }}
        onDrop={(e) => {
          if ((e.target as HTMLElement).closest('.all-day-cell')) return;
          e.preventDefault();
          const rect = e.currentTarget.getBoundingClientRect();
          const column = Math.min(
            days.length - 1,
            Math.max(
              0,
              Math.floor(((e.clientX - rect.left - 64) / (rect.width - 64)) * days.length),
            ),
          );
          onMove(e.dataTransfer.getData(DRAG_TYPE), days[column]);
        }}
        style={{
          minHeight:
            Math.max(1, ...layoutAllDaySpans(events, days).map((span) => span.lane + 1)) * 26 + 10,
        }}
      >
        <span className="all-day-label">all-day</span>
        {days.map((day) => (
          <div
            className="all-day-cell"
            key={dateKey(day)}
            tabIndex={0}
            role="group"
            aria-label={`All-day events on ${day.toISODate()}`}
            onKeyDown={(e) => {
              if (e.target === e.currentTarget && e.key === 'Enter')
                onCreate(day, e.currentTarget, true);
            }}
            onDragOver={(e) => {
              if (Array.from(e.dataTransfer.types).includes(DRAG_TYPE)) e.preventDefault();
            }}
            onDrop={(e) => {
              e.preventDefault();
              onMove(e.dataTransfer.getData(DRAG_TYPE), day);
            }}
            onDoubleClick={(e) => {
              if (!(e.target as HTMLElement).closest('button'))
                onCreate(day, e.currentTarget, true);
            }}
          ></div>
        ))}
        <div className="all-day-spans">
          {layoutAllDaySpans(events, days).map(({ event, start, end, lane }) => (
            <div
              key={event.id}
              style={{ gridColumn: `${start + 1} / ${end + 1}`, gridRow: lane + 1 }}
            >
              <EventChip event={event} day={days[start]} compact onClick={onEdit} />
            </div>
          ))}
        </div>
      </div>
      <div className="time-scroll" ref={scrollRef}>
        <div className="time-columns">
          <div className="hour-labels">
            {Array.from({ length: 24 }, (_, hour) => (
              <span key={hour} style={{ top: hour * HOUR_HEIGHT }}>
                {hour === 0 ? '' : dayHour(hour)}
              </span>
            ))}
          </div>
          {days.map((day) => (
            <div
              className="time-column"
              key={dateKey(day)}
              data-date={dateKey(day)}
              tabIndex={0}
              role="group"
              aria-label={`Times on ${day.toISODate()}; Enter to create at 9 AM`}
              onKeyDown={(e) => {
                if (e.target === e.currentTarget && e.key === 'Enter') {
                  e.preventDefault();
                  onCreate(day.set({ hour: 9 }), e.currentTarget);
                }
              }}
              onPointerDown={(e) => {
                if (e.button !== 0) return;
                const handle = (e.target as HTMLElement).closest<HTMLElement>('[data-resize]');
                if (
                  handle
                    ? !calendars.find(
                        (c) =>
                          c.id ===
                          events.find((event) => event.id === handle.dataset.resize)?.calendarId,
                      )?.writable
                    : !calendar.writable
                )
                  return;
                if (!handle && (e.target as HTMLElement).closest('button')) return;
                gesture.current = {
                  start: minuteAt(e.clientY, e.currentTarget),
                  y: e.clientY,
                  id: handle?.dataset.resize,
                };
                e.currentTarget.setPointerCapture(e.pointerId);
              }}
              onPointerMove={(e) => {
                if (!gesture.current) return;
                const scroll = scrollRef.current;
                if (scroll) {
                  const rect = scroll.getBoundingClientRect();
                  if (e.clientY > rect.bottom - 28) scroll.scrollTop += 18;
                  else if (e.clientY < rect.top + 28) scroll.scrollTop -= 18;
                }
                setSelection({
                  day: dateKey(day),
                  start: gesture.current.start,
                  end: minuteAt(e.clientY, e.currentTarget),
                });
              }}
              onPointerCancel={() => {
                gesture.current = null;
                setSelection(null);
              }}
              onPointerUp={(e) => {
                const g = gesture.current;
                gesture.current = null;
                setSelection(null);
                if (!g) return;
                if (g.id) suppressClick.current = true;
                if (Math.abs(e.clientY - g.y) < 5) return;
                suppressClick.current = true;
                const end = minuteAt(e.clientY, e.currentTarget);
                const anchor = e.currentTarget;
                perform(async () => {
                  if (g.id) onResize(g.id, await at(day, end));
                  else
                    onCreate(
                      await at(day, Math.min(g.start, end, 1425)),
                      anchor,
                      false,
                      await at(day, Math.max(g.start, end, Math.min(g.start, end) + 15)),
                    );
                });
              }}
              onClick={(e) => clickSlot(e, day)}
              onDragOver={(e) => {
                if (Array.from(e.dataTransfer.types).includes(DRAG_TYPE)) e.preventDefault();
              }}
              onDrop={(e) => {
                e.preventDefault();
                const id = e.dataTransfer.getData(DRAG_TYPE);
                const minute = Math.min(1425, minuteAt(e.clientY, e.currentTarget));
                perform(async () => onMove(id, await at(day, minute), true));
              }}
            >
              {Array.from({ length: 24 }, (_, hour) => (
                <div className="hour-slot" key={hour} />
              ))}
              {selection?.day === dateKey(day) && (
                <div
                  className="time-selection"
                  style={{
                    top: (Math.min(selection.start, selection.end) / 60) * HOUR_HEIGHT,
                    height:
                      (Math.max(15, Math.abs(selection.end - selection.start)) / 60) * HOUR_HEIGHT,
                  }}
                />
              )}
              {layoutTimedEvents(events, day).map(({ event, start, end, lane, lanes }) => {
                const eventCalendar = editableEventCalendar(
                  calendars.find((c) => c.id === event.calendarId) ?? emptyCalendar,
                  event,
                  capabilities,
                );
                return (
                  <button
                    key={event.id}
                    className="time-event"
                    draggable={eventCalendar.writable}
                    title={event.title || 'Untitled event'}
                    aria-label={`${event.title || 'Untitled event'}${event.recurrence ? ', recurring' : ''}, ${eventCalendar.name}, ${event.time.kind === 'timed' ? timeLabel(event.time.start) : 'all day'}`}
                    style={{
                      ...calendarStyle(eventCalendar),
                      top: (start / 60) * HOUR_HEIGHT + 2,
                      height: Math.max(28, ((end - start) / 60) * HOUR_HEIGHT - 4),
                      left: `calc(${(lane / lanes) * 100}% + 5px)`,
                      width: `calc(${100 / lanes}% - 10px)`,
                    }}
                    onDragStart={(e) => {
                      if (gesture.current?.id) {
                        e.preventDefault();
                        return;
                      }
                      e.dataTransfer.setData(DRAG_TYPE, event.id);
                      e.dataTransfer.effectAllowed = 'move';
                    }}
                    onClick={(e) => {
                      e.stopPropagation();
                      onEdit(event, e.currentTarget);
                    }}
                  >
                    <span className="time-event-title">
                      {event.recurrence && <Repeat2 size={11} aria-label="Recurring event" />}{' '}
                      {event.title || 'Untitled event'}
                    </span>
                    <span className="time-event-time">
                      {event.time.kind === 'timed'
                        ? `${timeLabel(event.time.start)} – ${timeLabel(event.time.end)}`
                        : ''}
                    </span>
                    {eventCalendar.writable && (
                      <span className="resize-handle" data-resize={event.id} aria-hidden="true" />
                    )}
                    {event.location && end - start >= 90 && (
                      <span className="time-event-location">
                        <MapPin size={11} />
                        {event.location}
                      </span>
                    )}
                  </button>
                );
              })}
              {day.hasSame(today, 'day') && (
                <div
                  className="now-line"
                  style={{
                    top: (DateTime.local().hour + DateTime.local().minute / 60) * HOUR_HEIGHT,
                  }}
                >
                  <span />
                </div>
              )}
            </div>
          ))}
        </div>
      </div>
    </div>
  );
}

function dayHour(hour: number) {
  return DateTime.local().set({ hour, minute: 0 }).toLocaleString({ hour: 'numeric' });
}

function EventEditor({
  event,
  existing,
  onSave,
  onDelete,
  onClose,
  scope = 'series',
  onScope,
  resetAffected = false,
  onPreview,
}: {
  scope?: MutationScope;
  onScope?: (scope: MutationScope) => void;
  resetAffected?: boolean;
  onPreview: (event: CalendarEvent, countChanged: boolean) => Promise<CalendarEvent[]>;
  event: CalendarEvent;
  existing: boolean;
  onSave: (event: CalendarEvent, resetExceptions: boolean, countChanged: boolean) => Promise<void>;
  onDelete: (resetExceptions: boolean) => Promise<void>;
  onClose: () => void;
}) {
  const calendars = useContext(CalendarsContext);
  const [calendarId, setCalendarId] = useState(event.calendarId);
  const sourceCalendar = calendars.find((c) => c.id === event.calendarId);
  const calendar = calendars.find((c) => c.id === calendarId) ?? emptyCalendar;
  const capabilities = useContext(CapabilitiesContext);
  const caps =
    capabilities.events[event.id] ??
    capabilities.events[event.id.split('@')[0]] ??
    capabilities.calendars[event.calendarId];
  const destinationCaps = capabilities.calendars[calendarId];
  const actions =
    capabilities.actions?.[event.id] ?? capabilities.actions?.[event.id.split('@')[0]];
  const editRestriction = actions?.edit.find((a) => a.scope === scope)?.disabledReason;
  const deleteRestriction = actions?.delete.find((a) => a.scope === scope)?.disabledReason;
  const writable = Boolean(
    sourceCalendar?.writable &&
      calendar.writable &&
      !editRestriction &&
      (existing ? (caps?.edit ?? true) : (destinationCaps?.create ?? true)) &&
      (!(event.recurrence || event.occurrence) || (caps?.recurrence ?? true)),
  );
  const google = calendar.providerId === 'google';
  const metadataOnly = google && existing && scope === 'series' && Boolean(event.recurrence);
  const canDelete = !deleteRestriction && (caps?.delete ?? true);
  const canMove = caps?.moveEvent ?? true;
  const [title, setTitle] = useState(event.title);
  const [recurrence, setRecurrence] = useState<Recurrence | undefined>(event.recurrence);
  const [resetExceptions, setResetExceptions] = useState(false);
  const [countChanged, setCountChanged] = useState(false);
  const [preview, setPreview] = useState<CalendarEvent[]>([]);
  const eventZone = event.time.kind === 'timed' ? event.time.timeZone : DateTime.local().zoneName!;
  const [allDay, setAllDay] = useState(event.time.kind === 'allDay');
  const start =
    event.time.kind === 'timed'
      ? DateTime.fromISO(event.time.start, { zone: eventZone })
      : DateTime.fromISO(event.time.startDate);
  const end =
    event.time.kind === 'timed'
      ? DateTime.fromISO(event.time.end, { zone: eventZone })
      : DateTime.fromISO(event.time.endDate).minus({ days: 1 });
  const [startDate, setStartDate] = useState(dateKey(start));
  const [endDate, setEndDate] = useState(dateKey(end));
  const [startTime, setStartTime] = useState(start.toFormat('HH:mm'));
  const [endTime, setEndTime] = useState(end.toFormat('HH:mm'));
  const [location, setLocation] = useState(event.location);
  const [notes, setNotes] = useState(event.notes);
  const [details, setDetails] = useState(Boolean(location || notes));
  const [error, setError] = useState('');

  const [busy, setBusy] = useState(false);
  const [offsets, setOffsets] = useState<{ start?: number[]; end?: number[] }>({});
  const [chosenOffsets, setChosenOffsets] = useState<{ start?: number; end?: number }>({});

  const buildDraft = async (): Promise<CalendarEvent> => {
    let time: CalendarEvent['time'];
    if (allDay) {
      const until = DateTime.fromISO(endDate).plus({ days: 1 });
      if (!until.isValid || endDate < startDate)
        throw new Error('The end date must be on or after the start date.');
      time = { kind: 'allDay', startDate, endDate: dateKey(until) };
    } else {
      const resolve = async (field: 'start' | 'end', local: string) => {
        try {
          return await resolveWallTime(local, chosenOffsets[field], eventZone);
        } catch (err) {
          const failure = err as ServiceError;
          if (failure.code === 'ambiguousTime')
            setOffsets((current) => ({ ...current, [field]: failure.offsets }));
          throw err;
        }
      };
      const from = await resolve('start', `${startDate}T${startTime}`);
      const until = await resolve('end', `${endDate}T${endTime}`);
      if (DateTime.fromISO(until) <= DateTime.fromISO(from))
        throw new Error('The event must end after it starts.');
      time = { kind: 'timed', start: from, end: until, timeZone: eventZone };
    }
    return {
      ...event,
      calendarId,
      title: title.trim() || 'Preview',
      location: location.trim(),
      notes,
      time,
      recurrence,
    };
  };
  const submit = async (e: FormEvent) => {
    e.preventDefault();
    if (busy || !writable) return;
    if (!title.trim()) {
      setError('Give your event a title.');
      return;
    }
    if (resetAffected && !resetExceptions) {
      setError('Confirm resetting individual changes before saving.');
      return;
    }
    setBusy(true);
    setError('');
    try {
      await onSave(await buildDraft(), resetExceptions, countChanged);
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setBusy(false);
    }
  };
  const showPreview = async () => {
    setBusy(true);
    setError('');
    try {
      setPreview(await onPreview(await buildDraft(), countChanged));
    } catch (err) {
      setPreview([]);
      setError(errorMessage(err));
    } finally {
      setBusy(false);
    }
  };

  return (
    <form className="editor" onSubmit={submit}>
      <IconButton
        className="editor-close"
        label="Close event editor"
        onClick={onClose}
        disabled={busy}
      >
        <X size={16} />
      </IconButton>
      {!sourceCalendar && (
        <p role="alert" className="editor-error">
          This calendar was deleted. Close this draft and create a new event in another calendar.
        </p>
      )}
      <fieldset
        disabled={busy || !writable}
        style={{ border: 0, padding: 0, margin: 0, minWidth: 0 }}
      >
        {editRestriction && <p role="status">{editRestriction}</p>}
        <div className="editor-heading">
          <span className="eyebrow">
            {!writable ? 'Event details' : existing ? 'Edit event' : 'New event'}
          </span>
        </div>
        {onScope && (
          <label className="repeat-row">
            Change
            <select
              aria-label="Event change scope"
              value={scope}
              onChange={(e) => onScope(e.target.value as MutationScope)}
            >
              {(['occurrence', 'following', 'series'] as const).map((value) => {
                const reason = actions?.edit.find((a) => a.scope === value)?.disabledReason;
                return (
                  <option
                    key={value}
                    value={value}
                    disabled={Boolean(reason)}
                    title={reason ?? undefined}
                  >
                    {value === 'occurrence'
                      ? 'This occurrence'
                      : value === 'following'
                        ? 'This and following'
                        : 'Entire series'}
                  </option>
                );
              })}
            </select>
          </label>
        )}
        <input
          className="event-title-input"
          aria-label="Event title"
          placeholder="Event title"
          value={title}
          onChange={(e) => setTitle(e.target.value)}
          maxLength={300}
          autoFocus
        />
        <div className="editor-calendar" style={calendarStyle(calendar)}>
          <span className="event-dot" />
          <select
            aria-label="Event calendar"
            value={calendarId}
            onChange={(e) => setCalendarId(e.target.value)}
          >
            {!calendars.some((c) => c.id === calendarId) && (
              <option value={calendarId}>Calendar deleted</option>
            )}
            {calendars
              .filter(
                (c) =>
                  c.id === event.calendarId ||
                  (c.writable &&
                    (existing
                      ? canMove && (capabilities.calendars[c.id]?.moveEvent ?? true)
                      : (capabilities.calendars[c.id]?.create ?? true))),
              )
              .map((c) => (
                <option key={c.id} value={c.id}>
                  {c.name}
                </option>
              ))}
          </select>
          <span className="muted">{writable ? 'Editable calendar' : 'Read-only calendar'}</span>
        </div>
        <div className="editor-rule" />
        <div className="all-day-control">
          <span>
            <CalendarDays size={16} />
            All-day
          </span>
          <button
            type="button"
            role="switch"
            aria-label="All-day event"
            disabled={metadataOnly}
            aria-checked={allDay}
            className={`switch ${allDay ? 'on' : ''}`}
            onClick={() => {
              setAllDay(!allDay);
              if (allDay) {
                setStartTime('09:00');
                setEndTime('10:00');
              }
            }}
          >
            <span />
          </button>
        </div>
        <div className="date-fields">
          <Clock3 size={16} />
          <div>
            <div className="date-field-row">
              <label htmlFor="event-start-date">Starts</label>
              <input
                id="event-start-date"
                disabled={metadataOnly}
                type="date"
                required
                value={startDate}
                onChange={(e) => {
                  setOffsets({});
                  setChosenOffsets({});
                  setStartDate(e.target.value);
                  if (endDate < e.target.value) setEndDate(e.target.value);
                }}
              />
              {!allDay && (
                <input
                  aria-label="Start time"
                  disabled={metadataOnly}
                  type="time"
                  required
                  value={startTime}
                  onChange={(e) => {
                    setStartTime(e.target.value);
                    setOffsets({});
                    setChosenOffsets({});
                  }}
                />
              )}
            </div>
            <div className="date-field-row">
              <label htmlFor="event-end-date">Ends</label>
              <input
                id="event-end-date"
                disabled={metadataOnly}
                type="date"
                required
                value={endDate}
                min={startDate}
                onChange={(e) => {
                  setEndDate(e.target.value);
                  setOffsets({});
                  setChosenOffsets({});
                }}
              />
              {!allDay && (
                <input
                  aria-label="End time"
                  disabled={metadataOnly}
                  type="time"
                  required
                  value={endTime}
                  onChange={(e) => {
                    setEndTime(e.target.value);
                    setOffsets({});
                    setChosenOffsets({});
                  }}
                />
              )}
            </div>
          </div>
        </div>
        {!allDay && <p className="timezone-note">{eventZone.replaceAll('_', ' ')}</p>}
        {metadataOnly && (
          <p className="timezone-note">
            Google series edits preserve individual changes. Edit a single occurrence to change its
            time.
          </p>
        )}
        {google && !existing && (
          <p className="timezone-note">
            Google repeats support all-day and UTC date-based patterns. Regional time zones are
            unavailable.
          </p>
        )}
        <RepeatControls
          value={recurrence}
          onChange={(value, explicitCount) => {
            setCountChanged((current) => current || Boolean(explicitCount));
            setRecurrence(value);
            setPreview([]);
          }}
          startDate={startDate}
          disabled={
            !writable ||
            metadataOnly ||
            !(caps?.recurrence ?? true) ||
            Boolean(onScope && scope === 'occurrence')
          }
        />
        {scope === 'following' && recurrence?.count && !countChanged && (
          <p className="timezone-note">
            An unchanged count keeps the remaining repeats. Edit the count to choose a new total.
          </p>
        )}
        {recurrence && scope !== 'occurrence' && (
          <>
            <button type="button" className="text-button" onClick={showPreview}>
              Preview next dates
            </button>
            {preview.length > 0 && (
              <ol className="repeat-preview" aria-label="Repeat preview">
                {preview.map((e) => (
                  <li key={e.id}>
                    {e.time.kind === 'allDay'
                      ? e.time.startDate
                      : DateTime.fromISO(e.time.start, { zone: e.time.timeZone }).toFormat(
                          'ccc, d LLL yyyy HH:mm',
                        )}
                  </li>
                ))}
              </ol>
            )}
          </>
        )}
        {resetAffected && (
          <label className="repeat-reset">
            <input
              type="checkbox"
              aria-label="Reset individual changes"
              checked={resetExceptions}
              onChange={(e) => setResetExceptions(e.target.checked)}
            />
            {google && scope === 'following'
              ? 'Reset future individual edits and skipped occurrences, including changes outside this cache. Undo can restore only cached changes.'
              : 'This edit discards affected individual edits and skipped occurrences. Reset those changes.'}
          </label>
        )}
        {details ? (
          <div className="optional-fields">
            <label>
              <span>
                <MapPin size={14} />
                Location
              </span>
              <input
                placeholder="Location"
                value={location}
                onChange={(e) => setLocation(e.target.value)}
              />
            </label>
            <label>
              <span>Notes</span>
              <textarea
                placeholder="Notes"
                value={notes}
                onChange={(e) => setNotes(e.target.value)}
                rows={3}
              />
            </label>
          </div>
        ) : (
          <button type="button" className="details-button" onClick={() => setDetails(true)}>
            <Plus size={13} />
            Add location or notes
          </button>
        )}
        {!allDay &&
          (['start', 'end'] as const).map(
            (field) =>
              offsets[field] && (
                <label key={field} className="timezone-note">
                  {field === 'start' ? 'Start' : 'End'} UTC offset
                  <select
                    aria-label={`${field} UTC offset`}
                    value={chosenOffsets[field] ?? ''}
                    onChange={(e) =>
                      setChosenOffsets((current) => ({
                        ...current,
                        [field]: Number(e.target.value),
                      }))
                    }
                  >
                    <option value="" disabled>
                      Choose offset
                    </option>
                    {offsets[field]!.map((offset) => (
                      <option
                        key={offset}
                        value={offset}
                      >{`UTC${offset >= 0 ? '+' : '-'}${String(Math.floor(Math.abs(offset) / 60)).padStart(2, '0')}:${String(Math.abs(offset) % 60).padStart(2, '0')}`}</option>
                    ))}
                  </select>
                </label>
              ),
          )}
        {error && (
          <p role="alert" className="editor-error">
            {error}
          </p>
        )}
        <div className="editor-footer">
          {existing ? (
            <IconButton
              label="Delete event"
              disabled={!canDelete}
              className="delete-button"
              onClick={async () => {
                setBusy(true);
                try {
                  if (google && scope === 'following' && !resetExceptions) {
                    setError('Confirm reset of future Google exceptions before deleting.');
                    return;
                  }
                  await onDelete(resetExceptions);
                } catch (err) {
                  setError(errorMessage(err));
                } finally {
                  setBusy(false);
                }
              }}
            >
              <Trash2 size={16} />
            </IconButton>
          ) : (
            <span />
          )}
          <div>
            <button type="button" className="text-button" onClick={onClose}>
              Cancel
            </button>
            <button type="submit" className="primary-button">
              {existing ? 'Save changes' : 'Create event'}
            </button>
          </div>
        </div>
      </fieldset>
      {!writable && (
        <button type="button" className="text-button" onClick={onClose}>
          Close
        </button>
      )}
    </form>
  );
}

export function App({ repository = null }: { repository?: PrototypeRepository | null }) {
  const [date, setDate] = useState<DateTime>(initialDate.startOf('day'));
  const [now, setNow] = useState(DateTime.local());
  useEffect(() => {
    const timer = window.setInterval(() => setNow(DateTime.local()), 30_000);
    return () => clearInterval(timer);
  }, []);
  const today = desktop || !params.get('date') ? now.startOf('day') : initialDate.startOf('day');
  const [view, setView] = useState<View>('month');
  const [sidebar, setSidebar] = useState(window.innerWidth >= 900);
  const [initialSnapshot] = useState(() => repository?.snapshot());
  const [accounts, setAccounts] = useState<AccountSummary[]>([]);
  const [syncStatus, setSyncStatus] = useState<AccountSyncStatus[]>([]);
  const [syncRequestFailure, setSyncRequestFailure] = useState('');
  const [calendars, setCalendars] = useState<Calendar[]>(initialSnapshot?.calendars ?? []);
  const [defaultCalendarId, setDefaultCalendarId] = useState<string | null>(
    initialSnapshot?.defaultCalendarId ?? null,
  );
  const [hiddenCalendarIds, setHiddenCalendarIds] = useState<string[]>(
    initialSnapshot?.hiddenCalendarIds ?? [],
  );
  const [calendarDialog, setCalendarDialog] = useState<{ calendar: Calendar | null } | null>(null);
  const [deletingCalendar, setDeletingCalendar] = useState<{
    calendar: Calendar;
    count: number;
    review: CalendarRemovalReview | null;
  } | null>(null);
  const [confirmCalendarDiscard, setConfirmCalendarDiscard] = useState(false);
  const calendarDialogAnchor = useRef<HTMLElement | null>(null);
  const [calendarBusy, setCalendarBusy] = useState(false);
  const [calendarFailure, setCalendarFailure] = useState('');
  const [appearance, setAppearance] = useState<Appearance>(initialAppearance);
  const [activePopup, setActivePopup] = useState<'settings' | 'shortcuts' | null>(null);
  const popupOpener = useRef<HTMLElement | null>(null);
  const [runtime, setRuntime] = useState('browser');

  const calendar = calendars.find((c) => c.id === defaultCalendarId) ?? emptyCalendar;
  const [capabilityState, setCapabilityState] = useState<{
    calendars: Record<string, Capabilities>;
    events: Record<string, Capabilities>;
    actions?: Record<string, EventActions>;
  }>({ calendars: {}, events: {} });
  const [calendarReady, setCalendarReady] = useState(!desktop);
  const [events, setEvents] = useState<CalendarEvent[]>(() => repository?.list() ?? []);
  const [editing, setEditing] = useState<{
    event: CalendarEvent;
    existing: boolean;
    selected?: CalendarEvent;
    series?: EventSeries;
    scope?: MutationScope;
  } | null>(null);
  const editorAnchor = useRef<HTMLElement | null>(null);
  const editorPositionRef = useRef<ReturnType<typeof editorPosition> | null>(null);
  const newButton = useRef<HTMLButtonElement>(null);
  const [announcement, setAnnouncement] = useState('');
  const [failure, setFailure] = useState('');
  const [preferenceFailure, setPreferenceFailure] = useState('');
  const [canUndo, setCanUndo] = useState(false);
  const [ready, setReady] = useState(!desktop);
  const [revision, setRevision] = useState(0);
  const [syncConflicts, setSyncConflicts] = useState<SyncConflict[]>([]);
  const [mutating, setMutating] = useState(false);
  const requestVersion = useRef(0);
  const syncRange = useCallback(() => {
    const days = daysForView(date, view);
    const start = days[0].startOf('day');
    const end = days[days.length - 1].plus({ days: 1 }).startOf('day');
    return {
      start: start.toUTC().toISO()!,
      end: end.toUTC().toISO()!,
      startDate: dateKey(start),
      endDate: dateKey(end),
    };
  }, [date, view]);
  const requestSync = useCallback(() => {
    if (!desktop) return;
    void calendarClient
      .requestSync(syncRange())
      .then(() => setSyncRequestFailure(''))
      .catch((e) => setSyncRequestFailure(errorMessage(e)));
  }, [syncRange]);
  useEffect(() => {
    if (!desktop) return;
    let disposed = false;
    void calendarClient
      .conflicts()
      .then((value) => {
        if (!disposed) setSyncConflicts(value);
      })
      .catch((e) => {
        if (!disposed) setSyncRequestFailure(errorMessage(e));
      });
    return () => {
      disposed = true;
    };
  }, [revision]);
  const writeSyncTimer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  useEffect(() => () => clearTimeout(writeSyncTimer.current), []);
  const connectedSelection = accounts
    .filter((a) => a.connectionState === 'connected')
    .map((a) => `${a.id}:${a.selectedCalendarIds.join(',')}`)
    .join('|');
  useEffect(() => {
    if (!desktop || !connectedSelection) return;
    // Defer startup so Strict Mode cleanup cancels its first effect before provider work.
    const startup = window.setTimeout(requestSync, 0);
    const timer = window.setInterval(requestSync, 60_000);
    const focus = () => {
      setRevision((v) => v + 1);
      requestSync();
    };
    const resume = () => {
      if (document.visibilityState === 'visible') focus();
    };
    window.addEventListener('focus', focus);
    document.addEventListener('visibilitychange', resume);
    return () => {
      clearTimeout(startup);
      clearInterval(timer);
      window.removeEventListener('focus', focus);
      document.removeEventListener('visibilitychange', resume);
    };
  }, [requestSync, connectedSelection]);
  useEffect(() => {
    if (!desktop) return;
    const refreshOnFocus = () => setRevision((value) => value + 1);
    window.addEventListener('focus', refreshOnFocus);
    let disposed = false;
    let unlisten: (() => void) | undefined;
    void listen('calendar-sync-changed', refreshOnFocus)
      .then((cleanup) => {
        if (disposed) cleanup();
        else unlisten = cleanup;
      })
      .catch(() => undefined);
    return () => {
      disposed = true;
      unlisten?.();
      window.removeEventListener('focus', refreshOnFocus);
    };
  }, []);
  useEffect(() => {
    if (!desktop) return;
    calendarClient
      .preferences()
      .then((p) => {
        setView(p.view as View);
        setSidebar(window.innerWidth >= 900 && p.sidebar);
        setAppearance(p.appearance as Appearance);
        setReady(true);
        setPreferenceFailure('');
      })
      .catch((err) => setPreferenceFailure(errorMessage(err)));
  }, []);
  useEffect(() => {
    if (!desktop || !ready) return;
    calendarClient
      .setPreferences({ appearance, view, sidebar })
      .then(() => setPreferenceFailure(''))
      .catch((err) => setPreferenceFailure(errorMessage(err)));
  }, [appearance, view, sidebar, ready]);
  useEffect(() => {
    const token = ++requestVersion.current;
    const days = daysForView(date, view);
    const start = days[0].startOf('day');
    const end = days[days.length - 1].plus({ days: 1 }).startOf('day');
    const range = {
      start: start.toUTC().toISO()!,
      end: end.toUTC().toISO()!,
      startDate: dateKey(start),
      endDate: dateKey(end),
    };
    const snapshot = desktop
      ? calendarClient.snapshot(range)
      : Promise.resolve().then(() => repository!.snapshot(range));
    snapshot
      .then((snapshot) => {
        if (token !== requestVersion.current) return;
        setAccounts(snapshot.accounts ?? []);
        setSyncStatus(snapshot.syncStatus ?? []);
        setCalendars(snapshot.calendars);
        setCapabilityState({
          calendars: snapshot.calendarCapabilities ?? {},
          events: snapshot.eventCapabilities ?? {},
          actions: snapshot.eventActions ?? {},
        });
        setDefaultCalendarId(snapshot.defaultCalendarId);
        setHiddenCalendarIds(snapshot.hiddenCalendarIds);
        setCalendarReady(true);
        setEvents(snapshot.events);
        setCanUndo(snapshot.canUndo);
        setFailure('');
      })
      .catch((err) => {
        if (token === requestVersion.current) setFailure(errorMessage(err));
      });
    return () => {
      requestVersion.current++;
    };
  }, [date, view, revision]);

  useEffect(() => {
    const adaptToWindow = () => {
      if (window.innerWidth < 900) setSidebar(false);
    };
    window.addEventListener('resize', adaptToWindow);
    return () => window.removeEventListener('resize', adaptToWindow);
  }, []);

  useEffect(() => {
    const media = window.matchMedia('(prefers-color-scheme: dark)');
    const update = () => {
      document.documentElement.dataset.theme =
        appearance === 'system' ? (media.matches ? 'dark' : 'light') : appearance;
    };
    update();
    if (!desktop) localStorage.setItem('cal.appearance', appearance);
    media.addEventListener('change', update);
    return () => media.removeEventListener('change', update);
  }, [appearance]);
  useEffect(() => {
    if (isTauri())
      invoke<AppInfo>('get_app_info')
        .then(() => setRuntime('desktop'))
        .catch(() => setRuntime('desktop-error'));
  }, []);
  const applySnapshot = (snapshot: CalendarCollectionSnapshot) => {
    setCalendars(snapshot.calendars);
    setCapabilityState({
      calendars: snapshot.calendarCapabilities ?? {},
      events: snapshot.eventCapabilities ?? {},
      actions: snapshot.eventActions ?? {},
    });
    setDefaultCalendarId(snapshot.defaultCalendarId);
    setHiddenCalendarIds(snapshot.hiddenCalendarIds);
    setEvents(snapshot.events);
    setCanUndo(snapshot.canUndo);
  };
  const refresh = (message: string) => {
    setFailure('');
    if (repository) {
      const days = daysForView(date, view);
      const start = days[0].startOf('day');
      const end = days[days.length - 1].plus({ days: 1 }).startOf('day');
      applySnapshot(
        repository.snapshot({
          start: start.toUTC().toISO()!,
          end: end.toUTC().toISO()!,
          startDate: dateKey(start),
          endDate: dateKey(end),
        }),
      );
    } else {
      requestVersion.current++;
      setRevision((value) => value + 1);
    }
    setAnnouncement(message);
    if (desktop) {
      clearTimeout(writeSyncTimer.current);
      writeSyncTimer.current = setTimeout(requestSync, 400);
    }
  };
  const calendarAction = async (action: () => Promise<void>, message: string) => {
    setCalendarBusy(true);
    try {
      await action();
      refresh(message);
    } catch (err) {
      setFailure(errorMessage(err));
    } finally {
      setCalendarBusy(false);
    }
  };
  const create = useCallback(
    (
      day: DateTime,
      anchor: HTMLElement,
      allDay = false,
      end?: DateTime,
      position?: ClickPosition,
    ) => {
      if (
        !calendarReady ||
        !calendar.writable ||
        capabilityState.calendars[calendar.id]?.create === false
      )
        return;
      editorAnchor.current = anchor;
      editorPositionRef.current = editorPosition(anchor, position);
      const event = { ...makeEvent(day, allDay), calendarId: calendar.id };
      if (end && event.time.kind === 'timed') event.time.end = end.toUTC().toISO()!;
      if (end && event.time.kind === 'allDay') event.time.endDate = dateKey(end);
      setEditing({ event, existing: false });
    },
    [calendar, calendarReady, capabilityState],
  );
  const edit = async (event: CalendarEvent, anchor: HTMLElement) => {
    editorAnchor.current = anchor;
    editorPositionRef.current = editorPosition(anchor);
    if (!event.occurrence) {
      setEditing({ event, existing: true });
      return;
    }
    try {
      const series = desktop
        ? await calendarClient.getSeries(event.id)
        : repository!.getSeries(event.id);
      setEditing({ event, selected: event, series, scope: 'occurrence', existing: true });
    } catch (err) {
      setFailure(errorMessage(err));
    }
  };
  const requestForEdit = (
    event: CalendarEvent | null,
    resetExceptions = false,
    countChanged = false,
  ): EventMutation => {
    const selected = editing!.selected!;
    const scope = editing!.scope ?? 'occurrence';
    return {
      target: scope === 'series' ? selected.occurrence!.seriesId : selected.id,
      scope,
      event,
      expected: selected.occurrence!.expected,
      resetExceptions:
        calendars.find((c) => c.id === selected.calendarId)?.providerId === 'google' &&
        scope !== 'following'
          ? false
          : resetExceptions,
      exceptionPolicy:
        calendars.find((c) => c.id === selected.calendarId)?.providerId === 'google' &&
        scope !== 'following'
          ? 'preserve'
          : 'reset',
      countChanged,
    };
  };
  const performRecurring = async (request: EventMutation) => {
    if (desktop) await calendarClient.mutateEvent(request);
    else repository!.mutate(request);
  };
  const [gestureEdit, setGestureEdit] = useState<{
    original: CalendarEvent;
    changed: CalendarEvent;
  } | null>(null);
  const [gestureScope, setGestureScope] = useState<MutationScope>('occurrence');
  const [gestureReset, setGestureReset] = useState(false);
  const [gestureFailure, setGestureFailure] = useState('');
  const move = async (id: string, target: DateTime, precise = false) => {
    const event = events.find((item) => item.id === id);
    if (
      !event ||
      !calendars.find((c) => c.id === event.calendarId)?.writable ||
      capabilityState.events[event.id]?.edit === false ||
      ((event.recurrence || event.occurrence) &&
        capabilityState.events[event.id]?.recurrence === false)
    )
      return;
    try {
      const moved = moveEvent(event, target);
      if (event.time.kind === 'timed' && moved.time.kind === 'timed') {
        const oldStart = DateTime.fromISO(event.time.start);
        const start = await resolveWallTime(
          `${dateKey(target)}T${(precise ? target : oldStart.setZone(event.time.timeZone)).toFormat('HH:mm')}`,
          undefined,
          event.time.timeZone,
        );
        const duration = DateTime.fromISO(event.time.end).diff(oldStart).milliseconds;
        moved.time = {
          kind: 'timed',
          start,
          end: DateTime.fromISO(start).plus({ milliseconds: duration }).toUTC().toISO()!,
          timeZone: event.time.timeZone,
        };
      }
      if (event.occurrence) {
        setGestureEdit({ original: event, changed: moved });
        setGestureScope('occurrence');
        setGestureReset(false);
        setGestureFailure('');
        return;
      }
      if (desktop) await calendarClient.save(moved, true, event);
      else repository!.save(moved);
      setFailure('');
      refresh('Event moved. Undo is available.');
    } catch (err) {
      setFailure(errorMessage(err));
    }
  };
  const resize = async (id: string, target: DateTime) => {
    const event = events.find((item) => item.id === id);
    if (
      !event ||
      !calendars.find((c) => c.id === event.calendarId)?.writable ||
      capabilityState.events[event.id]?.edit === false ||
      ((event.recurrence || event.occurrence) &&
        capabilityState.events[event.id]?.recurrence === false) ||
      event.time.kind !== 'timed'
    )
      return;
    try {
      const end = await resolveWallTime(
        `${dateKey(target)}T${target.toFormat('HH:mm')}`,
        undefined,
        event.time.timeZone,
      );
      if (DateTime.fromISO(end).toMillis() <= DateTime.fromISO(event.time.start).toMillis())
        throw new Error('End must be after start.');
      const resized = { ...event, time: { ...event.time, end } };
      if (event.occurrence) {
        setGestureEdit({ original: event, changed: resized });
        setGestureScope('occurrence');
        setGestureReset(false);
        setGestureFailure('');
        return;
      }
      if (desktop) await calendarClient.save(resized, true, event);
      else repository!.save(resized);
      setFailure('');
      refresh('Event resized. Undo is available.');
    } catch (err) {
      setFailure(errorMessage(err));
    }
  };
  const undo = async () => {
    try {
      if (desktop) await calendarClient.undo();
      else repository!.undo();
      refresh('Change undone.');
    } catch (err) {
      setFailure(errorMessage(err));
    }
  };
  const navigate = (direction: number) =>
    setDate((current) =>
      view === 'month'
        ? current.plus({ months: direction })
        : current.plus({ days: direction * (view === 'week' ? 7 : 1) }),
    );

  useEffect(() => {
    const handler = (e: globalThis.KeyboardEvent) => {
      const target = e.target as HTMLElement;
      if (
        target.closest('input, textarea, select, [contenteditable="true"]') ||
        editing ||
        gestureEdit ||
        activePopup ||
        calendarDialog ||
        deletingCalendar ||
        document.querySelector('[role="menu"]')
      )
        return;
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === 'z') {
        e.preventDefault();
        undo();
        return;
      }
      if (e.metaKey || e.ctrlKey || e.altKey) return;
      if (e.key.toLowerCase() === 't') setDate(today);
      if (e.key.toLowerCase() === 'n' && newButton.current) {
        e.preventDefault();
        create(date.set({ hour: 9 }), newButton.current);
      }
      if (e.key === '1') setView('month');
      if (e.key === '2') setView('week');
      if (e.key === '3') setView('day');
    };
    window.addEventListener('keydown', handler);
    return () => window.removeEventListener('keydown', handler);
  }, [date, editing, gestureEdit, activePopup, calendarDialog, deletingCalendar, create]);

  const heading = view === 'day' ? date.toFormat('cccc') : date.toFormat('LLLL');
  const subheading = view === 'day' ? date.toFormat('d LLLL yyyy') : String(date.year);
  const shownEvents = events.filter((e) => !hiddenCalendarIds.includes(e.calendarId));
  const selectedCount = eventsOnDate(shownEvents, date).length;
  const week = weekStart(date, firstWeekday());

  return (
    <CalendarProviders calendars={calendars} capabilities={capabilityState}>
      <CalendarContext.Provider value={calendar}>
        <div className={`app-shell ${sidebar ? '' : 'sidebar-hidden'}`} data-runtime={runtime}>
          {sidebar && (
            <aside className="sidebar">
              <div className="sidebar-brand">
                <span className="wordmark">aika</span>
                <IconButton label="Hide sidebar" onClick={() => setSidebar(false)}>
                  <PanelLeft size={17} />
                </IconButton>
              </div>
              <div className="sidebar-date">
                <div>{today.toFormat('cccc, LLLL d')}</div>
              </div>
              <MiniCalendar selected={date} today={today} onSelect={setDate} />
              <div className="calendar-list">
                <div className="calendar-list-heading">
                  <span className="section-label">Calendars</span>
                  <ProviderControls
                    busy={!calendarReady || calendarBusy}
                    accounts={accounts}
                    onRefresh={() => refresh('Account selection updated.')}
                    onLocal={(anchor) => {
                      calendarDialogAnchor.current = anchor;
                      setCalendarFailure('');
                      setCalendarDialog({ calendar: null });
                    }}
                  />
                </div>
                {calendars.map((item) => {
                  const isVisible = !hiddenCalendarIds.includes(item.id);
                  const visibilityLabel = `${isVisible ? 'Hide' : 'Show'} ${item.name} calendar`;
                  return (
                    <div className="calendar-row" key={item.id} style={calendarStyle(item)}>
                      <div className="calendar-label">
                        <span className="calendar-dot" aria-hidden="true" />
                        <span className="calendar-name" title={item.name}>
                          {item.name}
                        </span>
                        {item.id === defaultCalendarId && (
                          <span className="calendar-local">Default</span>
                        )}
                      </div>
                      <CalendarMenu
                        calendar={item}
                        isDefault={item.id === defaultCalendarId}
                        busy={calendarBusy}
                        onDefault={() =>
                          void calendarAction(async () => {
                            if (desktop) await calendarClient.setDefaultCalendar(item.id);
                            else repository!.setDefaultCalendar(item.id);
                            setDefaultCalendarId(item.id);
                          }, `${item.name} is the default calendar.`)
                        }
                        onSettings={(anchor) => {
                          calendarDialogAnchor.current = anchor;
                          setCalendarFailure('');
                          setCalendarDialog({ calendar: item });
                        }}
                        onDelete={async (anchor) => {
                          calendarDialogAnchor.current = anchor;
                          setCalendarBusy(true);
                          try {
                            const count = desktop
                              ? await calendarClient.calendarEventCount(item.id)
                              : repository!.calendarEventCount(item.id);
                            setCalendarFailure('');
                            const review =
                              desktop && item.providerId !== 'local'
                                ? await calendarClient.reviewCalendarRemoval(item.id)
                                : null;
                            setConfirmCalendarDiscard(false);
                            setDeletingCalendar({ calendar: item, count, review });
                            setCalendarBusy(false);
                          } catch (err) {
                            setFailure(errorMessage(err));
                            setCalendarBusy(false);
                            requestAnimationFrame(() => anchor.focus());
                          }
                        }}
                      />
                      <button
                        type="button"
                        className="icon-button calendar-visibility"
                        aria-label={visibilityLabel}
                        title={visibilityLabel}
                        aria-pressed={isVisible}
                        disabled={calendarBusy}
                        onClick={() => {
                          const visible = !isVisible;
                          void calendarAction(
                            async () => {
                              if (desktop)
                                await calendarClient.setCalendarVisibility(item.id, visible);
                              else repository!.setCalendarVisibility(item.id, visible);
                              setHiddenCalendarIds((current) =>
                                visible
                                  ? current.filter((id) => id !== item.id)
                                  : [...current.filter((id) => id !== item.id), item.id],
                              );
                            },
                            visible ? `${item.name} shown.` : `${item.name} hidden.`,
                          );
                        }}
                      >
                        {isVisible ? (
                          <Eye size={16} aria-hidden="true" />
                        ) : (
                          <EyeOff size={16} aria-hidden="true" />
                        )}
                      </button>
                    </div>
                  );
                })}
                {calendarReady && calendars.length === 0 && (
                  <p className="calendar-empty">No calendars yet. Use + to create one.</p>
                )}
              </div>
              {desktop && accounts.length > 0 && (
                <section className="sync-summary" aria-label="Calendar synchronization">
                  <button className="text-button" onClick={requestSync}>
                    Refresh calendars
                  </button>
                  {syncRequestFailure && <p role="alert">{syncRequestFailure}</p>}
                  {syncStatus.map((status) => (
                    <div key={status.accountId}>
                      <p className="muted">
                        {accounts.find((a) => a.id === status.accountId)?.displayName}
                      </p>
                      <p role="status">
                        {status.syncing
                          ? 'Synchronizing…'
                          : status.lastSuccess
                            ? `Last synchronized ${DateTime.fromISO(status.lastSuccess).toLocaleString(DateTime.DATETIME_SHORT)}`
                            : 'Waiting for the first synchronization.'}
                      </p>
                      {status.failure && (
                        <p role="alert">{status.failure.message} Cached events remain available.</p>
                      )}
                      {status.calendars.some((c) => !c.available) && (
                        <p>Some selected calendars are unavailable.</p>
                      )}
                      {status.calendars.some((c) => c.available && !c.rangeComplete) && (
                        <p>
                          Some events in this range have not been cached. Connect and refresh to
                          complete this range.
                        </p>
                      )}
                      {status.pendingCount > 0 && (
                        <>
                          <p>{status.pendingCount} pending changes</p>
                          <button
                            className="text-button"
                            onClick={() =>
                              void calendarAction(
                                () => calendarClient.retryOutgoing(status.accountId),
                                'Pending changes scheduled for retry.',
                              )
                            }
                          >
                            Retry pending changes
                          </button>
                        </>
                      )}
                      {status.recoveryGroups?.map((group) => (
                        <p key={group.id} role="status">
                          {group.title}: {group.completedSteps} of {group.totalSteps} Google steps
                          completed{group.blocked ? ' — needs attention' : ' — pending'}. Retry
                          resumes unfinished steps. Discard keeps successful remote changes.
                        </p>
                      ))}
                      {syncConflicts
                        .filter((c) => c.accountId === status.accountId)
                        .map((conflict) => (
                          <section key={conflict.id} aria-label="Event conflict">
                            <p>
                              Local:{' '}
                              {conflict.local?.event.title ||
                                (conflict.local ? 'Untitled event' : 'Deleted event')}
                            </p>
                            <p>
                              Google:{' '}
                              {conflict.remote?.event.title ||
                                (conflict.remote ? 'Untitled event' : 'Deleted event')}
                            </p>
                            {(conflict.recoverySteps?.length ?? 0) > 1 && (
                              <div>
                                <p>
                                  This action affects several Google resources. Successful steps
                                  remain effective. Keep local replans unfinished changes; Keep
                                  Google discards the remainder and keeps the observed partial
                                  result.
                                </p>
                                <ul aria-label="Grouped action progress">
                                  {conflict.recoverySteps!.map((step, index) => (
                                    <li key={index}>
                                      {step.title}: {step.operation} — {step.state}
                                    </li>
                                  ))}
                                </ul>
                              </div>
                            )}
                            <details>
                              <summary>Compare versions</summary>
                              {[
                                ['Local', conflict.local],
                                ['Google', conflict.remote],
                              ].map(([label, value]) => {
                                const version = value as SyncConflict['local'];
                                return (
                                  <div key={String(label)}>
                                    <p>{String(label)}</p>
                                    {version && (
                                      <>
                                        <p>{version.event.location}</p>
                                        <p>{version.event.notes}</p>
                                        {version.exceptions.length > 0 && (
                                          <ul aria-label={`${String(label)} individual changes`}>
                                            {version.exceptions.map((exception) => (
                                              <li key={exception.originalStart}>
                                                {exception.originalStart}:{' '}
                                                {exception.event
                                                  ? `${exception.event.title || 'Untitled event'} — ${exception.event.time.kind === 'timed' ? `${exception.event.time.start} – ${exception.event.time.end}` : `${exception.event.time.startDate} – ${exception.event.time.endDate}`}`
                                                  : 'Cancelled occurrence'}
                                              </li>
                                            ))}
                                          </ul>
                                        )}
                                        <p>
                                          {version.event.time.kind === 'timed'
                                            ? `${version.event.time.start} – ${version.event.time.end}`
                                            : `${version.event.time.startDate} – ${version.event.time.endDate}`}
                                        </p>
                                      </>
                                    )}
                                  </div>
                                );
                              })}
                            </details>
                            <button
                              className="text-button"
                              disabled={calendarBusy}
                              onClick={() =>
                                void calendarAction(
                                  () => calendarClient.resolveConflict(conflict, true),
                                  'Local version kept. Synchronization scheduled.',
                                )
                              }
                            >
                              Keep local
                            </button>
                            <button
                              className="text-button"
                              disabled={calendarBusy}
                              onClick={() =>
                                void calendarAction(
                                  () => calendarClient.resolveConflict(conflict, false),
                                  'Google version kept.',
                                )
                              }
                            >
                              Keep Google
                            </button>
                          </section>
                        ))}
                      {status.conflictCount > 0 && (
                        <p>{status.conflictCount} conflicts require attention</p>
                      )}
                    </div>
                  ))}
                </section>
              )}
              <div className="sidebar-bottom">
                <div className="sidebar-footer">
                  <IconButton
                    label="Open settings"
                    onClick={(event) => {
                      popupOpener.current = event.currentTarget;
                      setActivePopup('settings');
                    }}
                  >
                    <Settings2 size={16} />
                  </IconButton>
                  <IconButton
                    label="Open keyboard shortcuts"
                    onClick={(event) => {
                      popupOpener.current = event.currentTarget;
                      setActivePopup('shortcuts');
                    }}
                  >
                    <Keyboard size={16} />
                  </IconButton>
                </div>
              </div>
            </aside>
          )}
          <main className="main-content">
            <header className="toolbar">
              <div className="toolbar-start">
                {!sidebar && (
                  <IconButton label="Show sidebar" onClick={() => setSidebar(true)}>
                    <PanelLeft size={18} />
                  </IconButton>
                )}
                <div className="heading">
                  <h1 aria-label={`${heading} ${subheading}`}>
                    {heading}
                    <span>{subheading}</span>
                  </h1>
                  {view === 'week' && (
                    <p>
                      {week.toFormat('LLL d')} – {week.plus({ days: 6 }).toFormat('LLL d')}
                    </p>
                  )}
                </div>
                <div className="navigation">
                  <button className="today-button" onClick={() => setDate(today)}>
                    Today
                  </button>
                  <IconButton label={`Previous ${view}`} onClick={() => navigate(-1)}>
                    <ChevronLeft size={17} />
                  </IconButton>
                  <IconButton label={`Next ${view}`} onClick={() => navigate(1)}>
                    <ChevronRight size={17} />
                  </IconButton>
                </div>
              </div>
              <div className="toolbar-end">
                <div className="view-switcher" role="group" aria-label="Calendar view">
                  {(['month', 'week', 'day'] as const).map((item) => (
                    <button key={item} aria-pressed={view === item} onClick={() => setView(item)}>
                      {item[0].toUpperCase() + item.slice(1)}
                    </button>
                  ))}
                </div>
                <button
                  className="new-event-button"
                  ref={newButton}
                  disabled={
                    !calendarReady ||
                    !calendar.writable ||
                    capabilityState.calendars[calendar.id]?.create === false
                  }
                  aria-label="New event"
                  onClick={(e) => create(date.set({ hour: 9 }), e.currentTarget)}
                >
                  <Plus size={16} />
                  <span>New event</span>
                </button>
              </div>
            </header>
            {(failure || preferenceFailure) && (
              <div role="alert" className="editor-error">
                {failure || preferenceFailure}{' '}
                <button
                  onClick={() => {
                    setRevision((value) => value + 1);
                    if (!ready)
                      calendarClient
                        .preferences()
                        .then((p) => {
                          setView(p.view as View);
                          setSidebar(p.sidebar);
                          setAppearance(p.appearance as Appearance);
                          setReady(true);
                          setPreferenceFailure('');
                        })
                        .catch((err) => setPreferenceFailure(errorMessage(err)));
                    else if (preferenceFailure)
                      calendarClient
                        .setPreferences({ appearance, view, sidebar })
                        .then(() => setPreferenceFailure(''))
                        .catch((err) => setPreferenceFailure(errorMessage(err)));
                  }}
                >
                  Retry
                </button>
              </div>
            )}
            <div className="calendar-surface">
              {calendarReady && calendars.length === 0 && (
                <div className="calendar-empty-banner">
                  <span>Create a calendar to start adding events.</span>
                  <button
                    className="primary-button"
                    onClick={(e) => {
                      calendarDialogAnchor.current = e.currentTarget;
                      setCalendarFailure('');
                      setCalendarDialog({ calendar: null });
                    }}
                  >
                    Create calendar
                  </button>
                </div>
              )}
              {view === 'month' ? (
                <MonthView
                  date={date}
                  today={today}
                  events={shownEvents}
                  onSelect={setDate}
                  onCreate={create}
                  onEdit={edit}
                  onMove={move}
                />
              ) : (
                <TimeView
                  date={date}
                  today={today}
                  view={view}
                  events={shownEvents}
                  onCreate={create}
                  onEdit={edit}
                  onMove={move}
                  onResize={resize}
                />
              )}
            </div>
            <footer className="main-footer">
              <span>
                {selectedCount === 0
                  ? `No events on ${date.toFormat('LLL d')}`
                  : `${selectedCount} ${selectedCount === 1 ? 'event' : 'events'} on ${date.toFormat('LLL d')}`}
              </span>
              <div>
                {canUndo && (
                  <button className="undo-button" onClick={undo}>
                    <Undo2 size={13} />
                    Undo
                  </button>
                )}
                <span className="footer-zone">
                  {DateTime.local().zoneName?.replaceAll('_', ' ')}
                </span>
              </div>
            </footer>
          </main>
          <Popover.Root
            open={Boolean(editing)}
            onOpenChange={(open) => {
              if (!open && !mutating) setEditing(null);
            }}
          >
            <Popover.Anchor virtualRef={editorPositionRef} />
            <Popover.Portal>
              <Popover.Content
                className="event-popover"
                side="right"
                align="start"
                sideOffset={12}
                collisionPadding={EDITOR_MARGIN}
                sticky="always"
                updatePositionStrategy="always"
                onOpenAutoFocus={(e) => {
                  e.preventDefault();
                  requestAnimationFrame(() =>
                    document.querySelector<HTMLInputElement>('.event-title-input')?.focus(),
                  );
                }}
                onCloseAutoFocus={(e) => {
                  e.preventDefault();
                  editorAnchor.current?.focus();
                }}
                aria-label="Event editor"
              >
                {editing && (
                  <EventEditor
                    key={`${editing.event.id}:${editing.scope ?? 'single'}`}
                    event={editing.event}
                    existing={editing.existing}
                    scope={editing.scope ?? 'series'}
                    onScope={
                      editing.selected
                        ? (scope) =>
                            setEditing({
                              ...editing,
                              scope,
                              event: scope === 'series' ? editing.series!.event : editing.selected!,
                            })
                        : undefined
                    }
                    resetAffected={Boolean(
                      (calendars.find((c) => c.id === editing.event.calendarId)?.providerId !==
                        'google' ||
                        editing.scope === 'following') &&
                        editing.series &&
                        editing.scope !== 'occurrence' &&
                        ((editing.scope === 'following' &&
                          calendars.find((c) => c.id === editing.event.calendarId)?.providerId ===
                            'google') ||
                          editing.series.exceptions.some(
                            (e) =>
                              editing.scope === 'series' ||
                              e.originalStart >= editing.selected!.occurrence!.originalStart,
                          )),
                    )}
                    onPreview={async (event, countChanged) => {
                      if (editing.selected && editing.series && editing.scope !== 'occurrence') {
                        const request = requestForEdit(event, true, countChanged);
                        if (desktop) return calendarClient.preview(event, editing.series, request);
                        const changes = recurrenceDomain.plan(editing.series, request);
                        const after = changes[changes.length - 1].after;
                        return after ? previewRecurrence(after.event) : [];
                      }
                      return previewRecurrence(event);
                    }}
                    onClose={() => setEditing(null)}
                    onSave={async (event, resetExceptions, countChanged) => {
                      setMutating(true);
                      try {
                        if (editing.selected)
                          await performRecurring(
                            requestForEdit(event, resetExceptions, countChanged),
                          );
                        else if (desktop)
                          await calendarClient.save(
                            event,
                            editing.existing,
                            editing.existing ? editing.event : null,
                          );
                        else repository!.save(event);
                        setEditing(null);
                        refresh(editing.existing ? 'Event updated.' : 'Event created.');
                      } finally {
                        setMutating(false);
                      }
                    }}
                    onDelete={async (resetExceptions) => {
                      setMutating(true);
                      try {
                        if (editing.selected)
                          await performRecurring(requestForEdit(null, resetExceptions));
                        else if (desktop)
                          await calendarClient.delete(editing.event.id, editing.event);
                        else repository!.delete(editing.event.id);
                        setEditing(null);
                        refresh('Event deleted. Undo is available.');
                      } finally {
                        setMutating(false);
                      }
                    }}
                  />
                )}
              </Popover.Content>
            </Popover.Portal>
          </Popover.Root>
          <Dialog.Root
            open={Boolean(gestureEdit)}
            onOpenChange={(open) => {
              if (!open && !mutating) setGestureEdit(null);
            }}
          >
            <Dialog.Portal>
              <Dialog.Overlay className="dialog-overlay" />
              <Dialog.Content
                className="settings-dialog"
                aria-describedby="gesture-repeat-description"
              >
                <Dialog.Title>Change recurring event</Dialog.Title>
                <Dialog.Description id="gesture-repeat-description">
                  Choose which occurrences to move or resize.
                </Dialog.Description>
                <label className="repeat-row">
                  Change
                  <select
                    aria-label="Gesture change scope"
                    value={gestureScope}
                    onChange={(e) => setGestureScope(e.target.value as MutationScope)}
                  >
                    <option value="occurrence">This occurrence</option>
                    <option value="following">This and following</option>
                    <option
                      value="series"
                      disabled={
                        calendars.find((c) => c.id === gestureEdit?.original.calendarId)
                          ?.providerId === 'google'
                      }
                    >
                      Entire series
                    </option>
                  </select>
                </label>
                {gestureScope !== 'occurrence' && (
                  <label className="repeat-reset">
                    <input
                      type="checkbox"
                      aria-label="Reset gesture individual changes"
                      checked={gestureReset}
                      onChange={(e) => setGestureReset(e.target.checked)}
                    />
                    Reset affected individual edits and skipped occurrences, if any.
                  </label>
                )}
                {gestureFailure && (
                  <p role="alert" className="editor-error">
                    {gestureFailure}
                  </p>
                )}
                <div className="dialog-actions">
                  <button type="button" onClick={() => setGestureEdit(null)} disabled={mutating}>
                    Cancel
                  </button>
                  <button
                    type="button"
                    className="primary-button"
                    disabled={mutating}
                    onClick={async () => {
                      if (!gestureEdit) return;
                      setMutating(true);
                      setGestureFailure('');
                      try {
                        await performRecurring({
                          target: gestureEdit.original.id,
                          scope: gestureScope,
                          event: gestureEdit.changed,
                          expected: gestureEdit.original.occurrence!.expected,
                          resetExceptions:
                            calendars.find((c) => c.id === gestureEdit.original.calendarId)
                              ?.providerId === 'google' && gestureScope !== 'following'
                              ? false
                              : gestureReset,
                          exceptionPolicy:
                            calendars.find((c) => c.id === gestureEdit.original.calendarId)
                              ?.providerId === 'google' && gestureScope !== 'following'
                              ? 'preserve'
                              : 'reset',
                        });
                        setGestureEdit(null);
                        refresh('Recurring event changed. Undo is available.');
                      } catch (err) {
                        setGestureFailure(errorMessage(err));
                      } finally {
                        setMutating(false);
                      }
                    }}
                  >
                    Save changes
                  </button>
                </div>
              </Dialog.Content>
            </Dialog.Portal>
          </Dialog.Root>
          <Dialog.Root
            open={Boolean(calendarDialog)}
            onOpenChange={(open) => {
              if (!open && !calendarBusy) setCalendarDialog(null);
            }}
          >
            <Dialog.Portal>
              <Dialog.Overlay className="dialog-overlay" />
              <Dialog.Content
                className="settings-dialog calendar-dialog"
                aria-describedby={undefined}
                onCloseAutoFocus={(e) => {
                  e.preventDefault();
                  (calendarDialogAnchor.current?.isConnected
                    ? calendarDialogAnchor.current
                    : newButton.current
                  )?.focus();
                }}
              >
                <Dialog.Title>
                  {calendarDialog?.calendar ? 'Calendar settings' : 'Create calendar'}
                </Dialog.Title>
                {calendarDialog && (
                  <CalendarForm
                    key={calendarDialog.calendar?.id ?? 'new'}
                    calendar={calendarDialog.calendar}
                    busy={calendarBusy}
                    error={calendarFailure}
                    onCancel={() => setCalendarDialog(null)}
                    onSave={async (details) => {
                      setCalendarBusy(true);
                      setCalendarFailure('');
                      try {
                        if (calendarDialog.calendar) {
                          if (desktop)
                            await calendarClient.updateCalendar(
                              calendarDialog.calendar.id,
                              details,
                              calendarDialog.calendar,
                            );
                          else repository!.updateCalendar(calendarDialog.calendar.id, details);
                        } else {
                          if (desktop) await calendarClient.createCalendar(details);
                          else repository!.createCalendar(details);
                        }
                        setCalendarDialog(null);
                        refresh('Calendar saved.');
                      } catch (err) {
                        setCalendarFailure(errorMessage(err));
                      } finally {
                        setCalendarBusy(false);
                      }
                    }}
                  />
                )}
              </Dialog.Content>
            </Dialog.Portal>
          </Dialog.Root>
          <Dialog.Root
            open={Boolean(deletingCalendar)}
            onOpenChange={(open) => {
              if (!open && !calendarBusy) setDeletingCalendar(null);
            }}
          >
            <Dialog.Portal>
              <Dialog.Overlay className="dialog-overlay" />
              <Dialog.Content
                className="settings-dialog calendar-dialog"
                onOpenAutoFocus={(e) => {
                  e.preventDefault();
                  document.getElementById('cancel-calendar-delete')?.focus();
                }}
                onCloseAutoFocus={(e) => {
                  e.preventDefault();
                  (calendarDialogAnchor.current?.isConnected
                    ? calendarDialogAnchor.current
                    : document.querySelector<HTMLElement>('[aria-label="Create calendar"]')
                  )?.focus();
                }}
              >
                <Dialog.Title>
                  {deletingCalendar?.calendar.providerId === 'local' ? 'Delete' : 'Remove'}{' '}
                  {deletingCalendar?.calendar.name}?
                </Dialog.Title>
                <Dialog.Description>
                  {deletingCalendar?.calendar.providerId === 'local'
                    ? `This permanently deletes the calendar and all ${deletingCalendar?.count} of its events. This cannot be undone.`
                    : 'This excludes the calendar from aika and clears its cached events. The calendar remains available with its provider. Include it again through Manage account.'}
                </Dialog.Description>
                {deletingCalendar?.review &&
                  Boolean(
                    deletingCalendar.review.pendingCount || deletingCalendar.review.conflictCount,
                  ) && (
                    <section aria-label="Review calendar changes">
                      <p>
                        This calendar has {deletingCalendar.review.pendingCount} pending changes and{' '}
                        {deletingCalendar.review.conflictCount}{' '}
                        {deletingCalendar.review.conflictCount === 1 ? 'conflict' : 'conflicts'}.
                        Removing it discards these local changes and clears its cached events. This
                        cannot be undone.
                      </p>
                      <p>
                        {deletingCalendar.review.attemptedCount} pending{' '}
                        {deletingCalendar.review.attemptedCount === 1
                          ? 'change has'
                          : 'changes have'}{' '}
                        already been attempted. Changes already sent to Google may still take
                        effect. No changes are sent to Google by this removal. Other calendars stay
                        connected.
                      </p>
                      <label>
                        <input
                          type="checkbox"
                          checked={confirmCalendarDiscard}
                          disabled={calendarBusy}
                          onChange={(e) => setConfirmCalendarDiscard(e.target.checked)}
                        />{' '}
                        Discard this calendar’s local changes
                      </label>
                    </section>
                  )}
                {calendarFailure && (
                  <p role="alert" className="editor-error">
                    {calendarFailure}
                  </p>
                )}
                <div className="calendar-dialog-actions">
                  <button
                    id="cancel-calendar-delete"
                    className="text-button"
                    disabled={calendarBusy}
                    onClick={() => setDeletingCalendar(null)}
                  >
                    Cancel
                  </button>
                  <button
                    className="primary-button danger-button"
                    disabled={
                      calendarBusy ||
                      Boolean(
                        deletingCalendar?.review &&
                          (deletingCalendar.review.pendingCount ||
                            deletingCalendar.review.conflictCount) &&
                          !confirmCalendarDiscard,
                      )
                    }
                    onClick={async () => {
                      if (!deletingCalendar) return;
                      setCalendarBusy(true);
                      setCalendarFailure('');
                      try {
                        if (desktop && deletingCalendar.review)
                          await calendarClient.discardAndRemoveCalendar(deletingCalendar.review);
                        else if (desktop)
                          await calendarClient.deleteCalendar(deletingCalendar.calendar.id);
                        else repository!.deleteCalendar(deletingCalendar.calendar.id);
                        setDeletingCalendar(null);
                        refresh(
                          deletingCalendar.calendar.providerId === 'local'
                            ? 'Calendar deleted.'
                            : 'Calendar removed from aika.',
                        );
                      } catch (err) {
                        setCalendarFailure(errorMessage(err));
                        // A changed review must be shown again before another confirmation.
                        if (deletingCalendar.review) {
                          setConfirmCalendarDiscard(false);
                          try {
                            const review = await calendarClient.reviewCalendarRemoval(
                              deletingCalendar.calendar.id,
                            );
                            setDeletingCalendar({ ...deletingCalendar, review });
                          } catch {
                            setDeletingCalendar(null);
                            refresh('Calendar selection changed.');
                            setFailure(errorMessage(err));
                          }
                        }
                      } finally {
                        setCalendarBusy(false);
                      }
                    }}
                  >
                    {calendarBusy
                      ? 'Removing…'
                      : deletingCalendar?.calendar.providerId === 'local'
                        ? 'Delete calendar'
                        : deletingCalendar?.review &&
                            (deletingCalendar.review.pendingCount ||
                              deletingCalendar.review.conflictCount)
                          ? 'Discard and remove'
                          : 'Remove from aika'}
                  </button>
                </div>
              </Dialog.Content>
            </Dialog.Portal>
          </Dialog.Root>
          <SidebarPopup
            open={activePopup === 'settings'}
            onOpenChange={(open) => setActivePopup(open ? 'settings' : null)}
            title="Settings"
            closeLabel="Close settings"
            opener={popupOpener}
          >
            <div className="settings-row">
              <span>Appearance</span>
              <select
                aria-label="Appearance preference"
                value={appearance}
                onChange={(e) => setAppearance(e.target.value as Appearance)}
              >
                <option value="system">Follow system</option>
                <option value="light">Light</option>
                <option value="dark">Dark</option>
              </select>
            </div>
            <div className="settings-row">
              <span>Time zone</span>
              <span className="muted">{DateTime.local().zoneName}</span>
            </div>
            <div className="settings-row">
              <span>Region</span>
              <span className="muted">{resolveLocale()}</span>
            </div>
            <p className="settings-footnote">
              Dates and times follow your device’s regional preferences.
            </p>
          </SidebarPopup>
          <SidebarPopup
            open={activePopup === 'shortcuts'}
            onOpenChange={(open) => setActivePopup(open ? 'shortcuts' : null)}
            title="Keyboard shortcuts"
            closeLabel="Close keyboard shortcuts"
            opener={popupOpener}
            className="shortcuts-dialog"
          >
            <KeyboardShortcuts />
          </SidebarPopup>
          <div className="sr-only" role="status" aria-live="polite">
            {announcement}
          </div>
        </div>
      </CalendarContext.Provider>
    </CalendarProviders>
  );
}
