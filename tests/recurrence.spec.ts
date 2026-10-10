import { expect, test } from '@playwright/test';
import AxeBuilder from '@axe-core/playwright';

for (const theme of ['light', 'dark']) {
  test(`recurring editor, scopes, exception reset and undo in ${theme}`, async ({ page }) => {
    await page.goto(`/?date=2026-10-09&scenario=empty&theme=${theme}`);
    await page.getByRole('button', { name: 'New event' }).click();
    await page.getByLabel('Event title', { exact: true }).fill('Daily routine');
    await page.getByRole('switch', { name: 'All-day event' }).click();
    await page.getByLabel('Repeat pattern').selectOption('daily');
    await page.getByLabel('Repeat ending').selectOption('count');
    await page.getByLabel('Repeat count').fill('5');
    await page.getByRole('button', { name: 'Preview next dates' }).click();
    await expect(page.getByLabel('Repeat preview').locator('li')).toHaveCount(5);
    const axe = await new AxeBuilder({ page })
      .withTags(['wcag2a', 'wcag2aa', 'wcag21aa'])
      .analyze();
    expect(axe.violations).toEqual([]);
    await page.getByRole('button', { name: 'Create event', exact: true }).click();
    await expect(page.getByRole('button', { name: /Daily routine, recurring/ })).toHaveCount(5);
    await page
      .getByRole('button', { name: /Daily routine, recurring/ })
      .nth(2)
      .click();
    await expect(page.getByLabel('Event change scope')).toHaveValue('occurrence');
    await expect(page.getByLabel('Repeat pattern')).toBeDisabled();
    await page.getByLabel('Event title', { exact: true }).fill('One change');
    await page.getByRole('button', { name: 'Save changes', exact: true }).click();
    await expect(page.getByRole('button', { name: /One change, recurring/ })).toHaveCount(1);
    await page
      .getByRole('button', { name: /Daily routine, recurring/ })
      .first()
      .click();
    await page.getByLabel('Event change scope').selectOption('series');
    await page.getByLabel('Event title', { exact: true }).fill('Whole series');
    await page.getByRole('button', { name: 'Save changes', exact: true }).click();
    await expect(page.getByRole('alert')).toContainText('Confirm resetting');
    await page.getByLabel('Reset individual changes', { exact: true }).check();
    await page.getByRole('button', { name: 'Save changes', exact: true }).click();
    await expect(page.getByRole('button', { name: /Whole series, recurring/ })).toHaveCount(5);
    await page
      .getByRole('button', { name: /Whole series, recurring/ })
      .nth(2)
      .click();
    await page.getByLabel('Event change scope').selectOption('following');
    await page.getByLabel('Event title', { exact: true }).fill('Future routine');
    await page.getByRole('button', { name: 'Preview next dates' }).click();
    await expect(page.getByLabel('Repeat preview').locator('li')).toHaveCount(3);
    await page.getByRole('button', { name: 'Save changes', exact: true }).click();
    await expect(page.getByRole('button', { name: /Whole series, recurring/ })).toHaveCount(2);
    await expect(page.getByRole('button', { name: /Future routine, recurring/ })).toHaveCount(3);
    await page.getByRole('button', { name: 'Undo', exact: true }).click();
    await expect(page.getByRole('button', { name: /Whole series, recurring/ })).toHaveCount(5);
    await page
      .getByRole('button', { name: /Whole series, recurring/ })
      .nth(2)
      .click();
    await page.getByLabel('Event change scope').selectOption('following');
    await page.getByLabel('Repeat count').fill('6');
    await page.getByLabel('Repeat count').fill('5');
    await page.getByLabel('Event title', { exact: true }).fill('Explicit five');
    await page.getByRole('button', { name: 'Preview next dates' }).click();
    await expect(page.getByLabel('Repeat preview').locator('li')).toHaveCount(5);
    await page.getByRole('button', { name: 'Save changes', exact: true }).click();
    await expect(page.getByRole('button', { name: /Explicit five, recurring/ })).toHaveCount(5);
    await expect(page.getByRole('button', { name: /Whole series, recurring/ })).toHaveCount(2);
    await page.getByRole('button', { name: 'Undo', exact: true }).click();
    await expect(page.getByRole('button', { name: /Whole series, recurring/ })).toHaveCount(5);
    await page
      .getByRole('button', { name: /Whole series, recurring/ })
      .nth(1)
      .click();
    await page.getByRole('button', { name: 'Delete event', exact: true }).click();
    await expect(page.getByRole('button', { name: /Whole series, recurring/ })).toHaveCount(4);
    await page.getByRole('button', { name: 'Undo', exact: true }).click();
    await expect(page.getByRole('button', { name: /Whole series, recurring/ })).toHaveCount(5);
    await page.getByRole('button', { name: 'Week', exact: true }).click();
    await expect(page.locator('.all-day-spans [aria-label="Recurring event"]')).not.toHaveCount(0);
  });
}

test('recurring drag requests scope and cancels without saving', async ({ page }) => {
  await page.goto('/?date=2026-10-09&scenario=empty');
  await page.getByRole('button', { name: 'New event' }).click();
  await page.getByLabel('Event title', { exact: true }).fill('Recurring drag');
  await page.getByLabel('Repeat pattern').selectOption('daily');
  await page.getByLabel('Repeat ending').selectOption('count');
  await page.getByLabel('Repeat count').fill('3');
  await page.getByRole('button', { name: 'Create event', exact: true }).click();
  const event = page.getByRole('button', { name: /Recurring drag, recurring/ }).first();
  await event.dragTo(page.locator('.month-cell[data-date="2026-10-15"]'));
  await expect(page.getByRole('dialog', { name: 'Change recurring event' })).toBeVisible();
  await page.getByRole('dialog').getByRole('button', { name: 'Cancel', exact: true }).focus();
  await page.keyboard.press('n');
  await page.keyboard.press('2');
  await page.keyboard.press('Control+z');
  await expect(page.getByLabel('Event title', { exact: true })).toHaveCount(0);
  await expect(page.locator('.month-cell[role="gridcell"]')).toHaveCount(42);
  await page.getByRole('dialog').getByRole('button', { name: 'Cancel', exact: true }).click();
  await expect(page.getByRole('button', { name: /Recurring drag, recurring/ })).toHaveCount(3);
  await event.dragTo(page.locator('.month-cell[data-date="2026-10-15"]'));
  await page.getByRole('dialog').getByRole('button', { name: 'Save changes', exact: true }).click();
  await expect(
    page
      .locator('.month-cell[data-date="2026-10-15"]')
      .getByRole('button', { name: /Recurring drag, recurring/ }),
  ).toHaveCount(1);
  await page.getByRole('button', { name: 'Undo', exact: true }).click();
  await expect(page.locator('.month-cell[data-date="2026-10-15"] .event-chip')).toHaveCount(0);
});
