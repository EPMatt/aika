import { expect, test } from '@playwright/test';

test('renders when a desktop webview exposes a POSIX locale', async ({ page }) => {
  await page.addInitScript(() =>
    Object.defineProperty(navigator, 'language', { value: 'en_US.UTF-8' }),
  );
  await page.goto('/?date=2026-10-09&theme=light');
  await expect(page.getByRole('gridcell')).toHaveCount(42);
  await page.getByRole('button', { name: 'New event' }).click();
  await expect(page.getByRole('textbox', { name: 'Event title' })).toBeVisible();
});

test('renders all views and both appearances without runtime errors', async ({ page }) => {
  const errors: string[] = [];
  page.on('pageerror', (error) => errors.push(error.message));
  await page.goto('/?date=2026-10-09&theme=light');
  await expect(page.getByRole('heading', { name: 'October 2026' })).toBeVisible();
  await expect(page.getByRole('gridcell')).toHaveCount(42);
  await page.getByRole('button', { name: 'Week', exact: true }).click();
  await expect(page.getByLabel('Week calendar')).toBeVisible();
  await page.getByRole('button', { name: 'Day', exact: true }).click();
  await expect(page.getByLabel('Day calendar')).toBeVisible();
  await expect(page.getByRole('button', { name: 'Dark appearance' })).toHaveCount(0);
  await page.getByRole('button', { name: 'Open settings' }).click();
  await page.getByLabel('Appearance preference').selectOption('dark');
  await page.getByRole('button', { name: 'Close settings' }).click();
  await expect(page.locator('html')).toHaveAttribute('data-theme', 'dark');
  await page.goto('/?date=2026-10-09');
  await expect(page.locator('html')).toHaveAttribute('data-theme', 'dark');
  await page.getByRole('button', { name: 'Open settings' }).click();
  await page.getByLabel('Appearance preference').selectOption('system');
  await page.getByRole('button', { name: 'Close settings' }).click();
  await page.emulateMedia({ colorScheme: 'light' });
  await expect(page.locator('html')).toHaveAttribute('data-theme', 'light');
  await page.getByRole('button', { name: 'Hide sidebar' }).click();
  await expect(page.getByRole('button', { name: 'Show sidebar' })).toBeVisible();
  await page.getByRole('button', { name: 'Show sidebar' }).click();
  expect(errors).toEqual([]);
});

test('creates, edits, deletes, and undoes events through the editor', async ({ page }) => {
  await page.goto('/?date=2026-10-09&scenario=empty&theme=light');
  await page.getByRole('button', { name: 'New event' }).click();
  await page.getByRole('textbox', { name: 'Event title' }).fill('A considered afternoon');
  await page.getByRole('button', { name: 'Add location or notes' }).click();
  await page.getByRole('textbox', { name: 'Location', exact: true }).fill('At the studio');
  await page.getByRole('textbox', { name: 'Notes', exact: true }).fill('Bring the sketchbook');
  await page.getByRole('button', { name: 'Create event', exact: true }).click();
  await page.getByRole('button', { name: /A considered afternoon,/ }).click();
  await expect(page.getByRole('textbox', { name: 'Location', exact: true })).toHaveValue(
    'At the studio',
  );
  await page.getByRole('textbox', { name: 'Event title' }).fill('A lovely afternoon');
  await page.getByRole('button', { name: 'Save changes' }).click();
  await page.getByRole('button', { name: /A lovely afternoon,/ }).click();
  await page.getByRole('button', { name: 'Delete event' }).click();
  await expect(page.getByRole('button', { name: /A lovely afternoon,/ })).toHaveCount(0);
  await page.getByRole('button', { name: 'Undo', exact: true }).click();
  await expect(page.getByRole('button', { name: /A lovely afternoon,/ })).toBeVisible();
});

test('provides keyboard creation, cancellation, view switching, and undo', async ({ page }) => {
  await page.goto('/?date=2026-10-09&scenario=empty');
  await page.keyboard.press('n');
  await expect(page.getByRole('textbox', { name: 'Event title' })).toBeFocused();
  await page.keyboard.type('Keyboard event');
  await page.getByRole('button', { name: 'Create event', exact: true }).click();
  await page.keyboard.press('Control+z');
  await expect(page.getByRole('button', { name: /Keyboard event,/ })).toHaveCount(0);
  await page.getByRole('gridcell', { name: '9 October 2026', exact: true }).focus();
  await page.keyboard.press('ArrowRight');
  await expect(page.getByRole('gridcell', { name: '10 October 2026', exact: true })).toBeFocused();
  await page.keyboard.press('ArrowDown');
  await expect(page.getByRole('gridcell', { name: '17 October 2026', exact: true })).toBeFocused();
  await page.keyboard.press('2');
  await expect(page.getByLabel('Week calendar')).toBeVisible();
  await page.keyboard.press('n');
  await page.keyboard.press('Escape');
  await expect(page.getByRole('textbox', { name: 'Event title' })).toHaveCount(0);
});

test('captures the review scenes', async ({ page }, testInfo) => {
  test.setTimeout(60_000);
  for (const theme of ['light', 'dark']) {
    await page.goto(`/?date=2026-10-09&theme=${theme}`);
    for (const view of ['Month', 'Week', 'Day']) {
      await page.getByRole('button', { name: view, exact: true }).click();
      await page.screenshot({
        path: testInfo.outputPath(`${view.toLowerCase()}-${theme}.png`),
        animations: 'disabled',
      });
    }
    await page.getByRole('button', { name: 'New event' }).click();
    await page.getByRole('textbox', { name: 'Event title' }).fill('Project planning');
    await page.screenshot({
      path: testInfo.outputPath(`editor-${theme}.png`),
      animations: 'disabled',
    });
    await page.keyboard.press('Escape');
  }
  await page.goto('/?date=2026-10-09&scenario=empty&theme=light');
  await page.screenshot({ path: testInfo.outputPath('month-empty.png') });
  await page.goto('/?date=2026-10-09&scenario=dense&theme=light');
  await page.screenshot({ path: testInfo.outputPath('month-dense.png') });
  await page.getByRole('button', { name: /more$/ }).first().click();
  await expect(
    page.getByRole('dialog').getByText('Friday, October 9', { exact: true }),
  ).toBeVisible();
  await page.screenshot({ path: testInfo.outputPath('month-overflow.png') });
  await page.keyboard.press('Escape');
  await page.setViewportSize({ width: 900, height: 600 });
  await page.screenshot({ path: testInfo.outputPath('month-compact.png') });
  expect(await page.evaluate(() => document.documentElement.scrollWidth > innerWidth)).toBe(false);
  expect(
    await page
      .locator('.month-cell')
      .evaluateAll((cells) =>
        cells.every((cell) =>
          Array.from(cell.querySelectorAll('.cell-events > button')).every(
            (button) =>
              button.getBoundingClientRect().bottom <= cell.getBoundingClientRect().bottom + 1,
          ),
        ),
      ),
  ).toBe(true);
  await page.emulateMedia({ reducedMotion: 'reduce' });
  await page.getByRole('button', { name: 'New event' }).click();
  expect(
    await page
      .locator('.event-popover')
      .evaluate((element) => getComputedStyle(element).animationName),
  ).toBe('none');
  await page.keyboard.press('Escape');
  await page.setViewportSize({ width: 621, height: 750 });
  await expect(page.getByRole('button', { name: 'Show sidebar' })).toBeVisible();
  await page.screenshot({ path: testInfo.outputPath('window-tiled.png') });
  expect(await page.evaluate(() => document.documentElement.scrollWidth > innerWidth)).toBe(false);
});
