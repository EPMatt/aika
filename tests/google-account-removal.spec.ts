import { expect, test } from '@playwright/test';
import AxeBuilder from '@axe-core/playwright';

test('full account removal reviews excluded data, checks consent and stale reviews, and retries cleanup', async ({
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
      customizeMetadata: false,
      moveEvent: false,
    };
    const account = {
      id: 'account',
      providerId: 'google',
      remoteSubject: 'fixture',
      displayName: 'remove@example.invalid',
      connectionState: 'disconnected',
      selectedCalendarIds: ['imported'],
    };
    const fixture = { revision: 'first', removing: false, removed: false, requests: 0 };
    (window as any).removalFixture = fixture;
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
                calendars: fixture.removed
                  ? []
                  : [
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
                accounts: fixture.removed ? [] : [account],
                calendarCapabilities: { imported: caps },
                eventCapabilities: {},
                syncStatus: [],
              };
            case 'review_account_removal':
              return {
                accountId: account.id,
                selectedCalendarCount: 1,
                excludedCalendarCount: 2,
                cachedEventCount: 5,
                pendingCount: 2,
                conflictCount: 1,
                attemptedCount: 1,
                removalInProgress: fixture.removing,
                revision: fixture.revision,
              };
            case 'remove_account':
              if (args.expected.revision !== fixture.revision)
                throw { code: 'conflict', message: 'Account data changed. Review removal again.' };
              if (!args.discard) throw new Error('Missing discard consent');
              fixture.requests++;
              if (!fixture.removing) {
                fixture.removing = true;
                fixture.revision = 'cleanup';
                throw { code: 'credentialUnavailable', message: 'Unlock the credential store.' };
              }
              fixture.removed = true;
              return;
            default:
              throw new Error(`Unexpected command ${command}`);
          }
        },
      },
    });
  });
  await page.goto('/');
  await page.getByRole('button', { name: /Manage remove@example.invalid/ }).click();
  const dialog = page.getByRole('dialog', { name: 'Manage connected account' });
  await dialog.getByRole('button', { name: 'Remove account from aika', exact: true }).click();
  const review = dialog.getByRole('region', { name: 'Review account removal' });
  await expect(review).toContainText('1 selected and 2 excluded calendars and 5 cached events');
  await expect(review).toContainText('Changes already sent to Google may still take effect.');
  await expect(review).toContainText(
    'Google calendars and the Google authorization grant remain intact.',
  );
  const remove = review.getByRole('button', { name: 'Remove account permanently', exact: true });
  await expect(remove).toBeDisabled();
  await review.getByRole('checkbox', { name: 'Permanently remove this account from aika' }).check();
  await expect(remove).toBeDisabled();
  await review.getByRole('checkbox', { name: 'Discard these local changes' }).check();
  const audit = await new AxeBuilder({ page })
    .withTags(['wcag2a', 'wcag2aa', 'wcag21aa'])
    .analyze();
  expect(audit.violations.filter((v) => ['critical', 'serious'].includes(v.impact ?? ''))).toEqual(
    [],
  );
  await page.evaluate(() => {
    (window as any).removalFixture.revision = 'newer';
  });
  await remove.click();
  await expect(dialog.getByRole('alert')).toHaveText('Account data changed. Review removal again.');
  expect(await page.evaluate(() => (window as any).removalFixture.requests)).toBe(0);
  await expect(remove).toBeDisabled();
  await review.getByRole('checkbox', { name: 'Permanently remove this account from aika' }).check();
  await review.getByRole('checkbox', { name: 'Discard these local changes' }).check();
  await remove.click();
  await expect(dialog.getByRole('alert')).toHaveText('Unlock the credential store.');
  await expect(review).toContainText('Cleanup is in progress.');
  const retry = review.getByRole('button', { name: 'Retry account removal', exact: true });
  await expect(retry).toBeDisabled();
  await review.getByRole('checkbox', { name: 'Permanently remove this account from aika' }).check();
  await review.getByRole('checkbox', { name: 'Discard these local changes' }).check();
  await retry.click();
  await expect(dialog).not.toBeVisible();
  await expect(page.getByRole('button', { name: /Manage remove@example.invalid/ })).toHaveCount(0);
  await expect(page.getByText('Cached calendar', { exact: true })).toHaveCount(0);
  expect(await page.evaluate(() => (window as any).removalFixture.requests)).toBe(2);
});
