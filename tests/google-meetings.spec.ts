import { expect, test, type Page } from '@playwright/test';
import AxeBuilder from '@axe-core/playwright';

async function fixture(page: Page, owned = true, recurring = false) {
  await page.addInitScript(
    ({ owned, recurring }) => {
      Object.defineProperty(window, 'isTauri', { value: true });
      const date = new Date();
      const key = (d: Date) =>
        `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`;
      const tomorrow = new Date(date.getFullYear(), date.getMonth(), date.getDate() + 1);
      const event = {
        id: '11111111-1111-4111-8111-111111111111',
        calendarId: 'google',
        title: 'Planning meeting',
        location: '',
        notes: '',
        time: { kind: 'allDay', startDate: key(date), endDate: key(tomorrow) },
      };
      const master = recurring
        ? {
            ...event,
            recurrence: {
              frequency: 'daily',
              interval: 1,
              weekdays: [],
              monthlyOn: 'date',
              until: null,
              count: 3,
            },
          }
        : event;
      if (recurring) {
        (event as any).occurrence = {
          seriesId: master.id,
          originalStart: event.time.startDate,
          expected: 'fixture',
        };
        event.id = `${master.id}@${event.time.startDate}`;
      }
      const caps = {
        read: true,
        create: true,
        edit: owned,
        delete: owned,
        recurrence: true,
        customizeMetadata: true,
        moveEvent: false,
      };
      const account = {
        id: 'account',
        providerId: 'google',
        remoteSubject: 'owner',
        displayName: 'owner@example.invalid',
        connectionState: 'connected',
        selectedCalendarIds: ['google'],
      };
      const state = {
        requests: [] as any[],
        event: event as any,
        responses: [] as any[],
        response: 'needsAction',
        failResponse: false,
      };
      (window as any).meetingFixture = state;
      Object.defineProperty(window, '__TAURI_INTERNALS__', {
        value: {
          invoke: async (command: string, args: any) => {
            switch (command) {
              case 'get_app_info':
                return { name: 'aika', version: 'fixture', provider: 'local' };
              case 'get_preferences':
                return { appearance: 'light', view: 'month', sidebar: true };
              case 'set_preferences':
              case 'request_sync':
                return;
              case 'list_providers':
              case 'sync_conflicts':
                return [];
              case 'calendar_snapshot':
                return {
                  calendars: [
                    {
                      id: 'google',
                      providerId: 'google',
                      accountId: 'account',
                      name: 'Meetings',
                      description: '',
                      color: '#718675',
                      writable: true,
                    },
                  ],
                  events: state.event ? [state.event] : [],
                  defaultCalendarId: 'google',
                  hiddenCalendarIds: [],
                  canUndo: state.requests.length > 0,
                  accounts: [account],
                  calendarCapabilities: { google: { ...caps, edit: true, delete: true } },
                  eventCapabilities: { [event.id]: caps },
                  syncStatus: [],
                };
              case 'get_event_series':
                return { event: recurring ? master : state.event, exceptions: [] };
              case 'meeting_details':
                return {
                  organizer: 'owner@example.invalid',
                  organizerOwned: owned,
                  response: owned
                    ? null
                    : {
                        current: state.response,
                        scopes:
                          recurring && args.target.includes('@')
                            ? ['occurrence', 'series']
                            : ['series'],
                      },
                  attendees: [
                    { email: 'guest@example.invalid', optional: true, response: 'accepted' },
                  ],
                  attendeesComplete: true,
                  conferenceLinks: ['https://meet.google.com/fixture'],
                  revision: recurring && args.target.includes('@') ? 'occurrence-r1' : 'meeting-r1',
                };
              case 'respond_to_invitation':
                if (state.failResponse)
                  throw {
                    code: 'conflict',
                    message: 'Invitation changed. Reload before responding.',
                  };
                state.responses.push(args.request);
                state.response = args.request.response;
                return;
              case 'mutate_meeting':
                state.requests.push(args.request);
                state.event = args.request.event;
                return state.event ? [state.event] : [];
              default:
                throw new Error(`Unexpected command: ${command}`);
            }
          },
        },
      });
    },
    { owned, recurring },
  );
  await page.goto('/');
  await page
    .getByRole('button', { name: /Planning meeting/ })
    .first()
    .click();
  await expect(page.getByText('guest@example.invalid (optional): accepted')).toBeVisible();
}

