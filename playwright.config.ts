import { defineConfig } from '@playwright/test';

export default defineConfig({
  testDir: './tests',
  timeout: 30_000,
  use: {
    baseURL: 'http://127.0.0.1:1420',
    viewport: { width: 1440, height: 960 },
    locale: 'en-GB',
    timezoneId: 'Europe/Zurich',
    launchOptions: process.env.CAL_CHROMIUM_PATH
      ? { executablePath: process.env.CAL_CHROMIUM_PATH, args: ['--no-sandbox'] }
      : {},
    trace: 'retain-on-failure',
    screenshot: 'only-on-failure',
  },
  webServer: {
    command: 'pnpm dev',
    url: 'http://127.0.0.1:1420',
    reuseExistingServer: !process.env.CI,
  },
});
