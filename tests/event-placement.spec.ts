import { expect, test, type Page } from '@playwright/test';

async function expectInsideWindow(page: Page) {
  await expect(page.locator('.event-popover')).toBeVisible();
  await expect
    .poll(() =>
      page.locator('.event-popover').evaluate((element) => {
        const rect = element.getBoundingClientRect();
        return (
          rect.left >= 17 &&
          rect.top >= 17 &&
          rect.right <= innerWidth - 17 &&
          rect.bottom <= innerHeight - 17
        );
      }),
    )
    .toBe(true);
}

test('empty month space opens the same all-day draft as the plus button', async ({ page }) => {
  await page.goto('/?date=2026-10-09&scenario=empty');
  const cell = page.locator('.month-cell[data-date="2026-10-10"]');
  const rect = await cell.boundingBox();
  if (!rect) throw new Error('Missing month cell');
  await page.mouse.click(rect.x + 30, rect.y + rect.height - 15);
  await expect(page.getByRole('textbox', { name: 'Event title' })).toBeFocused();
  await expect(page.getByRole('switch', { name: 'All-day event' })).toBeChecked();
  await expect(page.locator('#event-start-date')).toHaveValue('2026-10-10');
  await expect(cell).toHaveAttribute('aria-selected', 'true');
  await expectInsideWindow(page);
  await page.keyboard.press('Escape');
  await expect(cell).toBeFocused();
  await cell.locator('.cell-add').click();
  await expect(page.getByRole('switch', { name: 'All-day event' })).toBeChecked();
  await expect(page.locator('#event-start-date')).toHaveValue('2026-10-10');
});

for (const view of ['Day', 'Week']) {
  test(`${view} empty space anchors the editor to the click after scrolling`, async ({ page }) => {
    await page.goto('/?date=2026-10-09&scenario=empty');
    await page.getByRole('button', { name: view, exact: true }).click();
    await page.locator('.time-scroll').evaluate((element) => {
      element.scrollTop = 8 * 72;
    });
    const column = page.locator('.time-column').first();
    const rect = await column.boundingBox();
    if (!rect) throw new Error('Missing time column');
    const x = rect.x + 40;
    const y = rect.y + 9.25 * 72;
    await page.mouse.click(x, y);
    await expect(page.getByLabel('Start time', { exact: true })).toHaveValue('09:15');
    const editor = page.locator('.event-popover');
    await expect(editor).toHaveAttribute('data-side', 'right');
    await expect
      .poll(async () => {
        const box = await editor.boundingBox();
        return box && Math.abs(box.x - (x + 12)) < 1 && Math.abs(box.y - y) < 1;
      })
      .toBe(true);
    await expectInsideWindow(page);
    await page.keyboard.press('Escape');
    await expect(column).toBeFocused();
  });

  test(`${view} edge clicks flip left and remain bounded throughout resizing`, async ({ page }) => {
    await page.goto('/?date=2026-10-09&scenario=empty');
    await page.getByRole('button', { name: view, exact: true }).click();
    const column = await page.locator('.time-column').last().boundingBox();
    const scroll = await page.locator('.time-scroll').boundingBox();
    if (!column || !scroll) throw new Error('Missing time grid');
    const x = column.x + column.width - 20;
    const y = scroll.y + scroll.height - 15;
    await page.mouse.click(x, y);
    const editor = page.locator('.event-popover');
    await expect(editor).toHaveAttribute('data-side', 'left');
    await expectInsideWindow(page);
    await page.getByRole('textbox', { name: 'Event title' }).fill('Keep this draft');
    await page.getByRole('button', { name: 'Add location or notes' }).click();
    await page.getByRole('textbox', { name: 'Notes', exact: true }).fill('Keep these notes');
    for (const size of [
      { width: 900, height: 600 },
      { width: 621, height: 750 },
      { width: 360, height: 500 },
      { width: 1440, height: 960 },
    ]) {
      await page.setViewportSize(size);
      await expectInsideWindow(page);
      await expect(page.getByRole('textbox', { name: 'Event title' })).toHaveValue(
        'Keep this draft',
      );
      await expect(page.getByRole('textbox', { name: 'Notes', exact: true })).toHaveValue(
        'Keep these notes',
      );
      await expect(page.getByRole('textbox', { name: 'Notes', exact: true })).toBeFocused();
    }
    await page.getByRole('button', { name: 'Create event', exact: true }).click();
    await expect(editor).toHaveCount(0);
    await expect(page.getByRole('button', { name: /Keep this draft,/ })).toBeVisible();
  });
}

test('button and existing-event anchors stay bounded during resizing', async ({ page }) => {
  await page.goto('/?date=2026-10-09&scenario=empty');
  await page.getByRole('button', { name: 'New event' }).click();
  await page.getByRole('textbox', { name: 'Event title' }).fill('Anchored event');
  await page.setViewportSize({ width: 900, height: 600 });
  await expectInsideWindow(page);
  await page.getByRole('button', { name: 'Create event', exact: true }).click();
  await page.getByRole('button', { name: /Anchored event,/ }).click();
  await expect(page.getByRole('button', { name: 'Save changes' })).toBeVisible();
  await page.setViewportSize({ width: 621, height: 500 });
  await expectInsideWindow(page);
  await page.keyboard.press('Escape');
  await expect(page.getByRole('button', { name: /Anchored event,/ })).toBeFocused();
});
