import { DateTime } from 'luxon';
import type { Recurrence } from './generated/Recurrence';
import type { RepeatFrequency } from './generated/RepeatFrequency';

const weekdays = ['Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat', 'Sun'];
export function RepeatControls({
  value,
  onChange,
  startDate,
  disabled = false,
}: {
  value: Recurrence | undefined;
  onChange: (value: Recurrence | undefined, countChanged?: boolean) => void;
  startDate: string;
  disabled?: boolean;
}) {
  const start = DateTime.fromISO(startDate);
  const selectedDays = value?.weekdays.length ? value.weekdays : [start.weekday];
  const ending =
    value?.count !== null && value?.count !== undefined
      ? 'count'
      : value?.until
        ? 'until'
        : 'never';
  const update = (patch: Partial<Recurrence>) =>
    value && onChange({ ...value, ...patch }, Object.hasOwn(patch, 'count'));
  return (
    <fieldset className="repeat-controls" disabled={disabled}>
      <legend>Repeat</legend>
      <label className="repeat-row">
        Pattern
        <select
          aria-label="Repeat pattern"
          value={value?.frequency ?? 'none'}
          onChange={(e) => {
            const frequency = e.target.value;
            onChange(
              frequency === 'none'
                ? undefined
                : {
                    frequency: frequency as RepeatFrequency,
                    interval: 1,
                    weekdays: [],
                    monthlyOn: 'date',
                    until: null,
                    count: null,
                  },
            );
          }}
        >
          <option value="none">Does not repeat</option>
          <option value="daily">Daily</option>
          <option value="weekly">Weekly</option>
          <option value="monthly">Monthly</option>
          <option value="yearly">Yearly</option>
        </select>
      </label>
      {value && (
        <>
          <label className="repeat-row">
            Every
            <input
              aria-label="Repeat interval"
              type="number"
              min={1}
              max={10000}
              required
              value={value.interval}
              onChange={(e) => update({ interval: Number(e.target.value) })}
            />
            <span>
              {
                { daily: 'days', weekly: 'weeks', monthly: 'months', yearly: 'years' }[
                  value.frequency
                ]
              }
            </span>
          </label>
          {value.frequency === 'weekly' && (
            <div className="repeat-weekdays" role="group" aria-label="Repeat weekdays">
              {weekdays.map((day, index) => (
                <label key={day}>
                  <input
                    type="checkbox"
                    aria-label={`Repeat on ${day}`}
                    checked={selectedDays.includes(index + 1)}
                    onChange={(e) =>
                      update({
                        weekdays: e.target.checked
                          ? [...selectedDays, index + 1].sort()
                          : selectedDays.filter((d) => d !== index + 1),
                      })
                    }
                  />
                  {day}
                </label>
              ))}
            </div>
          )}
          {value.frequency === 'monthly' && (
            <label className="repeat-row">
              On
              <select
                aria-label="Monthly repeat day"
                value={value.monthlyOn}
                onChange={(e) => update({ monthlyOn: e.target.value as Recurrence['monthlyOn'] })}
              >
                <option value="date">Day {start.day} of the month</option>
                <option value="weekday">
                  The{' '}
                  {['first', 'second', 'third', 'fourth', 'fifth'][Math.floor((start.day - 1) / 7)]}{' '}
                  {start.toFormat('cccc')}
                </option>
              </select>
            </label>
          )}
          <label className="repeat-row">
            Ends
            <select
              aria-label="Repeat ending"
              value={ending}
              onChange={(e) =>
                update({
                  count: e.target.value === 'count' ? 10 : null,
                  until: e.target.value === 'until' ? startDate : null,
                })
              }
            >
              <option value="never">Never</option>
              <option value="until">On a date</option>
              <option value="count">After a count</option>
            </select>
          </label>
          {ending === 'until' && (
            <label className="repeat-row">
              Last date
              <input
                aria-label="Repeat until"
                type="date"
                min={startDate}
                required
                value={value.until ?? ''}
                onChange={(e) => update({ until: e.target.value })}
              />
            </label>
          )}
          {ending === 'count' && (
            <label className="repeat-row">
              Occurrences
              <input
                aria-label="Repeat count"
                type="number"
                min={1}
                max={100000}
                required
                value={value.count ?? 10}
                onChange={(e) => update({ count: Number(e.target.value) })}
              />
            </label>
          )}
        </>
      )}
      {disabled && (
        <p className="muted">Change the pattern using “Entire series” or “This and following”.</p>
      )}
    </fieldset>
  );
}
