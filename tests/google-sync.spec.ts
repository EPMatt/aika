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
            case 'sync_conflicts':
              return [];
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
                eventActions: {
                  '9eed7866-4246-4a81-9281-9c5d94c174df': {
                    edit: [
                      { scope: 'series', disabledReason: 'This calendar or event is read-only.' },
                    ],
                    delete: [
                      { scope: 'series', disabledReason: 'This calendar or event is read-only.' },
                    ],
                  },
                },
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
  await expect(
    page.getByRole('status').filter({ hasText: 'This calendar or event is read-only.' }),
  ).toBeVisible();
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

test('writable ordinary Google events queue offline edits and offer checked conflict choices', async ({
  page,
}) => {
  await page.addInitScript(() => {
    Object.defineProperty(window, 'isTauri', { value: true });
    const caps = {
      read: true,
      create: true,
      edit: true,
      delete: true,
      recurrence: false,
      customizeMetadata: true,
      moveEvent: false,
    };
    const day = new Date();
    const date = (d: Date) =>
      `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`;
    const end = new Date(day.getFullYear(), day.getMonth(), day.getDate() + 1);
    const original = {
      id: '9eed7866-4246-4a81-9281-9c5d94c174df',
      calendarId: 'imported',
      title: 'Editable Google event',
      location: 'Room',
      notes: 'Local notes',
      time: { kind: 'allDay', startDate: date(day), endDate: date(end) },
      recurrence: null,
      occurrence: null,
    };
    const fixture = {
      event: original,
      pending: 0,
      requests: 0,
      conflicts: [] as any[],
      resolutions: [] as boolean[],
      retries: 0,
    };
    (window as any).outgoingFixture = fixture;
    Object.defineProperty(window, '__TAURI_INTERNALS__', {
      value: {
        invoke: async (command: string, args: any) => {
          switch (command) {
            case 'get_app_info':
              return { name: 'aika', version: 'fixture', provider: 'local' };
            case 'get_preferences':
              return { appearance: 'light', view: 'month', sidebar: true };
            case 'set_preferences':
              return;
            case 'list_providers':
              return [];
            case 'request_sync':
              fixture.requests++;
              return;
            case 'sync_conflicts':
              return structuredClone(fixture.conflicts);
            case 'calendar_snapshot':
              return {
                calendars: [
                  {
                    id: 'imported',
                    providerId: 'google',
                    name: 'Imported',
                    color: '#718675',
                    description: '',
                    writable: true,
                  },
                ],
                events: [fixture.event],
                defaultCalendarId: 'imported',
                hiddenCalendarIds: [],
                canUndo: false,
                accounts: [
                  {
                    id: 'account',
                    providerId: 'google',
                    remoteSubject: 'fixture',
                    displayName: 'fixture@example.invalid',
                    connectionState: 'connected',
                    selectedCalendarIds: ['imported'],
                  },
                ],
                calendarCapabilities: { imported: caps },
                eventCapabilities: { [original.id]: caps },
                syncStatus: [
                  {
                    accountId: 'account',
                    syncing: false,
                    lastSuccess: null,
                    failure: {
                      code: 'network',
                      message: 'Offline. Changes remain queued.',
                      offsets: [],
                    },
                    pendingCount: fixture.pending,
                    conflictCount: fixture.conflicts.length,
                    calendars: [
                      {
                        calendarId: 'imported',
                        available: true,
                        rangeComplete: true,
                        lastSuccess: null,
                      },
                    ],
                  },
                ],
              };
            case 'get_event_series':
              return { event: fixture.event, exceptions: [] };
            case 'save_event':
              fixture.event = structuredClone(args.event);
              fixture.pending++;
              return fixture.event;
            case 'resolve_sync_conflict':
              if (args.expected.id !== fixture.conflicts[0]?.id)
                throw { message: 'Conflict changed. Review again.' };
              fixture.resolutions.push(args.keepLocal);
              if (!args.keepLocal) fixture.event = fixture.conflicts[0].remote.event;
              fixture.conflicts = [];
              fixture.pending = args.keepLocal ? 1 : 0;
              return;
            case 'retry_outgoing':
              fixture.retries++;
              return;
            default:
              throw new Error(`Unexpected fixture command: ${command}`);
          }
        },
      },
    });
  });
  await page.goto('/');
  await page.getByText('Editable Google event', { exact: true }).first().click();
  await expect(page.getByLabel('Event title', { exact: true })).toBeEnabled();
  await page.getByLabel('Event title', { exact: true }).fill('Offline local edit');
  const before = await page.evaluate(() => (window as any).outgoingFixture.requests);
  await page.getByRole('button', { name: 'Save changes', exact: true }).click();
  await expect(page.getByText('1 pending changes', { exact: true })).toBeVisible();
  await expect
    .poll(() => page.evaluate(() => (window as any).outgoingFixture.requests))
    .toBeGreaterThan(before);
  const showConflict = async () => {
    await page.evaluate(() => {
      const fixture = (window as any).outgoingFixture;
      fixture.conflicts = [
        {
          id: 'conflict',
          accountId: 'account',
          calendarId: 'imported',
          eventId: fixture.event.id,
          local: { event: fixture.event, exceptions: [] },
          remote: {
            event: { ...fixture.event, title: 'Google version', notes: 'Remote notes' },
            exceptions: [],
          },
          remoteRevision: 'remote-2',
        },
      ];
      window.dispatchEvent(new Event('focus'));
    });
    await expect(page.getByRole('region', { name: 'Event conflict' })).toBeVisible();
  };
  await showConflict();
  await page.getByText('Compare versions', { exact: true }).click();
  await expect(page.getByText('Remote notes', { exact: true })).toBeVisible();
  const audit = await new AxeBuilder({ page })
    .withTags(['wcag2a', 'wcag2aa', 'wcag21aa'])
    .analyze();
  expect(audit.violations.filter((v) => ['critical', 'serious'].includes(v.impact ?? ''))).toEqual(
    [],
  );
  await page.getByRole('button', { name: 'Keep local', exact: true }).click();
  await expect(page.getByRole('button', { name: 'Keep local', exact: true })).toHaveCount(0);
  await expect(page.getByText('Offline local edit', { exact: true }).first()).toBeVisible();
  await showConflict();
  await page.getByRole('button', { name: 'Keep Google', exact: true }).click();
  await expect(page.getByText('Google version', { exact: true }).first()).toBeVisible();
  expect(await page.evaluate(() => (window as any).outgoingFixture.resolutions)).toEqual([
    true,
    false,
  ]);
});

