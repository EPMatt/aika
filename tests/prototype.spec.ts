import { expect, test } from '@playwright/test';

test('renders when a desktop webview exposes a POSIX locale', async ({ page }) => {
  await page.addInitScript(() =>
    Object.defineProperty(navigator, 'language', { value: 'en_US.UTF-8' }),
  );
  await page.goto('/?date=2026-10-09&theme=light');
  await expect(page.locator('.month-cell[role="gridcell"]')).toHaveCount(42);
  await page.getByRole('button', { name: 'New event' }).click();
  await expect(page.getByRole('textbox', { name: 'Event title' })).toBeVisible();
});

test('renders all views and both appearances without runtime errors', async ({ page }) => {
  const errors: string[] = [];
  page.on('pageerror', (error) => errors.push(error.message));
  await page.goto('/?date=2026-10-09&theme=light');
  await expect(page.getByRole('heading', { name: 'October 2026' })).toBeVisible();
  await expect(page.locator('.month-cell[role="gridcell"]')).toHaveCount(42);
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
  await expect(page.getByRole('button', { name: 'New event' })).toBeVisible();
  await page.keyboard.press('n');
  await expect(page.getByRole('textbox', { name: 'Event title' })).toBeFocused();
  await page.keyboard.type('Keyboard event');
  await page.getByRole('button', { name: 'Create event', exact: true }).click();
  await expect(page.getByRole('button', { name: /Keyboard event,/ })).toBeVisible();
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

for (const popup of ['Settings', 'Keyboard shortcuts']) {
  test(`${popup} dismisses, restores focus, and suppresses calendar shortcuts`, async ({
    page,
  }) => {
    await page.goto('/?date=2026-10-09&scenario=empty&theme=light');
    await page.getByRole('button', { name: 'New event' }).click();
    await page.getByRole('textbox', { name: 'Event title' }).fill('Keep this event');
    await page.getByRole('button', { name: 'Create event', exact: true }).click();
    await page.getByRole('button', { name: 'Previous month' }).click();

    const opener = page.getByRole('button', { name: `Open ${popup.toLowerCase()}`, exact: true });
    const dialog = page.getByRole('dialog', { name: popup, exact: true });
    const close = page.getByRole('button', { name: `Close ${popup.toLowerCase()}`, exact: true });

    for (const dismissal of ['Escape', 'outside', 'close']) {
      await opener.click();
      await expect(page.getByRole('dialog')).toHaveCount(1);
      await expect(dialog).toBeVisible();
      await expect(dialog.getByRole('list', { name: 'Keyboard shortcuts' })).toHaveCount(
        popup === 'Keyboard shortcuts' ? 1 : 0,
      );
      await expect(dialog.getByLabel('Appearance preference')).toHaveCount(
        popup === 'Settings' ? 1 : 0,
      );
      await close.focus();
      for (const key of ['n', 't', '1', '2', '3', 'Control+z', 'Meta+z']) {
        await page.keyboard.press(key);
      }
      await expect(page.getByRole('textbox', { name: 'Event title' })).toHaveCount(0);

      if (dismissal === 'outside') await page.mouse.click(10, 10);
      else if (dismissal === 'close') await close.click();
      else await page.keyboard.press('Escape');
      await expect(dialog).toHaveCount(0);
      await expect(opener).toBeFocused();
      await expect(page.getByRole('heading', { name: 'September 2026' })).toBeVisible();
      await expect(page.locator('.month-cell[role="gridcell"]')).toHaveCount(42);
    }

    await page.keyboard.press('t');
    await expect(page.getByRole('button', { name: /Keep this event,/ })).toBeVisible();
    await page.keyboard.press('Control+z');
    await expect(page.getByRole('button', { name: /Keep this event,/ })).toHaveCount(0);
  });
}

for (const [platform, modifier] of [
  ['MacIntel', 'Cmd'],
  ['Linux x86_64', 'Ctrl'],
  ['Win32', 'Ctrl'],
]) {
  test(`shortcuts show labeled keycaps and the ${platform} modifier`, async ({ page }) => {
    await page.addInitScript((value) => {
      Object.defineProperty(navigator, 'platform', { get: () => value });
    }, platform);
    await page.goto('/?date=2026-10-09&scenario=empty&theme=light');
    await page.getByRole('button', { name: 'Open keyboard shortcuts', exact: true }).click();
    const dialog = page.getByRole('dialog', { name: 'Keyboard shortcuts', exact: true });
    const rows = dialog.getByRole('list', { name: 'Keyboard shortcuts' }).getByRole('listitem');
    await expect(rows).toHaveCount(10);
    await expect(rows.locator(':scope > span:first-child')).toHaveText([
      'New event',
      'Go to today',
      'Month view',
      'Week view',
      'Day view',
      'Undo',
      'Edit focused event',
      'Create event on focused date or time slot',
      'Navigate month dates',
      'Dismiss popup/editor or cancel selection',
    ]);
    for (const [index, key] of ['N', 'T', '1', '2', '3'].entries()) {
      await expect(rows.nth(index).locator('kbd')).toHaveText(key);
    }
    const undoKeys = rows.nth(5).locator('kbd');
    await expect(undoKeys).toHaveCount(2);
    await expect(undoKeys.first().getByText(modifier, { exact: true })).toBeVisible();
    await expect(undoKeys.last()).toHaveText('Z');
    await expect(rows.nth(6).locator('kbd')).toHaveText('Enter');
    await expect(rows.nth(7).locator('kbd')).toHaveText('Enter');
    await expect(rows.nth(8).locator('kbd .sr-only')).toHaveText([
      'Arrow left',
      'Arrow up',
      'Arrow down',
      'Arrow right',
    ]);
    await expect(rows.nth(8).locator('.shortcut-separator .sr-only')).toHaveText([
      'or',
      'or',
      'or',
    ]);
    await expect(rows.nth(9).locator('kbd .sr-only')).toHaveText('Escape');
  });
}

for (const theme of ['light', 'dark']) {
  test(`${theme} shortcuts fit the window and keep keycaps aligned`, async ({ page }, testInfo) => {
    await page.goto(`/?date=2026-10-09&scenario=empty&theme=${theme}`);
    await page.getByRole('button', { name: 'Open keyboard shortcuts', exact: true }).click();
    const dialog = page.getByRole('dialog', { name: 'Keyboard shortcuts', exact: true });
    for (const viewport of [
      { width: 1440, height: 960 },
      { width: 900, height: 600 },
      { width: 900, height: 420 },
    ]) {
      await page.setViewportSize(viewport);
      const bounds = await dialog.boundingBox();
      expect(bounds).not.toBeNull();
      expect(bounds!.y).toBeGreaterThanOrEqual(17);
      expect(bounds!.y + bounds!.height).toBeLessThanOrEqual(viewport.height - 17);
      if (viewport.height >= 600) {
        const needsScroll = await dialog
          .getByRole('list', { name: 'Keyboard shortcuts' })
          .evaluate((list) => list.scrollHeight > list.clientHeight);
        expect(needsScroll).toBe(false);
      }
      const layout = await dialog.locator('.shortcut-row').evaluateAll((rows) =>
        rows.map((row) => {
          const label = row.children[0].getBoundingClientRect();
          const keys = row.children[1].getBoundingClientRect();
          return { labelRight: label.right, keysLeft: keys.left, keysRight: keys.right };
        }),
      );
      for (const row of layout) {
        expect(row.keysLeft - row.labelRight).toBeGreaterThanOrEqual(11);
        expect(Math.abs(row.keysRight - layout[0].keysRight)).toBeLessThan(1);
      }
      await dialog.getByText('Dismiss popup/editor or cancel selection').scrollIntoViewIfNeeded();
      await expect(dialog.getByText('Esc', { exact: true })).toBeInViewport();
      await dialog.getByText('New event', { exact: true }).scrollIntoViewIfNeeded();
      await page.screenshot({
        path: testInfo.outputPath(`shortcuts-${theme}-${viewport.width}x${viewport.height}.png`),
        animations: 'disabled',
      });
    }
  });
}

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

test('keeps DST gap input and requires an offset for repeated hours', async ({ page }) => {
  await page.goto('/?date=2026-03-29&scenario=empty');
  await page.getByRole('button', { name: 'New event' }).click();
  await page.getByRole('textbox', { name: 'Event title' }).fill('DST event');
  await page.getByLabel('Start time', { exact: true }).fill('02:30');
  await page.getByRole('button', { name: 'Create event', exact: true }).click();
  await expect(page.getByRole('alert')).toContainText('does not exist');
  await expect(page.getByLabel('Start time', { exact: true })).toHaveValue('02:30');
  await expect(page.getByRole('textbox', { name: 'Event title' })).toHaveValue('DST event');
  await page.locator('#event-start-date').fill('2026-10-25');
  await page.locator('#event-end-date').fill('2026-10-25');
  await page.getByLabel('End time', { exact: true }).fill('03:30');
  await page.getByRole('button', { name: 'Create event', exact: true }).click();
  await expect(page.getByRole('alert')).toContainText('occurs twice');
  await page.getByLabel('start UTC offset').selectOption('60');
  await page.getByRole('button', { name: 'Create event', exact: true }).click();
  await expect(page.getByRole('textbox', { name: 'Event title' })).toHaveCount(0);
});
