import { expect, test, type Page } from '@playwright/test';

async function createCalendar(page: Page, name: string) {
  await page
    .locator('.sidebar')
    .getByRole('button', { name: 'Create calendar', exact: true })
    .click();
  const dialog = page.getByRole('dialog', { name: 'Create calendar' });
  await dialog.getByLabel('Calendar name').fill(name);
  await dialog.getByLabel('Calendar description').fill(`${name} description`);
  await dialog.getByRole('button', { name: 'Blue', exact: true }).click();
  await dialog.getByRole('button', { name: 'Create calendar', exact: true }).click();
  await expect(dialog).not.toBeVisible();
  await expect(
    page.locator('.calendar-row').filter({ has: page.getByRole('button', { name, exact: true }) }),
  ).toBeVisible();
}
async function menu(page: Page, name: string) {
  const row = page
    .locator('.calendar-row')
    .filter({ has: page.getByRole('button', { name, exact: true }) });
  await row.hover();
  await row.getByRole('button', { name: `Options for ${name}` }).click();
  return page.getByRole('menu', { name: `${name} options` });
}
async function createEvent(page: Page, title: string) {
  await page.getByRole('button', { name: 'New event', exact: true }).click();
  await page.getByLabel('Event title').fill(title);
  await page.getByRole('button', { name: 'Create event', exact: true }).click();
  await expect(page.getByRole('button', { name: new RegExp(`^${title},`) })).toBeVisible();
}

test('create, default, visibility and settings work across all views', async ({ page }) => {
  await page.goto('/?date=2026-10-09&scenario=empty&theme=light');
  await createEvent(page, 'Personal appointment');
  await createCalendar(page, 'Work');
  await expect(page.getByRole('button', { name: 'Personal', exact: true })).toContainText(
    'Default',
  );
  await (await menu(page, 'Work')).getByRole('menuitem', { name: 'Set as default' }).click();
  await page.getByRole('button', { name: 'New event', exact: true }).click();
  await expect(page.getByLabel('Event calendar').locator('option:checked')).toHaveText('Work');
  await page.getByLabel('Event title').fill('Work appointment');
  await page.getByRole('button', { name: 'Create event', exact: true }).click();
  const event = page.getByRole('button', { name: /^Work appointment,/ });
  await expect(event).toBeVisible();
  await expect(event).toHaveCSS('--calendar-color', '#5B7FA3');
  await page.getByRole('button', { name: 'Work', exact: true }).click();
  await expect(event).toHaveCount(0);
  for (const view of ['Week', 'Day', 'Month']) {
    await page.getByRole('button', { name: view, exact: true }).click();
    await expect(event).toHaveCount(0);
    await expect(page.getByRole('button', { name: /^Personal appointment,/ })).toBeVisible();
    await expect(page.locator('.main-footer')).toContainText('1 event on');
  }
  await (await menu(page, 'Work')).getByRole('menuitem', { name: 'Settings', exact: true }).click();
  const dialog = page.getByRole('dialog', { name: 'Calendar settings' });
  await expect(dialog.getByLabel('Calendar name')).toHaveValue('Work');
  await expect(dialog.getByLabel('Calendar description')).toHaveValue('Work description');
  await dialog.getByLabel('Calendar name').fill('Office');
  await dialog.getByLabel('Calendar description').fill('Office hours');
  await dialog.getByRole('button', { name: 'Rose', exact: true }).click();
  await dialog.getByRole('button', { name: 'Save changes' }).click();
  await expect(page.getByRole('button', { name: 'Office', exact: true })).toHaveAttribute(
    'aria-pressed',
    'false',
  );
  await page.getByRole('button', { name: 'Office', exact: true }).click();
  await expect(event).toHaveCSS('--calendar-color', '#B56F86');
  await expect(event).toHaveAccessibleName(/Office/);
});

test('moving an event reveals its destination and undo restores its original calendar', async ({
  page,
}) => {
  await page.goto('/?date=2026-10-09&scenario=empty');
  await createEvent(page, 'Move me');
  await createCalendar(page, 'Work');
  await page.getByRole('button', { name: 'Work', exact: true }).click();
  await page.getByRole('button', { name: /^Move me,/ }).click();
  const selector = page.getByLabel('Event calendar');
  await selector.selectOption({ label: 'Work' });
  await page.getByRole('button', { name: 'Save changes' }).click();
  await expect(page.getByRole('button', { name: /^Move me,/ })).toHaveAccessibleName(/Work/);
  await expect(page.getByRole('button', { name: 'Work', exact: true })).toHaveAttribute(
    'aria-pressed',
    'true',
  );
  await page.getByRole('button', { name: 'Undo', exact: true }).click();
  await expect(page.getByRole('button', { name: /^Move me,/ })).toHaveAccessibleName(/Personal/);
  await page.getByRole('button', { name: 'New event', exact: true }).click();
  await expect(page.getByLabel('Event calendar')).toHaveValue('local-default');
});

test('delete confirmation counts all events, allows zero calendars, and creation recovers', async ({
  page,
}) => {
  await page.goto('/?date=2026-10-09&scenario=empty');
  await createEvent(page, 'Delete me');
  await (await menu(page, 'Personal')).getByRole('menuitem', { name: 'Delete calendar' }).click();
  const dialog = page.getByRole('dialog', { name: 'Delete Personal?' });
  await expect(dialog).toContainText('all 1 of its events');
  await expect(dialog.getByRole('button', { name: 'Cancel' })).toBeFocused();
  await dialog.getByRole('button', { name: 'Delete calendar', exact: true }).click();
  await expect(page.locator('.calendar-row')).toHaveCount(0);
  await expect(page.getByRole('button', { name: 'New event', exact: true })).toBeDisabled();
  await expect(page.getByRole('button', { name: 'Undo', exact: true })).toHaveCount(0);
  await page.keyboard.press('n');
  await expect(page.getByLabel('Event title')).toHaveCount(0);
  await page.locator('.month-cell[aria-selected="true"]').focus();
  await page.keyboard.press('Enter');
  await expect(page.getByLabel('Event title')).toHaveCount(0);
  await createCalendar(page, 'Fresh');
  await expect(page.getByRole('button', { name: 'Fresh', exact: true })).toContainText('Default');
  await createEvent(page, 'Fresh event');
  await expect(page.getByRole('button', { name: /^Delete me,/ })).toHaveCount(0);
});

test('calendar menus and settings work with the keyboard and restore focus', async ({ page }) => {
  await page.goto('/?date=2026-10-09&scenario=empty');
  const trigger = page.getByRole('button', { name: 'Options for Personal' });
  await trigger.focus();
  await page.keyboard.press('ArrowDown');
  await expect(page.getByRole('menuitem', { name: 'Settings', exact: true })).toBeFocused();
  await page.keyboard.press('ArrowDown');
  await expect(page.getByRole('menuitem', { name: 'Delete calendar' })).toBeFocused();
  await page.keyboard.press('ArrowUp');
  await page.keyboard.press('Enter');
  await expect(page.getByLabel('Calendar name')).toBeFocused();
  await page.keyboard.press('Escape');
  await expect(trigger).toBeFocused();
  await expect(page.getByRole('button', { name: 'Personal', exact: true })).toHaveAttribute(
    'aria-pressed',
    'true',
  );
});
