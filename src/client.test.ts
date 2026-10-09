import { describe, expect, it, vi } from 'vitest';
import fixtures from '../tests/fixtures/time.json';

vi.stubGlobal('window', {});
const { resolveWallTime } = await import('./client');

describe('shared wall-clock fixtures', () => {
  for (const fixture of fixtures) {
    it(`${fixture.local} ${fixture.zone} ${fixture.offset ?? fixture.error ?? ''}`, async () => {
      const result = resolveWallTime(fixture.local, fixture.offset, fixture.zone);
      if (fixture.error) {
        await expect(result).rejects.toMatchObject({
          code: fixture.error,
          ...(fixture.offsets ? { offsets: fixture.offsets } : {}),
        });
      } else {
        await expect(result).resolves.toBe(fixture.utc);
      }
    });
  }
});
