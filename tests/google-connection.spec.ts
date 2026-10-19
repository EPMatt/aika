import { expect, test } from '@playwright/test';
import AxeBuilder from '@axe-core/playwright';

test('provider picker supports keyboard focus and explains unavailable Google in preview', async ({
  page,
}) => {
  await page.goto('/?date=2026-10-09');
  const plus = page.getByRole('button', { name: 'Create calendar', exact: true });
  await plus.focus();
  await page.keyboard.press('ArrowDown');
  await expect(page.getByRole('menuitem', { name: 'On this device' })).toBeFocused();
  await expect(page.getByRole('menuitem', { name: 'Google Calendar' })).toBeDisabled();
  await expect(page.getByText('Connect Google accounts in the desktop app.')).toBeVisible();
  await page.keyboard.press('Escape');
  await expect(plus).toBeFocused();
  await plus.click();
  await page.getByRole('menuitem', { name: 'On this device' }).click();
  await expect(page.getByLabel('Calendar name')).toBeFocused();
  await page.keyboard.press('Escape');
  await expect(plus).toBeFocused();
});

for (const theme of ['light', 'dark']) {
  test(`mocked native ${theme} connection, selection revision, failure recovery and disconnect`, async ({
    page,
  }) => {
    await page.addInitScript(() => {
      Object.defineProperty(window, 'isTauri', { value: true });
      const caps = {
        read: true,
        create: false,
        edit: false,
        delete: false,
        recurrence: false,
        customizeMetadata: true,
        moveEvent: false,
      };
      const localCaps = {
        ...caps,
        create: true,
        edit: true,
        delete: true,
        recurrence: true,
        moveEvent: true,
      };
      const account = {
        id: 'account',
        providerId: 'google',
        remoteSubject: 'fictional',
        displayName: 'test@example.invalid',
        connectionState: 'connected',
        selectedCalendarIds: [] as string[],
      };
      let connected = false;
      let active = false;
      let selected: string[] = [];
      (window as any).googleTest = {
        commands: [] as string[],
        selections: [] as string[][],
        failDiscovery: false,
        failBrowser: false,
        failBrowserAfterStart: false,
        cancelCount: 0,
      };
      const fixture = (window as any).googleTest;
      Object.defineProperty(window, '__TAURI_INTERNALS__', {
        value: {
          invoke: async (command: string, args: any) => {
            fixture.commands.push(command);
            switch (command) {
              case 'sync_conflicts':
                return [];
              case 'get_app_info':
                return { name: 'aika', version: 'test', provider: 'local' };
              case 'get_preferences':
                return {
                  appearance: new URLSearchParams(location.search).get('theme') ?? 'light',
                  view: 'month',
                  sidebar: true,
                };
              case 'request_sync':
              case 'set_preferences':
                return;
              case 'list_providers':
                return [
                  {
                    id: 'local',
                    name: 'On this device',
                    connectionAvailable: true,
                    unavailableReason: null,
                    capabilities: localCaps,
                  },
                  {
                    id: 'google',
                    name: 'Google Calendar',
                    connectionAvailable: true,
                    unavailableReason: null,
                    capabilities: caps,
                  },
                ];
              case 'calendar_snapshot':
                return {
                  calendars: [
                    {
                      id: 'local-default',
                      providerId: 'local',
                      name: 'Personal',
                      description: '',
                      color: '#718675',
                      writable: true,
                    },
                    ...selected.map((id) => ({
                      id,
                      providerId: 'google',
                      name: id === 'one' ? 'Work' : 'Hidden remotely',
                      description: '',
                      color: '#718675',
                      writable: false,
                    })),
                  ],
                  events: [],
                  defaultCalendarId: 'local-default',
                  hiddenCalendarIds: [],
                  canUndo: false,
                  accounts: connected ? [{ ...account, selectedCalendarIds: selected }] : [],
                  calendarCapabilities: { 'local-default': localCaps, one: caps, two: caps },
                  eventCapabilities: {},
                };
              case 'start_connection':
                if (fixture.failBrowser) {
                  fixture.failBrowser = false;
                  throw { code: 'browser', message: 'Could not open the system browser.' };
                }
                active = true;
                return {
                  id: 'session',
                  progress: 'waitingForBrowser',
                  accountId: null,
                  failure: null,
                };
              case 'connection_status':
                if (fixture.failBrowserAfterStart) {
                  fixture.failBrowserAfterStart = false;
                  active = false;
                  return {
                    id: 'session',
                    progress: 'failed',
                    accountId: null,
                    failure: { code: 'browser', message: 'The browser launcher failed.' },
                  };
                }
                connected = true;
                active = false;
                return {
                  id: 'session',
                  progress: 'connected',
                  accountId: 'account',
                  failure: null,
                };
              case 'cancel_connection':
                active = false;
                fixture.cancelCount++;
                return;
              case 'discover_calendars':
                if (fixture.failDiscovery) {
                  fixture.failDiscovery = false;
                  throw { code: 'network', message: 'Discovery temporarily unavailable.' };
                }
                return {
                  id: 'discovery',
                  account,
                  calendars: [
                    {
                      remoteId: 'one',
                      name: 'Work',
                      color: '#718675',
                      description: '',
                      accessRole: 'owner',
                      eligible: true,
                      disabledReason: null,
                      selected: selected.length ? selected.includes('one') : true,
                    },
                    {
                      remoteId: 'two',
                      name: 'Hidden remotely',
                      color: '#718675',
                      description: '',
                      accessRole: 'reader',
                      eligible: true,
                      disabledReason: null,
                      selected: selected.length ? selected.includes('two') : true,
                    },
                    {
                      remoteId: 'busy',
                      name: 'Availability',
                      color: '#718675',
                      description: '',
                      accessRole: 'freeBusyReader',
                      eligible: false,
                      disabledReason: 'Only free/busy access is available.',
                      selected: false,
                    },
                  ],
                };
              case 'commit_calendar_selection':
                selected = args.selected;
                fixture.selections.push(selected);
                return;
              case 'review_disconnect':
                return {
                  accountId: 'account',
                  pendingCount: 0,
                  conflictCount: 0,
                  attemptedCount: 0,
                  revision: 'empty',
                };
              case 'disconnect_account':
                account.connectionState = 'disconnected';
                return;
              default:
                throw new Error(`Unexpected command ${command}; active=${active}`);
            }
          },
        },
      });
    });
    await page.goto(`/?date=2026-10-09&theme=${theme}`);
    await expect(page.locator('html')).toHaveAttribute('data-theme', theme);
    const plus = page.getByRole('button', { name: 'Create calendar', exact: true });
    await plus.click();
    await page.getByRole('menuitem', { name: 'Google Calendar' }).click();
    const dialog = page.getByRole('dialog', { name: 'Connect Google Calendar' });
    const signIn = dialog.getByRole('button', { name: 'Sign in with Google' });
    await expect(signIn).toBeVisible();
    expect(await page.evaluate(() => (window as any).googleTest.commands)).not.toContain(
      'start_connection',
    );
    await expect(dialog.getByRole('status')).toHaveCount(0);
    await page.evaluate(() => ((window as any).googleTest.failBrowser = true));
    await signIn.click();
    await expect(dialog.getByRole('alert')).toHaveText('Could not open the system browser.');
    await expect(signIn).toBeEnabled();
    await page.evaluate(() => ((window as any).googleTest.failBrowserAfterStart = true));
    await signIn.click();
    await expect(dialog.getByRole('alert')).toHaveText('The browser launcher failed.');
    await expect(dialog.getByRole('status')).toHaveCount(0);
    await expect(signIn).toBeEnabled();
    await signIn.click();
    await expect(dialog.getByRole('checkbox', { name: 'Work' })).toBeChecked();
    await expect(dialog.getByRole('checkbox', { name: 'Hidden remotely' })).toBeChecked();
    await expect(dialog.getByRole('checkbox', { name: 'Availability' })).toBeDisabled();
    await expect(dialog.getByText('test@example.invalid', { exact: true })).toBeVisible();
    const audit = await new AxeBuilder({ page })
      .withTags(['wcag2a', 'wcag2aa', 'wcag21aa'])
      .analyze();
    expect(
      audit.violations.filter((v) => ['serious', 'critical'].includes(v.impact ?? '')),
    ).toEqual([]);
    await dialog.getByRole('checkbox', { name: 'Hidden remotely' }).uncheck();
    await dialog.getByRole('button', { name: 'Save selection' }).click();
    await expect(dialog).not.toBeVisible();
    await expect(plus).toBeFocused();
    await expect(page.locator('.calendar-row').filter({ hasText: 'Work' })).toBeVisible();
    await page.getByRole('button', { name: /Manage test@example.invalid/ }).click();
    const management = page.getByRole('dialog', { name: 'Manage connected account' });
    await expect(management.getByRole('checkbox', { name: 'Hidden remotely' })).not.toBeChecked();
    await management.getByRole('checkbox', { name: 'Hidden remotely' }).check();
    await management.getByRole('button', { name: 'Save selection' }).click();
    await expect(management).not.toBeVisible();
    expect(await page.evaluate(() => (window as any).googleTest.selections)).toEqual([
      ['one'],
      ['one', 'two'],
    ]);
    await page.evaluate(() => {
      (window as any).googleTest.failDiscovery = true;
    });
    await page.getByRole('button', { name: /Manage test@example.invalid/ }).click();
    await expect(management.getByRole('alert')).toHaveText('Discovery temporarily unavailable.');
    await management.getByRole('button', { name: 'Retry discovery' }).click();
    await expect(management.getByRole('checkbox', { name: 'Work' })).toBeChecked();
    await management.getByRole('button', { name: 'Disconnect account', exact: true }).click();
    await expect(management).not.toBeVisible();
    await expect(
      page.getByRole('button', { name: /Manage test@example.invalid.*Reconnect/ }),
    ).toBeVisible();
    await expect(page.locator('.calendar-row').filter({ hasText: 'Work' })).toBeVisible();
  });
}

