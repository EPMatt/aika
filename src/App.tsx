import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import type { CSSProperties, FormEvent, KeyboardEvent, MouseEvent } from 'react';
import * as Popover from '@radix-ui/react-popover';
import * as Dialog from '@radix-ui/react-dialog';
import {
  CalendarDays,
  Check,
  ChevronLeft,
  ChevronRight,
  Clock3,
  MapPin,
  PanelLeft,
  Plus,
  Settings2,
  Trash2,
  Undo2,
  X,
} from 'lucide-react';
import { DateTime } from 'luxon';
import { invoke, isTauri } from '@tauri-apps/api/core';
import type { AppInfo } from './generated/AppInfo';
import type { CalendarEvent } from './generated/CalendarEvent';
import {
  dateKey,
  daysForView,
  eventsOnDate,
  firstWeekday,
  layoutTimedEvents,
  makeEvent,
  monthDays,
  moveEvent,
  timeLabel,
  resolveLocale,
  weekStart,
} from './calendar';
import type { Appearance, View } from './calendar';
import { PrototypeRepository, sampleEvents } from './prototype';
import type { Scenario } from './prototype';

const HOUR_HEIGHT = 72;
const DRAG_TYPE = 'application/x-cal-event';
const params = new URLSearchParams(window.location.search);
const fixtureDate = params.get('date');
const initialDate =
  fixtureDate && DateTime.fromISO(fixtureDate).isValid
    ? DateTime.fromISO(fixtureDate)
    : DateTime.local();
const scenario: Scenario =
  params.get('scenario') === 'empty'
    ? 'empty'
    : params.get('scenario') === 'dense'
      ? 'dense'
      : 'standard';
