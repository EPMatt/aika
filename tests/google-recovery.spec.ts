import { expect, test } from '@playwright/test';
import AxeBuilder from '@axe-core/playwright';

test('account recovery requires review and explicit discard, rejects stale intent, and retries credential cleanup', async ({
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
      displayName: 'recovery@example.invalid',
      connectionState: 'authenticationRequired',
      selectedCalendarIds: ['imported'],
    };
    const fixture = {
      revision: 'first',
      pending: 2,
      conflicts: 1,
      discards: 0,
      cleanups: 0,
      failCleanup: true,
    };
    (window as any).recoveryFixture = fixture;
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
            case 'sync_conflicts':
              return [];
            case 'list_providers':
              return [
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
                    id: 'imported',
                    providerId: 'google',
                    name: 'Cached calendar',
                    description: '',
                    color: '#718675',
                    writable: false,
                  },
                ],
                events: [],
                defaultCalendarId: null,
                hiddenCalendarIds: [],
                canUndo: false,
                accounts: [account],
                calendarCapabilities: { imported: caps },
                eventCapabilities: {},
                syncStatus: [],
              };
            case 'review_disconnect':
              return {
                accountId: 'account',
                pendingCount: fixture.pending,
                conflictCount: fixture.conflicts,
                attemptedCount: fixture.pending ? 1 : 0,
                revision: fixture.revision,
              };
            case 'discard_and_disconnect':
              if (args.expected.revision !== fixture.revision)
                throw {
                  code: 'conflict',
                  message: 'Pending changes changed. Review the disconnect again.',
                };
              fixture.discards++;
              fixture.pending = 0;
              fixture.conflicts = 0;
              account.connectionState = 'disconnected';
              if (fixture.failCleanup) {
                fixture.failCleanup = false;
                throw {
                  code: 'credentialUnavailable',
                  message: 'Unlock the credential store and retry disconnect.',
                };
              }
              return;
            case 'disconnect_account':
              fixture.cleanups++;
              return;
            default:
              throw new Error(`Unexpected command ${command}`);
          }
        },
      },
    });
  });
  await page.goto('/');
  await page.getByRole('button', { name: /Manage recovery@example.invalid/ }).click();
  const dialog = page.getByRole('dialog', { name: 'Manage connected account' });
  const disconnect = dialog.getByRole('button', { name: 'Disconnect account', exact: true });
  await disconnect.click();
  const review = dialog.getByRole('region', { name: 'Review pending changes' });
  await expect(review).toContainText('2 pending changes and 1 conflict');
  await expect(review).toContainText('Changes already sent to Google may still take effect.');
  const discard = review.getByRole('button', { name: 'Discard and disconnect', exact: true });
  await expect(discard).toBeDisabled();
  await review.getByRole('button', { name: 'Keep changes', exact: true }).click();
  expect(await page.evaluate(() => (window as any).recoveryFixture.discards)).toBe(0);
  await disconnect.click();
  await review.getByRole('checkbox', { name: 'Discard these local changes' }).check();
  const audit = await new AxeBuilder({ page })
    .withTags(['wcag2a', 'wcag2aa', 'wcag21aa'])
    .analyze();
  expect(audit.violations.filter((v) => ['critical', 'serious'].includes(v.impact ?? ''))).toEqual(
    [],
  );
  await page.evaluate(() => {
    (window as any).recoveryFixture.revision = 'newer';
  });
  await discard.click();
  await expect(dialog.getByRole('alert')).toHaveText(
    'Pending changes changed. Review the disconnect again.',
  );
  expect(await page.evaluate(() => (window as any).recoveryFixture.discards)).toBe(0);
  await disconnect.click();
  await expect(discard).toBeDisabled();
  await review.getByRole('checkbox', { name: 'Discard these local changes' }).check();
  await discard.click();
  await expect(dialog.getByRole('alert')).toHaveText(
    'Unlock the credential store and retry disconnect.',
  );
  expect(await page.evaluate(() => (window as any).recoveryFixture.discards)).toBe(1);
  await disconnect.click();
  await expect(dialog).not.toBeVisible();
  expect(await page.evaluate(() => (window as any).recoveryFixture.cleanups)).toBe(1);
  await expect(page.getByText('Cached calendar', { exact: true })).toBeVisible();
});
