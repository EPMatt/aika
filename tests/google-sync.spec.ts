import { expect, test } from '@playwright/test';
import AxeBuilder from '@axe-core/playwright';

test('cached Google events stay read-only and incomplete/offline coverage has recovery controls', async ({
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
    const account = {
      id: 'account',
      providerId: 'google',
      remoteSubject: 'fixture',
      displayName: 'fixture@example.invalid',
      connectionState: 'connected',
      selectedCalendarIds: ['google-calendar'],
    };
    const fixture = { requests: 0, complete: false, snapshots: 0 };
    (window as any).syncFixture = fixture;
    Object.defineProperty(window, '__TAURI_INTERNALS__', {
      value: {
        invoke: async (command: string) => {
          switch (command) {
            case 'get_app_info':
              return { name: 'aika', version: 'fixture', provider: 'local' };
            case 'get_preferences':
              return { appearance: 'light', view: 'month', sidebar: true };
            case 'set_preferences':
              return;
            case 'request_sync':
              fixture.requests++;
              return;
            case 'list_providers':
              return [];
            case 'calendar_snapshot': {
              fixture.snapshots++;
              const now = new Date();
              const date = `${now.getFullYear()}-${String(now.getMonth() + 1).padStart(2, '0')}-${String(now.getDate()).padStart(2, '0')}`;
              const tomorrow = new Date(now.getFullYear(), now.getMonth(), now.getDate() + 1);
              const end = `${tomorrow.getFullYear()}-${String(tomorrow.getMonth() + 1).padStart(2, '0')}-${String(tomorrow.getDate()).padStart(2, '0')}`;
              return {
                calendars: [
                  {
                    id: 'google-calendar',
                    providerId: 'google',
                    name: 'Imported',
                    description: '',
                    color: '#718675',
                    writable: false,
                  },
                ],
                events: [
                  {
                    id: '9eed7866-4246-4a81-9281-9c5d94c174df',
                    calendarId: 'google-calendar',
                    title: 'Cached remote event',
                    time: { kind: 'allDay', startDate: date, endDate: end },
                    notes: '',
                    location: '',
                  },
                  {
                    id: 'bcb0c222-9ef8-45bc-bf9c-12e0d1c43714',
                    calendarId: 'google-calendar',
                    title: '',
                    time: { kind: 'allDay', startDate: date, endDate: end },
                    notes: '',
                    location: '',
                  },
                ],
                defaultCalendarId: null,
                hiddenCalendarIds: [],
                canUndo: false,
                accounts: [account],
                calendarCapabilities: { 'google-calendar': caps },
                eventCapabilities: {},
                syncStatus: [
                  {
                    accountId: account.id,
                    syncing: false,
                    lastSuccess: '2026-10-13T12:00:00Z',
                    failure: fixture.complete
                      ? null
                      : { code: 'network', message: 'Could not reach Google.', offsets: [] },
                    pendingCount: 0,
                    conflictCount: 0,
                    calendars: [
                      {
                        calendarId: 'google-calendar',
                        available: true,
                        rangeComplete: fixture.complete,
                        lastSuccess: '2026-10-13T12:00:00Z',
                      },
                    ],
                  },
                ],
              };
            }
            default:
              throw new Error(`Unexpected fixture command: ${command}`);
          }
        },
      },
    });
  });
  await page.goto('/');
  await expect(page.getByText('Cached remote event').first()).toBeVisible();
  await expect(page.getByText('Untitled event').first()).toBeVisible();
  await expect(
    page.getByText('Could not reach Google. Cached events remain available.'),
  ).toBeVisible();
  await expect(
    page.getByText('Some events in this range have not been cached.', { exact: false }),
  ).toBeVisible();
  await expect.poll(() => page.evaluate(() => (window as any).syncFixture.requests)).toBe(1);
  await page.getByRole('button', { name: 'Refresh calendars', exact: true }).click();
  await expect.poll(() => page.evaluate(() => (window as any).syncFixture.requests)).toBe(2);
  const audit = await new AxeBuilder({ page })
    .withTags(['wcag2a', 'wcag2aa', 'wcag21aa'])
    .analyze();
  expect(audit.violations.filter((v) => ['critical', 'serious'].includes(v.impact ?? ''))).toEqual(
    [],
  );
  await page.getByText('Cached remote event').first().click();
  await expect(page.getByLabel('Event title', { exact: true })).toBeDisabled();
  await expect(page.getByRole('button', { name: 'Save event', exact: true })).toHaveCount(0);
  await page.keyboard.press('Escape');
  await page.evaluate(() => {
    (window as any).syncFixture.complete = true;
    window.dispatchEvent(new Event('focus'));
  });
  await expect(
    page.getByText('Could not reach Google. Cached events remain available.'),
  ).toHaveCount(0);
  await expect(
    page.getByText('Some events in this range have not been cached.', { exact: false }),
  ).toHaveCount(0);
  await expect(page.getByText('Cached remote event').first()).toBeVisible();
});