test('Google recurring controls preserve exceptions and explain unsupported scopes', async ({
  page,
}) => {
  await page.addInitScript(() => {
    Object.defineProperty(window, 'isTauri', { value: true });
    const id = '9eed7866-4246-4a81-9281-9c5d94c174df';
    const now = new Date();
    const day = (offset: number) => {
      const d = new Date(now.getFullYear(), now.getMonth(), now.getDate() + offset);
      return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`;
    };
    const master = {
      id,
      calendarId: 'imported',
      title: 'Google repeat',
      location: '',
      notes: '',
      time: { kind: 'allDay', startDate: day(0), endDate: day(1) },
      recurrence: {
        frequency: 'daily',
        interval: 1,
        weekdays: [],
        monthlyOn: 'date',
        count: 5,
        until: null,
      },
    };
    const occurrence = {
      ...master,
      id: `${id}@${day(0)}`,
      occurrence: { seriesId: id, originalStart: day(0), expected: 'checked-series' },
    };
    const caps = {
      read: true,
      create: true,
      edit: true,
      delete: true,
      recurrence: true,
      customizeMetadata: true,
      moveEvent: false,
    };
    const actions = ['occurrence', 'following', 'series'].map((scope) => ({
      scope,
      disabledReason: null,
    }));
    const fixture = { requests: [] as any[], event: occurrence, conflicts: [] as any[] };
    (window as any).recurringGoogleFixture = fixture;
    Object.defineProperty(window, '__TAURI_INTERNALS__', {
      value: {
        invoke: async (command: string, args: any) => {
          switch (command) {
            case 'get_app_info':
              return { name: 'aika', version: 'fixture', provider: 'local' };
            case 'get_preferences':
              return { appearance: 'light', view: 'month', sidebar: true };
            case 'list_providers':
              return [];
            case 'sync_conflicts':
              return structuredClone(fixture.conflicts);
            case 'set_preferences':
            case 'request_sync':
              return;
            case 'calendar_snapshot':
              return {
                calendars: [
                  {
                    id: 'imported',
                    providerId: 'google',
                    name: 'Imported',
                    color: '#718675',
                    description: '',
                    writable: true,
                  },
                ],
                events: [fixture.event],
                defaultCalendarId: 'imported',
                hiddenCalendarIds: [],
                canUndo: false,
                accounts: [
                  {
                    id: 'account',
                    providerId: 'google',
                    remoteSubject: 'fixture',
                    displayName: 'fixture@example.invalid',
                    connectionState: 'connected',
                    selectedCalendarIds: ['imported'],
                  },
                ],
                syncStatus: [
                  {
                    accountId: 'account',
                    syncing: false,
                    lastSuccess: null,
                    failure: null,
                    pendingCount: 0,
                    conflictCount: fixture.conflicts.length,
                    calendars: [],
                  },
                ],
                calendarCapabilities: { imported: caps },
                eventCapabilities: { [id]: caps, [occurrence.id]: caps },
                eventActions: {
                  [id]: { edit: actions, delete: actions },
                  [occurrence.id]: { edit: actions, delete: actions },
                },
              };
            case 'get_event_series':
              return { event: master, exceptions: [{ originalStart: day(1), event: null }] };
            case 'preview_recurrence':
              return [];
            case 'mutate_recurring_event':
              fixture.requests.push(args.request);
              fixture.event = {
                ...fixture.event,
                title: args.request.event?.title ?? fixture.event.title,
              };
              return [args.request.event];
            default:
              throw new Error(`Unexpected command: ${command}`);
          }
        },
      },
    });
  });
  await page.goto('/');
  await page
    .getByRole('button', { name: /Google repeat, recurring/ })
    .first()
    .click();
  await expect(page.getByLabel('Event change scope')).toHaveValue('occurrence');
  await expect(page.getByLabel('Repeat pattern')).toBeDisabled();
  await expect(
    page.getByLabel('Event change scope').locator('option[value="following"]'),
  ).toHaveJSProperty('disabled', false);
  await page.getByLabel('Event change scope').selectOption('following');
  await expect(page.getByLabel('Reset individual changes')).toBeVisible();
  await page.getByLabel('Event change scope').selectOption('series');
  await expect(page.getByLabel('All-day event')).toBeDisabled();
  await expect(page.locator('#event-start-date')).toBeDisabled();
  await expect(
    page.getByText('Google series edits preserve individual changes.', { exact: false }),
  ).toBeVisible();
  await expect(page.getByLabel('Reset individual changes', { exact: true })).toHaveCount(0);
  await page.getByLabel('Event title', { exact: true }).fill('Preserved repeat');
  const axe = await new AxeBuilder({ page }).withTags(['wcag2a', 'wcag2aa', 'wcag21aa']).analyze();
  expect(axe.violations).toEqual([]);
  await page.getByRole('button', { name: 'Save changes', exact: true }).click();
  await expect
    .poll(() => page.evaluate(() => (window as any).recurringGoogleFixture.requests.length))
    .toBe(1);
  expect(
    await page.evaluate(() => (window as any).recurringGoogleFixture.requests[0]),
  ).toMatchObject({
    scope: 'series',
    exceptionPolicy: 'preserve',
    resetExceptions: false,
    expected: 'checked-series',
  });
  await page
    .getByRole('button', { name: /Preserved repeat, recurring/ })
    .first()
    .click();
  await page.getByLabel('Event change scope').selectOption('following');
  await page.getByRole('button', { name: 'Save changes', exact: true }).click();
  await expect(page.getByRole('alert')).toContainText('Confirm');
  await page.getByLabel('Reset individual changes').check();
  await page.getByRole('button', { name: 'Save changes', exact: true }).click();
  await expect
    .poll(() => page.evaluate(() => (window as any).recurringGoogleFixture.requests.length))
    .toBe(2);
  expect(
    await page.evaluate(() => (window as any).recurringGoogleFixture.requests[1]),
  ).toMatchObject({
    scope: 'following',
    resetExceptions: true,
    exceptionPolicy: 'reset',
    expected: 'checked-series',
  });
  await page
    .getByRole('button', { name: /Preserved repeat, recurring/ })
    .first()
    .click();
  await page.getByLabel('Event change scope').selectOption('following');
  await page.getByRole('button', { name: 'Delete event', exact: true }).click();
  await expect(page.getByRole('alert')).toContainText('Confirm reset');
  await page.getByLabel('Reset individual changes').check();
  await page.getByRole('button', { name: 'Delete event', exact: true }).click();
  await expect
    .poll(() => page.evaluate(() => (window as any).recurringGoogleFixture.requests.length))
    .toBe(3);
  expect(
    await page.evaluate(() => (window as any).recurringGoogleFixture.requests[2]),
  ).toMatchObject({
    scope: 'following',
    event: null,
    resetExceptions: true,
    exceptionPolicy: 'reset',
  });
  await page.evaluate(() => {
    const fixture = (window as any).recurringGoogleFixture;
    fixture.conflicts = [
      {
        id: 'recurring-conflict',
        accountId: 'account',
        calendarId: 'imported',
        eventId: fixture.event.occurrence.seriesId,
        local: { event: fixture.event, exceptions: [{ originalStart: '2026-10-20', event: null }] },
        remote: {
          event: fixture.event,
          exceptions: [
            {
              originalStart: '2026-10-20',
              event: { ...fixture.event, title: 'External individual edit' },
            },
          ],
        },
        remoteRevision: 'checked',
        groupRevision: 'complete-review',
        recoverySteps: [
          { title: 'Original repeat', operation: 'update', state: 'acknowledged' },
          { title: 'Following tail', operation: 'create', state: 'blocked' },
        ],
      },
    ];
    window.dispatchEvent(new Event('focus'));
  });
  await expect(page.getByLabel('Grouped action progress')).toContainText(
    'Original repeat: update — acknowledged',
  );
  await expect(page.getByLabel('Grouped action progress')).toContainText(
    'Following tail: create — blocked',
  );
  await page.getByText('Compare versions', { exact: true }).click();
  await expect(page.getByLabel('Local individual changes')).toContainText('Cancelled occurrence');
  await expect(page.getByLabel('Google individual changes')).toContainText(
    'External individual edit',
  );
  expect(
    (await new AxeBuilder({ page }).withTags(['wcag2a', 'wcag2aa', 'wcag21aa']).analyze())
      .violations,
  ).toEqual([]);
});