test('closing during connection startup cancels the late session and preserves Local', async ({
  page,
}) => {
  await page.addInitScript(() => {
    Object.defineProperty(window, 'isTauri', { value: true });
    const caps = {
      read: true,
      create: true,
      edit: true,
      delete: true,
      recurrence: true,
      customizeMetadata: true,
      moveEvent: true,
    };
    (window as any).googleCancelled = 0;
    Object.defineProperty(window, '__TAURI_INTERNALS__', {
      value: {
        invoke: async (command: string) => {
          switch (command) {
            case 'sync_conflicts':
              return [];
            case 'get_app_info':
              return { name: 'aika', version: 'test', provider: 'local' };
            case 'get_preferences':
              return { appearance: 'light', view: 'month', sidebar: true };
            case 'set_preferences':
              return;
            case 'list_providers':
              await new Promise((resolve) => setTimeout(resolve, 500));
              return [
                {
                  id: 'local',
                  name: 'On this device',
                  connectionAvailable: true,
                  unavailableReason: null,
                  capabilities: caps,
                },
                {
                  id: 'google',
                  name: 'Google Calendar',
                  connectionAvailable: true,
                  unavailableReason: null,
                  capabilities: caps,
                },
              ];
            case 'calendar_snapshot':
              return {
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
                events: [],
                accounts: [],
                defaultCalendarId: 'local-default',
                hiddenCalendarIds: [],
                canUndo: false,
                calendarCapabilities: { 'local-default': caps },
                eventCapabilities: {},
              };
            case 'start_connection':
              await new Promise((resolve) => setTimeout(resolve, 1000));
              return { id: 'late', progress: 'waitingForBrowser', accountId: null, failure: null };
            case 'cancel_connection':
              (window as any).googleCancelled++;
              return;
            default:
              throw new Error(`Unexpected command ${command}`);
          }
        },
      },
    });
  });
  await page.goto('/');
  const plus = page.getByRole('button', { name: 'Create calendar', exact: true });
  await plus.focus();
  await page.keyboard.press('ArrowDown');
  await expect(page.getByRole('menuitem', { name: 'On this device' })).toBeFocused();
  await page.getByRole('menuitem', { name: 'Google Calendar' }).click();
  const connection = page.getByRole('dialog', { name: 'Connect Google Calendar' });
  await connection.getByRole('button', { name: 'Sign in with Google' }).click();
  await expect(connection.getByRole('status')).toHaveText('Opening your browser…');
  await connection.getByRole('button', { name: 'Cancel', exact: true }).click();
  await expect(connection).not.toBeVisible();
  await expect.poll(() => page.evaluate(() => (window as any).googleCancelled)).toBe(1);
  await expect(plus).toBeFocused();
  await plus.click();
  await page.getByRole('menuitem', { name: 'On this device' }).click();
  await expect(page.getByLabel('Calendar name')).toBeVisible();
});