test('organizer guest changes and silent policy require consent and use checked native intent', async ({
  page,
}) => {
  await fixture(page);
  const editor = page.getByRole('dialog', { name: 'Event editor' });
  await expect(editor.getByLabel('Notify guests')).toHaveValue('all');
  await expect(editor.getByRole('link', { name: 'Join meeting' })).toHaveAttribute(
    'href',
    'https://meet.google.com/fixture',
  );
  await editor
    .getByLabel('Meeting guests')
    .fill('guest@example.invalid optional\nnew@example.invalid');
  await editor.getByLabel('Notify guests').selectOption('none');
  await editor.getByRole('button', { name: 'Save changes', exact: true }).click();
  await expect(editor.getByText('Acknowledge the silent update warning.')).toBeVisible();
  expect(await page.evaluate(() => (window as any).meetingFixture.requests.length)).toBe(0);
  await editor.getByRole('checkbox', { name: /I understand silent updates/ }).check();
  const accessibility = await new AxeBuilder({ page }).include('.editor').analyze();
  expect(accessibility.violations).toEqual([]);
  await editor.getByRole('button', { name: 'Save changes', exact: true }).click();
  await expect
    .poll(() => page.evaluate(() => (window as any).meetingFixture.requests.length))
    .toBe(1);
  const request = await page.evaluate(() => (window as any).meetingFixture.requests[0]);
  expect(request).toMatchObject({
    notifications: 'none',
    acknowledgeSilent: true,
    expectedRevision: 'meeting-r1',
    scope: 'series',
    guests: [
      { email: 'guest@example.invalid', optional: true },
      { email: 'new@example.invalid', optional: false },
    ],
  });
  expect(request.expected.event.title).toBe('Planning meeting');
});

test('cancellation persists external notification choice and explains delivered invitations', async ({
  page,
}) => {
  await fixture(page);
  const editor = page.getByRole('dialog', { name: 'Event editor' });
  await expect(editor).toContainText('Delivered invitations cannot be recalled');
  await editor.getByLabel('Notify guests').selectOption('externalOnly');
  await editor.getByRole('button', { name: 'Delete event', exact: true }).click();
  await expect
    .poll(() => page.evaluate(() => (window as any).meetingFixture.requests.length))
    .toBe(1);
  expect(await page.evaluate(() => (window as any).meetingFixture.requests[0])).toMatchObject({
    event: null,
    notifications: 'externalOnly',
    guests: null,
  });
});

test('invitee sees responses and conference links while organizer fields remain read-only', async ({
  page,
}) => {
  await fixture(page, false);
  const editor = page.getByRole('dialog', { name: 'Event editor' });
  await expect(editor.getByRole('link', { name: 'Join meeting' })).toBeVisible();
  await expect(editor.getByLabel('Meeting guests')).toHaveCount(0);
  await expect(editor.getByLabel('Notify guests')).toHaveCount(0);
  await expect(editor.getByRole('button', { name: 'Save changes', exact: true })).toBeDisabled();
  await expect(editor.getByRole('button', { name: 'Delete event', exact: true })).toBeDisabled();
});

for (const [label, response] of [
  ['Accept', 'accepted'],
  ['Tentative', 'tentative'],
  ['Decline', 'declined'],
] as const) {
  test(`invitee can ${label.toLowerCase()} through checked participant-only intent`, async ({
    page,
  }) => {
    await fixture(page, false);
    const editor = page.getByRole('dialog', { name: 'Event editor' });
    await expect(editor.getByLabel('Invitation response scope')).toHaveValue('series');
    await expect(editor.getByLabel('Invitation response scope').getByRole('option')).toHaveCount(1);
    await expect(editor.getByText('Your response: needsAction')).toBeVisible();
    await expect(editor.getByRole('button', { name: 'Save changes', exact: true })).toBeDisabled();
    expect((await new AxeBuilder({ page }).include('.editor').analyze()).violations).toEqual([]);
    await editor.getByRole('button', { name: label, exact: true }).click();
    await expect(editor).not.toBeVisible();
    const requests = await page.evaluate(() => (window as any).meetingFixture.responses);
    expect(requests).toEqual([
      {
        target: '11111111-1111-4111-8111-111111111111',
        scope: 'series',
        expectedRevision: 'meeting-r1',
        response,
      },
    ]);
    expect(await page.evaluate(() => (window as any).meetingFixture.requests)).toEqual([]);
  });
}

test('stale invitation response stays open with a conflict message', async ({ page }) => {
  await fixture(page, false);
  await page.evaluate(() => {
    (window as any).meetingFixture.failResponse = true;
  });
  const editor = page.getByRole('dialog', { name: 'Event editor' });
  await editor.getByRole('button', { name: 'Accept', exact: true }).click();
  await expect(editor.getByText('Invitation changed. Reload before responding.')).toBeVisible();
  await expect(editor.getByRole('button', { name: 'Accept', exact: true })).toBeEnabled();
});

test('recurring invitee scopes review the occurrence and master separately without following RSVP', async ({
  page,
}) => {
  await fixture(page, false, true);
  const editor = page.getByRole('dialog', { name: 'Event editor' });
  await expect(editor.getByLabel('Invitation response scope')).toHaveValue('occurrence');
  await expect(editor.getByLabel('Invitation response scope').getByRole('option')).toHaveCount(2);
  await editor.getByLabel('Invitation response scope').selectOption('series');
  await expect(editor.getByRole('button', { name: 'Accept', exact: true })).toBeEnabled();
  await editor.getByRole('button', { name: 'Accept', exact: true }).click();
  await expect
    .poll(() => page.evaluate(() => (window as any).meetingFixture.responses.length))
    .toBe(1);
  expect(await page.evaluate(() => (window as any).meetingFixture.responses[0])).toMatchObject({
    target: '11111111-1111-4111-8111-111111111111',
    scope: 'series',
    expectedRevision: 'meeting-r1',
  });
});
