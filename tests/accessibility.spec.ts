import { expect, test, type Page } from '@playwright/test';
import AxeBuilder from '@axe-core/playwright';

async function audit(page: Page) {
  const result = await new AxeBuilder({ page })
    .withTags(['wcag2a', 'wcag2aa', 'wcag21aa'])
    .analyze();
  expect(
    result.violations
      .filter((v) => ['critical', 'serious'].includes(v.impact ?? ''))
      .map((v) => ({
        id: v.id,
        nodes: v.nodes.map((n) => ({ target: n.target, summary: n.failureSummary })),
      })),
  ).toEqual([]);
}
async function openCalendarMenu(page: Page) {
  await page.getByRole('button', { name: 'Options for Personal' }).focus();
  await page.keyboard.press('ArrowDown');
  await expect(page.getByRole('menu')).toBeVisible();
}

for (const theme of ['light', 'dark']) {
  test(`${theme} calendar views, editor, settings, and shortcuts have no serious accessibility violations`, async ({
    page,
  }) => {
    test.setTimeout(60_000);
    await page.emulateMedia({ reducedMotion: 'reduce' });
    await page.goto(`/?date=2026-10-09&theme=${theme}`);
    await audit(page);
    for (const view of ['Week', 'Day']) {
      await page.getByRole('button', { name: view, exact: true }).click();
      await audit(page);
    }
    await page.getByRole('button', { name: 'New event' }).click();
    await expect(page.getByRole('textbox', { name: 'Event title' })).toBeFocused();
    await audit(page);
    await page.keyboard.press('Escape');
    await page.getByRole('button', { name: 'Open settings', exact: true }).click();
    await audit(page);
    await page.keyboard.press('Escape');
    await page.getByRole('button', { name: 'Open keyboard shortcuts', exact: true }).click();
    await audit(page);
  });

  test(`${theme} calendar creation, menu, settings, and deletion have no serious accessibility violations`, async ({
    page,
  }) => {
    test.setTimeout(60_000);
    await page.emulateMedia({ reducedMotion: 'reduce' });
    await page.goto(`/?date=2026-10-09&theme=${theme}`);
    await page
      .locator('.sidebar')
      .getByRole('button', { name: 'Create calendar', exact: true })
      .click();
    await page.getByRole('menuitem', { name: 'On this device' }).click();
    await audit(page);
    await page.keyboard.press('Escape');
    await openCalendarMenu(page);
    await audit(page);
    // Axe's temporary focus changes can dismiss a non-modal popover; reopen it for the next dialog.
    await page.keyboard.press('Escape');
    await openCalendarMenu(page);
    await page.getByRole('menuitem', { name: 'Settings', exact: true }).click();
    await audit(page);
    await page.keyboard.press('Escape');
    await openCalendarMenu(page);
    await page.getByRole('menuitem', { name: 'Delete calendar' }).click();
    await audit(page);
  });
}
