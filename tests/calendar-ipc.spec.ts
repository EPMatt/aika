import { expect, test } from '@playwright/test';

// Exercise renderer refresh/error handling against the collection IPC contract.
// Durable SQLite behavior is independently covered by Rust and native tests.
test.beforeEach(async ({ page }) => {
  await page.addInitScript(() => {
    Object.defineProperty(window, 'isTauri', { value: true });
    const initial = {
      calendars: [
        {
          id: 'local-default',
          providerId: 'local',
          name: 'Personal',
          description: '',
          color: '#718675',
          writable: true,
        },
      ],
      events: [] as any[],
      defaultCalendarId: 'local-default' as string | null,
      hiddenCalendarIds: [] as string[],
      canUndo: false,
    };
    let state = JSON.parse(localStorage.getItem('calendar.test.state') ?? JSON.stringify(initial));
    (window as any).calendarTestState = state;
    Object.defineProperty(window, '__TAURI_INTERNALS__', {
      value: {
        invoke: async (command: string, args: any) => {
          if ((window as any).calendarFailCommand === command) {
            (window as any).calendarFailCommand = '';
            throw { code: 'storage', message: 'Calendar database is unavailable.' };
          }
          switch (command) {
            case 'get_app_info':
              return { name: 'aika', version: 'test', provider: 'local' };
            case 'get_preferences':
              return { appearance: 'light', view: 'month', sidebar: true };
            case 'set_preferences':
              return;
            case 'calendar_snapshot':
              return structuredClone(state);
            case 'create_calendar': {
              const calendar = {
                ...args.details,
                id: crypto.randomUUID(),
                providerId: 'local',
                writable: true,
              };
              state.calendars.push(calendar);
              state.defaultCalendarId ??= calendar.id;
              localStorage.setItem('calendar.test.state', JSON.stringify(state));
              return calendar;
            }
            case 'update_calendar': {
              const index = state.calendars.findIndex((c: any) => c.id === args.id);
              if (JSON.stringify(state.calendars[index]) !== JSON.stringify(args.expected))
                throw {
                  code: 'conflict',
                  message: 'The calendar changed. Reopen its settings before saving.',
                };
              state.calendars[index] = { ...state.calendars[index], ...args.details };
              break;
            }
            case 'set_default_calendar':
              state.defaultCalendarId = args.id;
              break;
            case 'set_calendar_visibility':
              state.hiddenCalendarIds = state.hiddenCalendarIds.filter(
                (id: string) => id !== args.id,
              );
              if (!args.visible) state.hiddenCalendarIds.push(args.id);
              break;
            case 'calendar_event_count':
              return state.events.filter((e: any) => e.calendarId === args.id).length;
            case 'delete_calendar':
              state.calendars = state.calendars.filter((c: any) => c.id !== args.id);
              state.events = state.events.filter((e: any) => e.calendarId !== args.id);
              state.hiddenCalendarIds = state.hiddenCalendarIds.filter(
                (id: string) => id !== args.id,
              );
              if (state.defaultCalendarId === args.id)
                state.defaultCalendarId = state.calendars[0]?.id ?? null;
              break;
            default:
              throw new Error(`Unexpected command ${command}`);
          }
          localStorage.setItem('calendar.test.state', JSON.stringify(state));
        },
      },
    });
  });
});

