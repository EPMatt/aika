import { useRef, useState } from 'react';
import * as Popover from '@radix-ui/react-popover';
import { MoreHorizontal } from 'lucide-react';
import type { Calendar } from './generated/Calendar';
import type { CalendarDetails } from './generated/CalendarDetails';
import { calendarPalette } from './calendars';

export function CalendarMenu({
  calendar,
  isDefault,
  busy,
  onDefault,
  onSettings,
  onDelete,
}: {
  calendar: Calendar;
  isDefault: boolean;
  busy: boolean;
  onDefault: () => void;
  onSettings: (anchor: HTMLElement) => void;
  onDelete: (anchor: HTMLElement) => void;
}) {
  const [open, setOpen] = useState(false);
  const trigger = useRef<HTMLButtonElement>(null);
  const content = useRef<HTMLDivElement>(null);
  const openingDialog = useRef(false);
  const items = () =>
    Array.from(
      content.current?.querySelectorAll<HTMLButtonElement>('[role="menuitem"]:not(:disabled)') ??
        [],
    );
  const select = (action: () => void) => {
    setOpen(false);
    action();
  };
  return (
    <Popover.Root open={open} onOpenChange={setOpen}>
      <Popover.Trigger asChild>
        <button
          ref={trigger}
          className="icon-button calendar-menu-trigger"
          aria-label={`Options for ${calendar.name}`}
          aria-haspopup="menu"
          disabled={busy}
          onKeyDown={(e) => {
            if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
              e.preventDefault();
              setOpen(true);
            }
          }}
        >
          <MoreHorizontal size={16} />
        </button>
      </Popover.Trigger>
      <Popover.Portal>
        <Popover.Content
          ref={content}
          className="calendar-menu"
          role="menu"
          aria-label={`${calendar.name} options`}
          side="right"
          align="start"
          sideOffset={6}
          onOpenAutoFocus={(e) => {
            e.preventDefault();
            openingDialog.current = false;
            items()[0]?.focus();
          }}
          onCloseAutoFocus={(e) => {
            if (openingDialog.current) e.preventDefault();
          }}
          onKeyDown={(e) => {
            const buttons = items();
            const index = buttons.indexOf(document.activeElement as HTMLButtonElement);
            if (['ArrowDown', 'ArrowUp', 'Home', 'End'].includes(e.key)) {
              e.preventDefault();
              const next =
                e.key === 'Home'
                  ? 0
                  : e.key === 'End'
                    ? buttons.length - 1
                    : (index + (e.key === 'ArrowDown' ? 1 : -1) + buttons.length) % buttons.length;
              buttons[next]?.focus();
            }
            if (e.key === 'Tab') {
              e.preventDefault();
              setOpen(false);
            }
          }}
        >
          <button role="menuitem" disabled={isDefault || busy} onClick={() => select(onDefault)}>
            {isDefault ? 'Default calendar' : 'Set as default'}
          </button>
          <button
            role="menuitem"
            disabled={busy}
            onClick={() => {
              openingDialog.current = true;
              select(() => onSettings(trigger.current!));
            }}
          >
            Settings
          </button>
          <button
            role="menuitem"
            className="calendar-delete-menu"
            disabled={busy}
            onClick={() => {
              openingDialog.current = true;
              select(() => onDelete(trigger.current!));
            }}
          >
            Delete calendar
          </button>
        </Popover.Content>
      </Popover.Portal>
    </Popover.Root>
  );
}

export function CalendarForm({
  calendar,
  busy,
  error,
  onCancel,
  onSave,
}: {
  calendar: Calendar | null;
  busy: boolean;
  error: string;
  onCancel: () => void;
  onSave: (details: CalendarDetails) => Promise<void>;
}) {
  const [name, setName] = useState(calendar?.name ?? '');
  const [description, setDescription] = useState(calendar?.description ?? '');
  const [color, setColor] = useState(calendar?.color ?? calendarPalette[0][1]);
  return (
    <form
      className="calendar-form"
      onSubmit={(e) => {
        e.preventDefault();
        if (!busy && name.trim()) void onSave({ name: name.trim(), description, color });
      }}
    >
      <fieldset disabled={busy}>
        <label>
          Calendar name
          <input
            aria-label="Calendar name"
            value={name}
            maxLength={100}
            required
            onChange={(e) => setName(e.target.value)}
            autoFocus
          />
        </label>
        <label>
          Description
          <textarea
            aria-label="Calendar description"
            value={description}
            maxLength={2000}
            rows={3}
            onChange={(e) => setDescription(e.target.value)}
          />
        </label>
        <div role="group" aria-label="Calendar color" className="calendar-palette">
          <span>Color</span>
          <div>
            {calendarPalette.map(([label, value]) => (
              <button
                type="button"
                key={value}
                aria-label={label}
                aria-pressed={color === value}
                title={label}
                className="calendar-swatch"
                style={{ backgroundColor: value }}
                onClick={() => setColor(value)}
              >
                {color === value && <span aria-hidden="true">✓</span>}
              </button>
            ))}
          </div>
        </div>
        {error && (
          <p role="alert" className="editor-error">
            {error}
          </p>
        )}
        <div className="calendar-dialog-actions">
          <button type="button" className="text-button" onClick={onCancel}>
            Cancel
          </button>
          <button type="submit" className="primary-button" disabled={!name.trim()}>
            {busy ? 'Saving…' : calendar ? 'Save changes' : 'Create calendar'}
          </button>
        </div>
      </fieldset>
    </form>
  );
}
