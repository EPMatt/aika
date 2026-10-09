// Drive the actual embedded OS webview and Rust/SQLite IPC. No command mocks.
import assert from 'node:assert/strict';
import { execFile, spawn } from 'node:child_process';
import { promisify } from 'node:util';
import { mkdtemp, mkdir, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { resolve, join } from 'node:path';
import { remote } from 'webdriverio';

const binary = resolve(
  process.env.AIKA_TEST_BINARY ??
    `src-tauri/target/debug/cal-desktop${process.platform === 'win32' ? '.exe' : ''}`,
);
const cliBinary = resolve(
  `src-tauri/target/debug/aika-cli${process.platform === 'win32' ? '.exe' : ''}`,
);
const runFile = promisify(execFile);
const directory = await mkdtemp(join(tmpdir(), 'aika-native-'));
async function cli(...args) {
  const { stdout } = await runFile(cliBinary, [
    '--database',
    join(directory, 'calendar.sqlite3'),
    'events',
    ...args,
  ]);
  return stdout;
}
const port = Number(process.env.TAURI_WEBDRIVER_PORT ?? 4445);
await mkdir('test-results/native', { recursive: true });
let app;
let browser;
async function start() {
  app = spawn(binary, [], {
    env: { ...process.env, AIKA_TEST_DATA_DIR: directory, TAURI_WEBDRIVER_PORT: String(port) },
    stdio: ['ignore', 'inherit', 'inherit'],
  });
  let spawnError;
  app.on('error', (error) => {
    spawnError = error;
  });
  for (let attempt = 0; attempt < 100; attempt++) {
    if (spawnError) throw spawnError;
    if (app.exitCode !== null) throw new Error(`Native test app exited: ${app.exitCode}`);
    try {
      if ((await fetch(`http://127.0.0.1:${port}/status`)).ok) break;
    } catch {
      /* startup */
    }
    await new Promise((done) => setTimeout(done, 100));
  }
  browser = await remote({
    hostname: '127.0.0.1',
    port,
    capabilities: { browserName: 'tauri' },
    logLevel: 'error',
    connectionRetryCount: 0,
  });
  await browser.waitUntil(async () => await browser.$('[data-runtime="desktop"]').isExisting(), {
    timeout: 15000,
  });
  await browser.$('[aria-label="New event"]').waitForEnabled();
  await browser.execute(() => {
    window.__nativeErrors = [];
    window.addEventListener('error', (e) => window.__nativeErrors.push(e.message));
    window.addEventListener('unhandledrejection', (e) =>
      window.__nativeErrors.push(String(e.reason)),
    );
  });
}
async function stop() {
  if (browser) {
    await browser.deleteSession();
    browser = undefined;
  }
  if (app && app.exitCode === null) {
    const exited = new Promise((done) => app.once('exit', done));
    app.kill();
    await exited;
  }
}
async function click(selector) {
  const e = await browser.$(selector);
  await e.waitForDisplayed();
  // The embedded driver's element click focuses *after* dispatch, which dismisses
  // Radix virtual-anchor popovers. Match normal focus-before-click ordering.
  await browser.execute((element) => {
    element.focus();
    element.click();
  }, e);
}
async function fill(selector, value) {
  const e = await browser.$(selector);
  await e.waitForDisplayed();
  await e.setValue(value);
}
async function appearance(value) {
  await click('[aria-label="Open settings"]');
  const select = await browser.$('[aria-label="Appearance preference"]');
  // The embedded driver clicks options without changing native select state.
  await browser.execute(
    (element, next) => {
      element.value = next;
      element.dispatchEvent(new Event('change', { bubbles: true }));
    },
    select,
    value,
  );
  await browser.waitUntil(
    async () => (await browser.$('html').getAttribute('data-theme')) === value,
  );
  await browser.keys('Escape');
}
async function eventCount(count) {
  await browser.waitUntil(async () => (await browser.$$('.event-chip')).length === count, {
    timeout: 10000,
  });
}
try {
  await start();
  await eventCount(0);
  await appearance('light');
  await click('[aria-label="New event"]');
  await fill('[aria-label="Event title"]', 'Native persistent event');
  await click('button[type="submit"]');
  await eventCount(1);
  await click('.event-chip');
  await fill('[aria-label="Event title"]', 'Native updated event');
  await click('button[type="submit"]');
  await browser.waitUntil(async () =>
    (await browser.$('.event-chip').getText()).includes('Native updated event'),
  );
  // Exercise the independent CLI while the actual native service holds the database open.
  const bounds = await browser.execute(() => {
    const today = new Date();
    const tomorrow = new Date(today);
    tomorrow.setDate(tomorrow.getDate() + 1);
    return {
      from: today.toLocaleDateString('en-CA'),
      to: tomorrow.toLocaleDateString('en-CA'),
      zone: Intl.DateTimeFormat().resolvedOptions().timeZone,
    };
  });
  const listed = await cli(
    'list',
    '--from',
    bounds.from,
    '--to',
    bounds.to,
    '--time-zone',
    bounds.zone,
  );
  const id = listed
    .split('\n')
    .find((line) => line.includes('Native updated event'))
    ?.split(' ')[0];
  assert.ok(id, 'CLI must read the desktop-created event');
  await click('.event-chip');
  await fill('[aria-label="Event title"]', 'Native unsaved draft');
  await cli('update', id, '--title', 'CLI changed event');
  await browser.execute(() => window.dispatchEvent(new Event('focus')));
  await browser.waitUntil(async () =>
    (await browser.$('.event-chip').getText()).includes('CLI changed event'),
  );
  assert.equal(await browser.$('[aria-label="Event title"]').getValue(), 'Native unsaved draft');
  await click('button[type="submit"]');
  await browser.waitUntil(async () =>
    (await browser.$('[role="alert"]').getText()).includes('event changed'),
  );
  assert.match(await cli('get', id), /Title: CLI changed event/);
  await browser.keys('Escape');
  await click('button=Undo');
  await browser.waitUntil(async () =>
    (await browser.$('[role="alert"]').getText()).includes('Undo cannot overwrite'),
  );
  assert.match(await cli('get', id), /Title: CLI changed event/);
  await cli('update', id, '--title', 'Native updated event');
  await browser.execute(() => window.dispatchEvent(new Event('focus')));
  await browser.waitUntil(async () =>
    (await browser.$('.event-chip').getText()).includes('Native updated event'),
  );
  await browser.saveScreenshot('test-results/native/month-light.png');
  await stop();
  await start();
  await eventCount(1);
  assert.match(await browser.$('.event-chip').getText(), /Native updated event/);
  assert.equal(await browser.$('button=Undo').isExisting(), false);
  await click('.event-chip');
  await click('[aria-label="Delete event"]');
  await eventCount(0);
  await click('button=Undo');
  await eventCount(1);
  await click('button=Week');
  await browser.$('.time-event').waitForDisplayed();
  await browser.saveScreenshot('test-results/native/week-light.png');
  await click('button=Day');
  await browser.$('.time-event').waitForDisplayed();
  // The time column has an explicit keyboard creation handler; native button
  // default Enter activation is outside the synthetic embedded driver coverage.
  await browser.execute(() => document.querySelector('.time-column').focus());
  await browser.keys('Enter');
  await fill('[aria-label="Event title"]', 'Keyboard edit');
  await browser.keys('Escape');
  await browser.waitUntil(
    async () => !(await browser.$('[aria-label="Event editor"]').isExisting()),
  );
  await appearance('dark');
  await browser.saveScreenshot('test-results/native/day-dark.png');
  await stop();
  await start();
  assert.equal(await browser.$('html').getAttribute('data-theme'), 'dark');
  await browser.$('.time-event').waitForDisplayed();
  await click('[aria-label="New event"]');
  await fill('[aria-label="Event title"]', 'Native all-day event');
  await click('[aria-label="All-day event"]');
  await click('button[type="submit"]');
  await eventCount(1);
  await stop();
  await start();
  await eventCount(1);
  assert.match(await browser.$('.event-chip').getText(), /Native all-day event/);
  await click('.event-chip');
  await click('[aria-label="Delete event"]');
  await eventCount(0);
  await click('.time-event');
  await click('[aria-label="Delete event"]');
  await browser.waitUntil(async () => (await browser.$$('.time-event')).length === 0);
  await stop();
  await start();
  assert.equal((await browser.$$('.time-event')).length, 0);
  await eventCount(0);
  console.log(
    'Native timed/all-day CRUD, CLI interoperability, stale edits/undo, restart, views, keyboard dismissal, and appearance passed.',
  );
} catch (error) {
  if (browser)
    console.log(
      await browser.execute(() => ({
        errors: window.__nativeErrors,
        runtime: document.querySelector('[data-runtime]')?.getAttribute('data-runtime'),
      })),
    );
  if (browser) await browser.saveScreenshot('test-results/native/failure.png').catch(() => {});
  throw error;
} finally {
  await stop();
  await rm(directory, { recursive: true, force: true });
}