test('calendar write failures retain drafts and visibility; reload hydrates metadata and empty state', async ({
  page,
}) => {
  await page.goto('/');
  await page.getByRole('button', { name: 'Create calendar', exact: true }).click();
  await page.getByLabel('Calendar name').fill('Work');
  await page.getByLabel('Calendar description').fill('Office schedule');
  await page.getByRole('button', { name: 'Teal', exact: true }).click();
  await page
    .getByRole('dialog')
    .getByRole('button', { name: 'Create calendar', exact: true })
    .click();
  await expect(page.getByRole('dialog')).toHaveCount(0);
  const openMenu = async (name: string) => {
    await page.locator('.calendar-name').getByText(name, { exact: true }).hover();
    await page.getByRole('button', { name: `Options for ${name}` }).click();
  };
  await openMenu('Work');
  await page.getByRole('menuitem', { name: 'Settings', exact: true }).click();
  await page.getByLabel('Calendar name').fill('Office');
  await page.evaluate(() => {
    (window as any).calendarFailCommand = 'update_calendar';
  });
  await page.getByRole('button', { name: 'Save changes' }).click();
  await expect(page.getByRole('dialog').getByRole('alert')).toContainText(
    'database is unavailable',
  );
  await expect(page.getByLabel('Calendar name')).toHaveValue('Office');
  await expect(page.locator('.calendar-name').getByText('Work', { exact: true })).toHaveCount(1);
  await page.getByRole('button', { name: 'Save changes' }).click();
  await expect(page.getByRole('dialog')).toHaveCount(0);
  await openMenu('Office');
  await page.getByRole('menuitem', { name: 'Set as default' }).click();
  const officeRow = page
    .locator('.calendar-row')
    .filter({ has: page.getByText('Office', { exact: true }) });
  await expect(officeRow).toContainText('Default');
  await page.evaluate(() => {
    (window as any).calendarFailCommand = 'set_calendar_visibility';
  });
  await page.getByRole('button', { name: 'Hide Office calendar', exact: true }).click();
  await expect(page.getByRole('alert')).toContainText('database is unavailable');
  await expect(
    page.getByRole('button', { name: 'Hide Office calendar', exact: true }),
  ).toHaveAttribute('aria-pressed', 'true');
  await page.getByRole('button', { name: 'Hide Office calendar', exact: true }).click();
  await expect(
    page.getByRole('button', { name: 'Show Office calendar', exact: true }),
  ).toHaveAttribute('aria-pressed', 'false');
  await page.reload();
  await expect(
    page.getByRole('button', { name: 'Show Office calendar', exact: true }),
  ).toHaveAttribute('aria-pressed', 'false');
  await expect(officeRow).toContainText('Default');
  await openMenu('Office');
  await page.getByRole('menuitem', { name: 'Settings', exact: true }).click();
  await expect(page.getByLabel('Calendar description')).toHaveValue('Office schedule');
  await expect(page.getByRole('button', { name: 'Teal', exact: true })).toHaveAttribute(
    'aria-pressed',
    'true',
  );
  await page.keyboard.press('Escape');
  for (const name of ['Office', 'Personal']) {
    await openMenu(name);
    await page.getByRole('menuitem', { name: 'Delete calendar' }).click();
    await page.getByRole('dialog').getByRole('button', { name: 'Delete calendar' }).click();
    await expect(page.getByRole('dialog')).toHaveCount(0);
  }
  await page.reload();
  await expect(page.locator('.calendar-empty-banner')).toBeVisible();
  await expect(page.locator('.calendar-row')).toHaveCount(0);
  await expect(page.getByRole('button', { name: 'New event', exact: true })).toBeDisabled();
});

test('focus refresh preserves calendar settings drafts and rejects stale metadata', async ({
  page,
}) => {
  await page.goto('/');
  await page.getByRole('button', { name: 'Options for Personal' }).focus();
  await page.keyboard.press('ArrowDown');
  await page.getByRole('menuitem', { name: 'Settings', exact: true }).click();
  await page.getByLabel('Calendar name').fill('Unsaved draft');
  await page.evaluate(() => {
    (window as any).calendarTestState.calendars[0].name = 'External name';
    window.dispatchEvent(new Event('focus'));
  });
  await expect(
    page.locator('.calendar-name').getByText('External name', { exact: true }),
  ).toHaveCount(1);
  await expect(page.getByLabel('Calendar name')).toHaveValue('Unsaved draft');
  await page.getByRole('button', { name: 'Save changes' }).click();
  await expect(page.getByRole('dialog').getByRole('alert')).toContainText('Reopen its settings');
  await expect(page.getByLabel('Calendar name')).toHaveValue('Unsaved draft');
});

test('deleting a calendar externally preserves an open event draft and disables saving', async ({
  page,
}) => {
  await page.goto('/');
  await page.getByRole('button', { name: 'New event', exact: true }).click();
  await page.getByLabel('Event title').fill('Unsaved event draft');
  await page.evaluate(() => {
    const state = (window as any).calendarTestState;
    state.calendars = [];
    state.events = [];
    state.defaultCalendarId = null;
    window.dispatchEvent(new Event('focus'));
  });
  await expect(page.locator('.calendar-empty-banner')).toBeVisible();
  await expect(page.getByLabel('Event title')).toHaveValue('Unsaved event draft');
  await expect(page.getByLabel('Event title')).toBeDisabled();
  await expect(page.getByLabel('Event editor').getByRole('alert')).toContainText(
    'calendar was deleted',
  );
  await page.getByRole('button', { name: 'Close event editor' }).click();
  await expect(page.getByLabel('Event editor')).toHaveCount(0);
});