const initialAppearance = (): Appearance => {
  const value = params.get('theme') ?? localStorage.getItem('cal.appearance');
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
  onClick?: () => void;
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
}: {
  event: CalendarEvent;
  onClick: (event: CalendarEvent, anchor: HTMLElement) => void;
  compact?: boolean;
}) {
  return (
    <button
      type="button"
      className={`event-chip ${event.time.kind === 'allDay' ? 'all-day-chip' : ''} ${compact ? 'compact' : ''}`}
      draggable
      onDragStart={(e) => {
        e.dataTransfer.setData(DRAG_TYPE, event.id);
        e.dataTransfer.effectAllowed = 'move';
      }}
      onClick={(e) => {
        e.stopPropagation();
        onClick(event, e.currentTarget);
      }}
      aria-label={`${event.title}, ${event.time.kind === 'allDay' ? 'all day' : timeLabel(event.time.start)}`}
    >
      {event.time.kind === 'timed' && <span className="event-dot" />}
      <span className="event-title">{event.title}</span>
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
  onCreate: (date: DateTime, anchor: HTMLElement, allDay?: boolean) => void;
  onEdit: (event: CalendarEvent, anchor: HTMLElement) => void;
  onMove: (id: string, date: DateTime) => void;
}) {
  const days = monthDays(date);
  const [overflow, setOverflow] = useState<string | null>(null);
  const [capacity, setCapacity] = useState({ full: 3, overflow: 2 });
  const gridRef = useRef<HTMLDivElement>(null);
  const keyboardFocus = useRef(false);
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
    <div className="month-view" aria-label="Month calendar">
      <div className="weekday-heading">
        {days.slice(0, 7).map((day) => (
          <span key={day.weekday}>{day.toFormat('ccc')}</span>
        ))}
      </div>
      <div className="month-grid" role="grid" ref={gridRef} aria-label={date.toFormat('LLLL yyyy')}>
        {Array.from({ length: 6 }, (_, row) => (
          <div className="month-row" role="row" key={row}>
            {days.slice(row * 7, row * 7 + 7).map((day) => {
              const items = eventsOnDate(events, day);
              const limit = items.length <= capacity.full ? capacity.full : capacity.overflow;
              const key = dateKey(day);
              return (
                <div
                  key={key}
                  role="gridcell"
                  aria-label={day.toLocaleString(DateTime.DATE_FULL)}
                  aria-selected={day.hasSame(date, 'day')}
                  tabIndex={day.hasSame(date, 'day') ? 0 : -1}
                  data-date={key}
                  className={`month-cell ${day.month !== date.month ? 'outside' : ''} ${day.hasSame(today, 'day') ? 'is-today' : ''}`}
                  onKeyDown={(e) => keyNavigate(e, day)}
                  onClick={() => onSelect(day)}
                  onDoubleClick={(e) => {
                    if (!(e.target as HTMLElement).closest('button'))
                      onCreate(day.set({ hour: 9 }), e.currentTarget, true);
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
                  <div className="cell-events">
                    {items.slice(0, limit).map((event) => (
                      <EventChip key={event.id} event={event} onClick={onEdit} />
                    ))}
                    {items.length > limit && (
                      <Popover.Root
                        open={overflow === key}
                        onOpenChange={(open) => setOverflow(open ? key : null)}
                      >
                        <Popover.Trigger asChild>
                          <button className="overflow-button" onClick={(e) => e.stopPropagation()}>
                            +{items.length - limit} more
                          </button>
                        </Popover.Trigger>
                        <Popover.Portal>
                          <Popover.Content
                            className="overflow-popover"
                            sideOffset={8}
                            collisionPadding={16}
                          >
                            <div className="overflow-heading">{day.toFormat('cccc, LLLL d')}</div>
                            {items.map((event) => (
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
}: {
  date: DateTime;
  today: DateTime;
  view: 'week' | 'day';
  events: CalendarEvent[];
  onCreate: (date: DateTime, anchor: HTMLElement, allDay?: boolean) => void;
  onEdit: (event: CalendarEvent, anchor: HTMLElement) => void;
  onMove: (id: string, date: DateTime) => void;
}) {
  const days = daysForView(date, view);
  const scrollRef = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (scrollRef.current) scrollRef.current.scrollTop = HOUR_HEIGHT * 7.5;
  }, [view]);
  const countStyle = { '--days': days.length } as CSSProperties;
  const clickSlot = (e: MouseEvent<HTMLDivElement>, day: DateTime) => {
    if ((e.target as HTMLElement).closest('button')) return;
    const rect = e.currentTarget.getBoundingClientRect();
    const minute = Math.min(
      1425,
      Math.max(0, Math.floor(((e.clientY - rect.top) / HOUR_HEIGHT) * 4) * 15),
    );
    onCreate(day.set({ hour: Math.floor(minute / 60), minute: minute % 60 }), e.currentTarget);
  };
  return (
    <div
      className="time-view"
      style={countStyle}
      aria-label={`${view === 'week' ? 'Week' : 'Day'} calendar`}
    >
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
      <div className="all-day-row">
        <span className="all-day-label">all-day</span>
        {days.map((day) => (
          <div
            className="all-day-cell"
            key={dateKey(day)}
            onDoubleClick={(e) => onCreate(day, e.currentTarget, true)}
          >
            {eventsOnDate(events, day)
              .filter((event) => event.time.kind === 'allDay')
              .map((event) => (
                <EventChip key={event.id} event={event} compact onClick={onEdit} />
              ))}
          </div>
        ))}
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
              onClick={(e) => clickSlot(e, day)}
              onDragOver={(e) => {
                if (Array.from(e.dataTransfer.types).includes(DRAG_TYPE)) e.preventDefault();
              }}
              onDrop={(e) => {
                e.preventDefault();
                onMove(e.dataTransfer.getData(DRAG_TYPE), day);
              }}
            >
              {Array.from({ length: 24 }, (_, hour) => (
                <div className="hour-slot" key={hour} />
              ))}
              {layoutTimedEvents(events, day).map(({ event, start, end, lane, lanes }) => (
                <button
                  key={event.id}
                  className="time-event"
                  draggable
                  title={event.title}
                  aria-label={`${event.title}, ${event.time.kind === 'timed' ? timeLabel(event.time.start) : 'all day'}`}
                  style={{
                    top: (start / 60) * HOUR_HEIGHT + 2,
                    height: Math.max(28, ((end - start) / 60) * HOUR_HEIGHT - 4),
                    left: `calc(${(lane / lanes) * 100}% + 5px)`,
                    width: `calc(${100 / lanes}% - 10px)`,
                  }}
                  onDragStart={(e) => {
                    e.dataTransfer.setData(DRAG_TYPE, event.id);
                    e.dataTransfer.effectAllowed = 'move';
                  }}
                  onClick={(e) => {
                    e.stopPropagation();
                    onEdit(event, e.currentTarget);
                  }}
                >
                  <span className="time-event-title">{event.title}</span>
                  <span className="time-event-time">
                    {event.time.kind === 'timed'
                      ? `${timeLabel(event.time.start)} – ${timeLabel(event.time.end)}`
                      : ''}
                  </span>
                  {event.location && end - start >= 90 && (
                    <span className="time-event-location">
                      <MapPin size={11} />
                      {event.location}
                    </span>
                  )}
                </button>
              ))}
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
}: {
  event: CalendarEvent;
  existing: boolean;
  onSave: (event: CalendarEvent) => void;
  onDelete: () => void;
  onClose: () => void;
}) {
  const [title, setTitle] = useState(event.title);
  const [allDay, setAllDay] = useState(event.time.kind === 'allDay');
  const start =
    event.time.kind === 'timed'
      ? DateTime.fromISO(event.time.start)
      : DateTime.fromISO(event.time.startDate);
  const end =
    event.time.kind === 'timed'
      ? DateTime.fromISO(event.time.end)
      : DateTime.fromISO(event.time.endDate).minus({ days: 1 });
  const [startDate, setStartDate] = useState(dateKey(start));
  const [endDate, setEndDate] = useState(dateKey(end));
  const [startTime, setStartTime] = useState(start.toFormat('HH:mm'));
  const [endTime, setEndTime] = useState(end.toFormat('HH:mm'));
  const [location, setLocation] = useState(event.location);
  const [notes, setNotes] = useState(event.notes);
  const [details, setDetails] = useState(Boolean(location || notes));
  const [error, setError] = useState('');

  const submit = (e: FormEvent) => {
    e.preventDefault();
    if (!title.trim()) {
      setError('Give your event a title.');
      return;
    }
    const from = DateTime.fromISO(allDay ? startDate : `${startDate}T${startTime}`);
    const until = DateTime.fromISO(allDay ? endDate : `${endDate}T${endTime}`);
    if (!from.isValid || !until.isValid || (allDay ? until < from : until <= from)) {
      setError(
        allDay
          ? 'The end date must be on or after the start date.'
          : 'The event must end after it starts.',
      );
      return;
    }
    onSave({
      ...event,
      title: title.trim(),
      location: location.trim(),
      notes,
      time: allDay
        ? { kind: 'allDay', startDate, endDate: dateKey(until.plus({ days: 1 })) }
        : {
            kind: 'timed',
            start: from.toUTC().toISO()!,
            end: until.toUTC().toISO()!,
            timeZone: from.zoneName!,
          },
    });
  };

  return (
    <form className="editor" onSubmit={submit}>
      <div className="editor-heading">
        <span className="eyebrow">{existing ? 'Edit event' : 'New event'}</span>
        <IconButton label="Close event editor" onClick={onClose}>
          <X size={16} />
        </IconButton>
      </div>
      <input
        className="event-title-input"
        aria-label="Event title"
        placeholder="Event title"
        value={title}
        onChange={(e) => setTitle(e.target.value)}
        maxLength={300}
        autoFocus
      />
      <div className="editor-calendar">
        <span className="event-dot" />
        <span>Personal</span>
        <span className="muted">Local calendar</span>
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
              type="date"
              required
              value={startDate}
              onChange={(e) => {
                setStartDate(e.target.value);
                if (endDate < e.target.value) setEndDate(e.target.value);
              }}
            />
            {!allDay && (
              <input
                aria-label="Start time"
                type="time"
                required
                value={startTime}
                onChange={(e) => setStartTime(e.target.value)}
              />
            )}
          </div>
          <div className="date-field-row">
            <label htmlFor="event-end-date">Ends</label>
            <input
              id="event-end-date"
              type="date"
              required
              value={endDate}
              min={startDate}
              onChange={(e) => setEndDate(e.target.value)}
            />
            {!allDay && (
              <input
                aria-label="End time"
                type="time"
                required
                value={endTime}
                onChange={(e) => setEndTime(e.target.value)}
              />
            )}
          </div>
        </div>
      </div>
      {!allDay && (
        <p className="timezone-note">{DateTime.local().zoneName?.replaceAll('_', ' ')}</p>
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
      {error && (
        <p role="alert" className="editor-error">
          {error}
        </p>
      )}
      <div className="editor-footer">
        {existing ? (
          <IconButton label="Delete event" className="delete-button" onClick={onDelete}>
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
    </form>
  );
}

export function App() {
  const [date, setDate] = useState<DateTime>(initialDate.startOf('day'));
  const today = initialDate.startOf('day');
  const [view, setView] = useState<View>('month');
  const [sidebar, setSidebar] = useState(window.innerWidth >= 900);
  const [visible, setVisible] = useState(true);
  const [appearance, setAppearance] = useState<Appearance>(initialAppearance);
  const [settings, setSettings] = useState(false);
  const [runtime, setRuntime] = useState('browser');
  const repository = useMemo(() => new PrototypeRepository(sampleEvents(today, scenario)), []);
  const [events, setEvents] = useState(() => repository.list());
  const [editing, setEditing] = useState<{ event: CalendarEvent; existing: boolean } | null>(null);
  const editorAnchor = useRef<HTMLElement | null>(null);
  const newButton = useRef<HTMLButtonElement>(null);
  const [announcement, setAnnouncement] = useState('');

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
    localStorage.setItem('cal.appearance', appearance);
    media.addEventListener('change', update);
    return () => media.removeEventListener('change', update);
  }, [appearance]);
  useEffect(() => {
    if (isTauri())
      invoke<AppInfo>('get_app_info')
        .then(() => setRuntime('desktop'))
        .catch(() => setRuntime('desktop-error'));
  }, []);
  const refresh = (message: string) => {
    setEvents(repository.list());
    setAnnouncement(message);
  };
  const create = useCallback((day: DateTime, anchor: HTMLElement, allDay = false) => {
    editorAnchor.current = anchor;
    setEditing({ event: makeEvent(day, allDay), existing: false });
  }, []);
  const edit = (event: CalendarEvent, anchor: HTMLElement) => {
    editorAnchor.current = anchor;
    setEditing({ event, existing: true });
  };
  const move = (id: string, target: DateTime) => {
    const event = events.find((item) => item.id === id);
    if (event) {
      repository.save(moveEvent(event, target));
      refresh('Event moved. Undo is available.');
    }
  };
  const undo = () => {
    repository.undo();
    refresh('Change undone.');
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
      if (target.closest('input, textarea, [contenteditable="true"]') || editing || settings)
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
  }, [date, editing, settings, create]);

  const heading = view === 'day' ? date.toFormat('cccc') : date.toFormat('LLLL');
  const subheading = view === 'day' ? date.toFormat('d LLLL yyyy') : String(date.year);
  const shownEvents = visible ? events : [];
  const selectedCount = eventsOnDate(events, date).length;
  const week = weekStart(date, firstWeekday());

  return (
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
            <div className="section-label">Your calendars</div>
            <button
              className="calendar-toggle"
              aria-pressed={visible}
              onClick={() => setVisible(!visible)}
            >
              <span className={`calendar-check ${visible ? 'checked' : ''}`}>
                {visible && <Check size={11} strokeWidth={3} />}
              </span>
              <span>Personal</span>
              <span className="calendar-local">Local</span>
            </button>
          </div>
          <div className="sidebar-bottom">
            <div className="sidebar-footer">
              <IconButton label="Open settings" onClick={() => setSettings(true)}>
                <Settings2 size={16} />
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
            <div className="view-switcher" aria-label="Calendar view">
              {(['month', 'week', 'day'] as const).map((item) => (
                <button key={item} aria-pressed={view === item} onClick={() => setView(item)}>
                  {item[0].toUpperCase() + item.slice(1)}
                </button>
              ))}
            </div>
            <button
              className="new-event-button"
              ref={newButton}
              aria-label="New event"
              onClick={(e) => create(date.set({ hour: 9 }), e.currentTarget)}
            >
              <Plus size={16} />
              <span>New event</span>
            </button>
          </div>
        </header>
        <div className="calendar-surface">
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
            {repository.canUndo() && (
              <button className="undo-button" onClick={undo}>
                <Undo2 size={13} />
                Undo
              </button>
            )}
            <span className="footer-zone">{DateTime.local().zoneName?.replaceAll('_', ' ')}</span>
          </div>
        </footer>
      </main>
      <Popover.Root
        open={Boolean(editing)}
        onOpenChange={(open) => {
          if (!open) setEditing(null);
        }}
      >
        <Popover.Anchor virtualRef={editorAnchor} />
        <Popover.Portal>
          <Popover.Content
            className="event-popover"
            side="right"
            align="start"
            sideOffset={12}
            collisionPadding={18}
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
                key={editing.event.id}
                event={editing.event}
                existing={editing.existing}
                onClose={() => setEditing(null)}
                onSave={(event) => {
                  repository.save(event);
                  setEditing(null);
                  refresh(editing.existing ? 'Event updated.' : 'Event created.');
                }}
                onDelete={() => {
                  repository.delete(editing.event.id);
                  setEditing(null);
                  refresh('Event deleted. Undo is available.');
                }}
              />
            )}
          </Popover.Content>
        </Popover.Portal>
      </Popover.Root>
      <Dialog.Root open={settings} onOpenChange={setSettings}>
        <Dialog.Portal>
          <Dialog.Overlay className="dialog-overlay" />
          <Dialog.Content className="settings-dialog" aria-describedby={undefined}>
            <div className="settings-heading">
              <Dialog.Title>Settings</Dialog.Title>
              <Dialog.Close asChild>
                <button className="icon-button" aria-label="Close settings">
                  <X size={17} />
                </button>
              </Dialog.Close>
            </div>
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
          </Dialog.Content>
        </Dialog.Portal>
      </Dialog.Root>
      <div className="sr-only" role="status" aria-live="polite">
        {announcement}
      </div>
    </div>
  );
}
