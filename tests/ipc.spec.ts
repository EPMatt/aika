import { expect, test } from '@playwright/test';

// Renderer contract test; actual SQLite/restart behavior is tested by the Rust service suite.
test('desktop IPC errors preserve input and saved preferences hydrate on reload', async ({
  page,
}) => {
  await page.addInitScript(() => {
    Object.defineProperty(window, 'isTauri', { value: true });
    let events = JSON.parse(localStorage.getItem('test.events') ?? '[]');
    let history: unknown[][] = [];
    let failSave = true;
    Object.defineProperty(window, '__TAURI_INTERNALS__', {
      value: {
        invoke: async (command: string, args: Record<string, any>) => {
          switch (command) {
            case 'get_app_info':
              return { name: 'aika', version: 'test', provider: 'local' };
            case 'get_preferences':
              return JSON.parse(
                localStorage.getItem('test.preferences') ??
                  '{"appearance":"dark","view":"week","sidebar":false}',
              );
            case 'set_preferences':
              localStorage.setItem('test.preferences', JSON.stringify(args.preferences));
              return;
            case 'calendar_snapshot':
              return {
                calendar: {
                  id: 'local-default',
                  providerId: 'local',
                  name: 'Personal',
                  color: '#718675',
                  writable: true,
                },
                events,
                canUndo: history.length > 0,
              };
            case 'resolve_local_time':
              return new Date(args.local + '+02:00').toISOString();
            case 'save_event':
              if (failSave) {
                failSave = false;
                throw {
                  code: 'storage',
                  message: 'Database is temporarily unavailable.',
                  offsets: [],
                };
              }
              history.push(structuredClone(events));
              events = [...events.filter((e: any) => e.id !== args.event.id), args.event];
              localStorage.setItem('test.events', JSON.stringify(events));
              return args.event;
            case 'delete_event':
              history.push(structuredClone(events));
              events = events.filter((e: any) => e.id !== args.id);
              localStorage.setItem('test.events', JSON.stringify(events));
              return;
            case 'undo_event':
              events = history.pop() ?? events;
              localStorage.setItem('test.events', JSON.stringify(events));
              return;
            default:
              throw new Error(`Unexpected command: ${command}`);
          }
        },
      },
    });
  });
  await page.goto('/?scenario=dense&theme=light&date=2000-01-01');
  await expect(page.getByLabel('Week calendar')).toBeVisible();
  await expect(page.locator('html')).toHaveAttribute('data-theme', 'dark');
  await expect(page.getByRole('button', { name: 'Show sidebar' })).toBeVisible();
  await expect(page.locator('.time-event')).toHaveCount(0);
  await page.getByRole('button', { name: 'New event' }).click();
  await page.getByRole('textbox', { name: 'Event title' }).fill('Persistent event');
  await page.getByRole('button', { name: 'Create event', exact: true }).click();
  await expect(page.getByRole('alert')).toContainText('temporarily unavailable');
  await expect(page.getByRole('textbox', { name: 'Event title' })).toHaveValue('Persistent event');
  await page.getByRole('button', { name: 'Create event', exact: true }).click();
  await expect(page.getByRole('button', { name: /Persistent event,/ })).toBeVisible();
  await page.getByRole('button', { name: 'Month', exact: true }).click();
  await expect
    .poll(() => page.evaluate(() => JSON.parse(localStorage.getItem('test.preferences')!).view))
    .toBe('month');
  await page.reload();
  await expect(page.getByLabel('Month calendar')).toBeVisible();
  await expect(page.getByRole('button', { name: /Persistent event,/ })).toBeVisible();
  await expect(page.getByRole('button', { name: 'Undo', exact: true })).toHaveCount(0);
});
