import { expect, test, type Page } from '@playwright/test';

function calendarRow(page: Page, name: string) {
  return page.locator('.calendar-row').filter({ has: page.getByText(name, { exact: true }) });
}

async function createCalendar(page: Page, name: string) {
  await page
    .locator('.sidebar')
    .getByRole('button', { name: 'Create calendar', exact: true })
    .click();
  await page.getByRole('menuitem', { name: 'On this device' }).click();
  const dialog = page.getByRole('dialog', { name: 'Create calendar' });
  await dialog.getByLabel('Calendar name').fill(name);
  await dialog.getByLabel('Calendar description').fill(`${name} description`);
  await dialog.getByRole('button', { name: 'Blue', exact: true }).click();
  await dialog.getByRole('button', { name: 'Create calendar', exact: true }).click();
  await expect(dialog).not.toBeVisible();
  await expect(calendarRow(page, name)).toBeVisible();
}
async function menu(page: Page, name: string) {
  const row = calendarRow(page, name);
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
  await expect(calendarRow(page, 'Personal')).toContainText('Default');
  await (await menu(page, 'Work')).getByRole('menuitem', { name: 'Set as default' }).click();
  await page.getByRole('button', { name: 'New event', exact: true }).click();
  await expect(page.getByLabel('Event calendar').locator('option:checked')).toHaveText('Work');
  await page.getByLabel('Event title').fill('Work appointment');
  await page.getByRole('button', { name: 'Create event', exact: true }).click();
  const event = page.getByRole('button', { name: /^Work appointment,/ });
  await expect(event).toBeVisible();
  await expect(event).toHaveCSS('--calendar-color', '#5B7FA3');
  await page.getByRole('button', { name: 'Hide Work calendar', exact: true }).click();
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
  await expect(
    page.getByRole('button', { name: 'Show Office calendar', exact: true }),
  ).toHaveAttribute('aria-pressed', 'false');
  await expect(calendarRow(page, 'Office').locator('.calendar-dot')).toHaveCSS(
    'background-color',
    'rgb(181, 111, 134)',
  );
  await page.getByRole('button', { name: 'Show Office calendar', exact: true }).click();
  await expect(event).toHaveCSS('--calendar-color', '#B56F86');
  await expect(event).toHaveAccessibleName(/Office/);
});

test('moving an event reveals its destination and undo restores its original calendar', async ({
  page,
}) => {
  await page.goto('/?date=2026-10-09&scenario=empty');
  await createEvent(page, 'Move me');
  await createCalendar(page, 'Work');
  await page.getByRole('button', { name: 'Hide Work calendar', exact: true }).click();
  await page.getByRole('button', { name: /^Move me,/ }).click();
  const selector = page.getByLabel('Event calendar');
  await selector.selectOption({ label: 'Work' });
  await page.getByRole('button', { name: 'Save changes' }).click();
  await expect(page.getByRole('button', { name: /^Move me,/ })).toHaveAccessibleName(/Work/);
  await expect(
    page.getByRole('button', { name: 'Hide Work calendar', exact: true }),
  ).toHaveAttribute('aria-pressed', 'true');
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
  await expect(calendarRow(page, 'Fresh')).toContainText('Default');
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
  await expect(
    page.getByRole('button', { name: 'Hide Personal calendar', exact: true }),
  ).toHaveAttribute('aria-pressed', 'true');
});

for (const theme of ['light', 'dark']) {
  test(`${theme} calendar dots and eye buttons keep visibility separate from labels`, async ({
    page,
  }) => {
    await page.goto(`/?date=2026-10-09&scenario=empty&theme=${theme}`);
    await createEvent(page, 'Personal appointment');
    const row = calendarRow(page, 'Personal');
    const dot = row.locator('.calendar-dot');
    const event = page.getByRole('button', { name: /^Personal appointment,/ });
    const eye = row.locator('.calendar-visibility');
    await expect(dot).toHaveCSS('background-color', 'rgb(113, 134, 117)');
    await expect(dot).toHaveCSS('border-radius', '50%');
    await expect(dot).toHaveCSS('width', '14px');
    await expect(dot).toHaveCSS('height', '14px');
    await page.locator('.sidebar-brand').hover();
    await expect(eye).toHaveCSS('opacity', '1');
    await expect(eye.locator('.lucide-eye')).toHaveCount(1);
    await row.locator('.calendar-name').click();
    await dot.click();
    await expect(event).toBeVisible();
    await expect(eye).toHaveAttribute('aria-pressed', 'true');
    await eye.focus();
    await page.keyboard.press('Space');
    await expect(eye).toHaveAccessibleName('Show Personal calendar');
    await expect(eye.locator('.lucide-eye-off')).toHaveCount(1);
    await expect(event).toHaveCount(0);
    await expect(dot).toHaveCSS('background-color', 'rgb(113, 134, 117)');
    await expect(eye).toBeFocused();
    await page.keyboard.press('Enter');
    await expect(eye).toHaveAccessibleName('Hide Personal calendar');
    await expect(event).toBeVisible();

    const longName = 'A calendar with a very long name that must leave room for both buttons';
    await createCalendar(page, longName);
    const longRow = calendarRow(page, longName);
    const layout = await longRow.evaluate((element) => {
      const name = element.querySelector('.calendar-name')!;
      const options = element.querySelector('.calendar-menu-trigger')!.getBoundingClientRect();
      const visibility = element.querySelector('.calendar-visibility')!.getBoundingClientRect();
      return {
        truncated: name.scrollWidth > name.clientWidth,
        nameRight: name.getBoundingClientRect().right,
        optionsLeft: options.left,
        optionsRight: options.right,
        visibilityLeft: visibility.left,
        visibilityRight: visibility.right,
        rowRight: element.getBoundingClientRect().right,
      };
    });
    expect(layout.truncated).toBe(true);
    expect(layout.nameRight).toBeLessThanOrEqual(layout.optionsLeft);
    expect(layout.optionsRight).toBeLessThanOrEqual(layout.visibilityLeft);
    expect(layout.visibilityRight).toBeLessThanOrEqual(layout.rowRight);
  });
}
