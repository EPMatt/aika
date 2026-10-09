import { expect, test } from '@playwright/test';

test('drag creates and resizes a timed event; precise drops and keyboard editing undo', async ({
  page,
}) => {
  await page.goto('/?date=2026-10-09&scenario=empty');
  await page.getByRole('button', { name: 'Day', exact: true }).click();
  const column = page.locator('.time-column');
  const rect = await column.boundingBox();
  if (!rect) throw new Error('Missing column');
  const x = rect.x + rect.width / 2;
  await page.mouse.move(x, rect.y + 9 * 72);
  await page.mouse.down();
  await page.mouse.move(x, rect.y + 10.5 * 72, { steps: 8 });
  await page.mouse.up();
  await expect(page.getByLabel('Start time', { exact: true })).toHaveValue('09:00');
  await expect(page.getByLabel('End time', { exact: true })).toHaveValue('10:30');
  await page.getByRole('textbox', { name: 'Event title' }).fill('Gesture event');
  await page.getByRole('button', { name: 'Create event', exact: true }).click();
  const handle = page.locator('[data-resize]');
  const box = await handle.boundingBox();
  if (!box) throw new Error('Missing handle');
  await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
  await page.mouse.down();
  await page.mouse.move(x, rect.y + 11 * 72, { steps: 8 });
  await page.mouse.up();
  await expect(page.locator('.time-event')).toHaveCSS('height', '140px');
  await page.getByRole('button', { name: 'Undo', exact: true }).click();
  await expect(page.locator('.time-event')).toHaveCSS('height', '104px');
  const transfer = await page.evaluateHandle(() => new DataTransfer());
  await page.locator('.time-event').dispatchEvent('dragstart', { dataTransfer: transfer });
  await column.dispatchEvent('drop', { dataTransfer: transfer, clientY: rect.y + 12.25 * 72 });
  await page.locator('.time-event').focus();
  await page.keyboard.press('Enter');
  await expect(page.getByLabel('Start time', { exact: true })).toHaveValue('12:15');
  await expect(page.getByLabel('End time', { exact: true })).toHaveValue('13:45');
  await page.getByLabel('End time', { exact: true }).fill('14:00');
  await page.getByRole('button', { name: 'Save changes' }).click();
  await expect(page.locator('.time-event')).toHaveCSS('height', '122px');
  const invalidHandle = await handle.boundingBox();
  if (!invalidHandle) throw new Error('Missing resize handle');
  await page.mouse.move(
    invalidHandle.x + invalidHandle.width / 2,
    invalidHandle.y + invalidHandle.height / 2,
  );
  await page.mouse.down();
  await page.mouse.move(x, rect.y + 8 * 72, { steps: 8 });
  await page.mouse.up();
  await expect(page.getByRole('alert')).toContainText('End must be after start');
  await expect(page.locator('.time-event')).toHaveCSS('height', '122px');
  await page.getByRole('button', { name: 'Undo', exact: true }).click();
  await expect(page.locator('.time-event')).toHaveCSS('height', '104px');
});

test('month drag creates an inclusive selection with exclusive stored end and continuation', async ({
  page,
}) => {
  await page.goto('/?date=2026-10-09&scenario=empty');
  const first = await page.locator('.month-cell[data-date="2026-10-09"]').boundingBox();
  const last = await page.locator('.month-cell[data-date="2026-10-11"]').boundingBox();
  if (!first || !last) throw new Error('Missing date cells');
  await page.mouse.move(first.x + 20, first.y + first.height - 10);
  await page.mouse.down();
  await page.mouse.move(last.x + 20, last.y + last.height - 10, { steps: 10 });
  await page.mouse.up();
  await page.getByRole('textbox', { name: 'Event title' }).fill('Three days');
  await expect(page.locator('#event-start-date')).toHaveValue('2026-10-09');
  await expect(page.locator('#event-end-date')).toHaveValue('2026-10-11');
  await page.getByRole('button', { name: 'Create event', exact: true }).click();
  await expect(page.getByRole('button', { name: /Three days,/ })).toHaveCount(1);
  await expect(page.locator('.month-cell[data-date="2026-10-12"] .event-chip')).toHaveCount(0);
  await page.getByRole('button', { name: 'Week', exact: true }).click();
  await expect(page.getByRole('button', { name: /Three days,/ })).toHaveCount(1);
  await page.locator('.all-day-cell').first().focus();
  await page.keyboard.press('Enter');
  await expect(page.getByRole('switch', { name: 'All-day event' })).toBeChecked();
});

test('rejects DST gesture gaps and cancels a selection with Escape', async ({ page }) => {
  await page.goto('/?date=2026-03-29&scenario=empty');
  await page.getByRole('button', { name: 'Day', exact: true }).click();
  const column = page.locator('.time-column');
  await page.locator('.time-scroll').evaluate((element) => {
    element.scrollTop = 0;
  });
  const rect = await column.boundingBox();
  if (!rect) throw new Error('Missing column');
  const x = rect.x + rect.width / 2;
  await page.mouse.move(x, rect.y + 2.5 * 72);
  await page.mouse.down();
  await page.mouse.move(x, rect.y + 3.5 * 72, { steps: 8 });
  await page.mouse.up();
  await expect(page.getByRole('alert')).toContainText('does not exist');
  await expect(page.getByRole('textbox', { name: 'Event title' })).toHaveCount(0);
  const next = await column.boundingBox();
  if (!next) throw new Error('Missing shifted column');
  await page.mouse.move(x, next.y + 4 * 72);
  await page.mouse.down();
  await page.mouse.move(x, next.y + 5 * 72, { steps: 8 });
  await page.keyboard.press('Escape');
  await page.mouse.up();
  await expect(page.locator('.time-selection')).toHaveCount(0);
  await expect(page.getByRole('textbox', { name: 'Event title' })).toHaveCount(0);
});

test('keeps crowded all-day spans in month overflow', async ({ page }) => {
  await page.goto('/?date=2026-10-09&scenario=empty');
  for (let i = 1; i <= 4; i++) {
    await page.getByRole('button', { name: 'New event' }).click();
    await page.getByRole('textbox', { name: 'Event title' }).fill(`All-day ${i}`);
    await page.getByRole('switch', { name: 'All-day event' }).click();
    await page.getByRole('button', { name: 'Create event', exact: true }).click();
  }
  await expect(page.locator('.month-spans .event-chip')).toHaveCount(2);
  await expect(page.locator('.cell-events .event-chip')).toHaveCount(0);
  await page.getByRole('button', { name: '+2 more' }).click();
  await expect(page.getByRole('dialog').getByRole('button', { name: /All-day \d,/ })).toHaveCount(
    4,
  );
});
