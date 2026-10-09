import { expect, test } from '@playwright/test';
import AxeBuilder from '@axe-core/playwright';

for (const theme of ['light', 'dark']) {
  test(`${theme} calendar views, editor, and settings have no serious accessibility violations`, async ({
    page,
  }) => {
    await page.emulateMedia({ reducedMotion: 'reduce' });
    await page.goto(`/?date=2026-10-09&theme=${theme}`);
    const audit = async () => {
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
    };
    await audit();
    for (const view of ['Week', 'Day']) {
      await page.getByRole('button', { name: view, exact: true }).click();
      await audit();
    }
    await page.getByRole('button', { name: 'New event' }).click();
    await expect(page.getByRole('textbox', { name: 'Event title' })).toBeFocused();
    await audit();
    await page.keyboard.press('Escape');
    await page.getByRole('button', { name: 'Open settings', exact: true }).click();
    await audit();
  });
}
